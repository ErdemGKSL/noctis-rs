//! Story 6.5 Matrix 1-17, 20, 35 and 45-51: the Command pipeline and the
//! Dispatcher over a recording upstream.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyper::Method;
use noctis_gateway::RawJson;
use noctis_rules::command::{OptionDecl, OptionType};
use noctis_ui::{ActionRow, Components, LinkButton, ResolvedMessage, ResolvedUser, V2Message};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;
use twilight_model::gateway::ShardId;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::MessageCreate;

use super::*;
use crate::edge::test_support::Answer;
use crate::events::{EventCtx, EventHandler};
use crate::options::__options::required;
use crate::options::{OptionError, OptionValue, Options};
use crate::responder::tests::{Fixture, discord, skip, v2};
use crate::responder::{ResponderError, Visibility};
use crate::test_support::{Capture, Recording, Seen};
use crate::{
    App, AppBuilder, CommandHandler, CommandMetadata, CommandResponder, ContextMenuHandler,
    ErrorOrigin, Module, Responded, Runtime,
};

/// A shared log of what handlers saw.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn push(&self, line: impl Into<String>) {
        self.0.lock().unwrap().push(line.into());
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// A running App with `module`, a recording Command sink and a recording
/// upstream.
struct Pipeline {
    fx: Fixture,
    sink: Arc<Recording>,
}

impl Pipeline {
    async fn start(module: Module) -> Self {
        Self::start_with(module, |_| {}).await
    }

    /// As [`Pipeline::start`], with `configure` applied to the builder.
    async fn start_with(module: Module, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        Self::start_answering(module, discord(), configure).await
    }

    /// As [`Pipeline::start_with`], with the upstream answering `answer`.
    async fn start_answering(
        module: Module,
        answer: Answer,
        configure: impl FnOnce(&mut AppBuilder),
    ) -> Self {
        let server = crate::edge::test_support::TestServer::start_with(false, answer).await;
        let sink = Arc::new(Recording::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .http_proxy(server.authority())
            .error_sink(ErrorOrigin::Command, Arc::clone(&sink))
            .module(module);
        configure(&mut builder);
        let app = builder.build().unwrap();
        Self {
            fx: Fixture { server, app },
            sink,
        }
    }

    /// As [`Pipeline::start`], without a custom Command sink (the default
    /// `LogSink` reports).
    async fn start_default_sink(module: Module) -> Self {
        let server = crate::edge::test_support::TestServer::start_with(false, discord()).await;
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .http_proxy(server.authority())
            .module(module);
        let app = builder.build().unwrap();
        Self {
            fx: Fixture { server, app },
            sink: Arc::new(Recording::default()),
        }
    }

    async fn run(&self, inbound: Inbound) {
        self.run_with(inbound, CancellationToken::new()).await;
    }

    async fn run_with(&self, inbound: Inbound, cancel: CancellationToken) {
        run_interaction(
            Arc::clone(self.fx.app.registry()),
            Arc::clone(self.fx.app.shared()),
            inbound,
            cancel,
        )
        .await;
    }

    fn requests(&self) -> Vec<(Method, String, Value)> {
        self.fx.requests()
    }

    fn reports(&self) -> Vec<Seen> {
        self.sink.seen()
    }

    fn dispatcher(&self) -> Dispatcher {
        Dispatcher::new(Arc::clone(self.fx.app.shared()))
    }
}

fn user(id: &str) -> Value {
    json!({ "id": id, "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// An `APPLICATION_COMMAND` interaction (id 100, token `tok`,
/// application 7, user 3) with `data` and the `extra` top-level fields.
#[expect(
    clippy::needless_pass_by_value,
    reason = "call sites pass `json!` literals"
)]
fn interaction(data: Value, extra: Value) -> Value {
    let mut d = json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user("3"),
        "data": data,
    });
    if let (Value::Object(d), Value::Object(extra)) = (&mut d, extra) {
        d.extend(extra);
    }
    d
}

/// A chat-input `data` for `name` with `options`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "call sites pass `json!` literals"
)]
fn slash(name: &str, options: Value) -> Value {
    json!({ "id": "9", "name": name, "type": 1, "options": options })
}

/// The inbound item of `parsed`, carrying `raw` as its exact bytes.
fn inbound_raw(parsed: &Value, raw: &Value) -> Inbound {
    let interaction = serde_json::from_value(parsed.clone()).unwrap();
    Inbound {
        shard: ShardId::ONE,
        received_at: Instant::now(),
        kind: InboundKind::Interaction(
            Box::new(interaction),
            RawJson::new(bytes::Bytes::from(raw.to_string())),
        ),
    }
}

fn inbound(json: &Value) -> Inbound {
    inbound_raw(json, json)
}

/// A handler at `paths` logging `ctx.command_path()` and replying.
fn logging(log: &Log, name: &'static str, paths: &'static [&'static str]) -> CommandHandler {
    let log = log.clone();
    CommandHandler::__new_plain(
        name,
        paths,
        "d",
        move |ctx: CommandCtx, _: CommandResponder| {
            tracing::info!("logged");
            log.push(format!("{name}:{}", ctx.command_path()));
            async { Ok(v2("ok")) }
        },
    )
}

/// Panics inside a handler future.
fn boom() -> V2Message {
    panic!("boom")
}

const CB: &str = "/api/v10/interactions/100/tok/callback";
/// A type-4 callback asks for the message (`with_response`).
const CB_REPLY: &str = "/api/v10/interactions/100/tok/callback?with_response=true";
/// `noctis.error.generic` in en-US.
const GENERIC: &str = "Something went wrong. Please try again later.";
/// `noctis.validation.failed` in en-US.
const VALIDATION: &str = "That input isn't valid. Check it and try again.";

/// Asserts `request` is the ephemeral type-4 notice callback of `text`.
fn assert_notice(request: &(Method, String, Value), text: &str) {
    assert_eq!(request.0, Method::POST, "{request:?}");
    assert_eq!(request.1, CB, "{request:?}");
    assert_eq!(request.2["type"], 4, "{request:?}");
    assert_eq!(request.2["data"]["flags"], 32832, "{request:?}");
    assert_eq!(
        request.2["data"]["components"],
        json!([{ "type": 10, "id": 1, "content": text }]),
        "{request:?}"
    );
}

/// Asserts `request` is an ephemeral follow-up notice of `text`.
fn assert_followup_notice(request: &(Method, String, Value), text: &str) {
    assert_eq!(request.0, Method::POST, "{request:?}");
    assert_eq!(
        request.1, "/api/v10/webhooks/7/tok?wait=true",
        "{request:?}"
    );
    assert_eq!(request.2["flags"], 32832, "{request:?}");
    assert_eq!(request.2["components"][0]["content"], text, "{request:?}");
}

/// Matrix 1 and the Resolution AC: one type-4 callback, V2 flags, and the
/// span carries `module` and `path`.
#[tokio::test]
async fn global_slash_replies() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let handler = CommandHandler::__new_plain(
        "m::ping",
        &["ping"],
        "d",
        |_: CommandCtx, _: CommandResponder| async {
            tracing::info!("inside");
            Ok(v2("pong"))
        },
    );
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    p.run(inbound(&interaction(slash("ping", json!([])), json!({}))))
        .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].1, CB_REPLY);
    assert_eq!(requests[0].2["type"], 4);
    assert_eq!(requests[0].2["data"]["flags"], 32768);
    assert!(requests[0].2["data"]["allowed_mentions"].is_object());
    let lines = capture.lines_with(&["inside"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(
        lines[0].contains("noctis.interaction{app=\"default\" shard=0 module=\"m\" path=\"ping\"}"),
        "{}",
        lines[0]
    );
    assert!(p.reports().is_empty());
}

/// Matrix 2: the declared ephemerality.
#[tokio::test]
async fn declared_ephemeral() {
    let log = Log::default();
    let handler = logging(&log, "m::ping", &["ping"]).__with_ephemerality(Ephemerality::Ephemeral);
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    p.run(inbound(&interaction(slash("ping", json!([])), json!({}))))
        .await;
    assert_eq!(p.requests()[0].2["data"]["flags"], 32832);
}

/// Matrix 3: a three-word path.
#[tokio::test]
async fn three_word_path() {
    let log = Log::default();
    let handler = logging(&log, "m::config", &["config set prefix"]);
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    let options = json!([{ "type": 2, "name": "set", "options": [{ "type": 1, "name": "prefix", "options": [] }] }]);
    p.run(inbound(&interaction(slash("config", options), json!({}))))
        .await;
    assert_eq!(log.lines(), ["m::config:config set prefix"]);
    assert_eq!(p.requests().len(), 1);
}

/// Matrix 4-7: scopes, without fallback, and unknown paths.
#[tokio::test]
async fn scopes_and_unknown_paths() {
    static GUILDS: [Id<GuildMarker>; 1] = [Id::new(9)];
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let log = Log::default();
    let module = Module::new("m")
        .__add(logging(&log, "m::ping", &["ping"]))
        .__add(
            logging(&log, "m::gping", &["gping"])
                .__with_metadata(CommandMetadata::new().guilds(&GUILDS)),
        );
    let p = Pipeline::start(module).await;
    let mut guild_data = slash("gping", json!([]));
    guild_data["guild_id"] = json!("9");
    p.run(inbound(&interaction(
        guild_data,
        json!({ "guild_id": "9" }),
    )))
    .await;
    p.run(inbound(&interaction(
        slash("ping", json!([])),
        json!({ "guild_id": "9" }),
    )))
    .await;
    assert_eq!(log.lines(), ["m::gping:gping", "m::ping:ping"]);
    assert_eq!(p.requests().len(), 2);

    // A guild-scoped invocation of a global path does not fall back.
    let mut stale = slash("ping", json!([]));
    stale["guild_id"] = json!("9");
    p.run(inbound(&interaction(stale, json!({ "guild_id": "9" }))))
        .await;
    for name in ["nope", "", "gping"] {
        p.run(inbound(&interaction(slash(name, json!([])), json!({}))))
            .await;
    }
    assert_eq!(log.lines().len(), 2);
    assert_eq!(p.requests().len(), 2);
    assert!(p.reports().is_empty());
    assert_eq!(
        capture
            .lines_with(&["noctis::dispatch", "unknown command"])
            .len(),
        4
    );
}

/// LOC-06 (Matrix 8 and 9): dispatch by default names; the locales and the
/// locale default.
#[tokio::test]
async fn loc_06_dispatch_resolves_default_names() {
    let log = Log::default();
    let record = |log: &Log, name: &'static str| {
        let log = log.clone();
        move |ctx: CommandCtx, _: CommandResponder| {
            log.push(format!(
                "{name}:{:?}:{:?}:{}",
                ctx.user_locale(),
                ctx.guild_locale(),
                ctx.response_locale()
            ));
            async { Ok(v2("ok")) }
        }
    };
    let module = Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::p",
            &["public"],
            "d",
            record(&log, "public"),
        ))
        .__add(
            CommandHandler::__new_plain("m::e", &["private"], "d", record(&log, "private"))
                .__with_ephemerality(Ephemerality::Ephemeral),
        );
    let p = Pipeline::start(module).await;
    let locales = json!({ "locale": "tr", "guild_locale": "de" });
    for name in ["public", "private"] {
        p.run(inbound(&interaction(
            slash(name, json!([])),
            locales.clone(),
        )))
        .await;
    }
    p.run(inbound(&interaction(slash("public", json!([])), json!({}))))
        .await;
    p.run(inbound(&interaction(
        slash("public", json!([])),
        json!({ "locale": "tr" }),
    )))
    .await;
    assert_eq!(
        log.lines(),
        [
            "public:Some(\"tr\"):Some(\"de\"):de",
            "private:Some(\"tr\"):Some(\"de\"):tr",
            "public:None:None:en-US",
            "public:Some(\"tr\"):None:tr",
        ]
    );
}

