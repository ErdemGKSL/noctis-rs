//! `FakeGateway` driven by real twilight shards built through
//! `noctis_gateway::shard_config` from Apps with `AppBuilder::gateway_url`
//! (Story 5.3, Matrix 13-32). Rows twilight cannot send use a raw
//! `tokio-websockets` client.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use noctis_core::{App, Runtime, Token};
use noctis_gateway::shard_config;
use noctis_testkit::fake::{
    CloseError, FakeDiscordHttp, FakeGateway, FakeGatewayConfig, FrameKind, GatewayBot,
    HeartbeatSeen, IdentifyReply, Method, ReadyData, RecordedFrame, ResumeReply,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_websockets::{ClientBuilder, CloseCode, MaybeTlsStream, Message, WebSocketStream};
use twilight_gateway::error::{ReceiveMessageError, ReceiveMessageErrorType};
use twilight_gateway::queue::{InMemoryQueue, Queue};
use twilight_gateway::{
    CloseFrame, Event, EventTypeFlags, Intents, Session, Shard, ShardId, StreamExt as _,
};

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(10);

/// An identify queue that lets every shard identify at once
/// (`InMemoryQueue` spaces identifies 5 s apart).
#[derive(Debug)]
struct InstantQueue;

impl Queue for InstantQueue {
    fn enqueue(&self, _: u32) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(());
        rx
    }
}

/// An App on `Runtime::for_test()` whose shards go to `gw` (building it also
/// installs the rustls provider twilight's connector needs).
fn app(gw: &FakeGateway) -> App {
    App::builder()
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .gateway_url(gw.url())
        .build()
        .unwrap()
}

/// A shard connecting to `gw` through the App's gateway URL.
fn shard(gw: &FakeGateway, id: ShardId) -> Shard<InstantQueue> {
    let app = app(gw);
    let config =
        shard_config("fake-token", app.intents(), app.gateway_url(), InstantQueue).unwrap();
    Shard::with_config(id, config)
}

/// The next item of `shard`, within [`WAIT`].
async fn next_item<Q: Queue + Unpin>(
    shard: &mut Shard<Q>,
) -> Option<Result<Event, ReceiveMessageError>> {
    tokio::time::timeout(WAIT, shard.next_event(EventTypeFlags::all()))
        .await
        .expect("shard item within the wait")
}

/// Drives `shard` until an event matches `pred`, skipping errors.
async fn until<Q: Queue + Unpin>(shard: &mut Shard<Q>, pred: impl Fn(&Event) -> bool) -> Event {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let item = tokio::time::timeout_at(deadline, shard.next_event(EventTypeFlags::all()))
            .await
            .expect("matching event within the wait");
        match item {
            Some(Ok(event)) if pred(&event) => return event,
            Some(_) => {}
            None => panic!("shard ended before a matching event"),
        }
    }
}

fn ready_of(event: Event) -> twilight_model::gateway::payload::incoming::Ready {
    match event {
        Event::Ready(ready) => ready,
        other => panic!("expected READY, got {other:?}"),
    }
}

async fn ready<Q: Queue + Unpin>(
    shard: &mut Shard<Q>,
) -> twilight_model::gateway::payload::incoming::Ready {
    ready_of(until(shard, |e| matches!(e, Event::Ready(_))).await)
}

fn close_code(event: &Event) -> Option<u16> {
    match event {
        Event::GatewayClose(frame) => frame.as_ref().map(|f| f.code),
        _ => None,
    }
}

