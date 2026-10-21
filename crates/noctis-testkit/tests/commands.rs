//! Commands end to end (Story 6.5, Matrix 1, 2, 18-21, 24, 45 and 49; Story
//! 6.6, Matrix 2, 5, 17 and 21; Story 6.7 autocomplete ACs; Story 6.8 ACs): a
//! `FakeGateway` `INTERACTION_CREATE` (or `MESSAGE_CREATE` through the
//! Dispatcher) reaches the Command pipeline, whose responses `FakeDiscordHttp`
//! records.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::events::{EventCtx, MessageCreate};
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};
use noctis::{
    AppBuilder, CommandInput, CommandInvocation, ErrorReport, ReportKind, Runtime, Token,
};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method, RecordedRequest, StatusCode,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const CALLBACK: &str = "/interactions/100/tok/callback";
const ORIGINAL: &str = "/webhooks/7/tok/messages/@original";
const FOLLOWUP: &str = "/webhooks/7/tok";
const CHANNEL: &str = "/channels/5/messages";
/// `noctis.error.generic` in en-US.
const GENERIC: &str = "Something went wrong. Please try again later.";

fn text(t: &str) -> Components {
    Components::new().push(TextDisplay::new(t.to_owned()))
}

#[derive(SlashCommand)]
#[command("ping", description = "Check that the bot answers")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Ok(text("pong"))
    }
}

#[derive(SlashCommand)]
#[command("secret", description = "A private answer", response = ephemeral)]
struct Secret;

impl Secret {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Ok(text("psst"))
    }
}

#[derive(SlashCommand)]
#[command("twice", description = "Reply, then return a message")]
struct Twice;

impl Twice {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Components> {
        let _ = r.reply(text("first")).await?.done();
        Ok(text("second"))
    }
}

#[derive(SlashCommand)]
#[command("slow", description = "Defer, then return a message")]
struct Slow;

impl Slow {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Components> {
        let _ = r.defer_reply_as(Ephemerality::Ephemeral).await?.done();
        Ok(text("later"))
    }
}

#[derive(SlashCommand)]
#[command("log", description = "Defer, then a public follow-up")]
struct Log;

impl Log {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.defer_reply_as(Ephemerality::Ephemeral).await?;
        let handle = acked.follow_up(text("logged"), Visibility::Public).await?;
        assert_eq!(handle.message_id().get(), 55);
        Ok(acked.done())
    }
}

#[derive(SlashCommand)]
#[command("lazy", description = "Answer after the auto-defer")]
struct Lazy;

impl Lazy {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(text("later"))
    }
}

#[derive(::noctis::Button)]
#[button("t.feedback", defaults(style = Secondary, label = "Go"))]
struct SendFeedback;

impl SendFeedback {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(SlashCommand)]
#[command("feedback", description = "Open the feedback form", auto_defer = false)]
struct Feedback;

impl Feedback {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        let modal =
            noctis::__private::ui::__macro::build_modal::<SendFeedback>(SendFeedback, "Feedback")
                .push(TextDisplay::new("Thanks!"));
        Ok(r.show_modal(modal).await?.done())
    }
}

#[derive(EventHandler)]
struct Prefix;

impl Prefix {
    #[handler]
    async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
        if let Some(path) = msg.content.strip_prefix('!') {
            let invocation = CommandInvocation::new(
                CommandInput::new(path),
                msg.channel_id,
                msg.id,
                msg.author.id,
            )
            .guild_id(msg.guild_id);
            ctx.dispatcher().run_command(invocation).await?;
        }
        Ok(())
    }
}

