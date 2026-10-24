//! Session resume through the `SessionStore` (Story 12.2): `Gateway::start`
//! against `FakeGateway` (R1-R3, W1-W7, L11, the stored resume URL) and
//! whole Apps sharing one store (A1-A5). A6 (log redaction) lives in
//! `session_log_redaction.rs`, which installs a global subscriber. Story
//! 12.3 F1-F6: a start with `GatewayOptions::resume == false` clears the
//! stored entry instead of loading it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::net::SocketAddr;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use noctis_core::{App, AppBuilder, Error, Runtime, Token};
use noctis_gateway::{
    CloseMode, EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink, ShardEvent,
};
use noctis_ports::memory::{MemoryIdentifyQueue, MemorySessionStore};
use noctis_ports::{
    BoxFuture, IdentifyQueue, Lease, PortError, SessionStore, decode_value, encode_value,
};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeGatewayConfig, FrameKind, GatewayBot, IdentifyReply,
    RecordedFrame, ResumeReply, SessionStartLimit,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_websockets::{Message, ServerBuilder};
use twilight_gateway::Intents;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// Shard 0 of 1's key, as the gateway passes it.
const KEY: &str = "shard:0:1";
/// The same key behind App `default`'s namespace.
const APP_KEY: &str = "default:session:shard:0:1";

// ---- the recording store ----

/// One `SessionStore` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Get,
    Put,
    Delete,
}

/// How `put` behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PutMode {
    Store,
    Fail,
    Hang,
}

/// How `delete` behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteMode {
    Delete,
    Fail,
    Hang,
}

/// A `MemorySessionStore` recording every call.
struct Recording {
    inner: MemorySessionStore,
    ops: Mutex<Vec<(Op, String, Option<Duration>)>>,
    put: Mutex<PutMode>,
    delete: Mutex<DeleteMode>,
    /// Whether `get` never answers.
    get_hangs: Mutex<bool>,
}

impl Recording {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: MemorySessionStore::new(),
            ops: Mutex::new(Vec::new()),
            put: Mutex::new(PutMode::Store),
            delete: Mutex::new(DeleteMode::Delete),
            get_hangs: Mutex::new(false),
        })
    }

    fn with_put(mode: PutMode) -> Arc<Self> {
        let store = Self::new();
        *store.put.lock().unwrap() = mode;
        store
    }

    fn record(&self, op: Op, key: &str, ttl: Option<Duration>) {
        self.ops.lock().unwrap().push((op, key.to_owned(), ttl));
    }

    /// The ops so far.
    fn ops(&self) -> Vec<Op> {
        self.ops
            .lock()
            .unwrap()
            .iter()
            .map(|(op, ..)| *op)
            .collect()
    }

    /// Every key used so far.
    fn keys(&self) -> Vec<String> {
        self.ops
            .lock()
            .unwrap()
            .iter()
            .map(|(_, key, _)| key.clone())
            .collect()
    }

    fn clear_ops(&self) {
        self.ops.lock().unwrap().clear();
    }

    /// The stored entry under `key`, decoded.
    async fn stored(&self, key: &str) -> Option<Stored> {
        let raw = self.inner.get(key).await.unwrap()?;
        decode_value(1, &raw).unwrap()
    }

    /// Writes an entry under `key` without recording it.
    async fn seed(&self, key: &str, stored: &Stored) {
        let raw = encode_value(1, stored).unwrap();
        self.inner.put(key, raw, None).await.unwrap();
    }
}

impl SessionStore for Recording {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.record(Op::Get, key, None);
        if *self.get_hangs.lock().unwrap() {
            return Box::pin(std::future::pending());
        }
        self.inner.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.record(Op::Put, key, ttl);
        match *self.put.lock().unwrap() {
            PutMode::Store => self.inner.put(key, value, ttl),
            PutMode::Fail => Box::pin(async { Err(PortError::backend("down")) }),
            PutMode::Hang => Box::pin(std::future::pending()),
        }
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.record(Op::Delete, key, None);
        match *self.delete.lock().unwrap() {
            DeleteMode::Delete => self.inner.delete(key),
            DeleteMode::Fail => Box::pin(async { Err(PortError::backend("down")) }),
            DeleteMode::Hang => Box::pin(std::future::pending()),
        }
    }
}