/// Waits until `cond` holds, polling every 10 ms, within [`WAIT`].
async fn eventually(cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(
            Instant::now() < deadline,
            "condition not met within the wait"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

type Raw = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A raw client connected to `gw`, past HELLO (returned).
async fn raw(gw: &FakeGateway) -> (Raw, Value) {
    let (mut ws, _) = ClientBuilder::new()
        .uri(&format!("{}/?v=10&encoding=json", gw.url()))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let hello = recv(&mut ws).await;
    assert_eq!(hello["op"], 10);
    (ws, hello)
}

async fn send(ws: &mut Raw, payload: &Value) {
    ws.send(Message::text(payload.to_string())).await.unwrap();
}

/// The next text frame as JSON.
async fn recv(ws: &mut Raw) -> Value {
    loop {
        let message = tokio::time::timeout(WAIT, ws.next())
            .await
            .expect("frame within the wait")
            .expect("stream open")
            .expect("valid frame");
        if let Some(text) = message.as_text() {
            return serde_json::from_str(text).unwrap();
        }
        assert!(
            !message.is_close(),
            "unexpected close {:?}",
            message.as_close()
        );
    }
}

/// Reads until the server's close frame and returns its code.
async fn recv_close(ws: &mut Raw) -> u16 {
    loop {
        let message = tokio::time::timeout(WAIT, ws.next())
            .await
            .expect("close within the wait")
            .expect("stream open")
            .expect("valid frame");
        if let Some((code, _)) = message.as_close() {
            return code.into();
        }
    }
}

/// Sends a client close and reads until the stream ends.
async fn client_close(mut ws: Raw, code: u16) {
    let code = CloseCode::try_from(code).unwrap();
    ws.send(Message::close(Some(code), "")).await.unwrap();
    while let Ok(Some(Ok(_))) = tokio::time::timeout(WAIT, ws.next()).await {}
}

fn identify(shard: Option<[u32; 2]>) -> Value {
    let mut d = json!({ "token": "Bot t", "intents": 1, "properties": {} });
    if let Some(shard) = shard {
        d["shard"] = json!(shard);
    }
    json!({ "op": 2, "d": d })
}

fn resume(session: &str, seq: u64) -> Value {
    json!({ "op": 6, "d": { "token": "Bot t", "session_id": session, "seq": seq } })
}

fn has_op(frames: &[RecordedFrame], op: u64) -> bool {
    frames.iter().any(|f| f.op() == Some(op))
}

fn config(f: impl FnOnce(&mut FakeGatewayConfig)) -> FakeGatewayConfig {
    let mut config = FakeGatewayConfig::default();
    f(&mut config);
    config
}

// ---- Matrix 13 (last row) ----

#[tokio::test(flavor = "multi_thread")]
async fn shard_config_accepts_every_normalized_url() {
    let a255 = format!("ws://{}", "a".repeat(255));
    for raw in [
        "ws://127.0.0.1:3000",
        "WS://Proxy:3000/",
        "wss://gateway.discord.gg",
        "Wss://[::1]:8080",
        "ws://p:1",
        "ws://p:65535",
        "ws://p:08080",
        a255.as_str(),
    ] {
        let app = App::builder()
            .runtime(&Runtime::for_test())
            .gateway_url(raw)
            .build()
            .unwrap();
        let config = shard_config("t", app.intents(), app.gateway_url(), InstantQueue);
        let config = config.unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(config.proxy_url(), app.gateway_url());
    }
}

// ---- Matrix 14 ----

#[tokio::test(flavor = "multi_thread")]
async fn two_fakes_are_independent() {
    let a = FakeGateway::start().await.unwrap();
    let b = FakeGateway::start().await.unwrap();
    assert_ne!(a.addr().port(), 0);
    assert_ne!(b.addr().port(), 0);
    assert_ne!(a.addr().port(), b.addr().port());
    assert_eq!(a.url(), format!("ws://127.0.0.1:{}", a.addr().port()));
    let (mut ws, _) = raw(&a).await;
    send(&mut ws, &json!({ "op": 1, "d": null })).await;
    assert_eq!(recv(&mut ws).await["op"], 11);
    assert_eq!(a.frames().len(), 1);
    assert_eq!(a.connections().len(), 1);
    assert!(b.frames().is_empty());
    assert!(b.connections().is_empty());
}

// ---- Matrix 15: smoke (AC) ----

#[tokio::test(flavor = "multi_thread")]
async fn smoke_shard_reaches_both_fakes() {
    let gw = FakeGateway::start().await.unwrap();
    let http = FakeDiscordHttp::start_with(GatewayBot {
        url: gw.url(),
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .build()
        .unwrap();
    let info = app
        .http()
        .gateway()
        .authed()
        .await
        .unwrap()
        .model()
        .await
        .unwrap();
    assert_eq!(info.url, gw.url());
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(requests[0].path, "/api/v10/gateway/bot");

    let queue = InMemoryQueue::new(1, 1000, Duration::from_secs(5), 1000);
    let config = shard_config("fake-token", app.intents(), app.gateway_url(), queue).unwrap();
    let mut shard = Shard::with_config(ShardId::ONE, config);

    let ready = ready(&mut shard).await;
    assert_eq!(ready.session_id, "fake-session-1");
    assert_eq!(ready.shard, Some(ShardId::ONE));
    assert_eq!(ready.user.id.get(), 1);

    assert_eq!(
        gw.dispatch(
            "MESSAGE_DELETE",
            json!({ "id": "3", "channel_id": "2", "guild_id": "1" })
        ),
        1
    );
    let event = until(&mut shard, |e| matches!(e, Event::MessageDelete(_))).await;
    let Event::MessageDelete(delete) = event else {
        unreachable!()
    };
    assert_eq!(delete.id.get(), 3);
    assert_eq!(shard.session().map(Session::sequence), Some(2));

    let identify = gw
        .wait_for_frame(WAIT, |f| f.op() == Some(2))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(identify["d"]["token"], "Bot fake-token");
    assert_eq!(identify["d"]["intents"], Intents::GUILDS.bits());
    assert_eq!(identify["d"]["shard"], json!([0, 1]));

    shard.close(CloseFrame::NORMAL);
    until(&mut shard, |e| matches!(e, Event::GatewayClose(_))).await;
    let close = gw
        .wait_for_frame(WAIT, |f| matches!(f.kind, FrameKind::Close { .. }))
        .await
        .unwrap();
    assert!(
        matches!(
            close.kind,
            FrameKind::Close {
                code: Some(1000),
                ..
            }
        ),
        "{close:?}"
    );
    eventually(|| gw.connections()[0].closed_at.is_some()).await;
    let connections = gw.connections();
    assert_eq!(connections.len(), 1);
    assert!(
        connections[0].uri.starts_with("/?v=10&encoding=json"),
        "{}",
        connections[0].uri
    );
}

// ---- Matrix 16 ----

#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_timing() {
    let gw = FakeGateway::start_with(config(|c| {
        c.heartbeat_interval = Duration::from_millis(100);
    }))
    .await
    .unwrap();
    let seen: Arc<Mutex<Vec<HeartbeatSeen>>> = Arc::default();
    let sink = Arc::clone(&seen);
    gw.on_heartbeat(move |beat| sink.lock().unwrap().push(beat.clone()));
    let mut shard = shard(&gw, ShardId::ONE);
    ready(&mut shard).await;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        let _ = tokio::time::timeout(
            Duration::from_millis(50),
            shard.next_event(EventTypeFlags::all()),
        )
        .await;
    }
    let seen = seen.lock().unwrap().clone();
    assert!(seen.len() >= 3, "{}", seen.len());
    for beat in &seen {
        if let Some(previous) = beat.since_previous {
            assert!(previous >= Duration::from_millis(50), "{previous:?}");
        }
        assert!(beat.seq.is_none() || beat.seq == Some(1), "{beat:?}");
    }
    assert_eq!(seen.last().unwrap().seq, Some(1));
    let identify_at = gw
        .frames()
        .iter()
        .find(|f| f.op() == Some(2))
        .unwrap()
        .received_at;
    for beat in seen.iter().filter(|b| b.received_at < identify_at) {
        assert_eq!(beat.seq, None);
    }
    // Every heartbeat was ACKed, so the shard never reconnected.
    assert_eq!(gw.connections().len(), 1);
}

// ---- Matrix 17 ----

#[tokio::test(flavor = "multi_thread")]
async fn zombie_connection_resumes() {
    let gw = FakeGateway::start_with(config(|c| {
        c.heartbeat_interval = Duration::from_millis(100);
        c.heartbeat_ack = false;
    }))
    .await
    .unwrap();
    let mut shard = shard(&gw, ShardId::ONE);
    ready(&mut shard).await;
    until(&mut shard, |e| matches!(e, Event::Resumed)).await;
    let frames = gw.frames();
    let resume = frames.iter().find(|f| f.op() == Some(6)).unwrap();
    assert_eq!(resume.connection, 2);
    assert_eq!(resume.json().unwrap()["d"]["session_id"], "fake-session-1");
}

// ---- Matrix 18 ----

#[tokio::test(flavor = "multi_thread")]
async fn resumable_close_resumes() {
    let gw = FakeGateway::start().await.unwrap();
    let mut shard = shard(&gw, ShardId::ONE);
    ready(&mut shard).await;
    assert_eq!(gw.close(4000, "reconnect"), Ok(1));
    let event = until(&mut shard, |e| matches!(e, Event::GatewayClose(_))).await;
    assert_eq!(close_code(&event), Some(4000));
    until(&mut shard, |e| matches!(e, Event::Resumed)).await;
    assert_eq!(shard.session().map(Session::sequence), Some(2));
    let frames = gw.frames();
    let resume = frames.iter().find(|f| f.op() == Some(6)).unwrap();
    assert_eq!(resume.connection, 2);
    assert_eq!(resume.json().unwrap()["d"]["seq"], 1);
    assert_eq!(gw.connections().len(), 2);
}

// ---- Matrix 19 ----

#[tokio::test(flavor = "multi_thread")]
async fn fatal_close_ends_the_shard() {
    for code in [4004, 4014] {
        let gw = FakeGateway::start_with(config(|c| c.identify = IdentifyReply::Close(code)))
            .await
            .unwrap();
        let mut shard = shard(&gw, ShardId::ONE);
        let event = until(&mut shard, |e| matches!(e, Event::GatewayClose(_))).await;
        assert_eq!(close_code(&event), Some(code));
        assert!(next_item(&mut shard).await.is_none(), "{code}");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(gw.connections().len(), 1, "{code}");
    }
}

// ---- Matrix 20 ----

#[tokio::test(flavor = "multi_thread")]
async fn invalid_session_then_identify_again() {
    let gw = FakeGateway::start_with(config(|c| {
        c.identify = IdentifyReply::InvalidSession { resumable: false };
    }))
    .await
    .unwrap();
    let mut shard = shard(&gw, ShardId::ONE);
    // twilight answers op 9 `false` with a 1000 close; flip afterwards.
    let flip = async {
        gw.wait_for_frame(WAIT, |f| matches!(f.kind, FrameKind::Close { .. }))
            .await
            .unwrap();
        gw.set_config(config(|_| {}));
    };
    let (ready, ()) = tokio::join!(ready(&mut shard), flip);
    assert_eq!(ready.session_id, "fake-session-2");
    let identifies: Vec<u64> = gw
        .frames()
        .iter()
        .filter(|f| f.op() == Some(2))
        .map(|f| f.connection)
        .collect();
    assert_eq!(identifies, [1, 2]);
}

// ---- Matrix 21 ----

#[tokio::test(flavor = "multi_thread")]
async fn resume_unknown_and_scripted() {
    let gw = FakeGateway::start().await.unwrap();
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &resume("nope", 1)).await;
    assert_eq!(recv(&mut ws).await, json!({ "op": 9, "d": false }));

    gw.set_config(config(|c| {
        c.resume = ResumeReply::InvalidSession { resumable: true };
    }));
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &resume("nope", 1)).await;
    assert_eq!(recv(&mut ws).await, json!({ "op": 9, "d": true }));

    gw.set_config(config(|c| c.resume = ResumeReply::Close(4009)));
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &resume("nope", 1)).await;
    assert_eq!(recv_close(&mut ws).await, 4009);
}