/// Records every Command report as `<handler>:<kind>`.
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Vec<String>>>);

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => error.to_string(),
            other => format!("{other:?}"),
        };
        self.0
            .lock()
            .unwrap()
            .push(format!("{}:{kind}", report.handler.unwrap_or_default()));
    }
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    reports: Reports,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(module: Module) -> Self {
        Self::start_with(module, |_| {}).await
    }

    /// As [`Running::start`], with `configure` applied to the builder.
    async fn start_with(module: Module, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards: 1,
            ..GatewayBot::default()
        })
        .await
        .unwrap();
        http.route(
            Method::POST,
            CALLBACK,
            FakeResponse::empty(StatusCode::NO_CONTENT),
        )
        .route(
            Method::PATCH,
            ORIGINAL,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "1" })),
        )
        .route(
            Method::POST,
            FOLLOWUP,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "55" })),
        )
        .route(
            Method::DELETE,
            ORIGINAL,
            FakeResponse::empty(StatusCode::NO_CONTENT),
        )
        .route(
            Method::POST,
            CHANNEL,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "77" })),
        );
        let gw = FakeGateway::start().await.unwrap();
        let reports = Reports::default();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .error_sink(ErrorOrigin::Command, reports.clone())
            .module(module);
        configure(&mut builder);
        let app = builder.build().unwrap();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let running = Self {
            gw,
            http,
            reports,
            run,
        };
        running.wait_ready().await;
        running
    }

    /// Waits until the fake holds the shard's session.
    async fn wait_ready(&self) {
        let deadline = Instant::now() + WAIT;
        while self.gw.dispatch("TYPING_START", json!({})) == 0 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Dispatches a chat-input interaction for `name` (id 100, token
    /// `tok`, application 7).
    fn invoke(&self, name: &str) {
        let d = json!({
            "id": "100",
            "application_id": "7",
            "type": 2,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "locale": "tr",
            "data": { "id": "9", "name": name, "type": 1 },
        });
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
    }

    /// As [`Running::invoke`], with the top-level `options`.
    fn invoke_with(&self, name: &str, options: &Value) {
        let d = json!({
            "id": "100",
            "application_id": "7",
            "type": 2,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "locale": "tr",
            "data": { "id": "9", "name": name, "type": 1, "options": options },
        });
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
    }

    /// The requests to `path` other than `GET /gateway/bot`.
    fn sent(&self) -> Vec<RecordedRequest> {
        self.http
            .requests()
            .into_iter()
            .filter(|r| r.route_path() != Some("/gateway/bot"))
            .collect()
    }

    /// Polls until `n` responses were sent.
    async fn wait_sent(&self, n: usize) -> Vec<RecordedRequest> {
        let deadline = Instant::now() + WAIT;
        loop {
            let sent = self.sent();
            if sent.len() >= n {
                return sent;
            }
            assert!(Instant::now() < deadline, "{n} requests missing: {sent:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_report(&self) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let reports = self.reports.0.lock().unwrap().clone();
            if !reports.is_empty() {
                return reports;
            }
            assert!(Instant::now() < deadline, "no report");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn body(request: &RecordedRequest) -> Value {
    request.json().unwrap_or(Value::Null)
}

/// Matrix 1 and 2: a returned message is the type-4 callback, with the
/// declared ephemerality.
#[tokio::test(flavor = "multi_thread")]
async fn returned_message_is_the_reply() {
    let running = Running::start(Module::new("m").add::<Ping>().add::<Secret>()).await;
    running.invoke("ping");
    let sent = running.wait_sent(1).await;
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert!(sent[0].headers.get("authorization").is_none());
    let reply = body(&sent[0]);
    assert_eq!(reply["type"], 4);
    assert_eq!(reply["data"]["flags"], 32768);
    assert_eq!(reply["data"]["components"][0]["content"], "pong");
    running.invoke("secret");
    let sent = running.wait_sent(2).await;
    assert_eq!(body(&sent[1])["data"]["flags"], 32832);
    running.run.abort();
}

/// Matrix 18-21: a reply then a returned message edits the original; a
/// defer then a returned message edits it with the V2 flag.
#[tokio::test(flavor = "multi_thread")]
async fn acknowledged_then_returned_edits_the_original() {
    let running = Running::start(Module::new("m").add::<Twice>().add::<Slow>()).await;
    running.invoke("twice");
    let sent = running.wait_sent(2).await;
    assert_eq!(body(&sent[0])["type"], 4);
    assert_eq!(sent[1].method, Method::PATCH);
    assert_eq!(sent[1].route_path(), Some(ORIGINAL));
    assert_eq!(body(&sent[1])["components"][0]["content"], "second");
    running.invoke("slow");
    let sent = running.wait_sent(4).await;
    assert_eq!(
        body(&sent[2]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    assert_eq!(sent[3].method, Method::PATCH);
    assert_eq!(body(&sent[3])["flags"], 32768);
    assert!(body(&sent[3]).get("content").is_none());
    running.run.abort();
}

/// Matrix 24 and the Responder AC: an ephemeral defer, then a public
/// follow-up.
#[tokio::test(flavor = "multi_thread")]
async fn defer_then_follow_up() {
    let running = Running::start(Module::new("m").add::<Log>()).await;
    running.invoke("log");
    let sent = running.wait_sent(2).await;
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    assert_eq!(sent[1].route_path(), Some(FOLLOWUP));
    assert_eq!(sent[1].query.as_deref(), Some("wait=true"));
    assert_eq!(body(&sent[1])["flags"], 32768);
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

/// Matrix 45 and 49 and the Dispatcher AC: a `MESSAGE_CREATE` handler runs
/// a Command whose message is posted as a reply; an unknown path sends
/// nothing; a declared ephemeral Command cannot answer there.
#[tokio::test(flavor = "multi_thread")]
async fn dispatcher_replies_to_the_message() {
    let running = Running::start(
        Module::new("m")
            .add::<Ping>()
            .add::<Secret>()
            .add::<Prefix>(),
    )
    .await;
    let message = |content: &str| {
        json!({
            "id": "6", "channel_id": "5", "author": user(), "content": content,
            "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0
        })
    };
    assert_eq!(running.gw.dispatch("MESSAGE_CREATE", message("!ping")), 1);
    let sent = running.wait_sent(1).await;
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].route_path(), Some(CHANNEL));
    let reply = body(&sent[0]);
    assert_eq!(reply["flags"], 32768);
    assert_eq!(
        reply["message_reference"],
        json!({ "message_id": "6", "fail_if_not_exists": false })
    );
    assert_eq!(running.gw.dispatch("MESSAGE_CREATE", message("!secret")), 1);
    let reports = running.wait_report().await;
    assert!(reports[0].contains("Secret"), "{reports:?}");
    assert!(reports[0].contains("cannot be ephemeral"), "{reports:?}");
    // Story 6.8: the failure is a public notice replying to the message.
    let sent = running.sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[1].route_path(), Some(CHANNEL));
    assert_eq!(body(&sent[1])["components"][0]["content"], GENERIC);
    running.run.abort();
}

/// Story 6.6 Matrix 2 and 5 and the Auto-defer AC: a slow handler is
/// deferred with exactly `{"type":5}`, then its returned message edits the
/// original; no type-4 callback.
#[tokio::test(flavor = "multi_thread")]
async fn auto_defer_then_returned_message_edits_the_original() {
    let running = Running::start_with(Module::new("m").add::<Lazy>(), |b| {
        b.ack_deadline(Duration::from_millis(50));
    })
    .await;
    running.invoke("lazy");
    let sent = running.wait_sent(2).await;
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert_eq!(body(&sent[0]), json!({ "type": 5 }));
    assert_eq!(sent[1].method, Method::PATCH);
    assert_eq!(sent[1].route_path(), Some(ORIGINAL));
    assert_eq!(body(&sent[1])["flags"], 32768);
    assert_eq!(body(&sent[1])["components"][0]["content"], "later");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let callbacks = running
        .sent()
        .iter()
        .filter(|r| r.route_path() == Some(CALLBACK))
        .count();
    assert_eq!(callbacks, 1);
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

/// Story 6.6 Matrix 17 and 21, Story 6.10 AC 2 and the Modals AC: an
/// `auto_defer = false` Command's modal is callback type 9 (no auto-defer,
/// even with a 0 ms deadline); over the Dispatcher it is
/// `ModalUnavailable`.
#[tokio::test(flavor = "multi_thread")]
async fn modal_opens_as_the_first_response() {
    let module = Module::new("m").add::<Feedback>().add::<Prefix>();
    let running = Running::start_with(module, |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    assert!(!Feedback::descriptor().auto_defer());
    running.invoke("feedback");
    let sent = running.wait_sent(1).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    let callback = body(&sent[0]);
    assert_eq!(callback["type"], 9);
    assert_eq!(callback["data"]["title"], "Feedback");
    assert_eq!(callback["data"]["custom_id"], "0t.feedback:");
    assert_eq!(callback["data"]["components"][0]["content"], "Thanks!");
    let message = json!({
        "id": "6", "channel_id": "5", "author": user(), "content": "!feedback",
        "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
        "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
        "attachments": [], "embeds": [], "pinned": false, "type": 0
    });
    assert_eq!(running.gw.dispatch("MESSAGE_CREATE", message), 1);
    let reports = running.wait_report().await;
    assert!(
        reports[0].contains("a modal can only be the first response"),
        "{reports:?}"
    );
    // Story 6.8: the Dispatcher's failure is a public generic notice.
    let sent = running.sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[1].route_path(), Some(CHANNEL));
    assert_eq!(body(&sent[1])["components"][0]["content"], GENERIC);
    running.run.abort();
}

// ---- Story 6.7: autocomplete ----

/// What `prefix_suggest` saw: the focused option and `partial.limit`.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(SlashCommand)]
#[command("config (set|get) prefix", description = "Prefix settings")]
#[expect(dead_code, reason = "only autocomplete reads it")]
struct Config {
    #[option(description = "The prefix", autocomplete)]
    value: String,
    #[option(description = "How many")]
    limit: i64,
    #[option(description = "Many suggestions", autocomplete)]
    many: Option<String>,
    #[option(description = "A failing suggestion", autocomplete)]
    failing: Option<String>,
}

impl Config {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<Components> {
        Ok(text("ok"))
    }

    #[autocomplete(value)]
    async fn prefix_suggest(
        ctx: AutocompleteCtx,
        typed: &str,
        filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        SEEN.lock().unwrap().push(format!(
            "{}:{}:{typed}:{:?}",
            ctx.command_path(),
            ctx.focused().name(),
            filled.limit
        ));
        Ok(["!", "?", "."]
            .into_iter()
            .map(|p| (p.to_owned(), format!("{typed}{p}")))
            .collect())
    }

    #[autocomplete(many)]
    async fn many(
        _ctx: AutocompleteCtx,
        _typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        Ok((0..30)
            .map(|i| (format!("v{i}"), format!("c{i}")))
            .collect())
    }

    #[autocomplete(failing)]
    async fn failing(
        _ctx: AutocompleteCtx,
        _typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        Err(HandlerError::new(std::io::Error::other("lookup failed")))
    }
}

/// Dispatches an autocomplete interaction of `config set prefix` focusing
/// `focused` with the text `typed`.
fn autocomplete_interaction(running: &Running, focused: &str, typed: &str) {
    let d = json!({
        "id": "100",
        "application_id": "7",
        "type": 4,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "data": {
            "id": "9", "name": "config", "type": 1,
            "options": [{ "type": 2, "name": "set", "options": [
                { "type": 1, "name": "prefix", "options": [
                    { "type": 3, "name": focused, "value": typed, "focused": true }
                ]}
            ]}]
        },
    });
    assert_eq!(running.gw.dispatch("INTERACTION_CREATE", d), 1);
}

/// Starts an App with `config` whose Autocomplete Error Sink is the
/// returned recorder.
async fn start_autocomplete() -> (Running, Reports) {
    let reports = Reports::default();
    let sink = reports.clone();
    let running = Running::start_with(Module::new("m").add::<Config>(), move |b| {
        b.error_sink(ErrorOrigin::Autocomplete, sink);
    })
    .await;
    (running, reports)
}

/// The 3-word path AC: the handler gets the focused option and an all-`None`
/// partial; one type-8 callback carries its choices.
#[tokio::test(flavor = "multi_thread")]
async fn autocomplete_three_word_path() {
    let (running, reports) = start_autocomplete().await;
    autocomplete_interaction(&running, "value", "pr");
    let sent = running.wait_sent(1).await;
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 8, "data": { "choices": [
            { "name": "pr!", "value": "!" },
            { "name": "pr?", "value": "?" },
            { "name": "pr.", "value": "." },
        ]}})
    );
    assert!(
        SEEN.lock()
            .unwrap()
            .contains(&"config set prefix:value:pr:None".to_owned())
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(running.sent().len(), 1);
    assert!(reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

/// The INT-07 AC: 30 choices, 25 sent.
#[tokio::test(flavor = "multi_thread")]
async fn autocomplete_truncates_to_twenty_five() {
    let (running, _reports) = start_autocomplete().await;
    autocomplete_interaction(&running, "many", "");
    let sent = running.wait_sent(1).await;
    let choices = body(&sent[0])["data"]["choices"].as_array().unwrap().len();
    assert_eq!(choices, 25);
    running.run.abort();
}

/// The failure AC: an erroring handler answers an empty list and reports
/// to the Autocomplete Error Sink.
#[tokio::test(flavor = "multi_thread")]
async fn autocomplete_error_sends_empty_choices() {
    let (running, reports) = start_autocomplete().await;
    autocomplete_interaction(&running, "failing", "x");
    let sent = running.wait_sent(1).await;
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 8, "data": { "choices": [] } })
    );
    let deadline = Instant::now() + WAIT;
    loop {
        let seen = reports.0.lock().unwrap().clone();
        if !seen.is_empty() {
            assert_eq!(seen.len(), 1);
            assert!(seen[0].ends_with("::failing:lookup failed"), "{seen:?}");
            break;
        }
        assert!(Instant::now() < deadline, "no report");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

// ---- Story 6.8: user-facing errors, notices and panics ----

const VALIDATION: &str = "That input isn't valid. Check it and try again.";

#[derive(SlashCommand)]
#[command("broken", description = "Always fails")]
struct Broken;

impl Broken {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Err(HandlerError::new(std::io::Error::other("database down")))
    }
}

#[derive(SlashCommand)]
#[command("explode", description = "Always panics")]
struct Explode;

impl Explode {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        tokio::task::yield_now().await;
        panic!("kaboom")
    }
}

#[derive(SlashCommand)]
#[command("sluggish", description = "Fails after the auto-defer")]
struct Sluggish;

impl Sluggish {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Err(HandlerError::new(std::io::Error::other("too slow")))
    }
}

static CHECKED_RAN: Mutex<Vec<i64>> = Mutex::new(Vec::new());

#[expect(clippy::trivially_copy_pass_by_ref, reason = "validators take `&T`")]
fn positive(days: &i64, _ctx: &CommandCtx) -> Result<(), MessageKey> {
    if *days > 0 {
        Ok(())
    } else {
        Err(MessageKey::new("noctis.validation.failed"))
    }
}

#[derive(SlashCommand)]
#[command("purge", description = "Validated options")]
struct Purge {
    #[option(description = "How many days", validate = positive)]
    days: i64,
}

impl Purge {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        CHECKED_RAN.lock().unwrap().push(self.days);
        Ok(text("purged"))
    }
}

/// Asserts `request` is the ephemeral type-4 notice of `text`.
fn assert_notice(request: &RecordedRequest, text: &str) {
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.route_path(), Some(CALLBACK));
    let callback = body(request);
    assert_eq!(callback["type"], 4, "{callback}");
    assert_eq!(callback["data"]["flags"], 32832, "{callback}");
    assert_eq!(
        callback["data"]["components"][0]["content"], text,
        "{callback}"
    );
}