/// Matrix 10 and 11: Unicode names, and a USER and a MESSAGE menu sharing
/// one name; a missing target goes to the sink.
#[tokio::test]
async fn unicode_names_and_menus() {
    let log = Log::default();
    let user_log = log.clone();
    let message_log = log.clone();
    let module = Module::new("m")
        .__add(logging(&log, "m::ayarlar", &["ayarlar"]))
        .__add(ContextMenuHandler::__new_user(
            "m::show",
            "Profil göster",
            CommandMetadata::new(),
            move |_: CommandCtx, target: ResolvedUser, _: CommandResponder| {
                user_log.push(format!("user:{}", target.user.id));
                async { Ok(v2("u")) }
            },
        ))
        .__add(ContextMenuHandler::__new_message(
            "m::quote",
            "Profil göster",
            CommandMetadata::new(),
            move |ctx: CommandCtx, target: ResolvedMessage, r: CommandResponder| {
                message_log.push(format!(
                    "message:{}:{}",
                    target.message.id,
                    ctx.command_path()
                ));
                async move { Ok(r.defer_reply_as(Ephemerality::Public).await?.done()) }
            },
        ));
    let p = Pipeline::start(module).await;
    p.run(inbound(&interaction(
        slash("ayarlar", json!([])),
        json!({}),
    )))
    .await;
    let user_data = json!({
        "id": "9", "name": "Profil göster", "type": 2, "target_id": "11",
        "resolved": { "users": { "11": user("11") } }
    });
    p.run(inbound(&interaction(user_data, json!({})))).await;
    let message = json!({
        "id": "44", "channel_id": "2", "author": user("3"), "content": "x",
        "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
        "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
        "attachments": [], "embeds": [], "pinned": false, "type": 0
    });
    let message_data = json!({
        "id": "9", "name": "Profil göster", "type": 3, "target_id": "44",
        "resolved": { "messages": { "44": message } }
    });
    p.run(inbound(&interaction(message_data, json!({})))).await;
    assert_eq!(
        log.lines(),
        ["m::ayarlar:ayarlar", "user:11", "message:44:Profil göster"]
    );
    assert_eq!(p.requests()[2].2, json!({ "type": 5 }));

    let missing = json!({ "id": "9", "name": "Profil göster", "type": 2, "target_id": "12" });
    p.run(inbound(&interaction(missing, json!({})))).await;
    let reports = p.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].kind,
        "error:target 12 is not in the resolved data"
    );
    assert_eq!(reports[0].handler.as_deref(), Some("m::show"));
    let requests = p.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_notice(&requests[3], GENERIC);
}

/// Matrix 13 and 14: malformed command data and a missing author.
#[tokio::test]
async fn malformed_and_authorless() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let log = Log::default();
    let p = Pipeline::start(Module::new("m").__add(logging(&log, "m::warn", &["warn"]))).await;
    let parsed = interaction(slash("warn", json!([])), json!({}));
    let mut raw = parsed.clone();
    raw["data"]["options"] =
        json!([{ "name": "days", "type": 4, "value": 9_223_372_036_854_775_808_u64 }]);
    p.run(inbound_raw(&parsed, &raw)).await;
    raw["data"]["options"] = json!([
        { "name": "days", "type": 4, "value": 1 },
        { "name": "days", "type": 4, "value": 2 }
    ]);
    p.run(inbound_raw(&parsed, &raw)).await;
    let reports = p.reports();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.origin == ErrorOrigin::Command));
    assert!(log.lines().is_empty());
    // Story 6.8 Matrix 6: a decode failure shows the validation notice.
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    for request in &requests {
        assert_notice(request, VALIDATION);
    }

    let mut authorless = interaction(slash("warn", json!([])), json!({}));
    authorless.as_object_mut().unwrap().remove("user");
    p.run(inbound(&authorless)).await;
    assert!(log.lines().is_empty());
    // No notice without an author.
    assert_eq!(p.requests().len(), 2);
    assert_eq!(capture.lines_with(&["without an author"]).len(), 1);
}

/// Matrix 15, 16 and 35: errors and panics go to the sink; the next
/// interaction is still served.
#[tokio::test]
async fn errors_and_panics_reach_the_sink() {
    let log = Log::default();
    let module = Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::fail",
            &["fail"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                Err::<V2Message, _>(HandlerError::new(std::io::Error::other("boom")))
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::sync",
            &["sync"],
            "d",
            |_: CommandCtx, _: CommandResponder| -> std::future::Ready<HandlerResult<V2Message>> {
                panic!("sync boom")
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::inner",
            &["inner"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                tokio::task::yield_now().await;
                Ok(boom())
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::big",
            &["big"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                Ok(Components::new().push(ActionRow::buttons([
                    LinkButton::new("https://x.y").label("a".repeat(81))
                ])))
            },
        ))
        .__add(logging(&log, "m::ok", &["ok"]));
    let p = Pipeline::start(module).await;
    for name in ["fail", "sync", "inner", "big", "ok"] {
        p.run(inbound(&interaction(
            slash(name, json!([])),
            json!({ "guild_id": "4" }),
        )))
        .await;
    }
    let reports = p.reports();
    assert_eq!(
        reports[0],
        Seen {
            origin: ErrorOrigin::Command,
            kind: "error:boom".to_owned(),
            app: "default".to_owned(),
            shard: Some(0),
            guild_id: Some(4),
            module: Some("m".to_owned()),
            handler: Some("m::fail".to_owned()),
            event_type: Some("INTERACTION_CREATE".to_owned()),
        }
    );
    assert_eq!(reports[1].kind, "panic:sync boom");
    assert_eq!(reports[2].kind, "panic:boom");
    assert!(
        reports[3].kind.starts_with("error:[CV2-08]"),
        "{}",
        reports[3].kind
    );
    assert_eq!(reports.len(), 4);
    assert_eq!(log.lines(), ["m::ok:ok"]);
    // Story 6.8 Matrix 1, 5 and 13: one generic notice per failure (the
    // invalid message was never sent), then the next one is served.
    let requests = p.requests();
    assert_eq!(requests.len(), 5, "{requests:?}");
    for request in &requests[..4] {
        assert_notice(request, GENERIC);
    }
    assert_eq!(requests[4].1, CB_REPLY);
}

/// Matrix 17: a shutdown cancel drops the handler and reports it.
#[tokio::test]
async fn cancel_mid_handler() {
    let started = Arc::new(Notify::new());
    let entered = Arc::clone(&started);
    let handler = CommandHandler::__new_plain(
        "m::slow",
        &["slow"],
        "d",
        move |_: CommandCtx, _: CommandResponder| {
            let entered = Arc::clone(&entered);
            async move {
                entered.notify_one();
                std::future::pending::<()>().await;
                Ok(v2("never"))
            }
        },
    );
    let p = Arc::new(Pipeline::start(Module::new("m").__add(handler)).await);
    let cancel = CancellationToken::new();
    let run = tokio::spawn({
        let p = Arc::clone(&p);
        let cancel = cancel.clone();
        async move {
            p.run_with(
                inbound(&interaction(slash("slow", json!([])), json!({}))),
                cancel,
            )
            .await;
        }
    });
    started.notified().await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.reports()[0].kind, "cancelled");
    assert!(p.requests().is_empty());
}

/// INT-12 (Matrix 20): a returned message after a defer edits the
/// original, with the V2 flag and no legacy clears.
#[tokio::test]
async fn int_12_returned_message_after_defer_edits_original() {
    let handler = CommandHandler::__new_plain(
        "m::slow",
        &["slow"],
        "d",
        |_: CommandCtx, r: CommandResponder| async move {
            let _ = r.defer_reply_as(Ephemerality::Public).await?.done();
            Ok(v2("done"))
        },
    );
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    p.run(inbound(&interaction(slash("slow", json!([])), json!({}))))
        .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        requests[0],
        (Method::POST, CB.to_owned(), json!({ "type": 5 }))
    );
    assert_eq!(requests[1].0, Method::PATCH);
    assert_eq!(requests[1].1, "/api/v10/webhooks/7/tok/messages/@original");
    assert_eq!(requests[1].2["flags"], 32768);
    assert!(requests[1].2.get("content").is_none());
    assert!(p.reports().is_empty());
}

/// Matrix 18 and 21: a reply then a returned message edits; `done` sends
/// nothing more.
#[tokio::test]
async fn reply_then_returned_message() {
    let module = Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::two",
            &["two"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                let _ = r.reply(v2("a")).await?.done();
                Ok(v2("b"))
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::one",
            &["one"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                Ok::<Responded, HandlerError>(r.reply(v2("a")).await?.done())
            },
        ));
    let p = Pipeline::start(module).await;
    p.run(inbound(&interaction(slash("two", json!([])), json!({}))))
        .await;
    p.run(inbound(&interaction(slash("one", json!([])), json!({}))))
        .await;
    let methods: Vec<Method> = p.requests().into_iter().map(|r| r.0).collect();
    assert_eq!(methods, [Method::POST, Method::PATCH, Method::POST]);
}

/// The options of [`warn`]: `days`, a required INTEGER.
struct Days(i64);

impl Options for Days {
    const OPTIONS: &'static [OptionDecl<'static>] =
        &[OptionDecl::new("days", OptionType::Integer, "d").required(true)];
    type Partial = crate::autocomplete::NoPartial<Self>;

    fn from_input(input: &CommandInput) -> Result<Self, OptionError> {
        Ok(Self(required(input, "days")?))
    }
}

