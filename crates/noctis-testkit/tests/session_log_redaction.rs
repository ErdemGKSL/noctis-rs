//! Session resume logs never carry the session id, the resume URL or the
//! token (Story 12.2 A6), and a resumed App completes its startup on
//! RESUMED (A1). Its own test binary, because it installs a global
//! capturing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis_core::{App, AppBuilder, Runtime, Token};
use noctis_ports::memory::MemorySessionStore;
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway};
use serde_json::json;
use tokio::sync::oneshot;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn since(&self, mark: usize) -> String {
        String::from_utf8(self.0.lock().unwrap()[mark..].to_vec()).unwrap()
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

fn builder(
    http: &FakeDiscordHttp,
    gw: &FakeGateway,
    store: &Arc<MemorySessionStore>,
) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .session_store(Arc::clone(store));
    builder
}

/// Runs `app` until `ready` holds for `gw`, then lets it settle, dispatches
/// one event and shuts it down gracefully.
async fn run_once(app: App, gw: &FakeGateway) {
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = tokio::spawn(app.run_until(async {
        let _ = stopped.await;
    }));
    let deadline = Instant::now() + WAIT;
    while gw.live_sessions() < 1 {
        assert!(Instant::now() < deadline, "no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    gw.dispatch("MESSAGE_DELETE", json!({ "id": "3", "channel_id": "2" }));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let _ = stop.send(());
    tokio::time::timeout(WAIT, handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// The `noctis::` lines of `text`.
fn noctis_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.contains(" noctis::"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a6_session_logs_are_redacted() {
    let capture = Capture::default();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .with_ansi(false)
        .with_writer(capture.clone())
        .init();

    let http = FakeDiscordHttp::start().await.unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let store = Arc::new(MemorySessionStore::new());
    run_once(builder(&http, &gw, &store).build().unwrap(), &gw).await;

    let mark = capture.len();
    run_once(builder(&http, &gw, &store).build().unwrap(), &gw).await;
    let second = capture.since(mark);
    let lines = noctis_lines(&second);
    let joined = lines.join("\n");
    // A1: the second run resumed and completed its startup on RESUMED.
    assert!(joined.contains("resuming a stored session"), "{joined}");
    assert!(joined.contains("shard resumed"), "{joined}");
    assert!(joined.contains("all 1 shards ready"), "{joined}");
    assert!(joined.contains("session stored"), "{joined}");

    let all = capture.since(0);
    let url = gw.url();
    for line in noctis_lines(&all) {
        assert!(!line.contains("fake-session-"), "{line}");
        assert!(!line.contains(&url), "{line}");
        assert!(!line.contains("fake-token"), "{line}");
    }
}
