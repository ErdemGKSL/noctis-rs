//! Middleware end to end (AC1, Matrix 21, 24-26, 32, 35 and 36): a
//! `FakeGateway` slash command, component click, `MESSAGE_CREATE` or
//! Dispatcher run goes through the before chain, the handler and the after
//! chain, and every response is asserted on the `FakeDiscordHttp` wire
//! (method, path, body).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::events::{EventCtx, MessageCreate};
use noctis::prelude::*;
use noctis::ui::LegacyMessage;
use noctis::{
    AfterCtx, AppBuilder, CommandInput, CommandInvocation, ErrorReport, Invocation, ReportKind,
    Runtime, Token,
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

/// The key every denying Middleware stops with, and its text.
const DENIED: MessageKey = MessageKey::new("noctis.guard.permissions");
const DENIED_TEXT: &str = "You don't have permission to use this.";

/// What the Middleware and handlers did, in order, shared through `State`.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// Appends `line` to the App's [`Seen`].
fn see(seen: Option<State<Arc<Seen>>>, line: impl Into<String>) {
    seen.unwrap().0.lock().unwrap().push(line.into());
}

/// The value the Middleware insert.
#[derive(Debug, PartialEq, Eq)]
struct Level(u32);

async fn r1(ctx: BeforeCtx) -> HandlerResult<Flow> {
    see(ctx.state(), "r1");
    ctx.insert(Level(1));
    Ok(Flow::Continue)
}

async fn r2(ctx: BeforeCtx) -> HandlerResult<Flow> {
    see(ctx.state(), "r2");
    Ok(Flow::Continue)
}

async fn c1(ctx: BeforeCtx) -> HandlerResult<Flow> {
    see(ctx.state(), "c1");
    ctx.insert(Level(2));
    Ok(Flow::Continue)
}

/// Logs `<name>:<outcome>:<Level>`.
fn after_line(name: &str, ctx: &AfterCtx) -> String {
    let level = ctx.get::<Level>().map_or(0, |l| l.0);
    format!("{name}:{:?}:{level}", ctx.outcome())
}

async fn a1(ctx: AfterCtx) {
    see(ctx.state(), after_line("a1", &ctx));
}

async fn a2(ctx: AfterCtx) {
    see(ctx.state(), after_line("a2", &ctx));
}

async fn ca1(ctx: AfterCtx) {
    see(ctx.state(), after_line("ca1", &ctx));
}

/// Stops each Command by its path: `deny` with [`DENIED`], `quiet`
/// silently, and the `slow …` ones after sleeping past the ack deadline.
async fn gate(ctx: BeforeCtx) -> HandlerResult<Flow> {
    let Invocation::Command { path } = ctx.invocation() else {
        return Ok(Flow::Continue);
    };
    see(ctx.state(), format!("gate {path}"));
    if path.starts_with("slow") {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Ok(match path {
        "deny" | "slow deny" => Flow::Stop(Some(DENIED)),
        "quiet" | "slow quiet" => Flow::Stop(None),
        _ => Flow::Continue,
    })
}

/// Stops each click after sleeping past the ack deadline.
async fn slow_click(ctx: BeforeCtx) -> HandlerResult<Flow> {
    see(ctx.state(), "slow click");
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(Flow::Stop(Some(DENIED)))
}

/// A Command every test can stop, by path.
#[derive(SlashCommand)]
#[command("(deny|quiet|open)", description = "Gated")]
struct Gated;

impl Gated {
    #[handler]
    async fn handle(self, ctx: CommandCtx, path: GatedPath) -> HandlerResult<LegacyMessage> {
        see(ctx.state(), format!("handler {}", path.as_str()));
        Ok(LegacyMessage::new("ran"))
    }
}

/// The deferred variants of [`Gated`].
#[derive(SlashCommand)]
#[command("slow (deny|quiet)", description = "Slow")]
struct Slow;

impl Slow {
    #[handler]
    async fn handle(self, ctx: CommandCtx, _path: SlowPath) -> HandlerResult<LegacyMessage> {
        see(ctx.state(), "handler slow");
        Ok(LegacyMessage::new("ran"))
    }
}

/// A Command whose handler reads the run's `Level`.
#[derive(SlashCommand)]
#[command("level", description = "Leveled")]
struct Leveled;

impl Leveled {
    #[handler]
    async fn handle(self, ctx: CommandCtx, level: Ext<Level>) -> HandlerResult<LegacyMessage> {
        see(ctx.state(), format!("handler level {}", level.0));
        Ok(LegacyMessage::new("ran"))
    }
}

/// Inserts `Level(7)` without logging.
async fn provide(ctx: BeforeCtx) -> HandlerResult<Flow> {
    ctx.insert(Level(7));
    Ok(Flow::Continue)
}

/// A button whose handler reads the run's `Level`.
#[derive(Button)]
#[button("mw.level", defaults(style = Primary, label = "Level"))]
struct LevelButton;

impl LevelButton {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, level: Ext<Level>) -> HandlerResult<Responded> {
        see(ctx.state(), format!("handler {}", level.0));
        Ok(noctis::__private::core::__testing::responded())
    }
}

/// A button whose handler fails after reading the run's `Level`.
#[derive(Button)]
#[button("mw.fail", defaults(style = Primary, label = "Fail"))]
struct FailButton;

impl FailButton {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, level: Ext<Level>) -> HandlerResult<Responded> {
        see(ctx.state(), format!("fail {}", level.0));
        Err(HandlerError::new(std::io::Error::other("disk full")))
    }
}