// ---- Matrix 22 ----

#[tokio::test(flavor = "multi_thread")]
async fn protocol_errors() {
    let gw = FakeGateway::start().await.unwrap();
    let cases: [(Message, u16); 4] = [
        (Message::text("not json"), 4002),
        (Message::text(r#"{"d":1}"#), 4002),
        (Message::binary(&b"\x01\x02"[..]), 4002),
        (Message::text(r#"{"op":99}"#), 4001),
    ];
    for (message, want) in cases {
        gw.clear_frames();
        let (mut ws, _) = raw(&gw).await;
        let sent = message.as_payload().to_vec();
        ws.send(message).await.unwrap();
        assert_eq!(recv_close(&mut ws).await, want);
        let first = &gw.frames()[0];
        let recorded = match &first.kind {
            FrameKind::Text(text) => text.as_bytes().to_vec(),
            FrameKind::Binary(bytes) => bytes.to_vec(),
            other => panic!("{other:?}"),
        };
        assert_eq!(recorded, sent);
    }

    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &json!({ "op": 8, "d": {} })).await;
    assert_eq!(recv_close(&mut ws).await, 4003);

    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &identify(None)).await;
    assert_eq!(recv(&mut ws).await["t"], "READY");
    send(&mut ws, &identify(None)).await;
    assert_eq!(recv_close(&mut ws).await, 4005);

    gw.clear_frames();
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &identify(None)).await;
    assert_eq!(recv(&mut ws).await["t"], "READY");
    send(&mut ws, &json!({ "op": 3, "d": {} })).await;
    send(&mut ws, &json!({ "op": 1, "d": 1 })).await;
    assert_eq!(recv(&mut ws).await, json!({ "op": 11 }));
    let ops: Vec<Option<u64>> = gw.frames().iter().map(RecordedFrame::op).collect();
    assert_eq!(ops, [Some(2), Some(3), Some(1)]);
}

// ---- Matrix 23 ----

#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_before_identify() {
    let gw = FakeGateway::start().await.unwrap();
    let seen: Arc<Mutex<Vec<HeartbeatSeen>>> = Arc::default();
    let sink = Arc::clone(&seen);
    gw.on_heartbeat(move |beat| sink.lock().unwrap().push(beat.clone()));
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &json!({ "op": 1, "d": null })).await;
    assert_eq!(recv(&mut ws).await, json!({ "op": 11 }));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].seq, None);
    assert_eq!(seen[0].connection, 1);
    assert_eq!(seen[0].since_previous, None);
}

