//! The lane metrics of a running App (Story 5.5, Matrix 27). Its own test
//! binary, because it installs a global `metrics` recorder and a global
//! capturing tracing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use metrics_util::MetricKind;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use noctis_core::{App, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// A labeled gauge in a snapshot: its name and sorted labels.
type GaugeId = (String, Vec<(String, String)>);

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

fn gauge_id(name: &str, labels: &[(&str, &str)]) -> GaugeId {
    let mut labels: Vec<_> = labels
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    labels.sort();
    (name.to_owned(), labels)
}

#[tokio::test(flavor = "multi_thread")]
async fn app_reports_lane_depths() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::set_global_recorder(recorder).unwrap();
    let capture = Capture::default();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_ansi(false)
        .with_writer(capture.clone())
        .init();

    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .build()
        .unwrap();
    let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));

    // READY follows IDENTIFY on the same connection.
    let deadline = Instant::now() + WAIT;
    while !gw.frames().iter().any(|f| f.op() == Some(2)) {
        assert!(Instant::now() < deadline, "the shard did not identify");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let d = r#"{"id":"5","application_id":"1","type":1,"token":"t","version":1,"authorizing_integration_owners":{},"entitlements":[]}"#;
    assert_eq!(
        gw.send_raw(format!(
            r#"{{"op":0,"s":2,"t":"INTERACTION_CREATE","d":{d}}}"#
        )),
        1
    );

    // The core drain received the interaction from the Interaction Lane.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !capture
        .text()
        .lines()
        .any(|line| line.contains("no handler registered") && line.contains("kind=Interaction"))
    {
        assert!(
            Instant::now() < deadline,
            "the drain did not see the interaction"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let interaction = gauge_id(
        "noctis_lane_depth",
        &[("app", "default"), ("lane", "interaction")],
    );
    let event = gauge_id(
        "noctis_lane_depth",
        &[("app", "default"), ("lane", "event")],
    );
    let overflow = gauge_id("noctis_event_overflow_depth", &[("app", "default")]);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let gauges: Vec<(GaugeId, f64)> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter_map(|(key, _, _, value)| match (key.kind(), value) {
                (MetricKind::Gauge, DebugValue::Gauge(value)) => {
                    let mut labels: Vec<_> = key
                        .key()
                        .labels()
                        .map(|l| (l.key().to_owned(), l.value().to_owned()))
                        .collect();
                    labels.sort();
                    Some(((key.key().name().to_owned(), labels), value.0))
                }
                _ => None,
            })
            .collect();
        let read = |id: &GaugeId| gauges.iter().find(|(g, _)| g == id).map(|(_, v)| *v);
        if read(&event).is_some() && read(&overflow).is_some() && read(&interaction) == Some(0.0) {
            break;
        }
        assert!(Instant::now() < deadline, "{gauges:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!run.is_finished());
    run.abort();
}