/// The stored value's layout (the gateway's private struct).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stored {
    session_id: String,
    sequence: u64,
    resume_url: String,
}

/// Records each bucket asked for, then delegates.
struct RecordingQueue {
    inner: MemoryIdentifyQueue,
    buckets: Arc<Mutex<Vec<u32>>>,
}

impl IdentifyQueue for RecordingQueue {
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>> {
        self.buckets.lock().unwrap().push(bucket);
        self.inner.acquire(bucket)
    }
}

// ---- Gateway-level helpers ----

/// Counts the items pushed into it.
#[derive(Default)]
struct Counting(Mutex<Vec<Inbound>>);

impl InboundSink for Counting {
    fn push(&self, inbound: Inbound) {
        self.0.lock().unwrap().push(inbound);
    }
}

impl Counting {
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// Polls until at least `n` items arrived, within [`WAIT`].
    async fn wait_for(&self, n: usize) {
        let deadline = Instant::now() + WAIT;
        while self.len() < n {
            assert!(
                Instant::now() < deadline,
                "only {} of {n} items",
                self.len()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Installs the rustls provider twilight's connector needs.
fn install_provider() {
    App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
}

fn options(
    url: String,
    total: u32,
    close_timeout: Duration,
    store: Arc<dyn SessionStore>,
    queue: Arc<dyn IdentifyQueue>,
    resume: bool,
) -> GatewayOptions {
    GatewayOptions {
        app: Arc::from("default"),
        token: "fake-token".to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some(url),
        event_types: EventTypeFlags::all(),
        identify_queue: queue,
        session_store: store,
        resume,
        shards: 0..total,
        total: NonZeroU32::new(total).unwrap(),
        max_concurrency: NonZeroU16::new(16).unwrap(),
        close_timeout,
    }
}

fn queue() -> Arc<dyn IdentifyQueue> {
    Arc::new(MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap()))
}

/// A running Gateway with its sink and events.
struct Running {
    gateway: Gateway,
    events: mpsc::UnboundedReceiver<ShardEvent>,
    sink: Arc<Counting>,
}

fn start(url: String, store: &Arc<Recording>, close_timeout: Duration) -> Running {
    start_with_queue(url, store, close_timeout, queue())
}

fn start_with_queue(
    url: String,
    store: &Arc<Recording>,
    close_timeout: Duration,
    queue: Arc<dyn IdentifyQueue>,
) -> Running {
    start_mode(url, store, close_timeout, queue, true)
}

/// Starts shard 0 of 1 with `resume` as `GatewayOptions::resume`.
fn start_mode(
    url: String,
    store: &Arc<Recording>,
    close_timeout: Duration,
    queue: Arc<dyn IdentifyQueue>,
    resume: bool,
) -> Running {
    let sink = Arc::new(Counting::default());
    let store: Arc<dyn SessionStore> = Arc::clone(store) as Arc<dyn SessionStore>;
    let (gateway, events) = Gateway::start(
        options(url, 1, close_timeout, store, queue, resume),
        Arc::clone(&sink) as Arc<dyn InboundSink>,
    )
    .unwrap();
    Running {
        gateway,
        events,
        sink,
    }
}

async fn recv(events: &mut mpsc::UnboundedReceiver<ShardEvent>) -> Option<ShardEvent> {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .expect("event within the wait")
}

/// Receives until `pred` holds for an event, returning it.
async fn until(
    events: &mut mpsc::UnboundedReceiver<ShardEvent>,
    pred: impl Fn(&ShardEvent) -> bool,
) -> ShardEvent {
    loop {
        match recv(events).await {
            Some(event) if pred(&event) => return event,
            Some(_) => {}
            None => panic!("channel closed"),
        }
    }
}

fn is_ready(event: &ShardEvent) -> bool {
    matches!(event, ShardEvent::Ready { .. })
}

fn is_resumed(event: &ShardEvent) -> bool {
    matches!(event, ShardEvent::Resumed { .. })
}

fn is_closed(event: &ShardEvent) -> bool {
    matches!(event, ShardEvent::Closed { .. })
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

/// Sends `n` `MESSAGE_DELETE` dispatches and waits until the sink has them.
async fn dispatch(gw: &FakeGateway, running: &Running, n: usize) {
    let before = running.sink.len();
    for _ in 0..n {
        assert_eq!(
            gw.dispatch("MESSAGE_DELETE", json!({ "id": "3", "channel_id": "2" })),
            1
        );
    }
    running.sink.wait_for(before + n).await;
}

/// Closes with `mode` and waits for `Closed`, returning how long it took.
async fn close(running: &mut Running, mode: CloseMode) -> Duration {
    let started = Instant::now();
    running.gateway.close_all(mode);
    until(&mut running.events, is_closed).await;
    let elapsed = started.elapsed();
    tokio::time::timeout(WAIT, running.gateway.closed())
        .await
        .unwrap();
    elapsed
}

/// A first run: identify, `dispatches` dispatches, a resumable close. The
/// entry is stored with sequence `1 + dispatches`.
async fn first_run(gw: &FakeGateway, store: &Arc<Recording>, dispatches: usize) {
    let mut running = start(gw.url(), store, Duration::from_secs(5));
    until(&mut running.events, is_ready).await;
    dispatch(gw, &running, dispatches).await;
    close(&mut running, CloseMode::Resumable).await;
}

// ---- L11: close while the start-time get is pending ----

#[tokio::test(flavor = "multi_thread")]
async fn l11_close_during_pending_get_ends_without_connecting() {
    install_provider();
    for (mode, ops) in [
        (CloseMode::Fresh, &[Op::Get, Op::Delete][..]),
        (CloseMode::Resumable, &[Op::Get][..]),
    ] {
        let gw = FakeGateway::start().await.unwrap();
        let store = Recording::new();
        *store.get_hangs.lock().unwrap() = true;
        let mut running = start(gw.url(), &store, Duration::from_secs(5));
        let deadline = Instant::now() + WAIT;
        while store.ops().is_empty() {
            assert!(Instant::now() < deadline, "{mode:?}: no get");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let elapsed = close(&mut running, mode).await;
        assert!(elapsed < Duration::from_secs(2), "{mode:?}: {elapsed:?}");
        assert!(gw.frames().is_empty(), "{mode:?}: no frame");
        assert_eq!(gw.live_sessions(), 0, "{mode:?}: no connection");
        assert_eq!(store.ops(), ops, "{mode:?}");
    }
}

// ---- the stored resume URL decides where the shard connects ----

#[tokio::test(flavor = "multi_thread")]
async fn stored_resume_url_is_where_the_shard_resumes() {
    install_provider();
    let a = FakeGateway::start().await.unwrap();
    let b = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    // Stores B's session with `resume_url = B.url()`.
    first_run(&b, &store, 0).await;
    let seeded = store.stored(KEY).await.unwrap();
    assert_eq!(seeded.resume_url, b.url());
    b.clear_frames();

    let mut running = start(a.url(), &store, Duration::from_secs(5));
    until(&mut running.events, is_resumed).await;
    let resumes = ops_of(&b.frames(), 6);
    assert_eq!(resumes.len(), 1);
    let d = resumes[0].json().unwrap()["d"].clone();
    assert_eq!(d["session_id"], seeded.session_id.as_str());
    assert_eq!(d["seq"], seeded.sequence);
    assert!(
        ops_of(&a.frames(), 6).is_empty(),
        "no RESUME to gateway_url"
    );
    close(&mut running, CloseMode::Resumable).await;
}

// ---- W1: resumable, acked ----

#[tokio::test(flavor = "multi_thread")]
async fn w1_acked_resumable_close_stores_the_session() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    until(&mut running.events, is_ready).await;
    dispatch(&gw, &running, 2).await;
    running.gateway.close_all(CloseMode::Resumable);
    until(&mut running.events, is_closed).await;
    // The put finished before `Closed` was sent.
    assert_eq!(store.ops(), [Op::Get, Op::Put]);
    assert_eq!(store.keys(), [KEY, KEY]);
    assert_eq!(store.ops.lock().unwrap()[1].2, None, "no TTL");
    assert_eq!(
        store.stored(KEY).await,
        Some(Stored {
            session_id: "fake-session-1".to_owned(),
            sequence: 3,
            resume_url: gw.url(),
        })
    );
    assert_eq!(client_closes(&gw.frames()), [Some(4000)]);
}

// ---- R1 and W2: resumed, then stored again ----

#[tokio::test(flavor = "multi_thread")]
async fn r1_stored_session_resumes_without_identify() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_run(&gw, &store, 2).await;
    assert_eq!(ops_of(&gw.frames(), 2).len(), 1);
    gw.clear_frames();
    store.clear_ops();

    let buckets = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(RecordingQueue {
        inner: MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap()),
        buckets: Arc::clone(&buckets),
    });
    let mut running = start_with_queue(gw.url(), &store, Duration::from_secs(5), queue);
    until(&mut running.events, is_resumed).await;
    let resumes = ops_of(&gw.frames(), 6);
    assert_eq!(resumes.len(), 1);
    let d = resumes[0].json().unwrap()["d"].clone();
    assert_eq!(d["session_id"], "fake-session-1");
    assert_eq!(d["seq"], 3);
    assert!(ops_of(&gw.frames(), 2).is_empty(), "no IDENTIFY");
    assert!(buckets.lock().unwrap().is_empty(), "no identify lease");
    assert_eq!(store.ops(), [Op::Get], "entry kept");

    // W2: RESUMED is seq 4, the dispatch seq 5.
    dispatch(&gw, &running, 1).await;
    close(&mut running, CloseMode::Resumable).await;
    assert_eq!(store.ops(), [Op::Get, Op::Put]);
    assert_eq!(store.stored(KEY).await.unwrap().sequence, 5);
}

// ---- R2: rejected ----

/// Starts against `gw` with `store`, expecting a rejected resume: one
/// delete, then IDENTIFY and READY.
async fn expect_rejected(gw: &FakeGateway, store: &Arc<Recording>) {
    let mut running = start(gw.url(), store, Duration::from_secs(5));
    until(&mut running.events, is_ready).await;
    assert_eq!(store.ops(), [Op::Get, Op::Delete]);
    let frames = gw.frames();
    assert_eq!(ops_of(&frames, 6).len(), 1, "one RESUME");
    assert_eq!(ops_of(&frames, 2).len(), 1, "one IDENTIFY");
    assert_eq!(store.stored(KEY).await, None);
    // R5: nothing more until the close, which stores the new session.
    close(&mut running, CloseMode::Resumable).await;
    assert_eq!(store.ops(), [Op::Get, Op::Delete, Op::Put]);
    let stored = store.stored(KEY).await.unwrap();
    assert_ne!(stored.session_id, "unknown");
}

#[tokio::test(flavor = "multi_thread")]
async fn r2_rejected_by_invalid_session_false() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_run(&gw, &store, 0).await;
    gw.clear_frames();
    store.clear_ops();
    gw.set_config(FakeGatewayConfig {
        resume: ResumeReply::InvalidSession { resumable: false },
        ..FakeGatewayConfig::default()
    });
    expect_rejected(&gw, &store).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn r2_unknown_session_is_rejected() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    store
        .seed(
            KEY,
            &Stored {
                session_id: "unknown".to_owned(),
                sequence: 9,
                resume_url: gw.url(),
            },
        )
        .await;
    expect_rejected(&gw, &store).await;
}

// ---- R3: op 9 resumable ----

#[tokio::test(flavor = "multi_thread")]
async fn r3_resumable_invalid_session_keeps_the_entry() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_run(&gw, &store, 0).await;
    gw.clear_frames();
    store.clear_ops();
    gw.set_config(FakeGatewayConfig {
        resume: ResumeReply::InvalidSession { resumable: true },
        ..FakeGatewayConfig::default()
    });
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    gw.wait_for_frame(WAIT, |f| f.op() == Some(6))
        .await
        .unwrap();
    // Twilight resumes again after op 9 `d: true`; let it in this time.
    gw.set_config(FakeGatewayConfig::default());
    until(&mut running.events, is_resumed).await;
    assert!(ops_of(&gw.frames(), 6).len() >= 2);
    assert!(ops_of(&gw.frames(), 2).is_empty());
    assert_eq!(store.ops(), [Op::Get], "no delete");
    close(&mut running, CloseMode::Resumable).await;
}

