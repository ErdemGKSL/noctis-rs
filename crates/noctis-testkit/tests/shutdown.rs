//! Graceful shutdown of a running App against `FakeDiscordHttp` and
//! `FakeGateway` (Story 5.8, Matrix 35-38 and 41; Story 5.9 Matrix 50: the
//! HTTP edge lives through the shutdown). Its own test binary,
//! because it installs a global capturing tracing subscriber and a global
//! metrics recorder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis::events::{EventCtx, MessageDelete};
use noctis::prelude::*;
use noctis::{ErrorReport, ReportKind, RunError, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeGatewayConfig, FakeResponse, FrameKind, GatewayBot,
    IdentifyReply, Method, StatusCode,
};
use serde_json::json;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

// ---- globals ----

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    /// The byte length so far, to read only later lines.
    fn mark(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// The text logged since `mark`.
    fn since(&self, mark: usize) -> String {
        String::from_utf8(self.0.lock().unwrap()[mark..].to_vec()).unwrap()
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

/// The process-wide capture and metrics snapshotter, installed once.
fn globals() -> (&'static Capture, &'static Snapshotter) {
    static GLOBALS: OnceLock<(Capture, Snapshotter)> = OnceLock::new();
    let (capture, snapshotter) = GLOBALS.get_or_init(|| {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        recorder.install().unwrap();
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        (capture, snapshotter)
    });
    (capture, snapshotter)
}

/// Serializes the tests: they share the log capture and the metrics.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

// ---- App ----

/// What `blocked` saw and waits on, shared through `State`.
struct Probe {
    lines: Mutex<Vec<String>>,
    /// Closed until a test opens it.
    gate: Semaphore,
    /// One permit `blocked` holds while it runs.
    held: Semaphore,
}

impl Probe {
    fn push(&self, line: impl Into<String>) {
        self.lines.lock().unwrap().push(line.into());
    }

    fn has(&self, line: &str) -> bool {
        self.lines.lock().unwrap().iter().any(|l| l == line)
    }
}

/// Holds `held`, waits for the gate, then calls `GET /users/@me`.
#[event]
async fn blocked(ctx: EventCtx, _ev: MessageDelete) -> HandlerResult<()> {
    let Some(probe) = ctx.state::<Arc<Probe>>() else {
        return Ok(());
    };
    let _held = probe.held.acquire().await;
    probe.push("start");
    // Held until the test opens the gate; never closed.
    let _permit = probe.gate.acquire().await;
    match ctx.http().current_user().await {
        Ok(response) if response.status().is_success() => probe.push("http-ok"),
        _ => probe.push("http-failed"),
    }
    probe.push("end");
    Ok(())
}

/// Records the kind of every report.
#[derive(Default)]
struct Recording(Mutex<Vec<String>>);

impl Recording {
    fn count(&self, kind: &str) -> usize {
        self.0.lock().unwrap().iter().filter(|k| *k == kind).count()
    }
}

impl ErrorSink for Recording {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Cancelled => "cancelled",
            ReportKind::Error(_) => "error",
            ReportKind::Panic(_) => "panic",
            _ => "other",
        };
        self.0.lock().unwrap().push(kind.to_owned());
    }
}

