//! Story 8.1: `ctx.respond`, `ctx.update` and `ctx.reply_text` end to end
//! (FR-61, UD-3; INT-03, INT-04, INT-12, MSG-05). A `FakeGateway`
//! `INTERACTION_CREATE` reaches the pipeline; every response is asserted on
//! the `FakeDiscordHttp` wire (method, path, headers, body).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::__private::codec::{Parts, encode};
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};
use noctis::{AppBuilder, ErrorReport, ReportKind, Runtime, Token};
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
/// `noctis.error.generic` in en-US.
const GENERIC: &str = "Something went wrong. Please try again later.";
/// `IS_COMPONENTS_V2`.
const V2: u64 = 32768;
/// `IS_COMPONENTS_V2 | EPHEMERAL`.
const V2_EPHEMERAL: u64 = 32832;

fn view(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

/// The ping View: a plain render function.
fn render_ping() -> V2Message {
    view("pong")
}

// --- The structs -------------------------------------------------------------

/// An option-less Command: a unit struct.
#[derive(SlashCommand)]
#[command("ping", description = "Check that the bot answers")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render_ping()).await
    }
}

/// An ephemeral Command that answers after `ms`.
#[derive(SlashCommand)]
#[command("secret", description = "A private answer", response = ephemeral)]
struct Secret;

impl Secret {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctx.respond(view("psst")).await
    }
}

/// A notice: ephemeral by default.
#[derive(SlashCommand)]
#[command("quiet", description = "An ephemeral notice")]
struct Quiet;

impl Quiet {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctx.reply_text("Saved.").await
    }
}

/// A public notice.
#[derive(SlashCommand)]
#[command("loud", description = "A public notice")]
struct Loud;

impl Loud {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text("Saved.").public().await
    }
}

/// `n` = 0 updates at once, else after 200 ms.
#[derive(Button, Debug, Clone, Copy)]
#[button("bump", defaults(style = Primary, label = "Bump"))]
struct Bump {
    n: u8,
}

impl Bump {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if self.n > 0 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        ctx.update(view("bumped")).await
    }
}

/// Answers through the ctx, then tries the typestate (row 38).
#[derive(Button, Debug, Clone, Copy)]
#[button("mixed", defaults(style = Primary, label = "Mixed"))]
struct Mixed;

impl Mixed {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        let _ = ctx.respond(view("first")).await?;
        Ok(r.reply(view("second")).await?.done())
    }
}

/// A modal whose handler updates its source message after `ms`.
#[derive(Modal, Debug)]
#[modal("note", defaults(title = "Note"))]
struct Note {
    ms: u64,
    #[text_input("Note")]
    #[allow(dead_code, reason = "the submitted input is decoded, not read")]
    note: Option<String>,
}

impl Note {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        ctx.update(view("noted")).await
    }
}

/// The Limits the C1 App is built with (Story 8.3).
fn custom_limits() -> noctis::rules::Limits {
    let mut limits = noctis::rules::Limits::default();
    limits.components = 30;
    limits
}

/// What a handler answers about `ctx.limits()`.
fn limits_view(limits: &noctis::rules::Limits) -> V2Message {
    view(if limits == &custom_limits() {
        "limits: app"
    } else {
        "limits: other"
    })
}

/// Answers whether `ctx.limits()` is the App's (C1).
#[derive(SlashCommand)]
#[command("limits", description = "Show the Limits")]
struct LimitsCommand;

impl LimitsCommand {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let msg = limits_view(ctx.limits());
        ctx.respond(msg).await
    }
}

/// Answers whether `ctx.limits()` is the App's (C1).
#[derive(Button, Debug, Clone, Copy)]
#[button("limits.button", defaults(style = Primary, label = "Limits"))]
struct LimitsButton;

impl LimitsButton {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let msg = limits_view(ctx.limits());
        ctx.update(msg).await
    }
}

/// Answers whether `ctx.limits()` is the App's (C1).
#[derive(Modal, Debug)]
#[modal("limits.modal", defaults(title = "Limits"))]
struct LimitsModal {
    #[text_input("Note")]
    #[allow(dead_code, reason = "the submitted input is decoded, not read")]
    note: Option<String>,
}

impl LimitsModal {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        let msg = limits_view(ctx.limits());
        ctx.respond(msg).await
    }
}

/// Records every report as its error text.
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Vec<String>>>);

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => error.to_string(),
            other => format!("{other:?}"),
        };
        self.0.lock().unwrap().push(kind);
    }
}