// ---- W3: ack timeout ----

/// A WebSocket server that sends HELLO, answers the first IDENTIFY with
/// READY and then never reads again.
async fn silent_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (_request, mut ws) = ServerBuilder::new().accept(stream).await.unwrap();
        let hello = json!({ "op": 10, "d": { "heartbeat_interval": 41_250 } });
        ws.send(Message::text(hello.to_string())).await.unwrap();
        while let Some(Ok(message)) = ws.next().await {
            if message
                .as_text()
                .is_some_and(|text| text.contains("\"op\":2"))
            {
                break;
            }
        }
        let ready = json!({
            "op": 0, "s": 1, "t": "READY",
            "d": {
                "v": 10,
                "user": { "id": "1", "username": "b", "discriminator": "0", "avatar": null },
                "guilds": [],
                "session_id": "silent",
                "resume_gateway_url": format!("ws://{addr}"),
                "application": { "id": "1", "flags": 0 },
            },
        });
        ws.send(Message::text(ready.to_string())).await.unwrap();
        std::future::pending::<()>().await;
    });
    addr
}

#[tokio::test(flavor = "multi_thread")]
async fn w3_unacknowledged_close_stores_nothing() {
    install_provider();
    let addr = silent_server().await;
    let store = Recording::new();
    let mut running = start(format!("ws://{addr}"), &store, Duration::from_millis(200));
    until(&mut running.events, is_ready).await;
    let elapsed = close(&mut running, CloseMode::Resumable).await;
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert_eq!(store.ops(), [Op::Get]);
    assert_eq!(store.stored(KEY).await, None);
}