/// The Errors AC: an `Err` is one ephemeral generic notice, then a report.
#[tokio::test(flavor = "multi_thread")]
async fn handler_error_is_an_ephemeral_notice() {
    let running = Running::start(Module::new("m").add::<Broken>()).await;
    running.invoke("broken");
    let sent = running.wait_sent(1).await;
    assert_notice(&sent[0], GENERIC);
    let reports = running.wait_report().await;
    assert_eq!(reports, ["commands::Broken:database down"]);
    assert_eq!(running.sent().len(), 1);
    running.run.abort();
}

/// The Panics AC (NFR-4): the user gets the generic notice, the sink a
/// panic, and the next interaction on the shard is answered.
#[tokio::test(flavor = "multi_thread")]
async fn panic_is_a_notice_and_the_shard_keeps_running() {
    let running = Running::start(Module::new("m").add::<Explode>().add::<Ping>()).await;
    running.invoke("explode");
    let sent = running.wait_sent(1).await;
    assert_notice(&sent[0], GENERIC);
    let reports = running.wait_report().await;
    assert!(
        reports[0].starts_with("commands::Explode:Panic"),
        "{reports:?}"
    );
    assert!(reports[0].contains("kaboom"), "{reports:?}");
    running.invoke("ping");
    let sent = running.wait_sent(2).await;
    assert_eq!(body(&sent[1])["data"]["components"][0]["content"], "pong");
    running.run.abort();
}