// --- The running App ---------------------------------------------------------

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    reports: Reports,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    /// The App with `ack_deadline`, every test struct mounted.
    async fn start(ack_deadline: Duration) -> Self {
        Self::start_with(|builder| {
            builder.ack_deadline(ack_deadline);
        })
        .await
    }

    async fn start_with(configure: impl FnOnce(&mut AppBuilder)) -> Self {
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
            FakeResponse::json(StatusCode::OK, &json!({ "id": "77", "channel_id": "55" })),
        )
        .route(
            Method::POST,
            FOLLOWUP,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "56" })),
        )
        .route(
            Method::DELETE,
            ORIGINAL,
            FakeResponse::empty(StatusCode::NO_CONTENT),
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
            .error_sink(ErrorOrigin::Component, reports.clone())
            .module(
                Module::new("ergo")
                    .add::<Ping>()
                    .add::<Secret>()
                    .add::<Quiet>()
                    .add::<Loud>()
                    .add::<Bump>()
                    .add::<Mixed>()
                    .add::<Note>()
                    .add::<LimitsCommand>()
                    .add::<LimitsButton>()
                    .add::<LimitsModal>(),
            );
        configure(&mut builder);
        let app = builder.build().unwrap();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let running = Self {
            gw,
            http,
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

    fn dispatch(&self, d: Value) {
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
    }

    /// Dispatches a chat-input interaction for `name`.
    fn invoke(&self, name: &str) {
        self.dispatch(json!({
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
        }));
    }

    /// The requests other than `GET /gateway/bot`.
    fn sent(&self) -> Vec<RecordedRequest> {
        self.http
            .requests()
            .into_iter()
            .filter(|r| r.route_path() != Some("/gateway/bot"))
            .collect()
    }

    /// Polls until `n` requests were sent, then checks that no more arrive
    /// for a moment; every one is an interaction request. Clears them.
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
        tokio::time::sleep(Duration::from_millis(300)).await;
        let sent = self.sent();
        assert_eq!(sent.len(), n, "{sent:?}");
        for request in &sent {
            assert_interaction_request(request);
        }
        self.http.clear_requests();
        sent
    }

    /// Polls until a report arrived; takes them.
    async fn wait_reports(&self) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let reports = std::mem::take(&mut *self.reports.0.lock().unwrap());
            if !reports.is_empty() {
                return reports;
            }
            assert!(Instant::now() < deadline, "no report");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn stop(self) {
        self.run.abort();
    }
}

// --- Wire helpers ------------------------------------------------------------

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn body(request: &RecordedRequest) -> Value {
    request.json().unwrap_or(Value::Null)
}

/// No bot token and no `with_components` (INT-13).
fn assert_interaction_request(request: &RecordedRequest) {
    assert!(
        request.headers.get("authorization").is_none(),
        "{request:?}"
    );
    assert!(
        !request
            .query
            .as_deref()
            .unwrap_or("")
            .contains("with_components"),
        "{request:?}"
    );
}

fn route(request: &RecordedRequest) -> (Method, &str) {
    (request.method.clone(), request.route_path().unwrap_or(""))
}

/// A complete V2 message 77 in channel 55.
fn source_message() -> Value {
    json!({
        "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "components": [],
    })
}

/// A button click of `custom_id` on message 77.
fn click(custom_id: &str) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": 3,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "channel": { "id": "55", "type": 0 },
        "message": source_message(),
        "data": { "custom_id": custom_id, "component_type": 2 },
    })
}

/// A submit of [`Note`] without a source message (opened from a slash
/// command).
fn slash_opened_submit(ms: u64) -> Value {
    let modal = serde_json::to_value(noctis::ui::Modal::from(Note::blank(ms))).unwrap();
    json!({
        "id": "100",
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "channel": { "id": "55", "type": 0 },
        "data": {
            "custom_id": modal["custom_id"],
            "components": [{ "type": 18, "id": 1, "component":
                { "type": 4, "id": 2, "custom_id": "note", "value": "n" } }],
        },
    })
}

// --- The tests ---------------------------------------------------------------

/// FR-61, UD-3: an option-less unit Command answering with
/// `ctx.respond(render_ping())` sends exactly one type-4 callback with the
/// V2 flag, pinging nobody.
#[tokio::test(flavor = "multi_thread")]
async fn option_less_command_responds() {
    let running = Running::start(Duration::from_secs(2)).await;
    running.invoke("ping");
    let sent = running.exactly(1).await;
    assert_eq!(route(&sent[0]), (Method::POST, CALLBACK));
    let reply = body(&sent[0]);
    assert_eq!(reply["type"], 4, "{reply}");
    assert_eq!(reply["data"]["flags"], V2, "{reply}");
    assert_eq!(reply["data"]["allowed_mentions"], json!({ "parse": [] }));
    assert_eq!(reply["data"]["components"][0]["content"], "pong");
    running.stop();
}