// ---- W4 and W5: not identified, fresh ----

async fn ignoring_gateway() -> FakeGateway {
    FakeGateway::start_with(FakeGatewayConfig {
        identify: IdentifyReply::Ignore,
        ..FakeGatewayConfig::default()
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn w4_close_before_ready_stores_nothing() {
    install_provider();
    let gw = ignoring_gateway().await;
    let store = Recording::new();
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .unwrap();
    close(&mut running, CloseMode::Resumable).await;
    assert_eq!(store.ops(), [Op::Get]);
    assert!(client_closes(&gw.frames()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn w5_fresh_close_deletes_the_entry() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    until(&mut running.events, is_ready).await;
    close(&mut running, CloseMode::Fresh).await;
    assert_eq!(store.ops(), [Op::Get, Op::Delete]);
    assert_eq!(client_closes(&gw.frames()), [Some(1000)]);

    // Not identified: still deleted, without a close frame.
    let gw = ignoring_gateway().await;
    let store = Recording::new();
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .unwrap();
    close(&mut running, CloseMode::Fresh).await;
    assert_eq!(store.ops(), [Op::Get, Op::Delete]);
    assert!(client_closes(&gw.frames()).is_empty());
}

// ---- W6: put fails or hangs ----

#[tokio::test(flavor = "multi_thread")]
async fn w6_failing_or_hanging_put_keeps_the_close_bound() {
    install_provider();
    let close_timeout = Duration::from_millis(500);
    for mode in [PutMode::Fail, PutMode::Hang] {
        let gw = FakeGateway::start().await.unwrap();
        let store = Recording::with_put(mode);
        let mut running = start(gw.url(), &store, close_timeout);
        until(&mut running.events, is_ready).await;
        let elapsed = close(&mut running, CloseMode::Resumable).await;
        assert!(
            elapsed <= close_timeout + Duration::from_millis(100),
            "{mode:?}: {elapsed:?}"
        );
        assert_eq!(store.ops(), [Op::Get, Op::Put], "{mode:?}");
        assert_eq!(store.stored(KEY).await, None, "{mode:?}");
    }
}

// ---- W7: no request ----

#[tokio::test(flavor = "multi_thread")]
async fn w7_fatal_close_and_reconnect_touch_nothing() {
    install_provider();
    // An in-process reconnect resumes without a store op.
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    until(&mut running.events, is_ready).await;
    gw.close(4000, "reconnect").unwrap();
    until(&mut running.events, is_resumed).await;
    assert_eq!(store.ops(), [Op::Get]);
    drop(running);

    // A fatal close ends the shard without a request.
    let gw = FakeGateway::start_with(FakeGatewayConfig {
        identify: IdentifyReply::Close(4004),
        ..FakeGatewayConfig::default()
    })
    .await
    .unwrap();
    let store = Recording::new();
    let mut running = start(gw.url(), &store, Duration::from_secs(5));
    until(&mut running.events, |e| {
        matches!(e, ShardEvent::Ended { .. })
    })
    .await;
    tokio::time::timeout(WAIT, running.gateway.closed())
        .await
        .unwrap();
    assert_eq!(store.ops(), [Op::Get]);
}

// ---- Story 12.3 F1-F6: start with `resume: false` ----

/// A recording identify queue and the buckets it was asked for.
fn recording_queue() -> (Arc<dyn IdentifyQueue>, Arc<Mutex<Vec<u32>>>) {
    let buckets = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(RecordingQueue {
        inner: MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap()),
        buckets: Arc::clone(&buckets),
    });
    (queue, buckets)
}

/// F1: a stored entry the gateway would resume is deleted first, and the
/// shard identifies through the queue.
#[tokio::test(flavor = "multi_thread")]
async fn f1_fresh_start_deletes_a_stored_entry_and_identifies() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_run(&gw, &store, 2).await;
    assert!(store.stored(KEY).await.is_some(), "seeded");
    gw.clear_frames();
    store.clear_ops();

    let (queue, buckets) = recording_queue();
    let mut running = start_mode(gw.url(), &store, Duration::from_secs(5), queue, false);
    until(&mut running.events, is_ready).await;
    assert_eq!(store.ops(), [Op::Delete]);
    assert_eq!(store.keys(), [KEY]);
    let frames = gw.frames();
    assert!(ops_of(&frames, 6).is_empty(), "no RESUME");
    assert_eq!(ops_of(&frames, 2).len(), 1, "one IDENTIFY");
    assert_eq!(buckets.lock().unwrap().len(), 1, "through the queue");
    assert_eq!(store.stored(KEY).await, None, "entry gone");
    close(&mut running, CloseMode::Fresh).await;
}

/// F2: deleting a missing entry is fine.
#[tokio::test(flavor = "multi_thread")]
async fn f2_fresh_start_without_an_entry_identifies() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    let mut running = start_mode(gw.url(), &store, Duration::from_secs(5), queue(), false);
    until(&mut running.events, is_ready).await;
    assert_eq!(store.ops(), [Op::Delete]);
    assert_eq!(ops_of(&gw.frames(), 2).len(), 1);
    close(&mut running, CloseMode::Fresh).await;
}

/// Captures every log line of the current thread while the guard lives.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Lines {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Lines {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// A thread-local capturing subscriber; the test runs on one thread so
/// the shard task logs into it.
fn capture_lines() -> (Lines, impl Sized) {
    use tracing_subscriber::util::SubscriberInitExt;

    let lines = Lines::default();
    let guard = tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::DEBUG)
        .with_ansi(false)
        .with_writer(lines.clone())
        .finish()
        .set_default();
    (lines, guard)
}

/// F3: a failing delete identifies at once.
#[tokio::test(flavor = "current_thread")]
async fn f3_failing_start_delete_identifies_at_once() {
    let (lines, _guard) = capture_lines();
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    *store.delete.lock().unwrap() = DeleteMode::Fail;
    let started = Instant::now();
    let mut running = start_mode(gw.url(), &store, Duration::from_secs(5), queue(), false);
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("an IDENTIFY");
    assert!(started.elapsed() < Duration::from_secs(4), "at once");
    until(&mut running.events, is_ready).await;
    assert_eq!(store.ops(), [Op::Delete], "no retry");
    let text = lines.text();
    assert!(
        text.lines()
            .any(|l| l.contains("WARN") && l.contains("session store delete failed")),
        "{text}"
    );
    *store.delete.lock().unwrap() = DeleteMode::Delete;
    close(&mut running, CloseMode::Resumable).await;
}

/// F3: a hanging delete is given up after `STORE_TIMEOUT` (5 s).
#[tokio::test(flavor = "current_thread")]
async fn f3_hanging_start_delete_times_out() {
    let (lines, _guard) = capture_lines();
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    *store.delete.lock().unwrap() = DeleteMode::Hang;
    let started = Instant::now();
    let mut running = start_mode(gw.url(), &store, Duration::from_secs(5), queue(), false);
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("an IDENTIFY");
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(4900), "{elapsed:?}");
    assert!(elapsed <= WAIT, "{elapsed:?}");
    until(&mut running.events, is_ready).await;
    assert_eq!(store.ops(), [Op::Delete], "no retry");
    let text = lines.text();
    assert!(
        text.lines()
            .any(|l| l.contains("WARN") && l.contains("session store delete timed out")),
        "{text}"
    );
    close(&mut running, CloseMode::Resumable).await;
}

