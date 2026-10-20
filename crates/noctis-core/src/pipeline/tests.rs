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
use crate::events::{EventCtx, EventHandler};
use crate::options::__options::required;
use crate::options::{OptionError, OptionValue, Options};
use crate::responder::tests::{Fixture, discord, v2};
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
        let server = crate::edge::test_support::TestServer::start_with(false, discord()).await;
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
    let p = Pipeline::start(Module::new("m").add(handler)).await;
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
    let p = Pipeline::start(Module::new("m").add(handler)).await;
    p.run(inbound(&interaction(slash("ping", json!([])), json!({}))))
        .await;
    assert_eq!(p.requests()[0].2["data"]["flags"], 32832);
}

/// Matrix 3: a three-word path.
#[tokio::test]
async fn three_word_path() {
    let log = Log::default();
    let handler = logging(&log, "m::config", &["config set prefix"]);
    let p = Pipeline::start(Module::new("m").add(handler)).await;
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
        .add(logging(&log, "m::ping", &["ping"]))
        .add(
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
        .add(CommandHandler::__new_plain(
            "m::p",
            &["public"],
            "d",
            record(&log, "public"),
        ))
        .add(
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
        .add(logging(&log, "m::ayarlar", &["ayarlar"]))
        .add(ContextMenuHandler::__new_user(
            "m::show",
            "Profil göster",
            CommandMetadata::new(),
            move |_: CommandCtx, target: ResolvedUser, _: CommandResponder| {
                user_log.push(format!("user:{}", target.user.id));
                async { Ok(v2("u")) }
            },
        ))
        .add(ContextMenuHandler::__new_message(
            "m::quote",
            "Profil göster",
            CommandMetadata::new(),
            move |ctx: CommandCtx, target: ResolvedMessage, r: CommandResponder| {
                message_log.push(format!(
                    "message:{}:{}",
                    target.message.id,
                    ctx.command_path()
                ));
                async move { Ok(r.defer(Ephemerality::Public).await?.done()) }
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
    assert_eq!(p.requests().len(), 3);
}

/// Matrix 13 and 14: malformed command data and a missing author.
#[tokio::test]
async fn malformed_and_authorless() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let log = Log::default();
    let p = Pipeline::start(Module::new("m").add(logging(&log, "m::warn", &["warn"]))).await;
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

    let mut authorless = interaction(slash("warn", json!([])), json!({}));
    authorless.as_object_mut().unwrap().remove("user");
    p.run(inbound(&authorless)).await;
    assert!(log.lines().is_empty());
    assert!(p.requests().is_empty());
    assert_eq!(capture.lines_with(&["without an author"]).len(), 1);
}

/// Matrix 15, 16 and 35: errors and panics go to the sink; the next
/// interaction is still served.
#[tokio::test]
async fn errors_and_panics_reach_the_sink() {
    let log = Log::default();
    let module = Module::new("m")
        .add(CommandHandler::__new_plain(
            "m::fail",
            &["fail"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                Err::<V2Message, _>(HandlerError::new(std::io::Error::other("boom")))
            },
        ))
        .add(CommandHandler::__new_plain(
            "m::sync",
            &["sync"],
            "d",
            |_: CommandCtx, _: CommandResponder| -> std::future::Ready<HandlerResult<V2Message>> {
                panic!("sync boom")
            },
        ))
        .add(CommandHandler::__new_plain(
            "m::inner",
            &["inner"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                tokio::task::yield_now().await;
                Ok(boom())
            },
        ))
        .add(CommandHandler::__new_plain(
            "m::big",
            &["big"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                Ok(Components::new().push(ActionRow::buttons([
                    LinkButton::new("https://x.y").label("a".repeat(81))
                ])))
            },
        ))
        .add(logging(&log, "m::ok", &["ok"]));
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
    assert_eq!(p.requests().len(), 1);
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
    let p = Arc::new(Pipeline::start(Module::new("m").add(handler)).await);
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
            let _ = r.defer(Ephemerality::Public).await?.done();
            Ok(v2("done"))
        },
    );
    let p = Pipeline::start(Module::new("m").add(handler)).await;
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
        .add(CommandHandler::__new_plain(
            "m::two",
            &["two"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                let _ = r.reply(v2("a")).await?.done();
                Ok(v2("b"))
            },
        ))
        .add(CommandHandler::__new_plain(
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
        .add(logging(&log, "m::ping", &["ping"]))
        .add(
            logging(&log, "m::gping", &["ping"])
                .__with_metadata(CommandMetadata::new().guilds(&GUILDS)),
        )
        .add(CommandHandler::__new_plain_with_options(
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
    let p = Pipeline::start(Module::new("m").add(logging(&log, "m::ping", &["ping"]))).await;
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
    let ctx = EventCtx::new(0, None, "m", Arc::clone(p.fx.app.shared()));
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
    let p = Pipeline::start_with(Module::new("profile/xp").add(handler), |b| {
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
    let p = Pipeline::start(Module::new("m").add(menu)).await;
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
    let p = Pipeline::start(Module::new("m").add(handler)).await;
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
        .add(sleeper("m::slow", &["slow"], Duration::from_millis(300)))
        .add(
            sleeper("m::hidden", &["hidden"], Duration::from_millis(300))
                .__with_ephemerality(Ephemerality::Ephemeral),
        );
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
    assert!(p.reports().is_empty());
    let (deferred, latencies) = metrics.read();
    assert_eq!((deferred, latencies.len()), (2, 2));
    assert!(
        latencies.iter().all(|l| (0.05..1.0).contains(l)),
        "{latencies:?}"
    );

    // Matrix 10: the deadline already passed while queued (default 2 s).
    let p = Pipeline::start(Module::new("m").add(sleeper(
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
    assert_eq!(metrics.read().0, 3);
}

/// Story 6.6 Matrix 1, 8, 9 and 40: a reply before the deadline, an
/// explicit defer before it and an explicit defer after the auto-defer.
#[tokio::test]
async fn int_01_handler_answers_first() {
    let metrics = Metrics::new();
    let _metrics = metrics::set_default_local_recorder(&metrics.recorder);
    let module = Module::new("m")
        .add(CommandHandler::__new_plain(
            "m::fast",
            &["fast"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(120)).await;
                Ok::<_, HandlerError>(r.reply(v2("fast")).await?.done())
            },
        ))
        .add(CommandHandler::__new_plain(
            "m::early",
            &["early"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                let _ = r.defer(Ephemerality::Public).await?.done();
                tokio::time::sleep(Duration::from_millis(700)).await;
                Ok(v2("done"))
            },
        ))
        .add(CommandHandler::__new_plain(
            "m::after",
            &["after"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(700)).await;
                Ok::<_, HandlerError>(r.defer(Ephemerality::Ephemeral).await?.done())
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
async fn int_03_late_reply_becomes_edit_original() {
    let module = Module::new("m")
        .add(CommandHandler::__new_plain(
            "m::late",
            &["late"],
            "d",
            |_: CommandCtx, r: CommandResponder| async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok::<_, HandlerError>(r.reply(v2("late")).await?.done())
            },
        ))
        .add(CommandHandler::__new_plain(
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
    let p = Pipeline::start_with(Module::new("m").add(race), |b| {
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
        Pipeline::start_with(Module::new("m").add(handler), |b| {
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
    let p = Pipeline::start(Module::new("m").add(handler)).await;
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
        .add(CommandHandler::__new_plain(
            "m::fail",
            &["fail"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Err::<V2Message, _>(HandlerError::new(std::io::Error::other("boom")))
            },
        ))
        .add(CommandHandler::__new_plain(
            "m::inner",
            &["inner"],
            "d",
            |_: CommandCtx, _: CommandResponder| async {
                tokio::task::yield_now().await;
                Ok(boom())
            },
        ))
        .add(CommandHandler::__new_plain(
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
    assert!(p.requests().is_empty(), "{:?}", p.requests());
    let kinds: Vec<String> = p.reports().into_iter().map(|r| r.kind).collect();
    assert_eq!(kinds, ["error:boom", "panic:boom", "cancelled"]);
}

/// Story 6.6 Matrix 16: the Dispatcher arms no auto-defer.
#[tokio::test]
async fn dispatcher_has_no_auto_defer() {
    let p = Pipeline::start_with(
        Module::new("m").add(sleeper("m::slow", &["slow"], Duration::from_millis(100))),
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

/// INT-05 (Story 6.6 Matrix 18): an `opens_modal` Command gets no
/// auto-defer; a missed ack deadline logs one error naming the handler.
#[tokio::test]
async fn int_05_opens_modal_never_auto_defers() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let handler = CommandHandler::__new_plain(
        "m::form",
        &["form"],
        "d",
        |_: CommandCtx, _: CommandResponder<crate::OpensModal>| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Err::<Responded, _>(HandlerError::new(std::io::Error::other("no modal")))
        },
    );
    assert!(handler.opens_modal());
    assert!(!sleeper("m::x", &["x"], Duration::ZERO).opens_modal());
    let p = Pipeline::start_with(Module::new("forms").add(handler), |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    p.run(inbound(&interaction(slash("form", json!([])), json!({}))))
        .await;
    assert!(p.requests().is_empty());
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
    assert!(p.requests().is_empty());
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
                    let acked = r.defer(Ephemerality::Public).await?;
                    let big = Components::new()
                        .push(ActionRow::buttons([
                            LinkButton::new("https://x.y").label("a".repeat(81))
                        ]));
                    let mut results = Vec::new();
                    let first = acked.followup(big, Visibility::Public).await;
                    results.push(first.map(drop));
                    for _ in 0..6 {
                        let sent = acked.followup(v2("f"), Visibility::Ephemeral).await;
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
    let p = Pipeline::start(Module::new("m").add(handler)).await;
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