/// Matrix 45-47 and 51: the Dispatcher.
#[tokio::test]
async fn dispatcher_runs_commands() {
    static GUILDS: [Id<GuildMarker>; 1] = [Id::new(9)];
    let log = Log::default();
    let days_log = log.clone();
    let module = Module::new("m")
        .__add(logging(&log, "m::ping", &["ping"]))
        .__add(
            logging(&log, "m::gping", &["ping"])
                .__with_metadata(CommandMetadata::new().guilds(&GUILDS)),
        )
        .__add(CommandHandler::__new_plain_with_options(
            "m::warn",
            &["warn"],
            "d",
            move |ctx: CommandCtx, _: CommandResponder, days: Days| {
                days_log.push(format!(
                    "warn:{}:{:?}:{}",
                    days.0,
                    ctx.user_locale(),
                    ctx.shard()
                ));
                async { Ok(v2("warned")) }
            },
        ));
    let p = Pipeline::start(module).await;
    let dispatcher = p.dispatcher();
    let invocation = |input| CommandInvocation::new(input, Id::new(5), Id::new(6), Id::new(3));
    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        dispatcher
            .run_command(invocation(CommandInput::new("ping")))
            .await
            .unwrap();
    }
    let lines = capture.lines_with(&["logged"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(
        lines[0].contains("noctis.dispatch{")
            && lines[0].contains("module=\"m\"")
            && lines[0].contains("path=\"ping\""),
        "{}",
        lines[0]
    );
    let requests = p.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, Method::POST);
    assert_eq!(requests[0].1, "/api/v10/channels/5/messages");
    assert_eq!(requests[0].2["flags"], 32768);
    assert_eq!(
        requests[0].2["message_reference"],
        json!({ "message_id": "6", "fail_if_not_exists": false })
    );
    dispatcher
        .run_command(invocation(CommandInput::new("ping")).guild_id(Some(Id::new(9))))
        .await
        .unwrap();
    dispatcher
        .run_command(invocation(CommandInput::new("ping")).guild_id(Some(Id::new(8))))
        .await
        .unwrap();
    let err = dispatcher
        .run_command(invocation(CommandInput::new("nope")))
        .await
        .unwrap_err();
    assert!(matches!(&err, DispatchError::UnknownCommand { path } if &**path == "nope"));
    assert_eq!(p.requests().len(), 3);

    let input = CommandInput::new("warn")
        .with_option("days", OptionValue::Integer(1))
        .with_option("days", OptionValue::Integer(3));
    dispatcher
        .run_command(invocation(input).locale("tr").shard(2))
        .await
        .unwrap();
    dispatcher
        .run_command(invocation(CommandInput::new("warn")))
        .await
        .unwrap();
    assert_eq!(
        log.lines(),
        [
            "m::ping:ping",
            "m::gping:ping",
            "m::ping:ping",
            "warn:3:Some(\"tr\"):2"
        ]
    );
    let reports = p.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].kind, "error:required option `days` is missing");
    assert_eq!(reports[0].handler.as_deref(), Some("m::warn"));
    assert!(format!("{dispatcher:?}").starts_with("Dispatcher"));
}

/// An `#[event]`-style handler dispatching through `EventCtx::dispatcher`.
#[tokio::test]
async fn event_handler_dispatches() {
    let log = Log::default();
    let p = Pipeline::start(Module::new("m").__add(logging(&log, "m::ping", &["ping"]))).await;
    let handler = EventHandler::__new(
        "m::prefix",
        false,
        false,
        |ctx: EventCtx, msg: MessageCreate| async move {
            let path = msg.content.strip_prefix('!').unwrap_or_default().to_owned();
            let invocation = CommandInvocation::new(
                CommandInput::new(path),
                msg.channel_id,
                msg.id,
                msg.author.id,
            )
            .guild_id(msg.guild_id);
            ctx.dispatcher().run_command(invocation).await?;
            Ok(())
        },
    );
    let message: MessageCreate = MessageCreate(
        serde_json::from_value(json!({
            "id": "6", "channel_id": "5", "author": user("3"), "content": "!ping",
            "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0
        }))
        .unwrap(),
    );
    let ctx = EventCtx::new(0, None, "m", Arc::clone(p.fx.app.shared()), None);
    handler
        .call(ctx, Event::MessageCreate(Box::new(message)))
        .unwrap()
        .await
        .unwrap();
    assert_eq!(log.lines(), ["m::ping:ping"]);
    assert_eq!(p.requests()[0].1, "/api/v10/channels/5/messages");
}

/// The context `run_interaction` builds: author, channel, guild, module,
/// App state and the App's HTTP client.
#[tokio::test]
async fn pipeline_ctx_contents() {
    let seen: Arc<Mutex<Option<CommandCtx>>> = Arc::default();
    let handler = {
        let seen = Arc::clone(&seen);
        CommandHandler::__new_plain(
            "m::probe",
            &["probe"],
            "d",
            move |ctx: CommandCtx, _: CommandResponder| {
                *seen.lock().unwrap() = Some(ctx);
                async { Ok(v2("ok")) }
            },
        )
    };
    let p = Pipeline::start_with(Module::new("profile/xp").__add(handler), |b| {
        b.state(7_u32);
    })
    .await;
    p.run(inbound(&interaction(
        slash("probe", json!([])),
        json!({ "guild_id": "4", "channel": { "id": "5", "type": 0 } }),
    )))
    .await;
    let ctx = seen.lock().unwrap().take().unwrap();
    assert_eq!(ctx.user_id(), Id::new(3));
    assert_eq!(ctx.channel_id(), Some(Id::new(5)));
    assert_eq!(ctx.guild_id(), Some(Id::new(4)));
    assert_eq!(ctx.module(), "profile/xp");
    assert_eq!(*ctx.state::<u32>().unwrap(), 7);
    assert!(ctx.state::<u64>().is_none());
    assert!(Arc::ptr_eq(ctx.http(), p.fx.app.http()));
}

/// An `ephemeral` USER menu: callback flags 32832 and the User Locale
/// first.
#[tokio::test]
async fn ephemeral_user_menu() {
    let log = Log::default();
    let menu = {
        let log = log.clone();
        ContextMenuHandler::__new_user(
            "m::show",
            "Show",
            CommandMetadata::new(),
            move |ctx: CommandCtx, _: ResolvedUser, _: CommandResponder| {
                log.push(ctx.response_locale());
                async { Ok(v2("u")) }
            },
        )
        .__with_ephemerality(Ephemerality::Ephemeral)
    };
    let p = Pipeline::start(Module::new("m").__add(menu)).await;
    let data = json!({
        "id": "9", "name": "Show", "type": 2, "target_id": "11",
        "resolved": { "users": { "11": user("11") } }
    });
    p.run(inbound(&interaction(
        data,
        json!({ "locale": "tr", "guild_locale": "de" }),
    )))
    .await;
    assert_eq!(log.lines(), ["tr"]);
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].2["type"], 4);
    assert_eq!(requests[0].2["data"]["flags"], 32832);
}

/// `execute` records one duration sample and one `kind="command"` error
/// for a failing Command.
#[tokio::test]
async fn command_metrics() {
    use metrics_util::MetricKind;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let handler = CommandHandler::__new_plain(
        "m::fail",
        &["fail"],
        "d",
        |_: CommandCtx, _: CommandResponder| async {
            Err::<V2Message, _>(HandlerError::new(std::io::Error::other("boom")))
        },
    );
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    p.run(inbound(&interaction(slash("fail", json!([])), json!({}))))
        .await;
    assert_eq!(p.reports().len(), 1);
    let snap = snapshotter.snapshot().into_vec();
    let durations = snap
        .iter()
        .find(|(k, ..)| k.key().name() == "noctis_handler_duration_seconds")
        .map(|(.., v)| v);
    assert!(
        matches!(durations, Some(DebugValue::Histogram(v)) if v.len() == 1),
        "{snap:?}"
    );
    let errors: Vec<_> = snap
        .iter()
        .filter(|(k, ..)| {
            k.kind() == MetricKind::Counter && k.key().name() == "noctis_handler_errors_total"
        })
        .collect();
    assert_eq!(errors.len(), 1, "{snap:?}");
    assert!(matches!(errors[0].3, DebugValue::Counter(1)), "{snap:?}");
    assert!(
        errors[0]
            .0
            .key()
            .labels()
            .any(|l| l.key() == "kind" && l.value() == "command"),
        "{snap:?}"
    );
}

// ---- Story 6.6: auto-defer, token lifetime and follow-up cap ----

const ORIGINAL: &str = "/api/v10/webhooks/7/tok/messages/@original";

/// `inbound(json)`, received `ago` before now (queueing time counts).
fn inbound_ago(json: &Value, ago: Duration) -> Inbound {
    let mut inbound = inbound(json);
    inbound.received_at = Instant::now().checked_sub(ago).unwrap();
    inbound
}

/// A Command at `paths` that sleeps `delay` (real time), then returns a
/// message.
fn sleeper(name: &'static str, paths: &'static [&'static str], delay: Duration) -> CommandHandler {
    CommandHandler::__new_plain(
        name,
        paths,
        "d",
        move |_: CommandCtx, _: CommandResponder| async move {
            tokio::time::sleep(delay).await;
            Ok(v2("late"))
        },
    )
}

/// A local metrics recorder: the auto-defer task runs on the test's
/// thread, so it records into it too. A snapshot drains the recorder, so
/// the totals accumulate here.
struct Metrics {
    recorder: metrics_util::debugging::DebuggingRecorder,
    totals: Mutex<(u64, Vec<f64>)>,
}

impl Metrics {
    fn new() -> Self {
        Self {
            recorder: metrics_util::debugging::DebuggingRecorder::new(),
            totals: Mutex::default(),
        }
    }

    /// `noctis_autodefer_total` and every `noctis_ack_latency_seconds`
    /// sample so far; both carry only the `app` label.
    fn read(&self) -> (u64, Vec<f64>) {
        use metrics_util::debugging::DebugValue;
        let mut totals = self.totals.lock().unwrap();
        let (count, samples) = &mut *totals;
        for (key, _, _, value) in self.recorder.snapshotter().snapshot().into_vec() {
            let name = key.key().name();
            if name != "noctis_autodefer_total" && name != "noctis_ack_latency_seconds" {
                continue;
            }
            let labels: Vec<_> = key.key().labels().map(|l| l.key().to_owned()).collect();
            assert_eq!(labels, ["app"], "{name}");
            match value {
                DebugValue::Counter(n) => *count += n,
                DebugValue::Histogram(v) => samples.extend(v.into_iter().map(f64::from)),
                DebugValue::Gauge(_) => {}
            }
        }
        totals.clone()
    }
}

/// INT-01 (Story 6.6 Matrix 2, 3 and 10): a handler that has not answered
/// by the deadline is deferred with exactly `{"type":5}` (plus `EPHEMERAL`
/// when declared), then its message edits the original.
#[tokio::test]
async fn int_01_auto_defer_at_deadline() {
    let metrics = Metrics::new();
    let _metrics = metrics::set_default_local_recorder(&metrics.recorder);
    let module = Module::new("m")
        .__add(sleeper("m::slow", &["slow"], Duration::from_millis(300)))
        .__add(
            sleeper("m::hidden", &["hidden"], Duration::from_millis(300))
                .__with_ephemerality(Ephemerality::Ephemeral),
        )
        .__add(ContextMenuHandler::__new_user(
            "m::show",
            "Show",
            CommandMetadata::new(),
            |_: CommandCtx, _: ResolvedUser, _: CommandResponder| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok(v2("late"))
            },
        ));
    let p = Pipeline::start_with(module, |b| {
        b.ack_deadline(Duration::from_millis(50));
    })
    .await;
    p.run(inbound(&interaction(slash("slow", json!([])), json!({}))))
        .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        requests[0],
        (Method::POST, CB.to_owned(), json!({ "type": 5 }))
    );
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(requests[1].2["flags"], 32768);
    p.run(inbound(&interaction(slash("hidden", json!([])), json!({}))))
        .await;
    assert_eq!(
        p.requests()[2].2,
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    // A context menu is auto-deferred too.
    let user_data = json!({
        "id": "9", "name": "Show", "type": 2, "target_id": "11",
        "resolved": { "users": { "11": user("11") } }
    });
    p.run(inbound(&interaction(user_data, json!({})))).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 6, "{requests:?}");
    assert_eq!(
        requests[4],
        (Method::POST, CB.to_owned(), json!({ "type": 5 }))
    );
    assert_eq!(
        (&requests[5].0, requests[5].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert!(p.reports().is_empty());
    let (deferred, latencies) = metrics.read();
    assert_eq!((deferred, latencies.len()), (3, 3));
    assert!(
        latencies.iter().all(|l| (0.05..1.0).contains(l)),
        "{latencies:?}"
    );

    // Matrix 10: the deadline already passed while queued (default 2 s).
    let p = Pipeline::start(Module::new("m").__add(sleeper(
        "m::slow",
        &["slow"],
        Duration::from_millis(300),
    )))
    .await;
    let started = Instant::now();
    let item = inbound_ago(
        &interaction(slash("slow", json!([])), json!({})),
        Duration::from_millis(2500),
    );
    p.run(item).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    let requests = p.requests();
    assert_eq!(requests[0].2, json!({ "type": 5 }));
    assert_eq!(requests[1].0, Method::PATCH);
    assert_eq!(metrics.read().0, 4);
}

/// INT-02: `run_interaction` hands the interaction's channel to the
/// transport, so after 15 minutes the original (id 77 from the
/// `with_response` callback, which names no channel) is edited in it with
/// the bot token.
#[tokio::test]
async fn int_02_fallback_uses_the_interaction_channel() {
    let base = discord();
    let answer: Answer = Arc::new(move |seen: &crate::edge::test_support::Seen| {
        if seen.path.ends_with("/callback") && seen.query.as_deref() == Some("with_response=true") {
            let body = br#"{"interaction":{"id":"100","type":2},"resource":{"type":4,"message":{"id":"77"}}}"#;
            (hyper::StatusCode::OK, bytes::Bytes::from_static(body))
        } else {
            base(seen)
        }
    });
    let module = Module::new("m").__add(CommandHandler::__new_plain(
        "m::later",
        &["later"],
        "d",
        |_: CommandCtx, r: CommandResponder| async move {
            let acked = r.reply(v2("a")).await?;
            skip(Duration::from_mins(15)).await;
            acked.edit_reply(v2("b")).await?;
            Ok::<_, HandlerError>(acked.done())
        },
    ));
    let p = Pipeline::start_answering(module, answer, |_| {}).await;
    p.run(inbound(&interaction(
        slash("later", json!([])),
        json!({ "channel": { "id": "12", "type": 0 } }),
    )))
    .await;
    assert!(p.reports().is_empty(), "{:?}", p.reports());
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::PATCH, "/api/v10/channels/12/messages/77")
    );
}