/// An event handler reading the run's `Level`.
#[derive(EventHandler)]
struct OnMessage;

impl OnMessage {
    #[handler]
    async fn handle(
        self,
        ctx: EventCtx,
        msg: MessageCreate,
        level: Ext<Level>,
    ) -> HandlerResult<()> {
        see(ctx.state(), format!("event {} {}", msg.content, level.0));
        Ok(())
    }
}

/// Stops `stop` messages with a key and fails `fail` messages.
async fn event_gate(ctx: BeforeCtx) -> HandlerResult<Flow> {
    see(ctx.state(), "event_gate");
    let content = ctx
        .get::<Content>()
        .map(|c| c.0.clone())
        .unwrap_or_default();
    match content.as_str() {
        "stop" => Ok(Flow::Stop(Some(DENIED))),
        "fail" => Err(HandlerError::new(std::io::Error::other("gate down"))),
        _ => Ok(Flow::Continue),
    }
}

/// The message content, as the root Middleware saw it.
struct Content(String);

/// Inserts the content of the message the run answers, read from `State`
/// (Middleware see no event payload).
async fn content(ctx: BeforeCtx) -> HandlerResult<Flow> {
    let next = ctx.state::<Arc<Next>>().unwrap();
    let text = next.0.lock().unwrap().clone();
    ctx.insert(Content(text));
    Ok(Flow::Continue)
}

/// The content of the next dispatched message.
#[derive(Default)]
struct Next(Mutex<String>);

/// Runs `!<path>` through the Dispatcher.
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
            );
            ctx.dispatcher().run_command(invocation).await?;
        }
        Ok(())
    }
}