/// The After auto-defer AC (AD-23): type 5, then the original is deleted,
/// then an ephemeral follow-up.
#[tokio::test(flavor = "multi_thread")]
async fn error_after_auto_defer_deletes_then_follows_up() {
    let running = Running::start_with(Module::new("m").add::<Sluggish>(), |b| {
        b.ack_deadline(Duration::from_millis(100));
    })
    .await;
    running.invoke("sluggish");
    let sent = running.wait_sent(3).await;
    assert_eq!(body(&sent[0]), json!({ "type": 5 }));
    assert_eq!(sent[1].method, Method::DELETE);
    assert_eq!(sent[1].route_path(), Some(ORIGINAL));
    assert_eq!(sent[2].method, Method::POST);
    assert_eq!(sent[2].route_path(), Some(FOLLOWUP));
    assert_eq!(body(&sent[2])["flags"], 32832);
    assert_eq!(body(&sent[2])["components"][0]["content"], GENERIC);
    assert_eq!(running.wait_report().await, ["commands::Sluggish:too slow"]);
    running.run.abort();
}

/// The Validator AC: the handler does not run, the user sees the key's
/// notice, and no sink report is made.
#[tokio::test(flavor = "multi_thread")]
async fn validator_failure_is_a_notice_without_a_report() {
    let running = Running::start(Module::new("m").add::<Purge>()).await;
    running.invoke_with("purge", &json!([{ "name": "days", "type": 4, "value": 0 }]));
    let sent = running.wait_sent(1).await;
    assert_notice(&sent[0], VALIDATION);
    running.invoke_with("purge", &json!([{ "name": "days", "type": 4, "value": 3 }]));
    let sent = running.wait_sent(2).await;
    assert_eq!(body(&sent[1])["data"]["components"][0]["content"], "purged");
    assert_eq!(*CHECKED_RAN.lock().unwrap(), [3]);
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

#[derive(SlashCommand)]
#[command(
    "survey",
    description = "Opens a modal, then fails",
    auto_defer = false
)]
struct Survey;