/// Story 6.6 Matrix 1, 8, 9 and 40: a reply before the deadline, an
/// explicit defer before it and an explicit defer after the auto-defer.
#[tokio::test]
async fn int_01_handler_answers_first() {
    let metrics = Metrics::new();
    let _metrics = metrics::set_default_local_recorder(&metrics.recorder);
    let module = Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::fast",
            &["fast"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(120)).await;
                Ok::<_, HandlerError>(r.reply(v2("fast")).await?.done())
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::early",
            &["early"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                let _ = r.defer_reply_as(Ephemerality::Public).await?.done();
                tokio::time::sleep(Duration::from_millis(700)).await;
                Ok(v2("done"))
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::after",
            &["after"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(700)).await;
                Ok::<_, HandlerError>(r.defer_reply_as(Ephemerality::Ephemeral).await?.done())
            },
        ));
    let p = Pipeline::start_with(module, |b| {
        b.ack_deadline(Duration::from_millis(400));
    })
    .await;
    // Matrix 1 and 40.
    p.run(inbound(&interaction(slash("fast", json!([])), json!({}))))
        .await;
    tokio::time::sleep(Duration::from_millis(450)).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].2["type"], 4);
    let (deferred, latencies) = metrics.read();
    assert_eq!(deferred, 0);
    assert_eq!(latencies.len(), 1);
    assert!((0.12..0.4).contains(&latencies[0]), "{latencies:?}");
    // Matrix 8.
    p.run(inbound(&interaction(slash("early", json!([])), json!({}))))
        .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert_eq!(requests[1].2, json!({ "type": 5 }));
    assert_eq!(requests[2].0, Method::PATCH);
    assert_eq!(metrics.read().0, 0);
    // Matrix 9: the late defer is a no-op.
    p.run(inbound(&interaction(slash("after", json!([])), json!({}))))
        .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_eq!(requests[3].2, json!({ "type": 5 }));
    let (deferred, latencies) = metrics.read();
    assert_eq!((deferred, latencies.len()), (1, 3));
    assert!(p.reports().is_empty());
}

/// INT-03 (Story 6.6 Matrix 4, 6 and 7): after the auto-defer a `reply` or
/// a returned legacy message edits the original, and at most one callback
/// is ever sent.
#[tokio::test]
async fn int_03_late_reply_becomes_edit_reply() {
    let module = Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::late",
            &["late"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok::<_, HandlerError>(r.reply(v2("late")).await?.done())
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::legacy",
            &["legacy"],
            "d",
            |_: CommandCtx, _: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(noctis_ui::LegacyMessage::new("x"))
            },
        ));
    let p = Pipeline::start_with(module, |b| {
        b.ack_deadline(Duration::from_millis(50));
    })
    .await;
    p.run(inbound(&interaction(slash("late", json!([])), json!({}))))
        .await;
    p.run(inbound(&interaction(slash("legacy", json!([])), json!({}))))
        .await;
    let requests = p.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_eq!(requests[0].2, json!({ "type": 5 }));
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(requests[1].2["flags"], 32768);
    assert_eq!(requests[1].2["components"][0]["content"], "late");
    assert_eq!(requests[2].2, json!({ "type": 5 }));
    assert_eq!(requests[3].0, Method::PATCH);
    assert_eq!(requests[3].2["content"], "x");
    assert!(requests[3].2.get("embeds").is_none(), "{:?}", requests[3].2);
    assert!(p.reports().is_empty(), "{:?}", p.reports());

    // Matrix 7: the reply and the timer are both ready at once.
    let race = CommandHandler::__new_plain(
        "m::race",
        &["race"],
        "d",
        |_: CommandCtx, r: CommandResponder| async move {
            Ok::<_, HandlerError>(r.reply(v2("now")).await?.done())
        },
    );
    let p = Pipeline::start_with(Module::new("m").__add(race), |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    for _ in 0..5 {
        p.fx.server.log.lock().unwrap().clear();
        p.run(inbound(&interaction(slash("race", json!([])), json!({}))))
            .await;
        let requests = p.requests();
        let callbacks = requests.iter().filter(|r| r.1.starts_with(CB)).count();
        assert_eq!(callbacks, 1, "{requests:?}");
        if requests[0].2["type"] == 5 {
            assert_eq!(requests.len(), 2, "{requests:?}");
            assert_eq!(requests[1].0, Method::PATCH);
        } else {
            assert_eq!(requests.len(), 1, "{requests:?}");
        }
    }
    assert!(p.reports().is_empty(), "{:?}", p.reports());
}

/// Story 6.6 Matrix 5 and the Late delivery AC: a handler answering 5 s
/// (virtual) after receipt edits the original; no second callback.
#[tokio::test]
async fn returned_message_five_seconds_late_edits_original() {
    let go = Arc::new(Notify::new());
    let handler = {
        let go = Arc::clone(&go);
        CommandHandler::__new_plain(
            "m::slow",
            &["slow"],
            "d",
            move |_: CommandCtx, _: CommandResponder| {
                let go = Arc::clone(&go);
                async move {
                    go.notified().await;
                    Ok(v2("late"))
                }
            },
        )
    };
    let p = Arc::new(
        Pipeline::start_with(Module::new("m").__add(handler), |b| {
            b.ack_deadline(Duration::from_millis(50));
        })
        .await,
    );
    let received_at = Instant::now();
    let run = tokio::spawn({
        let p = Arc::clone(&p);
        async move {
            p.run(inbound(&interaction(slash("slow", json!([])), json!({}))))
                .await;
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while p.requests().is_empty() {
        assert!(std::time::Instant::now() < deadline, "no auto-defer");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(5)).await;
    tokio::time::resume();
    assert!(received_at.elapsed() >= Duration::from_secs(5));
    go.notify_one();
    tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap();
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].2, json!({ "type": 5 }));
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(requests[1].2["flags"], 32768);
}

/// Story 6.6 Matrix 11: a failed auto-defer is logged and changes
/// nothing; the handler's own reply gets the typed error.
#[tokio::test]
async fn auto_defer_failure_is_logged() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let metrics = Metrics::new();
    let _metrics = metrics::set_default_local_recorder(&metrics.recorder);
    let handler = CommandHandler::__new_plain(
        "m::slow",
        &["slow"],
        "d",
        |_: CommandCtx, r: CommandResponder| async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok::<_, HandlerError>(r.reply(v2("late")).await?.done())
        },
    );
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    p.run(inbound_ago(
        &interaction(slash("slow", json!([])), json!({})),
        Duration::from_millis(3500),
    ))
    .await;
    assert!(p.requests().is_empty());
    let lines = capture.lines_with(&["noctis::dispatch", "auto-defer failed", "m::slow"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(!lines[0].contains("tok"), "{}", lines[0]);
    let reports = p.reports();
    assert_eq!(reports.len(), 1);
    assert!(
        reports[0].kind.contains("not acknowledged in time"),
        "{}",
        reports[0].kind
    );
    assert_eq!(metrics.read().0, 0);
}

/// Story 6.6 Matrix 12 and 13: the timer stops with the handler run (an
/// error, a panic, a shutdown cancel).
#[tokio::test]
async fn timer_stops_with_the_handler() {
    let started = Arc::new(Notify::new());
    let entered = Arc::clone(&started);
    let module = Module::new("m")
        .__add(CommandHandler::__new_plain(
            "m::fail",
            &["fail"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Err::<V2Message, _>(HandlerError::new(std::io::Error::other("boom")))
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::inner",
            &["inner"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                tokio::task::yield_now().await;
                Ok(boom())
            },
        ))
        .__add(CommandHandler::__new_plain(
            "m::stuck",
            &["stuck"],
            "d",
            move |_: CommandCtx, _: CommandResponder| {
                let entered = Arc::clone(&entered);
                async move {
                    entered.notify_one();
                    std::future::pending::<()>().await;
                    Ok(v2("never"))
                }
            },
        ));
    let p = Arc::new(
        Pipeline::start_with(module, |b| {
            b.ack_deadline(Duration::from_millis(100));
        })
        .await,
    );
    for name in ["fail", "inner"] {
        p.run(inbound(&interaction(slash(name, json!([])), json!({}))))
            .await;
    }
    let cancel = CancellationToken::new();
    let run = tokio::spawn({
        let p = Arc::clone(&p);
        let cancel = cancel.clone();
        async move {
            p.run_with(
                inbound(&interaction(slash("stuck", json!([])), json!({}))),
                cancel,
            )
            .await;
        }
    });
    started.notified().await;
    cancel.cancel();
    run.await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    // The error and the panic got their notices; no auto-defer fired and
    // the cancellation sent nothing.
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    for request in &requests {
        assert_notice(request, GENERIC);
    }
    let kinds: Vec<String> = p.reports().into_iter().map(|r| r.kind).collect();
    assert_eq!(kinds, ["error:boom", "panic:boom", "cancelled"]);
}

/// Story 6.6 Matrix 16: the Dispatcher arms no auto-defer.
#[tokio::test]
async fn dispatcher_has_no_auto_defer() {
    let p = Pipeline::start_with(
        Module::new("m").__add(sleeper("m::slow", &["slow"], Duration::from_millis(100))),
        |b| {
            b.ack_deadline(Duration::ZERO);
        },
    )
    .await;
    p.dispatcher()
        .run_command(CommandInvocation::new(
            CommandInput::new("slow"),
            Id::new(5),
            Id::new(6),
            Id::new(3),
        ))
        .await
        .unwrap();
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].1, "/api/v10/channels/5/messages");
}

