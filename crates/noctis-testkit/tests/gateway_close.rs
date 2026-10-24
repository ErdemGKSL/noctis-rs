//! `Gateway::close_all` and `Gateway::closed` against `FakeGateway` and a
//! silent WebSocket server (Story 5.8, Matrix 9-15).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::net::SocketAddr;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use noctis_core::{App, Runtime};
use noctis_gateway::{
    CloseMode, EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink, ShardEvent,
};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_testkit::fake::{
    FakeGateway, FakeGatewayConfig, FrameKind, IdentifyReply, RecordedFrame,
};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_websockets::{Message, ServerBuilder};
use twilight_gateway::Intents;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

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
}

/// Installs the rustls provider twilight's connector needs.
fn install_provider() {
    App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
}

fn options(url: String, total: u32, close_timeout: Duration) -> GatewayOptions {
    GatewayOptions {
        app: Arc::from("default"),
        token: "fake-token".to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some(url),
        event_types: EventTypeFlags::all(),
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap())),
        session_store: Arc::new(noctis_ports::memory::MemorySessionStore::new()),
        resume: true,
        shards: 0..total,
        total: NonZeroU32::new(total).unwrap(),
        max_concurrency: NonZeroU16::new(16).unwrap(),
        close_timeout,
    }
}

async fn recv(events: &mut mpsc::UnboundedReceiver<ShardEvent>) -> Option<ShardEvent> {
    tokio::time::timeout(WAIT, events.recv())
        .await
        .expect("event within the wait")
}

/// Receives every event until the channel closes.
async fn rest(events: &mut mpsc::UnboundedReceiver<ShardEvent>) -> Vec<ShardEvent> {
    let mut all = Vec::new();
    while let Some(event) = recv(events).await {
        all.push(event);
    }
    all
}

/// Waits for `Ready` from shards `0..total`.
async fn all_ready(events: &mut mpsc::UnboundedReceiver<ShardEvent>, total: u32) {
    let mut ready = 0;
    while ready < total {
        match recv(events).await {
            Some(ShardEvent::Ready { .. }) => ready += 1,
            Some(_) => {}
            None => panic!("channel closed before READY"),
        }
    }
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

fn identifies(gw: &FakeGateway) -> usize {
    gw.frames().iter().filter(|f| f.op() == Some(2)).count()
}

/// Two READY shards, closed with `first` then `second`.
async fn close_two(first: CloseMode, second: Option<CloseMode>) -> (FakeGateway, Vec<ShardEvent>) {
    install_provider();
    let gw = FakeGateway::start().await.unwrap();
    let (mut gateway, mut events) = Gateway::start(
        options(gw.url(), 2, Duration::from_secs(5)),
        Arc::new(Counting::default()),
    )
    .unwrap();
    all_ready(&mut events, 2).await;
    gateway.close_all(first);
    if let Some(second) = second {
        gateway.close_all(second);
    }
    tokio::time::timeout(WAIT, gateway.closed()).await.unwrap();
    drop(gateway);
    let after = rest(&mut events).await;
    (gw, after)
}

/// Matrix 9: a resumable close sends 4000 per connection and reports
/// `Closed` per shard, nothing else; no reconnect follows.
#[tokio::test(flavor = "multi_thread")]
async fn close_resumable() {
    let (gw, mut after) = close_two(CloseMode::Resumable, None).await;
    after.sort_by_key(|e| match e {
        ShardEvent::Closed { shard } => *shard,
        _ => u32::MAX,
    });
    assert_eq!(
        after,
        [
            ShardEvent::Closed { shard: 0 },
            ShardEvent::Closed { shard: 1 }
        ]
    );
    let closes = client_closes(&gw.frames());
    assert_eq!(closes, [Some(4000), Some(4000)]);
    let mut per_connection: Vec<u64> = gw
        .frames()
        .iter()
        .filter(|f| matches!(f.kind, FrameKind::Close { .. }))
        .map(|f| f.connection)
        .collect();
    per_connection.sort_unstable();
    per_connection.dedup();
    assert_eq!(per_connection.len(), 2);
    let connections = gw.connections().len();
    assert_eq!(connections, 2);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(gw.connections().len(), connections, "no reconnect");
    assert_eq!(identifies(&gw), 2);
}

/// Matrix 10: a fresh close sends 1000.
#[tokio::test(flavor = "multi_thread")]
async fn close_fresh() {
    let (gw, after) = close_two(CloseMode::Fresh, None).await;
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|e| matches!(e, ShardEvent::Closed { .. })));
    assert_eq!(client_closes(&gw.frames()), [Some(1000), Some(1000)]);
}