/// Rows 2 and 3: an ephemeral Command's `respond` is type 4 with
/// `EPHEMERAL`; after the type-5 auto-defer it edits `@original`.
#[tokio::test(flavor = "multi_thread")]
async fn ephemeral_command_and_auto_defer() {
    let running = Running::start(Duration::from_secs(2)).await;
    running.invoke("secret");
    let sent = running.exactly(1).await;
    let reply = body(&sent[0]);
    assert_eq!(reply["type"], 4, "{reply}");
    assert_eq!(reply["data"]["flags"], V2_EPHEMERAL, "{reply}");
    running.stop();

    let running = Running::start(Duration::ZERO).await;
    running.invoke("secret");
    let sent = running.exactly(2).await;
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    assert_eq!(route(&sent[1]), (Method::PATCH, ORIGINAL));
    let edit = body(&sent[1]);
    assert_eq!(edit["flags"], V2, "{edit}");
    assert_eq!(edit["components"][0]["content"], "psst");
    assert_eq!(edit["attachments"], json!([]));
    running.stop();
}

/// Rows 15 and 17: `ctx.update` is one type-7 callback while unanswered,
/// and a `PATCH @original` after the type-6 auto-defer; never `EPHEMERAL`.
#[tokio::test(flavor = "multi_thread")]
async fn button_update_and_type_6() {
    let running = Running::start(Duration::from_secs(2)).await;
    running.dispatch(click(
        encode::<Bump>(&Bump { n: 0 }, &Parts::new())
            .unwrap()
            .as_ref(),
    ));
    let sent = running.exactly(1).await;
    assert_eq!(route(&sent[0]), (Method::POST, CALLBACK));
    let update = body(&sent[0]);
    assert_eq!(update["type"], 7, "{update}");
    assert_eq!(update["data"]["flags"], V2, "{update}");
    assert_eq!(update["data"]["components"][0]["content"], "bumped");
    running.stop();

    let running = Running::start(Duration::ZERO).await;
    running.dispatch(click(
        encode::<Bump>(&Bump { n: 1 }, &Parts::new())
            .unwrap()
            .as_ref(),
    ));
    let sent = running.exactly(2).await;
    assert_eq!(body(&sent[0]), json!({ "type": 6 }));
    assert_eq!(route(&sent[1]), (Method::PATCH, ORIGINAL));
    let edit = body(&sent[1]);
    assert_eq!(edit["flags"], V2, "{edit}");
    assert_eq!(edit["components"][0]["content"], "bumped");
    running.stop();
}

/// Rows 25, 26 and 28: one Text Display, ephemeral by default, public with
/// `.public()`; after a public type-5 auto-defer the loading message is
/// deleted and the ephemeral text follows up.
#[tokio::test(flavor = "multi_thread")]
async fn reply_text_variants() {
    let running = Running::start(Duration::from_secs(2)).await;
    running.invoke("quiet");
    let sent = running.exactly(1).await;
    let notice = body(&sent[0]);
    assert_eq!(notice["type"], 4, "{notice}");
    assert_eq!(notice["data"]["flags"], V2_EPHEMERAL, "{notice}");
    assert_eq!(
        notice["data"]["components"],
        json!([{ "type": 10, "id": 1, "content": "Saved." }])
    );
    assert_eq!(notice["data"]["allowed_mentions"], json!({ "parse": [] }));
    running.invoke("loud");
    let sent = running.exactly(1).await;
    assert_eq!(body(&sent[0])["data"]["flags"], V2);
    running.stop();

    let running = Running::start(Duration::ZERO).await;
    running.invoke("quiet");
    let sent = running.exactly(3).await;
    assert_eq!(body(&sent[0]), json!({ "type": 5 }));
    assert_eq!(route(&sent[1]), (Method::DELETE, ORIGINAL));
    assert_eq!(route(&sent[2]), (Method::POST, FOLLOWUP));
    assert_eq!(sent[2].query.as_deref(), Some("wait=true"));
    let follow_up = body(&sent[2]);
    assert_eq!(follow_up["flags"], V2_EPHEMERAL, "{follow_up}");
    assert_eq!(follow_up["components"][0]["content"], "Saved.");
    running.stop();
}

