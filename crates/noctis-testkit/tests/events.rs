//! `#[event]` handlers end to end against `FakeDiscordHttp` and
//! `FakeGateway` (Story 5.6, Matrix 45-47). Its own test binary, because it
//! installs a global capturing tracing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use noctis::events::{EventCtx, MessageCreate, MessageDelete};
use noctis::prelude::*;
use noctis::twilight::model::gateway::Intents;
use noctis::{Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot};
use serde_json::json;
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// The parse-failure warning of the shard tasks.
const PARSE_WARNING: &str = "gateway payload could not be parsed";

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn count(&self, needle: &str) -> usize {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|line| line.contains(needle))
            .count()
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

/// The process-wide capture, installed once.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        capture
    })
}

/// What the handlers saw, shared through `State`.
#[derive(Default)]
struct Probe(Mutex<Vec<String>>);

impl Probe {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

#[event]
async fn on_delete(ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
    if let Some(probe) = ctx.state::<Arc<Probe>>() {
        probe.push(format!("delete:{}", ev.id));
    }
    Ok(())
}

#[event]
async fn on_create(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    if let Some(probe) = ctx.state::<Arc<Probe>>() {
        probe.push(format!("create:{}", msg.id));
    }
    Ok(())
}

#[event(ordered)]
async fn ordered_delete(ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
    if let Some(probe) = ctx.state::<Arc<Probe>>() {
        probe.push(format!("ordered:{}", ev.id));
    }
    assert!(ev.id.get() != 1, "ordered handler boom");
    Ok(())
}

#[event]
async fn concurrent_delete(ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
    if let Some(probe) = ctx.state::<Arc<Probe>>() {
        probe.push(format!("concurrent:{}", ev.id));
    }
    Ok(())
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    _http: FakeDiscordHttp,
    probe: Arc<Probe>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(module: Module) -> Self {
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards: 1,
            ..GatewayBot::default()
        })
        .await
        .unwrap();
        let gw = FakeGateway::start().await.unwrap();
        let probe = Arc::new(Probe::default());
        let app = App::builder()
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .state(Arc::clone(&probe))
            .module(module)
            .build()
            .unwrap();
        let run = tokio::spawn(app.run());
        let running = Self {
            gw,
            _http: http,
            probe,
            run,
        };
        running.wait_ready().await;
        running
    }

    /// Waits until the fake holds the shard's session (READY sent), probing
    /// with an unhandled `TYPING_START` the shard never parses.
    async fn wait_ready(&self) {
        let deadline = Instant::now() + WAIT;
        while self.gw.dispatch("TYPING_START", json!({})) == 0 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Polls until the probe holds `line` `n` times.
    async fn wait_for(&self, line: &str, n: usize) {
        let deadline = Instant::now() + WAIT;
        while self.probe.lines().iter().filter(|l| *l == line).count() < n {
            assert!(
                Instant::now() < deadline,
                "{line} x{n} missing from {:?}",
                self.probe.lines()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn delete(&self, id: u64) {
        let sent = self.gw.dispatch(
            "MESSAGE_DELETE",
            json!({ "id": id.to_string(), "channel_id": "2", "guild_id": "3" }),
        );
        assert_eq!(sent, 1);
    }
}

/// Matrix 45: only handled event types are deserialized.
#[tokio::test(flavor = "multi_thread")]
async fn only_handled_events_are_parsed() {
    let capture = capture();
    let before = capture.count(PARSE_WARNING);

    let running = Running::start(Module::new("filter").add(on_delete())).await;
    assert_eq!(
        running
            .gw
            .dispatch("MESSAGE_CREATE", json!({ "bogus": true })),
        1
    );
    running.delete(1);
    running.wait_for("delete:1", 1).await;
    assert_eq!(capture.count(PARSE_WARNING), before);
    assert_eq!(running.probe.lines(), ["delete:1"]);
    running.run.abort();

    // Control: with a MESSAGE_CREATE handler the same payload is parsed and
    // fails.
    let running = Running::start(Module::new("filter").add(on_delete()).add(on_create())).await;
    assert_eq!(
        running
            .gw
            .dispatch("MESSAGE_CREATE", json!({ "bogus": true })),
        1
    );
    running.delete(2);
    running.wait_for("delete:2", 1).await;
    assert_eq!(capture.count(PARSE_WARNING), before + 1);
    running.run.abort();
}

/// Matrix 46: IDENTIFY carries the derived intents.
#[tokio::test(flavor = "multi_thread")]
async fn identify_carries_derived_intents() {
    capture();
    let running = Running::start(Module::new("xp").add(on_create())).await;
    let identify = running
        .gw
        .frames()
        .into_iter()
        .find(|f| f.op() == Some(2))
        .unwrap();
    let intents = identify.json().unwrap()["d"]["intents"].as_u64().unwrap();
    assert_eq!(intents, (Intents::GUILDS | Intents::GUILD_MESSAGES).bits());
    running.run.abort();
}

/// Matrix 47: ordered then concurrent per event; a panic is logged and the
/// shard keeps its one connection.
#[tokio::test(flavor = "multi_thread")]
async fn ordered_concurrent_and_panic() {
    let capture = capture();
    let running = Running::start(
        Module::new("audit")
            .add(concurrent_delete())
            .add(ordered_delete()),
    )
    .await;
    running.delete(1);
    running.delete(2);
    running.wait_for("concurrent:2", 1).await;
    running.wait_for("concurrent:1", 1).await;
    let lines = running.probe.lines();
    let at = |line: &str| lines.iter().position(|l| l == line).unwrap();
    assert!(at("ordered:1") < at("concurrent:1"), "{lines:?}");
    assert!(at("ordered:2") < at("concurrent:2"), "{lines:?}");
    assert!(at("ordered:1") < at("ordered:2"), "{lines:?}");
    let deadline = Instant::now() + WAIT;
    while capture.count("event handler panicked") == 0 {
        assert!(Instant::now() < deadline, "no panic log");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(running.gw.connections().len(), 1);
    assert!(!running.run.is_finished());
    running.run.abort();
}