/// F4: a close while the start-time delete hangs ends the task without a
/// connection.
#[tokio::test(flavor = "multi_thread")]
async fn f4_close_during_start_delete_ends_without_connecting() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    *store.delete.lock().unwrap() = DeleteMode::Hang;
    let mut running = start_mode(gw.url(), &store, Duration::from_millis(500), queue(), false);
    let deadline = Instant::now() + WAIT;
    while store.ops().is_empty() {
        assert!(Instant::now() < deadline, "no delete");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let elapsed = close(&mut running, CloseMode::Fresh).await;
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert!(gw.frames().is_empty(), "no frame");
    assert_eq!(gw.live_sessions(), 0, "no connection");
    assert!(store.ops().iter().all(|op| *op == Op::Delete));
}

/// F5: only this run's key (current total) is cleared.
#[tokio::test(flavor = "multi_thread")]
async fn f5_fresh_start_leaves_other_totals_untouched() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    let other = Stored {
        session_id: "other".to_owned(),
        sequence: 4,
        resume_url: gw.url(),
    };
    store.seed("shard:0:2", &other).await;
    let mut running = start_mode(gw.url(), &store, Duration::from_secs(5), queue(), false);
    until(&mut running.events, is_ready).await;
    assert_eq!(store.keys(), [KEY]);
    close(&mut running, CloseMode::Fresh).await;
    assert_eq!(store.stored("shard:0:2").await, Some(other));
}

