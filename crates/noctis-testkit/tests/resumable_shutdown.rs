//! Resumable shutdown and `--no-resume` at App level (Story 12.3, S1-S6):
//! the AD-21 order (`shutdown started` -> 4000 close -> `put` -> return),
//! the fresh mode (1000, entries cleared at start and at shutdown) and
//! at-most-once delivery across a resume. Its own test binary, because one
//! ordered journal is shared by a global capturing tracing subscriber and a
//! recording `SessionStore`; the tests are serialized.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use noctis::cli::CliError;
use noctis::events::{EventCtx, MessageDelete};
use noctis::prelude::*;
use noctis::{Runtime, Token};
use noctis_ports::memory::MemorySessionStore;
use noctis_ports::{BoxFuture, PortError, SessionStore, decode_value};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, FrameKind, GatewayBot, RecordedFrame};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// Shard 0 of 1's key behind App `default`'s namespace.
const KEY: &str = "default:session:shard:0:1";

// ---- the journal ----

/// One ordered list of log lines (`log:<line>`) and store ops
/// (`put:<key>:<seq>`, `delete:<key>`, `get:<key>`).
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<String>>>);

impl Journal {
    fn push(&self, entry: String) {
        self.0.lock().unwrap().push(entry);
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// The entries from `mark` on.
    fn since(&self, mark: usize) -> Vec<String> {
        self.0.lock().unwrap()[mark..].to_vec()
    }
}

/// Writes each formatted log line into the journal, in emission order.
impl io::Write for Journal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        for line in text.lines().filter(|line| !line.is_empty()) {
            self.push(format!("log:{line}"));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Journal {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The process-wide journal, with the capturing subscriber installed once.
fn journal() -> &'static Journal {
    static JOURNAL: OnceLock<Journal> = OnceLock::new();
    JOURNAL.get_or_init(|| {
        let journal = Journal::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_writer(journal.clone())
            .init();
        journal
    })
}

/// Serializes the tests: they share the journal.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

/// The index of the first entry of `entries` matching `pred`.
fn position(entries: &[String], pred: impl Fn(&str) -> bool) -> Option<usize> {
    entries.iter().position(|entry| pred(entry))
}

fn is_log(entry: &str, message: &str) -> bool {
    entry.starts_with("log:") && entry.contains(" noctis::app: ") && entry.contains(message)
}

// ---- the recording store ----

/// The stored value's layout (the gateway's private struct).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Stored {
    session_id: String,
    sequence: u64,
    resume_url: String,
}

/// A `MemorySessionStore` writing each op into the journal. It holds the
/// fake gateway as a probe: at each `put` it records the client close
/// codes the fake has seen, at each `delete` the IDENTIFYs it has seen.
struct Recording {
    inner: MemorySessionStore,
    journal: Journal,
    probe: Arc<FakeGateway>,
    closes_at_put: Mutex<Vec<Vec<Option<u16>>>>,
    identifies_at_delete: Mutex<Vec<usize>>,
}

impl Recording {
    fn new(probe: &Arc<FakeGateway>) -> Arc<Self> {
        Arc::new(Self {
            inner: MemorySessionStore::new(),
            journal: journal().clone(),
            probe: Arc::clone(probe),
            closes_at_put: Mutex::default(),
            identifies_at_delete: Mutex::default(),
        })
    }

    /// The stored entry under `key`, decoded.
    async fn stored(&self, key: &str) -> Option<Stored> {
        let raw = self.inner.get(key).await.unwrap()?;
        decode_value(1, &raw).unwrap()
    }
}

impl SessionStore for Recording {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.journal.push(format!("get:{key}"));
        self.inner.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        let stored: Stored = decode_value(1, &value).unwrap().unwrap();
        self.closes_at_put
            .lock()
            .unwrap()
            .push(client_closes(&self.probe.frames()));
        self.journal.push(format!("put:{key}:{}", stored.sequence));
        self.inner.put(key, value, ttl)
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.identifies_at_delete
            .lock()
            .unwrap()
            .push(identifies(&self.probe.frames()));
        self.journal.push(format!("delete:{key}"));
        self.inner.delete(key)
    }
}

fn identifies(frames: &[RecordedFrame]) -> usize {
    frames.iter().filter(|f| f.op() == Some(2)).count()
}

fn ops_of(frames: &[RecordedFrame], op: u64) -> Vec<RecordedFrame> {
    frames
        .iter()
        .filter(|f| f.op() == Some(op))
        .cloned()
        .collect()
}

