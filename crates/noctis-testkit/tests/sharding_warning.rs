//! The misaligned shard total warning (Story 5.4, Matrix 23). Its own test
//! binary, because it installs a global capturing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis_core::{App, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot, SessionStartLimit};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

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

#[tokio::test(flavor = "multi_thread")]
async fn misaligned_fixed_total_warns_and_starts() {
    let capture = Capture::default();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::WARN)
        .with_writer(capture.clone())
        .init();

    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        session_start_limit: SessionStartLimit {
            max_concurrency: 2,
            ..SessionStartLimit::default()
        },
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
        .shard_total(3)
        .build()
        .unwrap();
    let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));

    let deadline = Instant::now() + Duration::from_secs(15);
    while gw.frames().iter().filter(|f| f.op() == Some(2)).count() < 3 {
        assert!(Instant::now() < deadline, "3 shards did not identify");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!run.is_finished());
    run.abort();

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let line = logs
        .lines()
        .find(|l| l.contains("shard total is not a multiple of max_concurrency"))
        .unwrap_or_else(|| panic!("no warning in:\n{logs}"));
    assert!(line.contains("WARN"), "{line}");
    assert!(line.contains("total=3"), "{line}");
    assert!(line.contains("max_concurrency=2"), "{line}");
    assert!(!logs.contains("fake-token"));
}