// ---- Matrix 24 ----

#[tokio::test(flavor = "multi_thread")]
async fn close_validation() {
    let gw = FakeGateway::start().await.unwrap();
    for code in [999, 1004, 1005, 1006, 1015, 1016, 2999, 5000] {
        assert_eq!(gw.close(code, ""), Err(CloseError::InvalidCode { code }));
    }
    for code in [1000, 1003, 1007, 1014, 3000, 4999] {
        assert_eq!(gw.close(code, ""), Ok(0), "{code}");
    }
    let cases = [
        ("a".repeat(123), None),
        ("a".repeat(124), Some(124)),
        ("ü".repeat(61), None),
        ("ü".repeat(62), Some(124)),
        ("🙂".repeat(31), Some(124)),
    ];
    for (reason, too_long) in cases {
        let want = too_long.map_or(Ok(0), |len| Err(CloseError::ReasonTooLong { len }));
        assert_eq!(gw.close(4000, &reason), want);
    }

    let (mut ws, _) = raw(&gw).await;
    assert_eq!(gw.close(4000, &"a".repeat(123)), Ok(1));
    assert_eq!(recv_close(&mut ws).await, 4000);

    gw.set_config(config(|c| c.identify = IdentifyReply::Close(5000)));
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &identify(None)).await;
    assert_eq!(recv_close(&mut ws).await, 1011);
}

