//! The observability contract and Error Sinks of a running App (Story 5.7,
//! Matrix 25, 29, 30 and 34). Its own test binary, because it installs a
//! global `metrics` recorder and a global capturing tracing subscriber.
//!
//! The recorder keeps every metric operation in order instead of the
//! latest value, so gauge transitions (shard status) can be checked
//! without a racy poll.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use metrics::{
    Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use noctis::events::{EventCtx, MessageDelete};
use noctis::observe::{METRICS, MetricType};
use noctis::prelude::*;
use noctis::{ErrorReport, ReportKind, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot, SessionStartLimit};
use serde_json::json;
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// Fixture snowflakes: guild, channel and message ids.
const GUILD: u64 = 1_234_567_890_123_456_789;
const CHANNEL: u64 = 1_111_111_111_111_111_111;
const MESSAGE_BASE: u64 = 1_300_000_000_000_000_000;

/// The largest shard total any test here starts.
const MAX_TOTAL: u32 = 2;

// ---- capture ----

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

// ---- recorder ----

/// What one operation did.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Value {
    Counter(u64),
    Gauge(f64),
    Histogram(f64),
}

/// One recorded metric operation.
#[derive(Debug, Clone)]
struct Op {
    name: String,
    labels: Vec<(String, String)>,
    value: Value,
}

/// Every operation, in order.
#[derive(Default)]
struct History(Mutex<Vec<Op>>);

impl History {
    fn ops(&self) -> Vec<Op> {
        self.0.lock().unwrap().clone()
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// The values set on `noctis_shard_status{shard}` since operation
    /// `from`, in order.
    fn shard_status(&self, from: usize, shard: &str) -> Vec<f64> {
        self.ops()
            .into_iter()
            .skip(from)
            .filter(|op| {
                op.name == "noctis_shard_status"
                    && op.labels.iter().any(|(k, v)| k == "shard" && v == shard)
            })
            .filter_map(|op| match op.value {
                Value::Gauge(v) => Some(v),
                _ => None,
            })
            .collect()
    }
}

/// A handle recording into the [`History`].
struct Handle {
    history: Arc<History>,
    name: String,
    labels: Vec<(String, String)>,
}

impl Handle {
    fn push(&self, value: Value) {
        self.history.0.lock().unwrap().push(Op {
            name: self.name.clone(),
            labels: self.labels.clone(),
            value,
        });
    }
}

impl CounterFn for Handle {
    fn increment(&self, value: u64) {
        self.push(Value::Counter(value));
    }

    fn absolute(&self, value: u64) {
        self.push(Value::Counter(value));
    }
}

impl GaugeFn for Handle {
    fn increment(&self, value: f64) {
        self.push(Value::Gauge(value));
    }

    fn decrement(&self, value: f64) {
        self.push(Value::Gauge(-value));
    }

    fn set(&self, value: f64) {
        self.push(Value::Gauge(value));
    }
}

impl HistogramFn for Handle {
    fn record(&self, value: f64) {
        self.push(Value::Histogram(value));
    }
}

/// Records every operation into a shared [`History`].
struct HistoryRecorder(Arc<History>);

impl HistoryRecorder {
    fn handle(&self, key: &Key) -> Arc<Handle> {
        Arc::new(Handle {
            history: Arc::clone(&self.0),
            name: key.name().to_owned(),
            labels: key
                .labels()
                .map(|l| (l.key().to_owned(), l.value().to_owned()))
                .collect(),
        })
    }
}

impl Recorder for HistoryRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        Counter::from_arc(self.handle(key))
    }

    fn register_gauge(&self, key: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::from_arc(self.handle(key))
    }

    fn register_histogram(&self, key: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::from_arc(self.handle(key))
    }
}

/// The process-wide capture (spans included) and metric history,
/// installed once.
fn globals() -> (&'static Capture, &'static Arc<History>) {
    static GLOBALS: OnceLock<(Capture, Arc<History>)> = OnceLock::new();
    let (capture, history) = GLOBALS.get_or_init(|| {
        let history = Arc::new(History::default());
        metrics::set_global_recorder(HistoryRecorder(Arc::clone(&history))).unwrap();
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_span_events(FmtSpan::NEW)
            .with_writer(capture.clone())
            .init();
        (capture, history)
    });
    (capture, history)
}

/// Serializes the tests: every test App labels its metrics `app="default"`,
/// so the shard-status history must not interleave with another App's.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

// ---- sink ----

/// The owned fields of one report.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    kind: String,
    shard: Option<u32>,
    guild_id: Option<u64>,
    event_type: Option<String>,
}

