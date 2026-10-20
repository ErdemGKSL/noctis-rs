//! `App::run` stops on SIGTERM and SIGINT (Story 5.8, Matrix 39). Its own
//! test binary, because it sends signals to its own process: once tokio's
//! handlers are installed, they stay installed for the process.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::time::{Duration, Instant};

use noctis_core::{App, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, FrameKind, GatewayBot};
use serde_json::json;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// Runs an App with `run()`, sends `signal` to this process once the shard
/// has a session, and checks that `run` returns `Ok(())` after a 4000 close.
async fn stops_on(signal: &str) {
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
    let run = tokio::spawn(app.run());
    let deadline = Instant::now() + WAIT;
    // The handlers are installed before `GET /gateway/bot`, so they are
    // in place once the shard has a session.
    while gw.dispatch("TYPING_START", json!({})) == 0 {
        assert!(Instant::now() < deadline, "the shard has no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let status = std::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(std::process::id().to_string())
        .status()
        .unwrap();
    assert!(status.success());
    let outcome = tokio::time::timeout(WAIT, run).await.unwrap().unwrap();
    assert!(outcome.is_ok(), "{signal}: {outcome:?}");
    let closes: Vec<_> = gw
        .frames()
        .into_iter()
        .filter_map(|f| match f.kind {
            FrameKind::Close { code, .. } => Some(code),
            _ => None,
        })
        .collect();
    assert_eq!(closes, [Some(4000)], "{signal}");
}

/// Matrix 39: SIGTERM, then SIGINT, each stop a running App gracefully.
#[tokio::test(flavor = "multi_thread")]
async fn sigterm_then_sigint() {
    stops_on("TERM").await;
    stops_on("INT").await;
}