/// INT-05 (Story 6.6 Matrix 18, Story 6.10 Matrix 23): an
/// `auto_defer = false` Command gets no auto-defer; a missed ack deadline
/// logs one error naming the handler and its module.
#[tokio::test]
async fn int_05_manual_never_auto_defers() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let handler = CommandHandler::__new_plain(
        "m::form",
        &["form"],
        "d",
        |_: CommandCtx, _: CommandResponder<crate::Manual>| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Err::<Responded, _>(HandlerError::new(std::io::Error::other("no modal")))
        },
    );
    assert!(!handler.auto_defer());
    assert!(sleeper("m::x", &["x"], Duration::ZERO).auto_defer());
    let p = Pipeline::start_with(Module::new("forms").__add(handler), |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    p.run(inbound(&interaction(slash("form", json!([])), json!({}))))
        .await;
    // No auto-defer: the only request is the error notice (Story 6.8).
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_notice(&requests[0], GENERIC);
    let missed = [
        "noctis::dispatch",
        "did not answer within the 3 s ack deadline",
    ];
    assert!(capture.lines_with(&missed).is_empty(), "{}", capture.text());
    p.run(inbound_ago(
        &interaction(slash("form", json!([])), json!({})),
        Duration::from_millis(2950),
    ))
    .await;
    let requests = p.requests();
    assert!(
        requests.iter().all(|r| r.2["type"] == 4),
        "no auto-defer: {requests:?}"
    );
    let lines = capture.lines_with(&missed);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(
        lines[0].contains("handler=\"m::form\"") && lines[0].contains("module=\"forms\""),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("ERROR"), "{}", lines[0]);
}

/// INT-11 (Story 6.6 Matrix 34-37 and 39): a user-install-only interaction
/// sends at most 5 follow-ups; a guild install or no owner has no cap; an
/// invalid follow-up uses no slot.
#[tokio::test]
async fn int_11_sixth_followup_capped() {
    let log = Log::default();
    let handler = {
        let log = log.clone();
        CommandHandler::__new_plain(
            "m::many",
            &["many"],
            "d",
            move |_: CommandCtx, r: CommandResponder| {
                let log = log.clone();
                async move {
                    let acked = r.defer_reply_as(Ephemerality::Public).await?;
                    let big = Components::new()
                        .push(ActionRow::buttons([
                            LinkButton::new("https://x.y").label("a".repeat(81))
                        ]));
                    let mut results = Vec::new();
                    let first = acked.follow_up(big, Visibility::Public).await;
                    results.push(first.map(drop));
                    for _ in 0..6 {
                        let sent = acked.follow_up(v2("f"), Visibility::Ephemeral).await;
                        results.push(sent.map(drop));
                    }
                    let line: Vec<&str> = results
                        .iter()
                        .map(|r| match r {
                            Ok(()) => "ok",
                            Err(ResponderError::Invalid(_)) => "invalid",
                            Err(ResponderError::FollowupCap) => "cap",
                            Err(_) => "other",
                        })
                        .collect();
                    log.push(line.join(","));
                    Ok::<_, HandlerError>(acked.done())
                }
            },
        )
    };
    let p = Pipeline::start(Module::new("m").__add(handler)).await;
    for owners in [
        json!({ "1": "3" }),
        json!({ "0": "4", "1": "3" }),
        json!({ "0": "4" }),
        json!({}),
    ] {
        p.fx.server.log.lock().unwrap().clear();
        p.run(inbound(&interaction(
            slash("many", json!([])),
            json!({ "authorizing_integration_owners": owners }),
        )))
        .await;
        let sent = p
            .requests()
            .iter()
            .filter(|r| r.1.starts_with("/api/v10/webhooks/7/tok?"))
            .count();
        let expected = if owners == json!({ "1": "3" }) { 5 } else { 6 };
        assert_eq!(sent, expected, "{owners}");
    }
    assert_eq!(
        log.lines(),
        [
            "invalid,ok,ok,ok,ok,ok,cap",
            "invalid,ok,ok,ok,ok,ok,ok",
            "invalid,ok,ok,ok,ok,ok,ok",
            "invalid,ok,ok,ok,ok,ok,ok",
        ]
    );
}

// ---- Story 6.7: autocomplete ----

mod autocomplete {
    use std::sync::atomic::{AtomicBool, Ordering};

    use bytes::Bytes;
    use hyper::StatusCode;

    use super::*;
    use crate::autocomplete::{__AutocompleteField, __AutocompleteMethod, PartialOptions};
    use crate::options::__options::partial;
    use crate::{__autocomplete_method, AutocompleteCtx, Suggestions};

    /// What the `value` handler saw: focused text and `partial.limit`.
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    /// Set when the `slow_deadline` handler started; only the INT-06
    /// deadline test focuses it.
    static SLOW_DEADLINE_STARTED: AtomicBool = AtomicBool::new(false);

    #[derive(Default)]
    struct AcPartial {
        limit: Option<i64>,
    }

    impl PartialOptions for AcPartial {
        type Options = AcOptions;

        fn __from_input(input: &CommandInput) -> Self {
            Self {
                limit: partial(input, "limit"),
            }
        }
    }

    /// The options of `config set prefix`; each focused option name picks
    /// a method behaviour.
    struct AcOptions;

    /// Every test method suggests `String`s for the field hashed `1`.
    impl __AutocompleteField<1> for AcOptions {
        type Value = String;
    }

    type AcAnswer = HandlerResult<Suggestions<String>>;

    fn method<F, Fut>(name: &'static str, f: F) -> __AutocompleteMethod
    where
        F: Fn(AutocompleteCtx, String, AcPartial) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AcAnswer> + Send + 'static,
    {
        __autocomplete_method::<AcOptions, 1, _, _, _>(name, f)
    }

    fn panics_at_call(_: AutocompleteCtx, _: String, _: AcPartial) -> std::future::Ready<AcAnswer> {
        panic!("at call")
    }

    async fn panics_inside(_: AutocompleteCtx, _: String, _: AcPartial) -> AcAnswer {
        panic!("inside")
    }

    impl Options for AcOptions {
        const OPTIONS: &'static [OptionDecl<'static>] = &[
            OptionDecl::new("value", OptionType::String, "d")
                .required(true)
                .autocomplete(true),
            OptionDecl::new("limit", OptionType::Integer, "d").required(true),
        ];
        type Partial = AcPartial;

        fn from_input(_: &CommandInput) -> Result<Self, OptionError> {
            Ok(Self)
        }

        fn __autocomplete(option: &str) -> Option<__AutocompleteMethod> {
            Some(match option {
                "value" => method("m::value", |_, typed, p| async move {
                    SEEN.lock().unwrap().push(format!("{typed}:{:?}", p.limit));
                    Ok(["a", "b", "c"]
                        .map(|v| (v.to_owned(), v))
                        .into_iter()
                        .collect())
                }),
                "many" => method("m::many", |_, _, _| async move {
                    Ok((0..30).map(|i| (i.to_string(), format!("c{i}"))).collect())
                }),
                "labels" => method("m::labels", |_, _, _| async move {
                    let mut s = Suggestions::new();
                    s.push("k".to_owned(), keys::GUARD_OWNER);
                    s.push("long".to_owned(), "a".repeat(101));
                    s.push("empty".to_owned(), "");
                    s.push("v".repeat(101), "too long a value");
                    Ok(s)
                }),
                "err" => method("m::err", |_, _, _| async {
                    Err(HandlerError::new(std::io::Error::other("boom")))
                }),
                "guild" => method("m::guild", |ctx, _, _| async move {
                    let guild = ctx.guild_id()?;
                    Ok([(guild.to_string(), "g")].into_iter().collect())
                }),
                "state" => method("m::state", |ctx, _, _| async move {
                    let _ = ctx.state::<std::net::Ipv4Addr>();
                    Ok(Suggestions::new())
                }),
                "panic_call" => method("m::panic_call", panics_at_call),
                "panic_inside" => method("m::panic_inside", panics_inside),
                "slow" => method("m::slow", |_, _, _| async move {
                    tokio::time::sleep(Duration::from_mins(1)).await;
                    Ok(Suggestions::new())
                }),
                "slow_deadline" => method("m::slow_deadline", |_, _, _| async move {
                    SLOW_DEADLINE_STARTED.store(true, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_mins(1)).await;
                    Ok(Suggestions::new())
                }),
                "empty" => method("m::empty", |_, _, _| async { Ok(Suggestions::new()) }),
                _ => return None,
            })
        }
    }

    pub(super) fn config() -> CommandHandler {
        CommandHandler::__new_plain_with_options(
            "m::config",
            &["config set prefix"],
            "d",
            |_: CommandCtx, _: CommandResponder, _: AcOptions| async { Ok(v2("ok")) },
        )
    }

    /// The App with `config` and a recording Autocomplete sink.
    async fn start(answer: Answer) -> (Pipeline, Arc<Recording>) {
        let sink = Arc::new(Recording::default());
        let module =
            Module::new("m")
                .__add(config())
                .__add(logging(&Log::default(), "m::ping", &["ping"]));
        let ac_sink = Arc::clone(&sink);
        let p = Pipeline::start_answering(module, answer, move |b| {
            b.error_sink(ErrorOrigin::Autocomplete, ac_sink);
        })
        .await;
        (p, sink)
    }

    pub(super) async fn run(p: &Pipeline, inbound: Inbound) {
        run_with(p, inbound, CancellationToken::new()).await;
    }

    async fn run_with(p: &Pipeline, inbound: Inbound, cancel: CancellationToken) {
        run_autocomplete(
            Arc::clone(p.fx.app.registry()),
            Arc::clone(p.fx.app.shared()),
            inbound,
            cancel,
        )
        .await;
    }

    /// A type-4 interaction of `config set prefix` focusing `focused`
    /// (typed `text`), with `limit` = 3 unless `focused` is `limit`.
    fn ac_json(focused: &str, text: &str) -> Value {
        let mut leaves =
            vec![json!({ "type": 3, "name": focused, "value": text, "focused": true })];
        if focused != "limit" {
            leaves.push(json!({ "type": 4, "name": "limit", "value": 3 }));
        }
        let data = json!({
            "id": "9", "name": "config", "type": 1,
            "options": [{ "type": 2, "name": "set", "options": [
                { "type": 1, "name": "prefix", "options": leaves }
            ]}]
        });
        interaction(data, json!({ "type": 4 }))
    }