impl Survey {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        let modal =
            noctis::__private::ui::__macro::build_modal::<SendFeedback>(SendFeedback, "Survey")
                .push(TextDisplay::new("Hi"));
        let _ = r.show_modal(modal).await?.done();
        Err(HandlerError::new(std::io::Error::other("after modal")))
    }
}

/// Story 6.8 Matrix 14: after a modal no notice is sent; the error is
/// still reported.
#[tokio::test(flavor = "multi_thread")]
async fn no_notice_after_a_modal() {
    let running = Running::start(Module::new("m").add::<Survey>()).await;
    running.invoke("survey");
    let reports = running.wait_report().await;
    assert_eq!(reports, ["commands::Survey:after modal"]);
    let sent = running.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(body(&sent[0])["type"], 9);
    running.run.abort();
}

#[derive(UserCommand)]
#[user_command("Report", response = ephemeral, auto_defer = false)]
struct ReportUser;

impl ReportUser {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: noctis::ui::ResolvedUser,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }
}

/// Story 6.10 AC 3: a `response = ephemeral, auto_defer = false` user
/// command's `defer_reply()` is the only callback, an ephemeral type 5,
/// and no auto-defer runs (even with a 0 ms deadline).
#[tokio::test(flavor = "multi_thread")]
async fn manual_user_command_defers_ephemerally() {
    let running = Running::start_with(Module::new("m").add::<ReportUser>(), |b| {
        b.ack_deadline(Duration::ZERO);
    })
    .await;
    let d = json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "locale": "tr",
        "data": {
            "id": "9", "name": "Report", "type": 2, "target_id": "3",
            "resolved": { "users": { "3": user() } }
        },
    });
    assert_eq!(running.gw.dispatch("INTERACTION_CREATE", d), 1);
    let sent = running.wait_sent(1).await;
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let sent = running.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