/// F6: `resume: true` keeps the 12.2 behaviour.
#[tokio::test(flavor = "multi_thread")]
async fn f6_resume_on_loads_first() {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_run(&gw, &store, 0).await;
    gw.clear_frames();
    store.clear_ops();
    let mut running = start_mode(gw.url(), &store, Duration::from_secs(5), queue(), true);
    until(&mut running.events, is_resumed).await;
    assert_eq!(store.ops().first(), Some(&Op::Get));
    assert_eq!(ops_of(&gw.frames(), 6).len(), 1);
    close(&mut running, CloseMode::Resumable).await;
}

// ---- App level ----

/// A builder on `Runtime::for_test()` wired to `http` and `gw`, with
/// `store` when given.
fn builder(http: &FakeDiscordHttp, gw: &FakeGateway, store: Option<&Arc<Recording>>) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url());
    if let Some(store) = store {
        builder.session_store(Arc::clone(store));
    }
    builder
}

/// A running App and the sender that stops it.
struct RunningApp {
    stop: oneshot::Sender<()>,
    handle: JoinHandle<Result<(), Error>>,
}

fn run(app: App) -> RunningApp {
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app.run_until(async {
        let _ = stopped.await;
    }));
    RunningApp { stop, handle }
}

impl RunningApp {
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

async fn http(shards: u32, max_concurrency: u32) -> FakeDiscordHttp {
    FakeDiscordHttp::start_with(GatewayBot {
        shards,
        session_start_limit: SessionStartLimit {
            max_concurrency,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    })
    .await
    .unwrap()
}

/// App 1 of A1-A4: identify, one dispatch, graceful shutdown.
async fn first_app(
    http: &FakeDiscordHttp,
    gw: &FakeGateway,
    store: &Arc<Recording>,
    shards: usize,
) {
    let app = run(builder(http, gw, Some(store)).build().unwrap());
    wait_sessions(gw, shards).await;
    settle().await;
    assert_eq!(
        gw.dispatch("MESSAGE_DELETE", json!({ "id": "3", "channel_id": "2" })),
        shards
    );
    settle().await;
    app.finish().await;
}

/// A1 and A5: a restarted App resumes through the shared store.
#[tokio::test(flavor = "multi_thread")]
async fn a1_restart_resumes() {
    let http = http(1, 1).await;
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_app(&http, &gw, &store, 1).await;
    // A5: only the gateway touched the store.
    assert_eq!(store.ops(), [Op::Get, Op::Put]);
    assert_eq!(store.keys(), [APP_KEY, APP_KEY]);
    assert_eq!(store.stored(APP_KEY).await.unwrap().sequence, 2);
    gw.clear_frames();
    store.clear_ops();

    let buckets = Arc::new(Mutex::new(Vec::new()));
    let mut second = builder(&http, &gw, Some(&store));
    second.identify_queue(RecordingQueue {
        inner: MemoryIdentifyQueue::new(NonZeroU16::MIN),
        buckets: Arc::clone(&buckets),
    });
    let app = run(second.build().unwrap());
    let resume = gw
        .wait_for_frame(WAIT, |f| f.op() == Some(6))
        .await
        .expect("a RESUME");
    let d = resume.json().unwrap()["d"].clone();
    assert_eq!(d["session_id"], "fake-session-1");
    assert_eq!(d["seq"], 2);
    wait_sessions(&gw, 1).await;
    settle().await;
    assert!(ops_of(&gw.frames(), 2).is_empty(), "no IDENTIFY");
    assert!(buckets.lock().unwrap().is_empty(), "no identify lease");
    app.finish().await;
    assert_eq!(store.ops(), [Op::Get, Op::Put]);
    assert_eq!(store.keys(), [APP_KEY, APP_KEY]);
}

/// A2: the restarted App meets a gateway that does not know the session.
#[tokio::test(flavor = "multi_thread")]
async fn a2_restart_rejected_identifies() {
    let http = http(1, 1).await;
    let old = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_app(&http, &old, &store, 1).await;
    drop(old);
    store.clear_ops();

    let gw = FakeGateway::start().await.unwrap();
    let app = run(builder(&http, &gw, Some(&store)).build().unwrap());
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .expect("an IDENTIFY");
    wait_sessions(&gw, 1).await;
    settle().await;
    let frames = gw.frames();
    assert_eq!(ops_of(&frames, 6).len(), 1, "one RESUME");
    assert_eq!(ops_of(&frames, 2).len(), 1, "exactly one IDENTIFY");
    app.finish().await;
    assert_eq!(store.ops(), [Op::Get, Op::Delete, Op::Put]);
}

/// A3: two shards, both stored, both resume.
#[tokio::test(flavor = "multi_thread")]
async fn a3_two_shards_resume() {
    let http = http(2, 2).await;
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_app(&http, &gw, &store, 2).await;
    let mut keys = store.keys();
    keys.sort();
    keys.dedup();
    assert_eq!(
        keys,
        ["default:session:shard:0:2", "default:session:shard:1:2"]
    );
    gw.clear_frames();

    let app = run(builder(&http, &gw, Some(&store)).build().unwrap());
    wait_sessions(&gw, 2).await;
    settle().await;
    assert_eq!(ops_of(&gw.frames(), 6).len(), 2);
    assert!(ops_of(&gw.frames(), 2).is_empty(), "no IDENTIFY");
    app.finish().await;
}

/// A4: without the shared store, the memory default is per App.
#[tokio::test(flavor = "multi_thread")]
async fn a4_memory_default_identifies() {
    let http = http(1, 1).await;
    let gw = FakeGateway::start().await.unwrap();
    let store = Recording::new();
    first_app(&http, &gw, &store, 1).await;
    gw.clear_frames();

    let app = run(builder(&http, &gw, None).build().unwrap());
    wait_sessions(&gw, 1).await;
    settle().await;
    assert_eq!(ops_of(&gw.frames(), 2).len(), 1);
    assert!(ops_of(&gw.frames(), 6).is_empty());
    app.finish().await;
}