    pub(super) fn ac(focused: &str, text: &str) -> Inbound {
        inbound(&ac_json(focused, text))
    }

    fn choices(p: &Pipeline) -> Vec<Value> {
        let requests = p.requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(
            (&requests[0].0, requests[0].1.as_str()),
            (&Method::POST, CB)
        );
        assert_eq!(requests[0].2["type"], 8);
        requests[0].2["data"]["choices"].as_array().unwrap().clone()
    }

    fn rejected() -> Answer {
        Arc::new(|_: &crate::edge::test_support::Seen| {
            (
                StatusCode::BAD_REQUEST,
                Bytes::from_static(br#"{"code":50035}"#),
            )
        })
    }

    /// Matrix 1 and 2: a 3-word path, the partial input and one type-8
    /// callback with the handler's choices, in order.
    #[tokio::test]
    async fn three_word_path_answers_choices() {
        let (p, sink) = start(discord()).await;
        run(&p, ac("value", "pr-1")).await;
        assert_eq!(
            choices(&p),
            [
                json!({ "name": "a", "value": "a" }),
                json!({ "name": "b", "value": "b" }),
                json!({ "name": "c", "value": "c" })
            ]
        );
        assert!(SEEN.lock().unwrap().contains(&"pr-1:Some(3)".to_owned()));
        assert!(sink.seen().is_empty());
        assert!(p.reports().is_empty());
    }

    /// INT-07 end to end: 30 choices, 25 sent, one warning.
    #[tokio::test]
    async fn int_07_thirty_choices_send_twenty_five() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        let (p, _sink) = start(discord()).await;
        run(&p, ac("many", "")).await;
        assert_eq!(choices(&p).len(), 25);
        let lines = capture.lines_with(&["noctis::autocomplete", "INT-07"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("returned=30"), "{}", lines[0]);
        assert!(lines[0].contains("sent=25"), "{}", lines[0]);
        assert!(lines[0].contains("handler=\"m::many\""), "{}", lines[0]);
    }

    /// Matrix 15 and 16: an error or a panic answers an empty list and
    /// reports to the Autocomplete sink.
    #[tokio::test]
    async fn failures_answer_empty_lists() {
        use metrics_util::debugging::{DebugValue, DebuggingRecorder};
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _metrics = metrics::set_default_local_recorder(&recorder);
        for (option, kind) in [
            ("err", "boom"),
            ("panic_call", "at call"),
            ("panic_inside", "inside"),
        ] {
            let (p, sink) = start(discord()).await;
            run(&p, ac(option, "x")).await;
            assert!(choices(&p).is_empty(), "{option}");
            let seen = sink.seen();
            assert_eq!(seen.len(), 1, "{option}");
            assert_eq!(seen[0].origin, ErrorOrigin::Autocomplete);
            assert!(seen[0].kind.contains(kind), "{}", seen[0].kind);
            assert_eq!(seen[0].module.as_deref(), Some("m"));
            assert_eq!(seen[0].handler.as_deref(), Some(&*format!("m::{option}")));
            assert!(p.reports().is_empty());
        }
        let snap = snapshotter.snapshot().into_vec();
        let errors = snap
            .iter()
            .find(|(k, ..)| {
                k.key().name() == "noctis_handler_errors_total"
                    && k.key()
                        .labels()
                        .any(|l| l.key() == "kind" && l.value() == "autocomplete")
            })
            .map(|(.., v)| v);
        assert!(matches!(errors, Some(DebugValue::Counter(3))), "{snap:?}");
    }

    /// INT-06 (Matrix 17): a handler still running at the 2.5 s deadline is
    /// dropped and an empty list is sent right after it.
    #[tokio::test]
    async fn int_06_deadline_missed_sends_empty_choices() {
        let (p, sink) = start(discord()).await;
        let p = Arc::new(p);
        let item = ac("slow_deadline", "x");
        let received_at = item.received_at;
        let run = tokio::spawn({
            let p = Arc::clone(&p);
            async move { run(&p, item).await }
        });
        let started = std::time::Instant::now();
        while !SLOW_DEADLINE_STARTED.load(Ordering::SeqCst) {
            assert!(started.elapsed() < Duration::from_secs(10), "never started");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        skip(Duration::from_millis(2550)).await;
        tokio::time::timeout(Duration::from_secs(10), run)
            .await
            .unwrap()
            .unwrap();
        assert!(received_at.elapsed() < Duration::from_millis(2900));
        assert!(choices(&p).is_empty());
        let seen = sink.seen();
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0].kind.contains("did not respond within 2.5s"),
            "{}",
            seen[0].kind
        );
    }

    /// INT-06 (Matrix 18): the deadline passed before the stage started:
    /// the handler never runs; an edge refusal is only a warning.
    #[tokio::test]
    async fn int_06_already_late_never_runs() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        let (p, sink) = start(discord()).await;
        let mut item = ac("value", "late-18");
        item.received_at = Instant::now()
            .checked_sub(Duration::from_millis(2600))
            .unwrap();
        run(&p, item).await;
        assert!(choices(&p).is_empty());
        assert!(
            !SEEN
                .lock()
                .unwrap()
                .iter()
                .any(|s| s.starts_with("late-18"))
        );
        assert_eq!(sink.seen().len(), 1);
        assert!(sink.seen()[0].kind.contains("did not respond"));

        let (p, sink) = start(discord()).await;
        let mut item = ac("value", "later-18");
        item.received_at = Instant::now()
            .checked_sub(Duration::from_millis(3500))
            .unwrap();
        run(&p, item).await;
        assert!(p.requests().is_empty());
        assert_eq!(sink.seen().len(), 1);
        let lines = capture.lines_with(&["noctis::autocomplete", "autocomplete callback failed"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(!lines[0].contains("tok"), "{}", lines[0]);
    }

    /// Matrix 19: a shutdown cancel answers an empty list.
    #[tokio::test]
    async fn shutdown_cancels() {
        let (p, sink) = start(discord()).await;
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            trigger.cancel();
        });
        run_with(&p, ac("slow", "x"), cancel).await;
        assert!(choices(&p).is_empty());
        let seen = sink.seen();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].kind.contains("cancel"), "{}", seen[0].kind);
    }

    /// Story 6.11 Matrix 24: no suggestions send an empty list, without a
    /// report.
    #[tokio::test]
    async fn empty_suggestions_send_an_empty_list() {
        let (p, sink) = start(discord()).await;
        run(&p, ac("empty", "x")).await;
        assert!(choices(&p).is_empty());
        assert!(sink.seen().is_empty());
    }

    /// Story 6.11 Matrix 25 and 26: a `MessageKey` label renders in the
    /// user's locale, a long label is cut with a warning, an empty label
    /// and a too-long value are dropped with one warning.
    #[tokio::test]
    async fn labels_are_lowered() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let (p, sink) = start(discord()).await;
        run(&p, ac("labels", "x")).await;
        let sent = choices(&p);
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(
            sent[0],
            json!({ "name": "Only the person who started this can use it.", "value": "k" })
        );
        let cut = sent[1]["name"].as_str().unwrap();
        assert_eq!(noctis_rules::len::discord_len(cut), 100);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(sink.seen().is_empty());
        assert_eq!(
            capture
                .lines_with(&["noctis::autocomplete", "OPT-09", "cut=1"])
                .len(),
            1,
            "{}",
            capture.text()
        );
        assert_eq!(
            capture
                .lines_with(&["noctis::autocomplete", "dropped=2"])
                .len(),
            1,
            "{}",
            capture.text()
        );
    }

    /// Story 6.11 Matrix 28: `guild_id()?` in a DM and an unregistered
    /// state answer an empty list and reach the sink.
    #[tokio::test]
    async fn guild_and_state_failures() {
        for (option, text) in [("guild", "not in a guild"), ("state", "ip_addr::Ipv4Addr")] {
            let (p, sink) = start(discord()).await;
            run(&p, ac(option, "x")).await;
            assert!(choices(&p).is_empty(), "{option}");
            let seen = sink.seen();
            assert_eq!(seen.len(), 1, "{option}");
            assert!(seen[0].kind.contains(text), "{option}: {}", seen[0].kind);
        }
    }

    /// Matrix 21 and 22: a rejected callback is reported once, never
    /// retried; after a handler failure it is only a warning.
    #[tokio::test]
    async fn rejected_callbacks() {
        let (p, sink) = start(rejected()).await;
        run(&p, ac("value", "x")).await;
        assert_eq!(choices(&p).len(), 3);
        let seen = sink.seen();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].kind.contains("400"), "{}", seen[0].kind);

        let (p, sink) = start(rejected()).await;
        run(&p, ac("err", "x")).await;
        assert_eq!(p.requests().len(), 1);
        let seen = sink.seen();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].kind.contains("boom"), "{}", seen[0].kind);
    }

    /// Matrix 23-26: unknown Commands and options warn without a report;
    /// malformed focus is reported.
    #[tokio::test]
    async fn unknown_and_malformed_answer_empty_lists() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        let (p, sink) = start(discord()).await;
        let unknown = interaction(slash("nope", json!([])), json!({ "type": 4 }));
        run(&p, inbound(&unknown)).await;
        assert!(choices(&p).is_empty());
        assert_eq!(
            capture
                .lines_with(&["noctis::dispatch", "unknown command"])
                .len(),
            1
        );
        p.fx.server.log.lock().unwrap().clear();
        run(&p, ac("nohandler", "x")).await;
        assert!(choices(&p).is_empty());
        assert_eq!(
            capture
                .lines_with(&["noctis::dispatch", "no autocomplete handler"])
                .len(),
            1
        );
        assert!(sink.seen().is_empty());

        // Matrix 25: two focused options.
        p.fx.server.log.lock().unwrap().clear();
        let mut two = ac_json("value", "a");
        let leaves = &mut two["data"]["options"][0]["options"][0]["options"];
        leaves
            .as_array_mut()
            .unwrap()
            .push(json!({ "type": 3, "name": "other", "value": "b", "focused": true }));
        run(&p, inbound(&two)).await;
        assert!(choices(&p).is_empty());
        let seen = sink.seen();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].kind.contains("focused"), "{}", seen[0].kind);
    }

    /// FR-22: a failing autocomplete handler is logged exactly once, with a
    /// custom Autocomplete sink and with the default one.
    #[tokio::test]
    async fn the_error_is_logged_once() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let (p, sink) = start(discord()).await;
        run(&p, ac("err", "x")).await;
        assert_eq!(sink.seen().len(), 1);
        let lines = capture.lines_with(&["ERROR", "noctis::autocomplete", "boom"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());

        let default = Pipeline::start_default_sink(Module::new("m").__add(config())).await;
        run(&default, ac("err", "x")).await;
        let lines = capture.lines_with(&["ERROR", "noctis::autocomplete", "boom"]);
        assert_eq!(lines.len(), 2, "{}", capture.text());
    }

    /// Matrix 35: the callback records the ack latency.
    #[tokio::test]
    async fn ack_latency_is_recorded() {
        let metrics = Metrics::new();
        let _metrics = metrics::set_default_local_recorder(&metrics.recorder);
        let (p, _sink) = start(discord()).await;
        let mut item = ac("value", "x");
        item.received_at = Instant::now()
            .checked_sub(Duration::from_millis(120))
            .unwrap();
        run(&p, item).await;
        let (_, latencies) = metrics.read();
        assert_eq!(latencies.len(), 1);
        assert!((0.12..0.6).contains(&latencies[0]), "{latencies:?}");
    }
}

