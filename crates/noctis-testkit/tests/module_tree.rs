//! Story 13.1 end to end: a 3-level Module tree (`ops` → `ops/mod` →
//! `ops/mod/audit`) built through the facade with real derives. A slash
//! Command and a `MESSAGE_DELETE` reach the leaf's handlers through every
//! level's Middleware (parent before child, after-Middleware in reverse),
//! and each failure's `ErrorReport.module` and span `module` field carry the
//! full-path Module id. Its own test binary, because it installs a global
//! capturing tracing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::events::{EventCtx, MessageDelete};
use noctis::prelude::*;
use noctis::ui::LegacyMessage;
use noctis::{
    AfterCtx, AfterMiddleware, BeforeMiddleware, BuildViolation, ErrorReport, Runtime, Token,
};
use noctis_testkit::fake::StatusCode;
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method};
use serde_json::{Value, json};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const CALLBACK: &str = "/interactions/100/tok/callback";
const ORIGINAL: &str = "/webhooks/7/tok/messages/@original";
const FOLLOWUP: &str = "/webhooks/7/tok";

/// The leaf Module's id.
const LEAF: &str = "ops/mod/audit";

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn lines_with(&self, needles: &[&str]) -> Vec<String> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|line| needles.iter().all(|n| line.contains(n)))
            .map(str::to_owned)
            .collect()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

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

/// A before-Middleware logging `before <id>`.
fn before(id: &'static str) -> BeforeMiddleware {
    Middleware::before(move |ctx: BeforeCtx| async move {
        see(ctx.state(), format!("before {id}"));
        Ok(Flow::Continue)
    })
}

/// An after-Middleware logging `after <id>`.
fn after(id: &'static str) -> AfterMiddleware {
    Middleware::after(move |ctx: AfterCtx| async move {
        see(ctx.state(), format!("after {id}"));
    })
}

/// The leaf's slash Command: records its run and fails.
#[derive(SlashCommand)]
#[command("audit", description = "Audit")]
struct Audit;

impl Audit {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
        see(ctx.state(), "handler command");
        Err(HandlerError::new(io::Error::other("audit down")))
    }
}

/// The leaf's `MESSAGE_DELETE` handler: records its run and fails.
#[derive(EventHandler)]
struct OnDelete;

impl OnDelete {
    #[handler]
    async fn handle(self, ctx: EventCtx, _ev: MessageDelete) -> HandlerResult<()> {
        see(ctx.state(), "handler event");
        Err(HandlerError::new(io::Error::other("log down")))
    }
}

/// One recorded report: `(origin, module, handler)`.
type Report = (ErrorOrigin, Option<String>, String);

/// Records every report.
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Vec<Report>>>);

impl Reports {
    fn all(&self) -> Vec<Report> {
        self.0.lock().unwrap().clone()
    }
}

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let handler = report
            .handler
            .unwrap_or_default()
            .rsplit("::")
            .next()
            .unwrap_or_default()
            .to_owned();
        self.0
            .lock()
            .unwrap()
            .push((report.origin, report.module.map(str::to_owned), handler));
    }
}

/// The tree: one before- and one after-Middleware per level, the handlers
/// in the leaf.
fn tree() -> Module {
    Module::new("ops")
        .before(before("ops"))
        .after(after("ops"))
        .module(
            Module::new("ops/mod")
                .before(before("ops/mod"))
                .after(after("ops/mod"))
                .module(
                    Module::new(LEAF)
                        .before(before(LEAF))
                        .after(after(LEAF))
                        .add::<Audit>()
                        .add::<OnDelete>(),
                ),
        )
}

/// The log of one run through every level, around `handler`.
fn chain(handler: &str) -> Vec<String> {
    [
        "before ops",
        "before ops/mod",
        "before ops/mod/audit",
        handler,
        "after ops/mod/audit",
        "after ops/mod",
        "after ops",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Polls `cond` within [`WAIT`].
async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// AC1: registrations of every level fold, Middleware runs parent before
/// child at depth 3, and failures carry the full-path Module id in the
/// Error Sink report and the span.
#[tokio::test(flavor = "multi_thread")]
async fn three_level_tree_end_to_end() {
    let capture = Capture::default();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_writer(capture.clone())
        .init();

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
    );
    let gw = FakeGateway::start().await.unwrap();
    let seen = Arc::new(Seen::default());
    let reports = Reports::default();
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .state(Arc::clone(&seen))
        .module(tree());
    for origin in ErrorOrigin::ALL {
        builder.error_sink(origin, reports.clone());
    }
    let app = builder.build().unwrap();
    let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
    let deadline = Instant::now() + WAIT;
    while gw.dispatch("TYPING_START", json!({})) == 0 {
        assert!(Instant::now() < deadline, "the shard has no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The slash Command.
    let command = json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "locale": "en-US",
        "data": { "id": "9", "name": "audit", "type": 1 },
    });
    assert_eq!(gw.dispatch("INTERACTION_CREATE", command), 1);
    until("the command chain", || seen.lines().len() >= 7).await;
    assert_eq!(seen.lines(), chain("handler command"));

    // The event.
    let delete = json!({ "id": "6", "channel_id": "5", "guild_id": "8" });
    assert_eq!(gw.dispatch("MESSAGE_DELETE", delete), 1);
    until("the event chain", || seen.lines().len() >= 14).await;
    assert_eq!(seen.lines()[7..], chain("handler event"));

    let has = |origin: ErrorOrigin, handler: &str| {
        reports
            .all()
            .iter()
            .any(|(o, _, h)| *o == origin && h == handler)
    };
    until("both reports", || {
        has(ErrorOrigin::Command, "Audit") && has(ErrorOrigin::Event, "OnDelete")
    })
    .await;
    for (origin, module, handler) in reports.all() {
        if handler == "Audit" || handler == "OnDelete" {
            assert_eq!(module.as_deref(), Some(LEAF), "{origin:?} {handler}");
        }
    }

    // Spans: the leaf's full path is the `module` field of both.
    let field = format!("module=\"{LEAF}\"");
    until("the spans", || {
        !capture
            .lines_with(&["noctis.interaction", &field])
            .is_empty()
            && !capture.lines_with(&["noctis.event", &field]).is_empty()
    })
    .await;
    run.abort();
}

/// AC2: two mounted roots with one id fail `App::build` with a
/// `DuplicateModule` naming it.
#[tokio::test]
async fn duplicate_root_fails_build() {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .module(Module::new("ops"))
        .module(Module::new("ops"));
    let err = builder.build().unwrap_err();
    assert!(
        err.violations()
            .contains(&BuildViolation::DuplicateModule { id: "ops" }),
        "{err}"
    );
}
