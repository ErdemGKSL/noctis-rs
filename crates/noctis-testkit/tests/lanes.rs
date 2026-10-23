//! The lanes under a gateway flood (Story 5.5, Matrix 26): shard tasks push
//! into a `LaneSender` while the consumer is stalled, and heartbeats keep
//! their interval.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::num::{NonZeroU16, NonZeroU32, NonZeroUsize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis_core::{App, Runtime};
use noctis_gateway::{
    EventTypeFlags, Gateway, GatewayOptions, LaneConfig, LaneDepths, ShardEvent, lanes,
};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_testkit::fake::{FakeGateway, FakeGatewayConfig, HeartbeatSeen};
use serde_json::json;
use twilight_gateway::Intents;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

#[tokio::test(flavor = "multi_thread")]
async fn heartbeats_unaffected_by_a_stalled_consumer() {
    let config = FakeGatewayConfig {
        heartbeat_interval: Duration::from_millis(100),
        ..FakeGatewayConfig::default()
    };
    let gw = FakeGateway::start_with(config).await.unwrap();
    let seen: Arc<Mutex<Vec<HeartbeatSeen>>> = Arc::default();
    let sink = Arc::clone(&seen);
    gw.on_heartbeat(move |beat| sink.lock().unwrap().push(beat.clone()));
    // Building an App installs the rustls provider twilight's connector needs.
    App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();

    let (sender, mut receiver) = lanes(LaneConfig {
        app: Arc::from("lanes-e2e"),
        event_capacity: NonZeroUsize::new(16).unwrap(),
        overflow_capacity: 16,
        ..LaneConfig::default()
    });
    let options = GatewayOptions {
        app: Arc::from("default"),
        token: "fake-token".to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some(gw.url()),
        event_types: EventTypeFlags::all(),
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::MIN)),
        shards: 0..1,
        total: NonZeroU32::MIN,
        max_concurrency: NonZeroU16::MIN,
        close_timeout: Duration::from_secs(5),
    };
    let (gateway, mut events) = Gateway::start(options, Arc::new(sender)).unwrap();
    let ready = tokio::time::timeout(WAIT, events.recv()).await.unwrap();
    assert_eq!(ready, Some(ShardEvent::Ready { shard: 0 }));

    // The consumer takes one item (READY), then stalls: it takes nothing
    // more for the rest of the test.
    let first = tokio::time::timeout(WAIT, receiver.recv()).await.unwrap();
    assert!(first.is_some());

    let flood_at = Instant::now();
    for n in 1..=2000_u64 {
        gw.dispatch(
            "MESSAGE_DELETE",
            json!({ "id": n.to_string(), "channel_id": "2" }),
        );
    }
    let window_end = flood_at + Duration::from_millis(1500);
    tokio::time::sleep_until(window_end.into()).await;

    let beats: Vec<HeartbeatSeen> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|beat| beat.received_at >= flood_at && beat.received_at <= window_end)
        .cloned()
        .collect();
    assert!(beats.len() >= 8, "{} heartbeats: {beats:?}", beats.len());
    for beat in &beats {
        if let Some(gap) = beat.since_previous {
            assert!(gap <= Duration::from_millis(300), "{gap:?}");
        }
    }
    assert_eq!(gw.connections().len(), 1);
    let deadline = Instant::now() + WAIT;
    let full = LaneDepths {
        interaction: 0,
        event: 16,
        overflow: 16,
    };
    while receiver.depths() != full {
        assert!(Instant::now() < deadline, "{:?}", receiver.depths());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(gateway);
    while tokio::time::timeout(WAIT, events.recv())
        .await
        .unwrap()
        .is_some()
    {}
}