// ---- Story 6.8: user-facing notices ----

mod notices {
    use super::*;
    use crate::InvalidOption;
    use noctis_i18n::keys;

    /// The follow-up route (`wait=true`).
    const FOLLOWUP: &str = "/api/v10/webhooks/7/tok?wait=true";
    const OWNER: &str = "Only the person who started this can use it.";
    const DISABLED: &str = "This feature is disabled here.";

    /// A Command at `path` failing with `error()`.
    fn failing(
        name: &'static str,
        paths: &'static [&'static str],
        error: fn() -> HandlerError,
    ) -> CommandHandler {
        CommandHandler::__new_plain(
            name,
            paths,
            "d",
            move |_: CommandCtx, _: CommandResponder| async move { Err::<V2Message, _>(error()) },
        )
    }

    fn io(message: &str) -> HandlerError {
        HandlerError::new(std::io::Error::other(message.to_owned()))
    }

    fn run_slash(name: &str) -> Inbound {
        inbound(&interaction(slash(name, json!([])), json!({})))
    }

    /// The `noctis_handler_errors_total` count in `snapshotter`.
    fn errors_total(snapshotter: &metrics_util::debugging::Snapshotter) -> u64 {
        use metrics_util::debugging::DebugValue;
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(k, ..)| k.key().name() == "noctis_handler_errors_total")
            .map(|(.., v)| match v {
                DebugValue::Counter(n) => n,
                _ => 0,
            })
            .sum()
    }

    /// Matrix 1-4: no key, `with_message`, a user condition and an unknown
    /// bot key.
    #[tokio::test]
    async fn notice_keys_and_user_conditions() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _metrics = metrics::set_default_local_recorder(&recorder);
        let module = Module::new("m")
            .__add(failing("m::plain", &["plain"], || io("boom")))
            .__add(failing("m::keyed", &["keyed"], || {
                io("db down").with_message(keys::GUARD_OWNER)
            }))
            .__add(failing("m::user", &["user"], || {
                HandlerError::user(keys::DISABLED_HERE)
            }))
            .__add(failing("m::bot", &["bot"], || {
                HandlerError::user(MessageKey::new("shop.missing"))
            }))
            .__add(failing("m::rekeyed", &["rekeyed"], || {
                HandlerError::user(keys::DISABLED_HERE).with_message(keys::GUARD_OWNER)
            }));
        let p = Pipeline::start(module).await;
        p.run(run_slash("plain")).await;
        assert_eq!(errors_total(&snapshotter), 1);
        p.run(run_slash("keyed")).await;
        assert_eq!(errors_total(&snapshotter), 1);
        p.run(run_slash("user")).await;
        p.run(run_slash("bot")).await;
        p.run(run_slash("rekeyed")).await;
        assert_eq!(
            errors_total(&snapshotter),
            0,
            "user conditions are not counted"
        );
        let requests = p.requests();
        assert_eq!(requests.len(), 5, "{requests:?}");
        assert_notice(&requests[0], GENERIC);
        assert_notice(&requests[1], OWNER);
        assert_notice(&requests[2], DISABLED);
        assert_notice(&requests[3], GENERIC);
        assert_notice(&requests[4], OWNER);
        let kinds: Vec<String> = p.reports().into_iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ["error:boom", "error:db down"]);
        let debug = capture.lines_with(&["DEBUG", "noctis::command", "user condition"]);
        assert_eq!(debug.len(), 3, "{}", capture.text());
        assert!(
            debug[0].contains("handler=\"m::user\"")
                && debug[0].contains("key=\"noctis.disabled_here\""),
            "{}",
            debug[0]
        );
        let missing = capture.lines_with(&["WARN", "noctis::i18n", "key=\"shop.missing\""]);
        assert_eq!(missing.len(), 1, "{}", capture.text());
        // No notice text or token in the logs (AD-29).
        assert!(!capture.text().contains(DISABLED), "{}", capture.text());
        assert!(!capture.text().contains("tok/"), "{}", capture.text());
    }

    /// Options whose validator fails with a reserved key (`days` 1) or a
    /// bot key (`days` 2).
    struct Checked;

    impl Options for Checked {
        const OPTIONS: &'static [OptionDecl<'static>] =
            &[OptionDecl::new("days", OptionType::Integer, "d").required(true)];
        type Partial = crate::autocomplete::NoPartial<Self>;

        fn from_input(input: &CommandInput) -> Result<Self, OptionError> {
            let days: i64 = required(input, "days")?;
            CHECKED_DAYS.with(|d| d.set(days));
            Ok(Self)
        }

        fn validate(&self, _: &CommandCtx) -> Result<(), InvalidOption> {
            match CHECKED_DAYS.with(std::cell::Cell::get) {
                1 => Err(InvalidOption::__new("days", keys::VALIDATION_FAILED)),
                2 => Err(InvalidOption::__new(
                    "days",
                    MessageKey::new("shop.bad_days"),
                )),
                _ => Ok(()),
            }
        }
    }

    thread_local! {
        static CHECKED_DAYS: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    }

    /// Matrix 7: a failed validator shows its key's notice, the handler
    /// does not run, no sink sees it.
    #[tokio::test]
    async fn validator_failure_is_a_user_condition() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let log = Log::default();
        let ran = log.clone();
        let module = Module::new("m").__add(CommandHandler::__new_plain_with_options(
            "m::check",
            &["check"],
            "d",
            move |_: CommandCtx, _: CommandResponder, _: Checked| {
                ran.push("ran");
                async { Ok(v2("ok")) }
            },
        ));
        let p = Pipeline::start(module).await;
        for days in [1, 2] {
            p.run(inbound(&interaction(
                slash(
                    "check",
                    json!([{ "name": "days", "type": 4, "value": days }]),
                ),
                json!({}),
            )))
            .await;
        }
        assert!(log.lines().is_empty());
        assert!(p.reports().is_empty());
        let requests = p.requests();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_notice(&requests[0], VALIDATION);
        assert_notice(&requests[1], GENERIC);
        assert_eq!(
            capture
                .lines_with(&["noctis::i18n", "key=\"shop.bad_days\""])
                .len(),
            1
        );
        assert_eq!(
            capture
                .lines_with(&[
                    "DEBUG",
                    "noctis::command",
                    "key=\"noctis.validation.failed\""
                ])
                .len(),
            1,
            "{}",
            capture.text()
        );
    }

    /// Matrix 9 (AD-23): after the auto-defer, the deferred original is
    /// deleted, then the notice is an ephemeral follow-up.
    #[tokio::test]
    async fn notice_after_auto_defer_deletes_original_then_follows_up() {
        let go = Arc::new(Notify::new());
        let handler = {
            let go = Arc::clone(&go);
            CommandHandler::__new_plain(
                "m::slow",
                &["slow"],
                "d",
                move |_: CommandCtx, _: CommandResponder| {
                    let go = Arc::clone(&go);
                    async move {
                        go.notified().await;
                        Err::<V2Message, _>(io("late boom"))
                    }
                },
            )
        };
        let p = Arc::new(
            Pipeline::start_with(Module::new("m").__add(handler), |b| {
                b.ack_deadline(Duration::from_millis(20));
            })
            .await,
        );
        let run = tokio::spawn({
            let p = Arc::clone(&p);
            async move { p.run(run_slash("slow")).await }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while p.requests().is_empty() {
            assert!(std::time::Instant::now() < deadline, "no auto-defer");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        go.notify_one();
        tokio::time::timeout(Duration::from_secs(10), run)
            .await
            .unwrap()
            .unwrap();
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_eq!(
            requests[0],
            (Method::POST, CB.to_owned(), json!({ "type": 5 }))
        );
        assert_eq!(
            (&requests[1].0, requests[1].1.as_str()),
            (&Method::DELETE, ORIGINAL)
        );
        assert_eq!(requests[2].1, FOLLOWUP);
        assert_followup_notice(&requests[2], GENERIC);
        assert_eq!(p.reports()[0].kind, "error:late boom");
    }

    /// Matrix 10, 12 and 14-16: explicit defer, after a reply, after a
    /// modal-free reply cap, and an expired token.
    #[tokio::test]
    async fn notice_after_explicit_acks() {
        let module = Module::new("m")
            .__add(CommandHandler::__new_plain(
                "m::deferred",
                &["deferred"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let _ = r.defer_reply_as(Ephemerality::Public).await?.done();
                    Err::<V2Message, _>(io("x"))
                },
            ))
            .__add(CommandHandler::__new_plain(
                "m::replied",
                &["replied"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let _ = r.reply(v2("a")).await?.done();
                    Err::<V2Message, _>(io("x"))
                },
            ))
            .__add(CommandHandler::__new_plain(
                "m::filled",
                &["filled"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let acked = r.defer_reply_as(Ephemerality::Public).await?;
                    acked.edit_reply(v2("a")).await?;
                    let _ = acked.done();
                    Err::<V2Message, _>(io("x"))
                },
            ));
        let p = Pipeline::start(module).await;
        p.run(run_slash("deferred")).await;
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_eq!(requests[0].2, json!({ "type": 5 }));
        assert_eq!(
            (&requests[1].0, requests[1].1.as_str()),
            (&Method::DELETE, ORIGINAL)
        );
        assert_followup_notice(&requests[2], GENERIC);

        p.fx.server.log.lock().unwrap().clear();
        p.run(run_slash("replied")).await;
        let requests = p.requests();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(requests[0].1, CB_REPLY);
        assert_followup_notice(&requests[1], GENERIC);

        // A deferred original filled by `edit_reply` is kept.
        p.fx.server.log.lock().unwrap().clear();
        p.run(run_slash("filled")).await;
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_eq!(requests[0].2, json!({ "type": 5 }));
        assert_eq!(requests[1].0, Method::PATCH, "{requests:?}");
        assert!(
            requests.iter().all(|r| r.0 != Method::DELETE),
            "{requests:?}"
        );
        assert_followup_notice(&requests[2], GENERIC);
        assert_eq!(p.reports().len(), 3);
    }

    /// Matrix 11 and 17: a failed delete still sends the follow-up; a
    /// rejected notice is a warn, never retried, never a sink report.
    #[tokio::test]
    async fn notice_failures_are_warnings() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let base = discord();
        let answer: Answer = Arc::new(move |seen: &crate::edge::test_support::Seen| {
            if seen.method == Method::DELETE {
                (
                    hyper::StatusCode::NOT_FOUND,
                    bytes::Bytes::from_static(br#"{"code":10008,"message":"Unknown"}"#),
                )
            } else if seen.path.ends_with("/callback")
                && seen.query.is_none()
                && seen.body.ends_with(br#""type":4}"#)
            {
                // Matrix 17: the notice callback is rejected.
                let body = br#"{"code":50035,"message":"Invalid Form Body"}"#;
                (
                    hyper::StatusCode::BAD_REQUEST,
                    bytes::Bytes::from_static(body),
                )
            } else {
                base(seen)
            }
        });
        let module = Module::new("m")
            .__add(failing("m::fail", &["fail"], || io("boom")))
            .__add(CommandHandler::__new_plain(
                "m::deferred",
                &["deferred"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let _ = r.defer_reply_as(Ephemerality::Ephemeral).await?.done();
                    Err::<V2Message, _>(io("x"))
                },
            ));
        let p = Pipeline::start_answering(module, answer, |_| {}).await;
        p.run(run_slash("fail")).await;
        let requests = p.requests();
        assert_eq!(requests.len(), 1, "no retry: {requests:?}");
        let warns = capture.lines_with(&[
            "WARN",
            "noctis::dispatch",
            "could not send the error notice",
        ]);
        assert_eq!(warns.len(), 1, "{}", capture.text());
        assert!(warns[0].contains("handler=\"m::fail\""), "{}", warns[0]);

        p.fx.server.log.lock().unwrap().clear();
        p.run(run_slash("deferred")).await;
        let routes: Vec<_> = p.requests().into_iter().map(|(m, p, _)| (m, p)).collect();
        assert_eq!(
            routes,
            [
                (Method::POST, CB.to_owned()),
                (Method::DELETE, ORIGINAL.to_owned()),
                (Method::POST, FOLLOWUP.to_owned()),
            ]
        );
        assert_eq!(
            capture
                .lines_with(&["WARN", "could not delete the deferred original"])
                .len(),
            1
        );
        let kinds: Vec<String> = p.reports().into_iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ["error:boom", "error:x"]);
    }

    /// Matrix 15: the follow-up cap holds; Matrix 16: an expired token
    /// sends nothing. Both are a notice warn plus the handler's report.
    #[tokio::test]
    async fn notice_respects_cap_and_expiry() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let module = Module::new("m")
            .__add(CommandHandler::__new_plain(
                "m::many",
                &["many"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let acked = r.reply(v2("a")).await?;
                    for _ in 0..5 {
                        let _ = acked.follow_up(v2("f"), Visibility::Public).await?;
                    }
                    let _ = acked.done();
                    Err::<V2Message, _>(io("after five"))
                },
            ))
            .__add(CommandHandler::__new_plain(
                "m::old",
                &["old"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let _ = r.defer_reply_as(Ephemerality::Public).await?.done();
                    skip(Duration::from_mins(16)).await;
                    Err::<V2Message, _>(io("expired"))
                },
            ));
        let p = Pipeline::start(module).await;
        p.run(inbound(&interaction(
            slash("many", json!([])),
            json!({ "authorizing_integration_owners": { "1": "3" } }),
        )))
        .await;
        assert_eq!(p.requests().len(), 6, "{:?}", p.requests());

        p.fx.server.log.lock().unwrap().clear();
        p.run(run_slash("old")).await;
        let requests = p.requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(requests[0].2, json!({ "type": 5 }));

        let warns = capture.lines_with(&["WARN", "could not send the error notice"]);
        assert_eq!(warns.len(), 2, "{}", capture.text());
        let cap = ResponderError::FollowupCap.to_string();
        let expired = ResponderError::TokenExpired.to_string();
        assert!(warns[0].contains(&cap), "{}", warns[0]);
        assert!(warns[1].contains(&expired), "{}", warns[1]);
        let kinds: Vec<String> = p.reports().into_iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ["error:after five", "error:expired"]);
    }

    /// Matrix 18: no notice for a cancellation.
    #[tokio::test]
    async fn no_notice_for_a_cancellation() {
        let p = Pipeline::start(Module::new("m").__add(failing("m::f", &["f"], || io("x")))).await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        p.run_with(run_slash("f"), cancel).await;
        assert!(p.requests().is_empty());
        assert_eq!(p.reports()[0].kind, "cancelled");
    }

    /// Matrix 19 and 20: the Dispatcher's notice is a public reply, or a
    /// public channel message after a reply.
    #[tokio::test]
    async fn dispatcher_notices_are_public() {
        let module = Module::new("m")
            .__add(failing("m::fail", &["fail"], || io("boom")))
            .__add(CommandHandler::__new_plain(
                "m::replied",
                &["replied"],
                "d",
                |_: CommandCtx, r: CommandResponder| async move {
                    let _ = r.reply(v2("a")).await?.done();
                    Err::<V2Message, _>(io("x"))
                },
            ));
        let p = Pipeline::start(module).await;
        let invocation = |path| {
            CommandInvocation::new(CommandInput::new(path), Id::new(5), Id::new(6), Id::new(3))
                .locale("tr")
        };
        p.dispatcher()
            .run_command(invocation("fail"))
            .await
            .unwrap();
        p.dispatcher()
            .run_command(invocation("replied"))
            .await
            .unwrap();
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        for request in &requests {
            assert_eq!(
                (&request.0, request.1.as_str()),
                (&Method::POST, "/api/v10/channels/5/messages")
            );
            assert_eq!(request.2["flags"], 32768);
        }
        assert_eq!(
            requests[0].2["message_reference"],
            json!({ "message_id": "6", "fail_if_not_exists": false })
        );
        assert_eq!(requests[0].2["components"][0]["content"], GENERIC);
        assert_eq!(requests[2].2["components"][0]["content"], GENERIC);
        assert_eq!(p.reports().len(), 2);
    }

    /// Matrix 21 (LOC-04): every locale renders the en-US text today.
    #[tokio::test]
    async fn locales_fall_back_to_en_us() {
        let p = Pipeline::start(Module::new("m").__add(failing("m::f", &["f"], || io("x")))).await;
        for extra in [
            json!({ "locale": "tr", "guild_locale": "de", "guild_id": "4" }),
            json!({ "locale": "xx-YY" }),
            json!({}),
        ] {
            p.run(inbound(&interaction(slash("f", json!([])), extra)))
                .await;
        }
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        for request in &requests {
            assert_notice(request, GENERIC);
        }
    }

    /// Matrix 22 and 23: the full error is logged exactly once, with a
    /// custom sink and with the default one.
    #[tokio::test]
    async fn the_error_is_logged_once() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let custom = Pipeline::start(
            Module::new("m")
                .__add(failing("m::f", &["f"], || io("boom")))
                .__add(panicking("m::p", &["p"], "custom kaboom")),
        )
        .await;
        custom.run(run_slash("f")).await;
        assert_eq!(custom.reports().len(), 1);
        let lines = capture.lines_with(&["ERROR", "noctis::command", "command handler failed"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(
            lines[0].contains("noctis.interaction{") && lines[0].contains("boom"),
            "{}",
            lines[0]
        );
        custom.run(run_slash("p")).await;
        assert_eq!(custom.reports().len(), 2);
        let lines = capture.lines_with(&["ERROR", "noctis::command", "custom kaboom"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());

        let default = Pipeline::start_default_sink(
            Module::new("m")
                .__add(failing("m::g", &["g"], || io("bang")))
                .__add(panicking("m::q", &["q"], "default kaboom")),
        )
        .await;
        default.run(run_slash("g")).await;
        let lines = capture.lines_with(&["ERROR", "noctis::command", "bang"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        default.run(run_slash("q")).await;
        let lines = capture.lines_with(&["ERROR", "noctis::command", "default kaboom"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
    }

    /// A Command at `path` panicking with `message`.
    fn panicking(
        name: &'static str,
        paths: &'static [&'static str],
        message: &'static str,
    ) -> CommandHandler {
        CommandHandler::__new_plain(
            name,
            paths,
            "d",
            move |_: CommandCtx, _: CommandResponder| async move {
                tokio::task::yield_now().await;
                Ok::<V2Message, HandlerError>(panic_with(message))
            },
        )
    }

    fn panic_with(message: &str) -> V2Message {
        panic!("{message}")
    }
}

/// Story 6.10 Matrix 23: `auto_defer = false` handlers send no auto
/// callback, and `reply`/`defer_reply` use the declared response.
#[tokio::test]
async fn int_05_manual_uses_the_declared_response() {
    let reply = |name: &'static str, path: &'static [&'static str]| {
        CommandHandler::__new_plain(
            name,
            path,
            "d",
            |_: CommandCtx, r: CommandResponder<crate::Manual>| async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(r.reply(v2("a")).await?.done())
            },
        )
    };
    let deferred = |name: &'static str, path: &'static [&'static str]| {
        CommandHandler::__new_plain(
            name,
            path,
            "d",
            |_: CommandCtx, r: CommandResponder<crate::Manual>| async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let acked = r.defer_reply().await?;
                acked.edit_reply(v2("b")).await?;
                Ok(acked.done())
            },
        )
    };
    let module = Module::new("m")
        .__add(reply("m::pub_reply", &["pub_reply"]))
        .__add(reply("m::eph_reply", &["eph_reply"]).__with_ephemerality(Ephemerality::Ephemeral))
        .__add(deferred("m::pub_defer", &["pub_defer"]))
        .__add(
            deferred("m::eph_defer", &["eph_defer"]).__with_ephemerality(Ephemerality::Ephemeral),
        );
    let p = Pipeline::start_with(module, |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    for name in ["pub_reply", "eph_reply", "pub_defer", "eph_defer"] {
        p.run(inbound(&interaction(slash(name, json!([])), json!({}))))
            .await;
    }
    let requests = p.requests();
    assert_eq!(requests.len(), 6, "{requests:?}");
    assert_eq!(
        (requests[0].1.as_str(), &requests[0].2["type"]),
        (CB_REPLY, &json!(4))
    );
    assert_eq!(requests[0].2["data"]["flags"], 32768);
    assert_eq!(
        (requests[1].1.as_str(), &requests[1].2["type"]),
        (CB_REPLY, &json!(4))
    );
    assert_eq!(requests[1].2["data"]["flags"], 32832);
    assert_eq!(
        requests[2],
        (Method::POST, CB.to_owned(), json!({ "type": 5 }))
    );
    assert_eq!(
        (&requests[3].0, requests[3].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(
        requests[4],
        (
            Method::POST,
            CB.to_owned(),
            json!({ "type": 5, "data": { "flags": 64 } })
        )
    );
    assert_eq!(
        (&requests[5].0, requests[5].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
}

/// Story 6.10 AC 3: a context-menu handler declared `auto_defer = false`
/// and `response = ephemeral` gets no auto-defer, and its `defer_reply`
/// is ephemeral.
#[tokio::test]
async fn int_05_manual_menu() {
    let menu = ContextMenuHandler::__new_user(
        "m::report",
        "Report",
        CommandMetadata::new(),
        |_: CommandCtx, _: ResolvedUser, r: CommandResponder<crate::Manual>| async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(r.defer_reply().await?.done())
        },
    )
    .__with_ephemerality(Ephemerality::Ephemeral);
    assert!(!menu.auto_defer());
    assert_eq!(menu.ephemerality(), Ephemerality::Ephemeral);
    let p = Pipeline::start_with(Module::new("m").__add(menu), |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    let user_data = json!({
        "id": "9", "name": "Report", "type": 2, "target_id": "11",
        "resolved": { "users": { "11": user("11") } }
    });
    p.run(inbound(&interaction(user_data, json!({})))).await;
    let requests = p.requests();
    assert_eq!(
        requests,
        [(
            Method::POST,
            CB.to_owned(),
            json!({ "type": 5, "data": { "flags": 64 } })
        )]
    );
}

mod components;
mod expired;
mod guards;
mod middleware;
mod refs;
