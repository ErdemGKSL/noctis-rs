//! The shard task's receive and parse warnings never carry the payload
//! (Story 5.4). Its own test binary, because it installs a global capturing
//! subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis_core::{App, Runtime};
use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink, ShardEvent};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_testkit::fake::FakeGateway;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use twilight_gateway::Intents;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
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

/// Drops every item.
struct Discard;

impl InboundSink for Discard {
    fn push(&self, _inbound: Inbound) {}
}

#[tokio::test(flavor = "multi_thread")]
async fn shard_warnings_omit_the_payload() {
    const UNREADABLE: &str = "sentinel-unreadable-frame";
    const UNPARSABLE: &str = "sentinel-unparsable-interaction";
    const DESERIALIZE_WARNING: &str = "gateway message could not be deserialized";
    const PARSE_WARNING: &str = "gateway payload could not be parsed";

    let capture = Capture::default();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .with_writer(capture.clone())
        .init();

    let gw = FakeGateway::start().await.unwrap();
    // Building an App installs the rustls provider twilight's connector needs.
    App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
    let options = GatewayOptions {
        app: Arc::from("default"),
        token: "fake-token".to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some(gw.url()),
        event_types: EventTypeFlags::all(),
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::MIN)),
        session_store: Arc::new(noctis_ports::memory::MemorySessionStore::new()),
        resume: true,
        presence: None,
        large_threshold: noctis_gateway::LargeThreshold::default(),
        shards: 0..1,
        total: NonZeroU32::MIN,
        max_concurrency: NonZeroU16::MIN,
        close_timeout: Duration::from_secs(5),
    };
    let (gateway, mut events) = Gateway::start(options, Arc::new(Discard)).unwrap();
    assert_eq!(
        tokio::time::timeout(WAIT, events.recv()).await.unwrap(),
        Some(ShardEvent::Ready { shard: 0 })
    );

    // No opcode: twilight's shard fails with `Deserializing`.
    assert_eq!(gw.send_raw(format!(r#"{{"sentinel":"{UNREADABLE}"}}"#)), 1);
    // A readable envelope whose `d` is not an interaction: `parse` fails.
    assert_eq!(
        gw.send_raw(format!(
            r#"{{"op":0,"s":2,"t":"INTERACTION_CREATE","d":{{"sentinel":"{UNPARSABLE}"}}}}"#
        )),
        1
    );

    let deadline = Instant::now() + WAIT;
    loop {
        let logs = capture.text();
        if logs.contains(DESERIALIZE_WARNING) && logs.contains(PARSE_WARNING) {
            break;
        }
        assert!(Instant::now() < deadline, "warnings missing in:\n{logs}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(gateway);

    let logs = capture.text();
    assert!(!logs.contains(UNREADABLE), "{logs}");
    assert!(!logs.contains(UNPARSABLE), "{logs}");
    assert!(!logs.contains("fake-token"), "{logs}");
}