/// Rows 19 and 20 (INT-04): `ctx.update` on a slash-opened submit sends no
/// type 7 and no `PATCH @original`, with or without the auto-defer; the
/// Component Error Sink gets `UpdateUnavailable` and the generic notice is
/// the only response.
#[tokio::test(flavor = "multi_thread")]
async fn slash_opened_submit_update_unavailable() {
    let unavailable = "there is no message to update";
    let running = Running::start(Duration::from_secs(2)).await;
    running.dispatch(slash_opened_submit(0));
    let sent = running.exactly(1).await;
    let notice = body(&sent[0]);
    assert_eq!(notice["type"], 4, "{notice}");
    assert_eq!(notice["data"]["flags"], V2_EPHEMERAL, "{notice}");
    assert_eq!(notice["data"]["components"][0]["content"], GENERIC);
    let reports = running.wait_reports().await;
    assert!(reports[0].contains(unavailable), "{reports:?}");
    running.stop();

    let running = Running::start(Duration::ZERO).await;
    running.dispatch(slash_opened_submit(100));
    let sent = running.exactly(3).await;
    assert_eq!(body(&sent[0]), json!({ "type": 5 }));
    assert_eq!(route(&sent[1]), (Method::DELETE, ORIGINAL));
    assert_eq!(route(&sent[2]), (Method::POST, FOLLOWUP));
    assert_eq!(body(&sent[2])["components"][0]["content"], GENERIC);
    for request in &sent {
        assert_ne!(request.method, Method::PATCH, "{request:?}");
        assert_ne!(body(request)["type"], 7, "{request:?}");
    }
    let reports = running.wait_reports().await;
    assert!(reports[0].contains(unavailable), "{reports:?}");
    running.stop();
}

/// Row 38 (INT-03): after `ctx.respond`, the typestate `reply` sends
/// nothing: exactly one callback, then only the error notice, and the
/// Component Error Sink gets `AlreadyResponded`.
#[tokio::test(flavor = "multi_thread")]
async fn mixed_already_responded() {
    let running = Running::start(Duration::from_secs(2)).await;
    running.dispatch(click(
        encode::<Mixed>(&Mixed, &Parts::new()).unwrap().as_ref(),
    ));
    let sent = running.exactly(2).await;
    let callbacks: Vec<_> = sent
        .iter()
        .filter(|r| r.route_path() == Some(CALLBACK))
        .collect();
    assert_eq!(callbacks.len(), 1, "{sent:?}");
    let reply = body(callbacks[0]);
    assert_eq!(reply["type"], 4, "{reply}");
    assert_eq!(reply["data"]["components"][0]["content"], "first");
    // The failed run's notice follows up; "second" is never sent.
    assert_eq!(route(&sent[1]), (Method::POST, FOLLOWUP));
    assert_eq!(body(&sent[1])["components"][0]["content"], GENERIC);
    let reports = running.wait_reports().await;
    assert!(
        reports[0].contains("already sent its initial response"),
        "{reports:?}"
    );
    running.stop();
}

/// C1 (Story 8.3): a command, a component and a modal-submit handler on an
/// App built with `.limits(l)` each read `ctx.limits() == &l`.
#[tokio::test(flavor = "multi_thread")]
async fn ctx_limits_are_the_apps() {
    let running = Running::start_with(|builder| {
        builder.limits(custom_limits());
    })
    .await;
    running.invoke("limits");
    let sent = running.exactly(1).await;
    assert_eq!(
        body(&sent[0])["data"]["components"][0]["content"],
        "limits: app"
    );

    running.dispatch(click(
        encode::<LimitsButton>(&LimitsButton, &Parts::new())
            .unwrap()
            .as_ref(),
    ));
    let sent = running.exactly(1).await;
    assert_eq!(
        body(&sent[0])["data"]["components"][0]["content"],
        "limits: app"
    );

    let modal = serde_json::to_value(noctis::ui::Modal::from(LimitsModal::blank())).unwrap();
    running.dispatch(json!({
        "id": "100",
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "channel": { "id": "55", "type": 0 },
        "data": {
            "custom_id": modal["custom_id"],
            "components": [{ "type": 18, "id": 1, "component":
                { "type": 4, "id": 2, "custom_id": "note", "value": "n" } }],
        },
    }));
    let sent = running.exactly(1).await;
    assert_eq!(
        body(&sent[0])["data"]["components"][0]["content"],
        "limits: app"
    );
    running.stop();
}