// ---- Matrix 25 ----

#[tokio::test(flavor = "multi_thread")]
async fn nothing_live() {
    let gw = FakeGateway::start().await.unwrap();
    assert_eq!(gw.dispatch("TYPING_START", json!({})), 0);
    assert_eq!(gw.send_raw("{}"), 0);
    assert_eq!(gw.close(4000, ""), Ok(0));
}

// ---- Matrix 26 ----

#[tokio::test(flavor = "multi_thread")]
async fn multiple_shards() {
    let gw = FakeGateway::start().await.unwrap();
    let mut a = shard(&gw, ShardId::new(0, 2));
    let mut b = shard(&gw, ShardId::new(1, 2));
    let (ready_a, ready_b) = tokio::join!(ready(&mut a), ready(&mut b));
    assert_eq!(ready_a.shard, Some(ShardId::new(0, 2)));
    assert_eq!(ready_b.shard, Some(ShardId::new(1, 2)));
    assert_ne!(ready_a.session_id, ready_b.session_id);
    assert_eq!(gw.connections().len(), 2);

    assert_eq!(
        gw.dispatch("MESSAGE_DELETE", json!({ "id": "3", "channel_id": "2" })),
        2
    );
    let is_delete = |e: &Event| matches!(e, Event::MessageDelete(_));
    tokio::join!(until(&mut a, is_delete), until(&mut b, is_delete));
    assert_eq!(a.session().map(Session::sequence), Some(2));
    assert_eq!(b.session().map(Session::sequence), Some(2));
}

