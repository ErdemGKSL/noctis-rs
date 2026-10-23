//! The Shard Range alignment warning (Story 12.1, P1, P2 and P4). Its own
//! test binary, because it installs a global capturing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis_core::{App, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot, SessionStartLimit};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

const RANGE_WARNING: &str =
    "shard range is not aligned to max_concurrency; identify buckets are split across processes";
const TOTAL_WARNING: &str = "shard total is not a multiple of max_concurrency";

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    /// The captured lines containing `needle`.
    fn lines_with(&self, needle: &str) -> Vec<String> {
        self.text()
            .lines()
            .filter(|l| l.contains(needle))
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

/// Runs an App with `shards` of `total` (`max_concurrency` 2) until every
/// shard of the range identified, then stops it.
async fn run_range(total: u32, shards: Range<u32>) {
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: total,
        session_start_limit: SessionStartLimit {
            max_concurrency: 2,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let expected = usize::try_from(shards.end - shards.start).unwrap();
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .shard_total(total)
        .shard_range(shards.clone())
        .build()
        .unwrap();
    let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let identifies: Vec<u64> = gw
            .frames()
            .iter()
            .filter(|f| f.op() == Some(2))
            .map(|f| f.json().unwrap()["d"]["shard"][0].as_u64().unwrap())
            .collect();
        if identifies.len() >= expected {
            let mut ids = identifies;
            ids.sort_unstable();
            let wanted: Vec<u64> = shards.clone().map(u64::from).collect();
            assert_eq!(ids, wanted, "{shards:?} of {total}");
            break;
        }
        assert!(Instant::now() < deadline, "{shards:?} did not identify");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!run.is_finished());
    run.abort();
    let _ = run.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn misaligned_shard_range_warns_and_starts() {
    let capture = Capture::default();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .with_writer(capture.clone())
        .init();

    // P2: start 1 is not a multiple of 2; shards 1 and 2 still start.
    run_range(4, 1..3).await;
    let lines = capture.lines_with(RANGE_WARNING);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert!(line.contains("WARN"), "{line}");
    assert!(line.contains("noctis::app"), "{line}");
    assert!(line.contains("start=1"), "{line}");
    assert!(line.contains("end=3"), "{line}");
    assert!(line.contains("max_concurrency=2"), "{line}");
    assert!(capture.lines_with(TOTAL_WARNING).is_empty());

    // P1: an aligned range does not warn.
    run_range(4, 2..4).await;
    assert_eq!(capture.lines_with(RANGE_WARNING).len(), 1);

    // P4: the last range of an uneven total does not warn about the range;
    // the total warning still fires.
    run_range(3, 2..3).await;
    assert_eq!(capture.lines_with(RANGE_WARNING).len(), 1);
    assert_eq!(capture.lines_with(TOTAL_WARNING).len(), 1);
    assert!(!capture.text().contains("fake-token"));
}