/// Matrix 11: the first `close_all` wins.
#[tokio::test(flavor = "multi_thread")]
async fn first_close_wins() {
    let (gw, _) = close_two(CloseMode::Resumable, Some(CloseMode::Fresh)).await;
    assert_eq!(client_closes(&gw.frames()), [Some(4000), Some(4000)]);
}

/// Matrix 12: a shard that is not identified stops at once, without a
/// close frame or another IDENTIFY.
#[tokio::test(flavor = "multi_thread")]
async fn close_before_identified() {
    install_provider();
    let gw = FakeGateway::start_with(FakeGatewayConfig {
        identify: IdentifyReply::Ignore,
        ..FakeGatewayConfig::default()
    })
    .await
    .unwrap();
    let (mut gateway, mut events) = Gateway::start(
        options(gw.url(), 1, Duration::from_secs(5)),
        Arc::new(Counting::default()),
    )
    .unwrap();
    gw.wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .unwrap();
    let started = Instant::now();
    gateway.close_all(CloseMode::Resumable);
    tokio::time::timeout(Duration::from_secs(1), gateway.closed())
        .await
        .expect("closed within 1 s");
    assert!(started.elapsed() <= Duration::from_secs(1));
    drop(gateway);
    assert_eq!(rest(&mut events).await, [ShardEvent::Closed { shard: 0 }]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(client_closes(&gw.frames()).is_empty());
    assert_eq!(identifies(&gw), 1);
}

/// A WebSocket server that sends HELLO, answers the first IDENTIFY with
/// READY and then never reads again; a message on `dispatch` makes it send
/// one `MESSAGE_DELETE`.
async fn silent_server(dispatch: oneshot::Receiver<()>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (_request, mut ws) = ServerBuilder::new().accept(stream).await.unwrap();
        let hello = json!({ "op": 10, "d": { "heartbeat_interval": 41_250 } });
        ws.send(Message::text(hello.to_string())).await.unwrap();
        while let Some(Ok(message)) = ws.next().await {
            let Some(text) = message.as_text() else {
                continue;
            };
            if text.contains("\"op\":2") {
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
        if dispatch.await.is_ok() {
            let delete = json!({
                "op": 0, "s": 2, "t": "MESSAGE_DELETE",
                "d": { "id": "3", "channel_id": "2", "guild_id": "3" },
            });
            let _ = ws.send(Message::text(delete.to_string())).await;
        }
        // Never read again; hold the connection open.
        std::future::pending::<()>().await;
    });
    addr
}

/// Matrix 13 and 14: a server that never acknowledges the close; the
/// shard stops after `close_timeout`, and a dispatch after `close_all`
/// never reaches the sink.
#[tokio::test(flavor = "multi_thread")]
async fn close_unacknowledged() {
    install_provider();
    let (tx, rx) = oneshot::channel();
    let addr = silent_server(rx).await;
    let sink = Arc::new(Counting::default());
    let (mut gateway, mut events) = Gateway::start(
        options(format!("ws://{addr}"), 1, Duration::from_millis(200)),
        Arc::clone(&sink) as Arc<dyn InboundSink>,
    )
    .unwrap();
    all_ready(&mut events, 1).await;
    let before = sink.len();
    let started = Instant::now();
    gateway.close_all(CloseMode::Resumable);
    tx.send(()).unwrap();
    tokio::time::timeout(WAIT, gateway.closed()).await.unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed <= Duration::from_millis(200) + Duration::from_secs(2),
        "{elapsed:?}"
    );
    drop(gateway);
    assert_eq!(rest(&mut events).await, [ShardEvent::Closed { shard: 0 }]);
    assert_eq!(
        sink.len(),
        before,
        "Matrix 14: nothing pushed after close_all"
    );
}

/// Matrix 15: a fatal close ends the shard without a request; `closed`
/// returns, and again at once.
#[tokio::test(flavor = "multi_thread")]
async fn fatal_close_without_request() {
    install_provider();
    let gw = FakeGateway::start_with(FakeGatewayConfig {
        identify: IdentifyReply::Close(4004),
        ..FakeGatewayConfig::default()
    })
    .await
    .unwrap();
    let (mut gateway, mut events) = Gateway::start(
        options(gw.url(), 1, Duration::from_secs(5)),
        Arc::new(Counting::default()),
    )
    .unwrap();
    let all = rest(&mut events).await;
    assert_eq!(
        all.last(),
        Some(&ShardEvent::Ended {
            shard: 0,
            close_code: Some(4004)
        }),
        "{all:?}"
    );
    tokio::time::timeout(WAIT, gateway.closed()).await.unwrap();
    let started = Instant::now();
    gateway.closed().await;
    assert!(started.elapsed() < Duration::from_millis(50));
    assert!(format!("{gateway:?}").contains("running: 0"));
}