/// Records every report.
#[derive(Default)]
struct Recording(Mutex<Vec<Seen>>);

impl Recording {
    fn seen(&self) -> Vec<Seen> {
        self.0.lock().unwrap().clone()
    }

    fn count(&self, kind: &str) -> usize {
        self.seen().iter().filter(|s| s.kind == kind).count()
    }
}

impl ErrorSink for Recording {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(_) => "error",
            ReportKind::Panic(_) => "panic",
            ReportKind::Dropped => "dropped",
            _ => "other",
        };
        self.0.lock().unwrap().push(Seen {
            kind: kind.to_owned(),
            shard: report.shard,
            guild_id: report.guild_id.map(noctis::twilight::model::id::Id::get),
            event_type: report.event_type.map(str::to_owned),
        });
    }
}

// ---- App ----

/// A gate the blocking handler waits on, shared through `State`.
struct Gate(tokio::sync::Semaphore);

/// Message `MESSAGE_BASE + 1` fails, `+ 2` panics, others block on the gate.
#[event]
async fn mixed(ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
    match ev.id.get() - MESSAGE_BASE {
        1 => Err(HandlerError::new(io::Error::other("disk full"))),
        2 => panic!("boom"),
        _ => {
            if let Some(gate) = ctx.state::<Gate>() {
                // Held until the test opens the gate; never closed.
                let _permit = gate.0.acquire().await;
            }
            Ok(())
        }
    }
}

/// A running App against both fakes.
struct Running {
    gw: FakeGateway,
    _http: FakeDiscordHttp,
    gate: State<Gate>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(shards: u32, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards,
            session_start_limit: SessionStartLimit {
                max_concurrency: shards,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        })
        .await
        .unwrap();
        let gw = FakeGateway::start().await.unwrap();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .state(Gate(tokio::sync::Semaphore::new(0)));
        configure(&mut builder);
        let app = builder.build().unwrap();
        let gate = app.state::<Gate>().unwrap();
        let run = tokio::spawn(app.run());
        let running = Self {
            gw,
            _http: http,
            gate,
            run,
        };
        // Every shard holds a session once an unparsed dispatch reaches
        // `shards` connections.
        let deadline = Instant::now() + WAIT;
        while running
            .gw
            .dispatch("TYPING_START", json!({}))
            .try_into()
            .unwrap_or(0_u32)
            < shards
        {
            assert!(Instant::now() < deadline, "the shards have no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        running
    }

    fn delete(&self, n: u64) {
        let sent = self.gw.dispatch(
            "MESSAGE_DELETE",
            json!({
                "id": (MESSAGE_BASE + n).to_string(),
                "channel_id": CHANNEL.to_string(),
                "guild_id": GUILD.to_string(),
            }),
        );
        assert_eq!(sent, 1);
    }
}

/// Polls `cond` within [`WAIT`].
async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---- label policy ----

/// Checks one recorded label value.
fn check_value(metric: &str, label: &str, value: &str) {
    match label {
        "app" => assert_eq!(value, "default", "{metric}"),
        "lane" => assert!(
            ["interaction", "event"].contains(&value),
            "{metric}: {value}"
        ),
        "kind" => assert_eq!(value, "event", "{metric}"),
        "event_type" => assert!(
            (1..=64).contains(&value.len())
                && value.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'),
            "{metric}: {value}"
        ),
        "shard" => {
            let shard: u32 = value
                .parse()
                .unwrap_or_else(|_| panic!("{metric}: {value}"));
            assert!(shard < MAX_TOTAL, "{metric}: {value}");
        }
        other => panic!("{metric}: unexpected label {other}"),
    }
    let digits = value.bytes().filter(u8::is_ascii_digit).count();
    assert!(digits < 15, "{metric}: {label}={value}");
    for fixture in [GUILD, CHANNEL, MESSAGE_BASE + 1, MESSAGE_BASE + 2] {
        assert_ne!(value, fixture.to_string(), "{metric}: {label}");
    }
}

/// Checks every operation against the catalog; returns the names seen.
fn check_history(ops: &[Op]) -> Vec<&'static str> {
    let mut seen = Vec::new();
    for op in ops {
        let info = METRICS
            .iter()
            .find(|m| m.name == op.name)
            .unwrap_or_else(|| panic!("{} is not in the catalog", op.name));
        let kind_matches = matches!(
            (info.metric_type, op.value),
            (MetricType::Counter, Value::Counter(_))
                | (MetricType::Gauge, Value::Gauge(_))
                | (MetricType::Histogram, Value::Histogram(_))
        );
        assert!(kind_matches, "{op:?}");
        let names: Vec<_> = op.labels.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, info.labels, "{}", op.name);
        for (label, value) in &op.labels {
            check_value(&op.name, label, value);
        }
        if !seen.contains(&info.name) {
            seen.push(info.name);
        }
    }
    seen
}