fn client_closes(frames: &[RecordedFrame]) -> Vec<Option<u16>> {
    frames
        .iter()
        .filter_map(|f| match f.kind {
            FrameKind::Close { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

// ---- handlers ----

/// What the handlers saw, shared through `State`, plus a gate that the
/// blocking handler waits on (closed until a test opens it).
struct Probe {
    lines: Mutex<Vec<String>>,
    gate: Semaphore,
}

impl Probe {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            lines: Mutex::default(),
            gate: Semaphore::new(0),
        })
    }

    fn push(&self, line: String) {
        self.lines.lock().unwrap().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    /// Polls until `line` was seen, within [`WAIT`].
    async fn wait_for(&self, line: &str) {
        let deadline = Instant::now() + WAIT;
        while !self.lines().iter().any(|l| l == line) {
            assert!(Instant::now() < deadline, "never saw {line}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[derive(EventHandler)]
struct OnDelete;

impl OnDelete {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
        if let Some(probe) = ctx.state::<Arc<Probe>>() {
            probe.push(format!("delete:{}", ev.id));
        }
        Ok(())
    }
}

#[derive(EventHandler)]
#[event(droppable)]
struct BlockedDelete;

impl BlockedDelete {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
        if let Some(probe) = ctx.state::<Arc<Probe>>() {
            probe.push(format!("blocked:{}", ev.id));
            // Held until the test opens the gate; never closed.
            let _permit = probe.gate.acquire().await;
        }
        Ok(())
    }
}

// ---- App ----

async fn http() -> FakeDiscordHttp {
    FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap()
}

/// A builder on `Runtime::for_test()` wired to both fakes, the shared
/// `store`, `probe` as State and `module`.
fn builder(
    http: &FakeDiscordHttp,
    gw: &FakeGateway,
    store: &Arc<Recording>,
    probe: &Arc<Probe>,
    module: Module,
) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .session_store(Arc::clone(store))
        .state(Arc::clone(probe))
        .module(module);
    builder
}

/// A running App and the sender that stops it.
struct RunningApp {
    stop: oneshot::Sender<()>,
    handle: JoinHandle<Result<(), noctis::Error>>,
}

fn run(app: App) -> RunningApp {
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app.run_until(async {
        let _ = stopped.await;
    }));
    RunningApp { stop, handle }
}

impl RunningApp {
    /// Stops the App and waits until `run_until` returned `Ok`.
    async fn finish(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(WAIT, self.handle)
            .await
            .expect("run finished within the wait")
            .unwrap()
            .unwrap();
    }
}

/// Polls until `gw` has `n` live sessions, within [`WAIT`].
async fn wait_sessions(gw: &FakeGateway, n: usize) {
    let deadline = Instant::now() + WAIT;
    while gw.live_sessions() < n {
        assert!(
            Instant::now() < deadline,
            "only {} sessions",
            gw.live_sessions()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Lets the shard tasks read what the fake sent.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// Sends `MESSAGE_DELETE` with each id to every session.
fn delete(gw: &FakeGateway, ids: impl IntoIterator<Item = u64>) {
    for id in ids {
        assert_eq!(
            gw.dispatch(
                "MESSAGE_DELETE",
                json!({ "id": id.to_string(), "channel_id": "2" })
            ),
            1
        );
    }
}

/// A default-mode App: READY (s 1), three dispatches (s 4) seen by the
/// handler, then a graceful stop.
async fn default_run(
    http: &FakeDiscordHttp,
    gw: &Arc<FakeGateway>,
    store: &Arc<Recording>,
    configure: impl FnOnce(&mut AppBuilder),
) {
    let probe = Probe::new();
    let mut builder = builder(
        http,
        gw,
        store,
        &probe,
        Module::new("ops").add::<OnDelete>(),
    );
    configure(&mut builder);
    let app = run(builder.build().unwrap());
    wait_sessions(gw, 1).await;
    settle().await;
    delete(gw, 1..=3);
    probe.wait_for("delete:3").await;
    app.finish().await;
}

/// S1: `shutdown started` < 4000 close < `put` (last received seq) <
/// `shutdown complete`, and the entry is readable once `run_until`
/// returned.
#[tokio::test(flavor = "multi_thread")]
async fn s1_default_shutdown_stores_the_session_in_order() {
    let _serial = serial().await;
    let journal = journal();
    let http = http().await;
    let gw = Arc::new(FakeGateway::start().await.unwrap());
    let store = Recording::new(&gw);
    let mark = journal.len();
    default_run(&http, &gw, &store, |_| {}).await;
    // No sleep: the write finished before `run_until` returned.
    let stored = store.stored(KEY).await.expect("stored on return");
    assert_eq!(stored.sequence, 4);
    assert_eq!(stored.session_id, "fake-session-1");
    assert_eq!(stored.resume_url, gw.url());

    let entries = journal.since(mark);
    let started = position(&entries, |e| is_log(e, "shutdown started")).expect("started");
    let put = position(&entries, |e| e == format!("put:{KEY}:4")).expect("a put with seq 4");
    let complete = position(&entries, |e| is_log(e, "shutdown complete")).expect("complete");
    assert!(started < put, "{entries:#?}");
    assert!(put < complete, "{entries:#?}");
    assert_eq!(
        entries.iter().filter(|e| e.starts_with("put:")).count(),
        1,
        "{entries:#?}"
    );
    assert_eq!(
        *store.closes_at_put.lock().unwrap(),
        [vec![Some(4000)]],
        "the 4000 close was on the wire before the put"
    );
    assert_eq!(client_closes(&gw.frames()), [Some(4000)]);
}

/// S2 and S3: `resume_sessions(false)` closes with 1000 and deletes the
/// entry after `shutdown started`; the next default App identifies.
#[tokio::test(flavor = "multi_thread")]
async fn s2_s3_fresh_shutdown_clears_and_the_restart_identifies() {
    let _serial = serial().await;
    let journal = journal();
    let http = http().await;
    let gw = Arc::new(FakeGateway::start().await.unwrap());
    let store = Recording::new(&gw);
    let mark = journal.len();
    default_run(&http, &gw, &store, |b| {
        b.resume_sessions(false);
    })
    .await;
    assert_eq!(store.stored(KEY).await, None, "store empty after return");
    assert_eq!(client_closes(&gw.frames()), [Some(1000)]);
    let entries = journal.since(mark);
    assert!(
        !entries.iter().any(|e| e.starts_with("put:")),
        "{entries:#?}"
    );
    assert!(
        !entries.iter().any(|e| e.starts_with("get:")),
        "{entries:#?}"
    );
    let started = position(&entries, |e| is_log(e, "shutdown started")).expect("started");
    let deleted = entries[started..]
        .iter()
        .any(|e| e == &format!("delete:{KEY}"));
    assert!(deleted, "a delete after shutdown started: {entries:#?}");

    // S3: a default App with the same store and fake identifies.
    gw.clear_frames();
    let probe = Probe::new();
    let app = run(builder(&http, &gw, &store, &probe, Module::new("ops"))
        .build()
        .unwrap());
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("an IDENTIFY");
    wait_sessions(&gw, 1).await;
    settle().await;
    assert!(ops_of(&gw.frames(), 6).is_empty(), "no RESUME");
    app.finish().await;
}

/// S4: a `resume_sessions(false)` start deletes the entry a default App
/// left before it identifies, and sends no RESUME.
#[tokio::test(flavor = "multi_thread")]
async fn s4_fresh_start_deletes_before_identify() {
    let _serial = serial().await;
    let journal = journal();
    let http = http().await;
    let gw = Arc::new(FakeGateway::start().await.unwrap());
    let store = Recording::new(&gw);
    default_run(&http, &gw, &store, |_| {}).await;
    assert!(store.stored(KEY).await.is_some(), "seeded by App 1");
    gw.clear_frames();
    let before = identifies(&gw.frames());
    store.identifies_at_delete.lock().unwrap().clear();
    let mark = journal.len();

    let probe = Probe::new();
    let mut second = builder(&http, &gw, &store, &probe, Module::new("ops"));
    second.resume_sessions(false);
    let app = run(second.build().unwrap());
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("App 2's IDENTIFY");
    wait_sessions(&gw, 1).await;
    settle().await;
    assert_eq!(
        store.identifies_at_delete.lock().unwrap().first(),
        Some(&before),
        "the delete came before App 2's IDENTIFY"
    );
    assert_eq!(identifies(&gw.frames()), before + 1);
    assert_eq!(ops_of(&gw.frames(), 6).len(), 0, "no RESUME");
    assert_eq!(store.stored(KEY).await, None);
    app.finish().await;
    let entries = journal.since(mark);
    assert!(
        !entries
            .iter()
            .any(|e| e.starts_with("get:") || e.starts_with("put:")),
        "{entries:#?}"
    );
}

/// S5: events dropped under back-pressure count as received: the stored
/// sequence is the fake's last one, the next App resumes from it and sees
/// none of them.
#[tokio::test(flavor = "multi_thread")]
async fn s5_dropped_events_are_not_replayed() {
    let _serial = serial().await;
    let journal = journal();
    let http = http().await;
    let gw = Arc::new(FakeGateway::start().await.unwrap());
    let store = Recording::new(&gw);
    let mark = journal.len();

    let probe = Probe::new();
    let mut first = builder(
        &http,
        &gw,
        &store,
        &probe,
        Module::new("flood").add::<BlockedDelete>(),
    );
    first.event_lane(1, 0).handler_concurrency(1);
    let app = run(first.build().unwrap());
    wait_sessions(&gw, 1).await;
    settle().await;
    // The first event takes the only permit and blocks in the handler.
    delete(&gw, [1]);
    probe.wait_for("blocked:1").await;
    delete(&gw, 2..=20);
    let deadline = Instant::now() + WAIT;
    while !journal
        .since(mark)
        .iter()
        .any(|e| e.contains("event lane is full"))
    {
        assert!(Instant::now() < deadline, "no event was dropped");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    probe.gate.add_permits(1_000);
    // The newest event survives the drops, so all 20 were received.
    probe.wait_for("blocked:20").await;
    let handled = probe.lines().len();
    assert!(handled < 20, "some events were dropped: {handled}");
    app.finish().await;
    // READY (s 1) and 20 dispatches: the last sent `s` is 21.
    let last_sent = 21;
    assert_eq!(store.stored(KEY).await.unwrap().sequence, last_sent);

    gw.clear_frames();
    let second_probe = Probe::new();
    let app = run(builder(
        &http,
        &gw,
        &store,
        &second_probe,
        Module::new("ops").add::<OnDelete>(),
    )
    .build()
    .unwrap());
    let resume = gw
        .wait_for_frame(WAIT, |f| f.op() == Some(6))
        .await
        .expect("a RESUME");
    assert_eq!(resume.json().unwrap()["d"]["seq"], last_sent);
    wait_sessions(&gw, 1).await;
    settle().await;
    assert!(ops_of(&gw.frames(), 2).is_empty(), "no IDENTIFY");
    assert!(
        second_probe.lines().is_empty(),
        "App 2 saw App 1's events: {:?}",
        second_probe.lines()
    );
    app.finish().await;
}

/// S6: `--no-resume` with a publish flag fails before any HTTP request.
#[tokio::test(flavor = "multi_thread")]
async fn s6_no_resume_with_publish_is_rejected_first() {
    let _serial = serial().await;
    let http = http().await;
    let gw = Arc::new(FakeGateway::start().await.unwrap());
    let store = Recording::new(&gw);
    let app = builder(&http, &gw, &store, &Probe::new(), Module::new("ops"))
        .build()
        .unwrap();
    let result = tokio::time::timeout(WAIT, app.run_with_args(["--no-resume", "--publish"]))
        .await
        .unwrap();
    assert!(
        matches!(
            result,
            Err(noctis::Error::Cli(CliError::NoResumeWithPublish))
        ),
        "{result:?}"
    );
    assert!(http.requests().is_empty(), "no HTTP request");
    assert!(gw.frames().is_empty(), "no shard");
}

/// S6: `--no-resume` alone turns the resume off for the run: logged, and
/// the start deletes the stored entry instead of reading it.
#[tokio::test(flavor = "multi_thread")]
async fn s6_no_resume_flag_starts_fresh() {
    let _serial = serial().await;
    let journal = journal();
    let http = http().await;
    let gw = Arc::new(FakeGateway::start().await.unwrap());
    let store = Recording::new(&gw);
    default_run(&http, &gw, &store, |_| {}).await;
    gw.clear_frames();
    let mark = journal.len();

    let app = builder(&http, &gw, &store, &Probe::new(), Module::new("ops"))
        .build()
        .unwrap();
    assert!(app.resume_sessions());
    let run = tokio::spawn(app.run_with_args(["--no-resume"]));
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("an IDENTIFY");
    wait_sessions(&gw, 1).await;
    // The process signal handlers keep it running; abort it.
    run.abort();
    assert!(ops_of(&gw.frames(), 6).is_empty(), "no RESUME");
    assert_eq!(store.stored(KEY).await, None);
    let entries = journal.since(mark);
    let disabled = position(&entries, |e| {
        is_log(e, "session resume disabled (--no-resume)") && e.contains("INFO")
    })
    .expect("the info line");
    let deleted = position(&entries, |e| e == format!("delete:{KEY}")).expect("a delete");
    assert!(disabled < deleted, "{entries:#?}");
    assert!(
        !entries.iter().any(|e| e.starts_with("get:")),
        "{entries:#?}"
    );
}