/// Records every report as `<handler>:<kind>:<middleware>:<extensions>`.
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Vec<String>>>);

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => error.to_string(),
            other => format!("{other:?}"),
        };
        let extensions = report
            .extensions
            .map(|e| e.get::<Level>().map_or(0, |l| l.0));
        self.0.lock().unwrap().push(format!(
            "{}:{kind}:{}:{extensions:?}",
            report
                .handler
                .unwrap_or_default()
                .rsplit("::")
                .next()
                .unwrap_or_default(),
            report
                .middleware
                .unwrap_or("-")
                .rsplit("::")
                .next()
                .unwrap_or_default(),
        ));
    }
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    seen: Arc<Seen>,
    next: Arc<Next>,
    reports: Reports,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(module: Module, configure: impl FnOnce(&mut AppBuilder)) -> Self {
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
        let seen = Arc::new(Seen::default());
        let next = Arc::new(Next::default());
        let reports = Reports::default();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .state(Arc::clone(&seen))
            .state(Arc::clone(&next))
            .module(module);
        for origin in ErrorOrigin::ALL {
            builder.error_sink(origin, reports.clone());
        }
        configure(&mut builder);
        let app = builder.build().unwrap();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let running = Self {
            gw,
            http,
            seen,
            next,
            reports,
            run,
        };
        let deadline = Instant::now() + WAIT;
        while running.gw.dispatch("TYPING_START", json!({})) == 0 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        running
    }

    /// Dispatches a chat-input interaction for `name` with one subcommand
    /// `sub` when given (id 100, token `tok`, application 7, a tr user).
    fn invoke(&self, name: &str, sub: Option<&str>) {
        let mut data = json!({ "id": "9", "name": name, "type": 1 });
        if let Some(sub) = sub {
            data["options"] = json!([{ "type": 1, "name": sub, "options": [] }]);
        }
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
            "data": data,
        });
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
    }

    /// Dispatches a click on `custom_id` (id 100, token `tok`).
    fn click(&self, custom_id: &str) {
        let d = json!({
            "id": "100",
            "application_id": "7",
            "type": 3,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "locale": "tr",
            "message": {
                "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
                "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
                "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
                "mention_everyone": false, "mentions": [], "mention_roles": [],
                "attachments": [], "embeds": [], "pinned": false, "type": 0, "components": [],
            },
            "data": { "custom_id": custom_id, "component_type": 2 },
        });
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
    }

    /// Dispatches a `MESSAGE_CREATE` with `content` in channel 5.
    fn message(&self, content: &str) {
        content.clone_into(&mut self.next.0.lock().unwrap());
        let d = json!({
            "id": "6", "channel_id": "5", "author": user(), "content": content,
            "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0
        });
        assert_eq!(self.gw.dispatch("MESSAGE_CREATE", d), 1);
    }

    /// The requests other than `GET /gateway/bot`.
    fn sent(&self) -> Vec<RecordedRequest> {
        self.http
            .requests()
            .into_iter()
            .filter(|r| r.route_path() != Some("/gateway/bot"))
            .collect()
    }

    fn clear(&self) {
        self.http.clear_requests();
    }

    /// Waits for `n` requests, then a little longer, and asserts there
    /// were exactly `n`.
    async fn exactly(&self, n: usize) -> Vec<RecordedRequest> {
        let deadline = Instant::now() + WAIT;
        while self.sent().len() < n {
            assert!(
                Instant::now() < deadline,
                "{n} requests missing: {:?}",
                self.sent()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        let sent = self.sent();
        assert_eq!(sent.len(), n, "{sent:?}");
        sent
    }

    /// Polls until the log holds `n` lines, then returns them.
    async fn wait_seen(&self, n: usize) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let lines = self.seen.lines();
            if lines.len() >= n {
                return lines;
            }
            assert!(Instant::now() < deadline, "{n} lines missing: {lines:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Polls until the sinks hold `n` reports, then returns them.
    async fn wait_reports(&self, n: usize) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let reports = self.reports.0.lock().unwrap().clone();
            if reports.len() >= n {
                return reports;
            }
            assert!(
                Instant::now() < deadline,
                "{n} reports missing: {reports:?}"
            );
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

/// The `custom_id` of the unit Route `route`.
fn id_of(route: &str) -> String {
    format!("0{route}:")
}

/// Matrix 24: a `Stop(Some(k))` before the deadline is one ephemeral type-4
/// callback with `k`'s text in the user's locale (tr resolves to the
/// built-in en-US text, LOC-04); the handler does not run.
#[tokio::test(flavor = "multi_thread")]
async fn stop_notice_is_ephemeral_reply() {
    let running = Running::start(
        Module::new("m")
            .before(Middleware::before(gate))
            .add::<Gated>(),
        |_| {},
    )
    .await;
    running.invoke("deny", None);
    let sent = running.exactly(1).await;
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert!(sent[0].headers.get("authorization").is_none());
    let callback = body(&sent[0]);
    assert_eq!(callback["type"], 4, "{callback}");
    assert_eq!(callback["data"]["flags"], 32832, "{callback}");
    assert_eq!(callback["data"]["components"][0]["content"], DENIED_TEXT);
    assert_eq!(running.seen.lines(), ["gate deny"]);
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

/// Matrix 25: a Middleware past the ack deadline gets the type-5 defer;
/// its `Stop(Some(k))` deletes the original, then follows up ephemerally.
/// A component (`response = update`) gets type 6 and only the follow-up.
#[tokio::test(flavor = "multi_thread")]
async fn stop_after_defer_deletes_original_then_follows_up() {
    let running = Running::start(
        Module::new("m")
            .before(Middleware::before(gate))
            .add::<Slow>()
            .module(
                Module::new("m/ui")
                    .before(Middleware::before(slow_click).provides::<Level>())
                    .add::<LevelButton>(),
            ),
        |b| {
            b.ack_deadline(Duration::from_millis(50));
        },
    )
    .await;
    running.invoke("slow", Some("deny"));
    let sent = running.exactly(3).await;
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert_eq!(body(&sent[0]), json!({ "type": 5 }));
    assert_eq!(sent[1].method, Method::DELETE);
    assert_eq!(sent[1].route_path(), Some(ORIGINAL));
    assert_eq!(sent[2].method, Method::POST);
    assert_eq!(sent[2].route_path(), Some(FOLLOWUP));
    assert_eq!(body(&sent[2])["flags"], 32832);
    assert_eq!(body(&sent[2])["components"][0]["content"], DENIED_TEXT);
    running.clear();
    running.click(&id_of("mw.level"));
    let sent = running.exactly(2).await;
    assert_eq!(body(&sent[0]), json!({ "type": 6 }));
    assert_eq!(sent[1].method, Method::POST);
    assert_eq!(sent[1].route_path(), Some(FOLLOWUP));
    assert_eq!(body(&sent[1])["flags"], 32832);
    assert_eq!(body(&sent[1])["components"][0]["content"], DENIED_TEXT);
    assert_eq!(running.seen.lines(), ["gate slow deny", "slow click"]);
    running.run.abort();
}

/// Matrix 26: a pending `Stop(None)` sends nothing; after a type-5 defer
/// it deletes the original and sends nothing else.
#[tokio::test(flavor = "multi_thread")]
async fn silent_stop_after_defer_deletes_original() {
    let running = Running::start(
        Module::new("m")
            .before(Middleware::before(gate))
            .add::<Gated>()
            .add::<Slow>(),
        |b| {
            b.ack_deadline(Duration::from_millis(50));
        },
    )
    .await;
    running.invoke("quiet", None);
    running.wait_seen(1).await;
    assert!(running.exactly(0).await.is_empty());
    running.invoke("slow", Some("quiet"));
    let sent = running.exactly(2).await;
    assert_eq!(body(&sent[0]), json!({ "type": 5 }));
    assert_eq!(sent[1].method, Method::DELETE);
    assert_eq!(sent[1].route_path(), Some(ORIGINAL));
    assert_eq!(running.seen.lines(), ["gate quiet", "gate slow quiet"]);
    running.run.abort();
}

/// Matrix 32: an event runs the chain in order with its Extensions; a
/// `Stop(Some(k))` sends nothing; an error goes to the Event sink naming
/// the Middleware.
#[tokio::test(flavor = "multi_thread")]
async fn event_middleware_order_and_extensions() {
    let running = Running::start(
        Module::new("root")
            .before(Middleware::before(content))
            .before(Middleware::before(r1).events_only().provides::<Level>())
            .after(Middleware::after(a1))
            .module(
                Module::new("root/child")
                    .before(Middleware::before(event_gate).events_only())
                    .after(Middleware::after(ca1))
                    .add::<OnMessage>(),
            ),
        |_| {},
    )
    .await;
    running.message("hi");
    assert_eq!(
        running.wait_seen(5).await,
        [
            "r1",
            "event_gate",
            "event hi 1",
            "ca1:Completed:1",
            "a1:Completed:1"
        ]
    );
    running.message("stop");
    assert_eq!(
        running.wait_seen(9).await[5..],
        ["r1", "event_gate", "ca1:Stopped:1", "a1:Stopped:1"]
    );
    running.message("fail");
    assert_eq!(
        running.wait_seen(13).await[9..],
        ["r1", "event_gate", "ca1:Failed:1", "a1:Failed:1"]
    );
    assert_eq!(
        running.wait_reports(1).await,
        ["OnMessage:gate down:event_gate:Some(1)"]
    );
    assert!(running.exactly(0).await.is_empty());
    running.run.abort();
}

/// AC1 over a slash Command: parent-before, child-before, handler,
/// child-after, parent-after; a parent before-Middleware's stop skips the
/// child's Middleware and the handler.
#[tokio::test(flavor = "multi_thread")]
async fn slash_command_parent_child_order_and_stop() {
    let running = Running::start(
        Module::new("root")
            .before(Middleware::before(r1).provides::<Level>())
            .before(Middleware::before(r2))
            .before(Middleware::before(gate))
            .after(Middleware::after(a1))
            .after(Middleware::after(a2))
            .module(
                Module::new("root/child")
                    .before(Middleware::before(c1))
                    .after(Middleware::after(ca1))
                    .add::<Leveled>()
                    .add::<Gated>(),
            ),
        |_| {},
    )
    .await;
    running.invoke("level", None);
    assert_eq!(
        running.wait_seen(8).await,
        [
            "r1",
            "r2",
            "gate level",
            "c1",
            "handler level 2",
            "ca1:Completed:2",
            "a2:Completed:2",
            "a1:Completed:2"
        ]
    );
    let sent = running.exactly(1).await;
    assert_eq!(sent[0].route_path(), Some(CALLBACK));
    assert_eq!(body(&sent[0])["data"]["content"], "ran");
    running.clear();
    running.invoke("deny", None);
    assert_eq!(
        running.wait_seen(14).await[8..],
        [
            "r1",
            "r2",
            "gate deny",
            "ca1:Stopped:1",
            "a2:Stopped:1",
            "a1:Stopped:1"
        ]
    );
    let sent = running.exactly(1).await;
    assert_eq!(
        body(&sent[0])["data"]["components"][0]["content"],
        DENIED_TEXT
    );
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.run.abort();
}

/// Matrix 35: a Dispatcher Command runs its Module's Middleware with
/// `Invocation::Command`; a `Stop(Some(k))` is a public reply notice to
/// the message; a Command taking `Ext<T>` sees the inserted value.
#[tokio::test(flavor = "multi_thread")]
async fn dispatcher_runs_middleware() {
    let running = Running::start(
        Module::new("m")
            .before(Middleware::before(gate).skip_events())
            .before(
                Middleware::before(provide)
                    .skip_events()
                    .provides::<Level>(),
            )
            .add::<Gated>()
            .add::<Leveled>()
            .add::<Prefix>(),
        |_| {},
    )
    .await;
    running.message("!deny");
    let sent = running.exactly(1).await;
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].route_path(), Some(CHANNEL));
    let reply = body(&sent[0]);
    assert_eq!(reply["flags"], 32768, "{reply}");
    assert_eq!(reply["components"][0]["content"], DENIED_TEXT);
    assert_eq!(
        reply["message_reference"],
        json!({ "message_id": "6", "fail_if_not_exists": false })
    );
    running.message("!open");
    let sent = running.exactly(2).await;
    assert_eq!(body(&sent[1])["content"], "ran");
    running.message("!level");
    let sent = running.exactly(3).await;
    assert_eq!(sent[2].route_path(), Some(CHANNEL));
    assert_eq!(body(&sent[2])["content"], "ran");
    assert_eq!(
        running.seen.lines(),
        [
            "gate deny",
            "gate open",
            "handler open",
            "gate level",
            "handler level 7"
        ]
    );
    running.run.abort();
}

/// Matrix 21 and 36: over a click, the chain runs parent-before,
/// child-before, handler, child-after, parent-after (after in reverse); the
/// child's insert wins, and the after-Middleware and the sink's
/// `ErrorReport.extensions` see it.
#[tokio::test(flavor = "multi_thread")]
async fn extension_reaches_handler_and_after() {
    let running = Running::start(
        Module::new("root")
            .before(Middleware::before(r1).provides::<Level>())
            .before(Middleware::before(r2))
            .after(Middleware::after(a1))
            .after(Middleware::after(a2))
            .module(
                Module::new("root/child")
                    .before(Middleware::before(c1))
                    .after(Middleware::after(ca1))
                    .add::<LevelButton>()
                    .add::<FailButton>(),
            ),
        |_| {},
    )
    .await;
    running.click(&id_of("mw.level"));
    assert_eq!(
        running.wait_seen(7).await,
        [
            "r1",
            "r2",
            "c1",
            "handler 2",
            "ca1:Completed:2",
            "a2:Completed:2",
            "a1:Completed:2"
        ]
    );
    assert!(running.exactly(0).await.is_empty());
    running.click(&id_of("mw.fail"));
    assert_eq!(
        running.wait_seen(14).await[7..],
        [
            "r1",
            "r2",
            "c1",
            "fail 2",
            "ca1:Failed:2",
            "a2:Failed:2",
            "a1:Failed:2"
        ]
    );
    assert_eq!(
        running.wait_reports(1).await,
        ["FailButton:disk full:-:Some(2)"]
    );
    running.run.abort();
}