// ---- tests ----

/// Matrix 25: an overflow drop reaches the `Event` sink with the event's
/// shard, type and guild.
#[tokio::test(flavor = "multi_thread")]
async fn overflow_drop_reaches_sink() {
    let _serial = serial().await;
    globals();
    let sink = Arc::new(Recording::default());
    let running = Running::start(1, |b| {
        b.event_lane(1, 0)
            .handler_concurrency(1)
            .error_sink(ErrorOrigin::Event, Arc::clone(&sink))
            .module(Module::new("flood").add(mixed()));
    })
    .await;
    for n in 3..=40 {
        running.delete(n);
    }
    until("a drop report", || sink.count("dropped") > 0).await;
    let dropped = sink
        .seen()
        .into_iter()
        .find(|s| s.kind == "dropped")
        .unwrap();
    assert_eq!(
        dropped,
        Seen {
            kind: "dropped".to_owned(),
            shard: Some(0),
            guild_id: Some(GUILD),
            event_type: Some("MESSAGE_DELETE".to_owned()),
        }
    );
    running.gate.0.add_permits(1_000);
    assert!(!running.run.is_finished());
    running.run.abort();
}

/// Matrix 29 and 30: `noctis.shard` spans per shard, and the status gauge
/// through start, READY, a resumable close and RESUMED. The fake closes
/// every connection, so both shards go down and come back.
#[tokio::test(flavor = "multi_thread")]
async fn shard_spans_and_status() {
    let _serial = serial().await;
    let (capture, history) = globals();
    let from = history.len();
    let running = Running::start(2, |_| {}).await;
    for shard in ["0", "1"] {
        let span = format!("noctis.shard{{app=\"default\" shard={shard}}}");
        assert!(
            !capture.lines_with(&[&span]).is_empty(),
            "{span} never logged"
        );
    }
    until("READY on both", || {
        history.shard_status(from, "0") == [0.0, 1.0]
            && history.shard_status(from, "1") == [0.0, 1.0]
    })
    .await;
    assert_eq!(running.gw.close(4000, "reconnect"), Ok(2));
    until("resumed on both", || {
        history.shard_status(from, "0") == [0.0, 1.0, 0.0, 1.0]
            && history.shard_status(from, "1") == [0.0, 1.0, 0.0, 1.0]
    })
    .await;
    assert!(
        running.gw.frames().iter().any(|f| f.op() == Some(6)),
        "no RESUME"
    );
    assert!(history.shard_status(0, "2").is_empty());
    running.run.abort();
}

/// Matrix 34: an erroring handler, a panicking one and an overflow flood
/// record only catalog metrics, with bounded label values, and every
/// catalog metric is recorded.
#[tokio::test(flavor = "multi_thread")]
async fn recorded_set_matches_catalog() {
    let _serial = serial().await;
    let (_, history) = globals();
    let sink = Arc::new(Recording::default());
    let running = Running::start(1, |b| {
        b.event_lane(1, 0)
            .handler_concurrency(1)
            .error_sink(ErrorOrigin::Event, Arc::clone(&sink))
            .module(Module::new("audit").add(mixed()));
    })
    .await;
    running.delete(1);
    until("the error", || sink.count("error") == 1).await;
    running.delete(2);
    until("the panic", || sink.count("panic") == 1).await;
    for n in 3..=40 {
        running.delete(n);
    }
    until("a drop", || sink.count("dropped") > 0).await;
    running.gate.0.add_permits(1_000);
    until("handler durations", || {
        history
            .ops()
            .iter()
            .any(|op| op.name == "noctis_handler_duration_seconds")
    })
    .await;
    let ops = history.ops();
    let mut seen = check_history(&ops);
    seen.sort_unstable();
    let mut all: Vec<_> = METRICS.iter().map(|m| m.name).collect();
    all.sort_unstable();
    assert_eq!(seen, all);
    let errors: u64 = ops
        .iter()
        .filter(|op| op.name == "noctis_handler_errors_total")
        .map(|op| match op.value {
            Value::Counter(n) => n,
            _ => 0,
        })
        .sum();
    assert!(errors >= 2, "{errors}");
    running.run.abort();
}