// ---- Story 6.11: one struct per interaction ----

/// What `Warn`'s handler decoded: `(user id, rule)`.
static WARNED: Mutex<Vec<(u64, i64)>> = Mutex::new(Vec::new());

#[derive(SlashCommand)]
#[command("warn", description = "d", response = ephemeral)]
struct Warn {
    #[option(description = "u")]
    user: noctis::ui::ResolvedUser,
    #[option(description = "r", autocomplete)]
    rule: i64,
}

impl Warn {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        WARNED
            .lock()
            .unwrap()
            .push((self.user.user.id.get(), self.rule));
        Ok(text("warned"))
    }

    #[autocomplete(rule)]
    async fn rule(
        _ctx: AutocompleteCtx,
        typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<i64>> {
        let base: i64 = typed.parse().unwrap_or(0);
        Ok([(base + 1, "next"), (base + 2, "after")]
            .into_iter()
            .collect())
    }
}

/// Story 6.11 AC "Slash command end to end": the handler runs with the
/// decoded `self`, and an autocomplete interaction for `rule` gets exactly
/// one type-8 callback with integer values.
#[tokio::test(flavor = "multi_thread")]
async fn slash_command_struct_end_to_end() {
    let running = Running::start(Module::new("m").add::<Warn>()).await;
    let d = json!({
        "id": "100", "application_id": "7", "type": 2, "token": "tok", "version": 1,
        "authorizing_integration_owners": {}, "entitlements": [], "user": user(),
        "data": {
            "id": "9", "name": "warn", "type": 1,
            "options": [
                { "type": 6, "name": "user", "value": "3" },
                { "type": 4, "name": "rule", "value": 12 }
            ],
            "resolved": { "users": { "3": user() } }
        },
    });
    assert_eq!(running.gw.dispatch("INTERACTION_CREATE", d), 1);
    let sent = running.wait_sent(1).await;
    assert_eq!(body(&sent[0])["type"], 4);
    assert_eq!(body(&sent[0])["data"]["flags"], 32768 | 64);
    assert!(WARNED.lock().unwrap().contains(&(3, 12)));

    let d = json!({
        "id": "100", "application_id": "7", "type": 4, "token": "tok", "version": 1,
        "authorizing_integration_owners": {}, "entitlements": [], "user": user(),
        "data": {
            "id": "9", "name": "warn", "type": 1,
            "options": [{ "type": 4, "name": "rule", "value": "4", "focused": true }]
        },
    });
    assert_eq!(running.gw.dispatch("INTERACTION_CREATE", d), 1);
    let sent = running.wait_sent(2).await;
    assert_eq!(
        body(&sent[1]),
        json!({ "type": 8, "data": { "choices": [
            { "name": "next", "value": 5 },
            { "name": "after", "value": 6 },
        ]}})
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(running.sent().len(), 2);
    running.run.abort();
}