// ---- Matrix 27 ----

#[tokio::test(flavor = "multi_thread")]
async fn client_close_semantics() {
    let gw = FakeGateway::start().await.unwrap();

    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &identify(None)).await;
    assert_eq!(recv(&mut ws).await["d"]["session_id"], "fake-session-1");
    client_close(ws, 1000).await;
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &resume("fake-session-1", 1)).await;
    assert_eq!(recv(&mut ws).await, json!({ "op": 9, "d": false }));

    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &identify(None)).await;
    assert_eq!(recv(&mut ws).await["d"]["session_id"], "fake-session-2");
    client_close(ws, 4000).await;
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &resume("fake-session-2", 1)).await;
    let resumed = recv(&mut ws).await;
    assert_eq!(resumed["t"], "RESUMED");
    assert_eq!(resumed["s"], 2);
}

// ---- Matrix 28 ----

#[tokio::test(flavor = "multi_thread")]
async fn plain_http_is_rejected() {
    let gw = FakeGateway::start().await.unwrap();
    let mut stream = TcpStream::connect(gw.addr()).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(WAIT, stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8_lossy(&response);
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(gw.connections().is_empty());

    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &json!({ "op": 1, "d": null })).await;
    assert_eq!(recv(&mut ws).await["op"], 11);
    assert_eq!(gw.connections().len(), 1);
}

// ---- Matrix 29 ----