/// A running App on both fakes, stopped through `stop`.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    probe: Arc<Probe>,
    sink: Arc<Recording>,
    stop: Option<oneshot::Sender<()>>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(
        shards: u32,
        config: FakeGatewayConfig,
        configure: impl FnOnce(&mut AppBuilder),
    ) -> Self {
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards,
            ..GatewayBot::default()
        })
        .await
        .unwrap();
        http.route(
            Method::GET,
            "/users/@me",
            FakeResponse::json(
                StatusCode::OK,
                &json!({
                    "id": "1", "username": "b", "discriminator": "0", "avatar": null,
                    "bot": true, "mfa_enabled": false,
                }),
            ),
        );
        let gw = FakeGateway::start_with(config).await.unwrap();
        let probe = Arc::new(Probe {
            lines: Mutex::default(),
            gate: Semaphore::new(0),
            held: Semaphore::new(1),
        });
        let sink = Arc::new(Recording::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .state(Arc::clone(&probe))
            .error_sink(ErrorOrigin::Event, Arc::clone(&sink))
            .module(Module::new("ops").add(blocked()));
        configure(&mut builder);
        let app = builder.build().unwrap();
        let (stop, stopped) = oneshot::channel::<()>();
        let run = tokio::spawn(app.run_until(async {
            let _ = stopped.await;
        }));
        Self {
            gw,
            http,
            probe,
            sink,
            stop: Some(stop),
            run,
        }
    }

    /// Waits until the fake holds `n` sessions (READY sent), probing with an
    /// unhandled `TYPING_START` the shards never parse.
    async fn wait_ready(&self, n: usize) {
        let deadline = Instant::now() + WAIT;
        while self.gw.dispatch("TYPING_START", json!({})) < n {
            assert!(Instant::now() < deadline, "the shards have no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Dispatches a `MESSAGE_DELETE` and waits until `blocked` started.
    async fn start_blocked(&self) {
        let sent = self.gw.dispatch(
            "MESSAGE_DELETE",
            json!({ "id": "5", "channel_id": "2", "guild_id": "3" }),
        );
        assert_eq!(sent, 1);
        let deadline = Instant::now() + WAIT;
        while !self.probe.has("start") {
            assert!(Instant::now() < deadline, "blocked never started");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn shut_down(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
    }

    async fn finish(&mut self, within: Duration) -> Result<(), noctis::Error> {
        tokio::time::timeout(within, &mut self.run)
            .await
            .expect("run_until returned in time")
            .expect("run_until did not panic")
    }
}

fn identifies(gw: &FakeGateway) -> usize {
    gw.frames().iter().filter(|f| f.op() == Some(2)).count()
}

/// The last value of `noctis_shard_status{shard}`.
fn shard_gauge(snapshotter: &Snapshotter, shard: &str) -> Option<f64> {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .find_map(|(key, .., value)| {
            let matches = key.key().name() == "noctis_shard_status"
                && key
                    .key()
                    .labels()
                    .any(|l| l.key() == "shard" && l.value() == shard);
            match value {
                DebugValue::Gauge(v) if matches => Some(v.0),
                _ => None,
            }
        })
}

/// Matrix 35: a handler started before the shutdown finishes within the
/// grace period, and its HTTP call succeeds after the shards closed.
#[tokio::test(flavor = "multi_thread")]
async fn graceful_shutdown() {
    let _serial = serial().await;
    let (capture, _) = globals();
    let mark = capture.mark();
    let mut running = Running::start(1, FakeGatewayConfig::default(), |_| {}).await;
    running.wait_ready(1).await;
    running.start_blocked().await;
    running.shut_down();
    let close = running
        .gw
        .wait_for_frame(WAIT, |f| {
            matches!(
                f.kind,
                FrameKind::Close {
                    code: Some(4000),
                    ..
                }
            )
        })
        .await;
    assert!(close.is_some(), "no 4000 close frame");
    // Story 5.9 Matrix 50: the edge's upstream connections outlive the
    // shards' close frames.
    let upstream = running.http.connections();
    assert!(!upstream.is_empty());
    assert!(
        upstream.iter().all(|c| c.closed_at.is_none()),
        "{upstream:?}"
    );
    let me = |running: &Running| {
        running
            .http
            .requests()
            .iter()
            .any(|r| r.route_path() == Some("/users/@me"))
    };
    assert!(!me(&running), "the handler waited for the gate");
    running.probe.gate.add_permits(1);
    let outcome = running.finish(WAIT).await;
    assert!(outcome.is_ok(), "{outcome:?}");
    let probe = &running.probe;
    assert!(probe.has("http-ok"), "{:?}", probe.lines.lock().unwrap());
    assert!(probe.has("end"));
    assert!(me(&running));
    assert_eq!(running.sink.count("cancelled"), 0);
    let returned = Instant::now();
    while running
        .http
        .connections()
        .iter()
        .any(|c| c.closed_at.is_none())
    {
        assert!(
            returned.elapsed() <= Duration::from_secs(2),
            "the edge kept upstream connections open after run_until returned"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let text = capture.since(mark);
    let started = text.find("shutdown started").expect("shutdown started");
    let complete = text.find("shutdown complete").expect("shutdown complete");
    assert!(started < complete, "{text}");
}

/// Matrix 36: a handler still blocked at the deadline is cancelled and
/// reported; `run_until` returns in time and the gauge is 0.
#[tokio::test(flavor = "multi_thread")]
async fn deadline_cancels() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let mut running = Running::start(1, FakeGatewayConfig::default(), |b| {
        b.shutdown_grace(Duration::from_millis(300));
    })
    .await;
    running.wait_ready(1).await;
    running.start_blocked().await;
    let started = Instant::now();
    running.shut_down();
    let outcome = running.finish(WAIT).await;
    let elapsed = started.elapsed();
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(
        elapsed <= Duration::from_millis(300) + Duration::from_secs(7),
        "{elapsed:?}"
    );
    assert_eq!(running.sink.count("cancelled"), 1);
    assert!(!running.probe.has("end"));
    assert_eq!(shard_gauge(snapshotter, "0"), Some(0.0));
}

/// Matrix 37: a shutdown during startup returns `Ok(())` without startup
/// finishing.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_during_startup() {
    let _serial = serial().await;
    let (capture, _) = globals();
    let mark = capture.mark();
    let config = FakeGatewayConfig {
        identify: IdentifyReply::Ignore,
        ..FakeGatewayConfig::default()
    };
    let mut running = Running::start(1, config, |_| {}).await;
    running
        .gw
        .wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("IDENTIFY");
    running.shut_down();
    let outcome = running.finish(Duration::from_secs(7)).await;
    assert!(outcome.is_ok(), "{outcome:?}");
    let text = capture.since(mark);
    assert!(!text.contains("all 1 shards ready"), "{text}");
    assert!(text.contains("shutdown complete"), "{text}");
    assert_eq!(identifies(&running.gw), 1);
}

/// Matrix 38: every shard closed fatally after startup: two loud lines and
/// `RunError::AllShardsEnded`.
#[tokio::test(flavor = "multi_thread")]
async fn all_shards_fatal() {
    let _serial = serial().await;
    let (capture, _) = globals();
    let mark = capture.mark();
    let mut running = Running::start(2, FakeGatewayConfig::default(), |_| {}).await;
    running.wait_ready(2).await;
    let deadline = Instant::now() + WAIT;
    while !capture.since(mark).contains("all 2 shards ready") {
        assert!(Instant::now() < deadline, "startup never finished");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(running.gw.close(4014, "").unwrap(), 2);
    let outcome = running.finish(WAIT).await;
    assert!(
        matches!(outcome, Err(noctis::Error::Run(RunError::AllShardsEnded))),
        "{outcome:?}"
    );
    let text = capture.since(mark);
    let fatal: Vec<_> = text
        .lines()
        .filter(|l| l.contains("shard closed fatally") && l.contains("close_code=4014"))
        .collect();
    assert_eq!(fatal.len(), 2, "{text}");
    assert!(text.contains("reason=\"all shards ended\""), "{text}");
    assert_eq!(identifies(&running.gw), 2);
}

/// Matrix 41: aborting the `run_until` task drops a running handler's
/// future (the cancel guard), releasing what it holds.
#[tokio::test(flavor = "multi_thread")]
async fn aborted_run_cancels_handlers() {
    let _serial = serial().await;
    let _ = globals();
    let running = Running::start(1, FakeGatewayConfig::default(), |_| {}).await;
    running.wait_ready(1).await;
    running.start_blocked().await;
    assert_eq!(running.probe.held.available_permits(), 0);
    running.run.abort();
    let deadline = Instant::now() + WAIT;
    while running.probe.held.available_permits() == 0 {
        assert!(
            Instant::now() < deadline,
            "the handler future was not dropped"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!running.probe.has("end"));
}