#[tokio::test(flavor = "multi_thread")]
async fn ready_data() {
    let gw = FakeGateway::start_with(config(|c| {
        c.ready = ReadyData {
            user_id: 7,
            application_id: 8,
            guild_ids: vec![1, 2],
        };
    }))
    .await
    .unwrap();
    let mut shard = shard(&gw, ShardId::ONE);
    let ready = ready(&mut shard).await;
    assert_eq!(ready.user.id.get(), 7);
    assert_eq!(ready.application.id.get(), 8);
    let guilds: Vec<(u64, bool)> = ready
        .guilds
        .iter()
        .map(|g| (g.id.get(), g.unavailable))
        .collect();
    assert_eq!(guilds, [(1, true), (2, true)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_with_zero_id_fails_to_deserialize() {
    let gw = FakeGateway::start_with(config(|c| c.ready.user_id = 0))
        .await
        .unwrap();
    let mut shard = shard(&gw, ShardId::ONE);
    let deadline = tokio::time::Instant::now() + WAIT;
    let err = loop {
        let item = tokio::time::timeout_at(deadline, shard.next_event(EventTypeFlags::all()))
            .await
            .unwrap();
        match item {
            Some(Err(err)) => break err,
            Some(Ok(Event::Ready(_))) => panic!("READY with id 0 deserialized"),
            Some(Ok(_)) => {}
            None => panic!("shard ended"),
        }
    };
    let ReceiveMessageErrorType::Deserializing { event } = err.kind() else {
        panic!("{err:?}")
    };
    assert!(event.contains(r#""id":"0""#), "{event}");
}

// ---- Matrix 30 ----

#[tokio::test(flavor = "multi_thread")]
async fn wait_for_frame() {
    let gw = FakeGateway::start().await.unwrap();
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &json!({ "op": 1, "d": null })).await;
    recv(&mut ws).await;
    let started = Instant::now();
    let frame = gw.wait_for_frame(WAIT, |f| f.op() == Some(1)).await;
    assert!(frame.is_some());
    assert!(started.elapsed() < Duration::from_millis(100));

    let started = Instant::now();
    let none = gw
        .wait_for_frame(Duration::from_millis(200), |f| f.op() == Some(42))
        .await;
    assert!(none.is_none());
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(190), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");

    // A frame arriving while waiting wakes the waiter.
    let waiter = gw.wait_for_frame(WAIT, |f| f.op() == Some(3));
    let sender = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        send(&mut ws, &identify(None)).await;
        recv(&mut ws).await;
        send(&mut ws, &json!({ "op": 3, "d": {} })).await;
    };
    let (frame, ()) = tokio::join!(waiter, sender);
    assert_eq!(frame.unwrap().op(), Some(3));
}

// ---- Matrix 31 ----

#[tokio::test(flavor = "multi_thread")]
async fn log_ops() {
    let gw = FakeGateway::start().await.unwrap();
    let (mut ws, _) = raw(&gw).await;
    send(&mut ws, &json!({ "op": 1, "d": null })).await;
    recv(&mut ws).await;
    send(&mut ws, &identify(Some([0, 1]))).await;
    let ready = recv(&mut ws).await;
    assert_eq!(ready["d"]["shard"], json!([0, 1]));
    assert_eq!(ready["s"], 1);
    assert!(has_op(&gw.frames(), 2));
    let ops: Vec<Option<u64>> = gw.frames().iter().map(RecordedFrame::op).collect();
    assert_eq!(ops, [Some(1), Some(2)]);
    gw.clear_frames();
    assert!(gw.frames().is_empty());
    assert_eq!(gw.connections().len(), 1);
}

// ---- Matrix 32 ----

#[tokio::test(flavor = "multi_thread")]
async fn dropped_fake() {
    let gw = FakeGateway::start().await.unwrap();
    let mut shard = shard(&gw, ShardId::ONE);
    ready(&mut shard).await;
    drop(gw);
    let mut errors = 0;
    let mut closes = 0;
    let deadline = tokio::time::Instant::now() + WAIT;
    while errors < 2 {
        let item = tokio::time::timeout_at(deadline, shard.next_event(EventTypeFlags::all()))
            .await
            .expect("items after the fake is gone");
        match item {
            Some(Err(_)) => errors += 1,
            Some(Ok(Event::GatewayClose(_))) => closes += 1,
            Some(Ok(_)) => {}
            None => panic!("shard ended"),
        }
    }
    assert!(closes + errors >= 2);
}
