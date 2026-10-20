//! `FakeDiscordHttp` driven by real Apps through `AppBuilder::http_proxy`
//! (Story 5.2, Matrix 12-25). Rows twilight cannot send go over raw TCP.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::net::SocketAddr;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use noctis_core::{App, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeResponse, GatewayBot, MAX_BODY_BYTES, Method, SessionStartLimit,
    StatusCode,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinSet;

/// An App on `Runtime::for_test()` whose requests go to `fake`.
fn app(fake: &FakeDiscordHttp, token: Option<&str>, timeout: Option<Duration>) -> App {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test()).http_proxy(fake.url());
    if let Some(token) = token {
        builder.token(Token::new(token).unwrap());
    }
    if let Some(timeout) = timeout {
        builder.http_timeout(timeout);
    }
    builder.build().unwrap()
}

/// Sends one HTTP/1.1 request (`head` is the request line plus any header
/// lines, each ending in `\r\n`) and returns the status and response body.
async fn raw_request(addr: SocketAddr, head: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!("{head}host: 127.0.0.1\r\nconnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let status_line = std::str::from_utf8(&response[..split])
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    let status = status_line.split(' ').nth(1).unwrap().parse().unwrap();
    (status, response[split + 4..].to_vec())
}

fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).unwrap()
}

fn id<T: From<NonZeroU64>>(n: u64) -> T {
    NonZeroU64::new(n).unwrap().into()
}

// ---- Matrix 25: smoke ----

#[tokio::test]
async fn smoke_gateway_bot_through_proxy() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(fake.url())
        .build()
        .unwrap();
    assert!(app.runtime().is_test());
    let info = app
        .http()
        .gateway()
        .authed()
        .await
        .unwrap()
        .model()
        .await
        .unwrap();
    assert_eq!(info.url, "wss://gateway.discord.gg");
    assert_eq!(info.shards, 1);
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(requests[0].path, "/api/v10/gateway/bot");
}

// ---- Matrix 12 ----

#[tokio::test]
async fn two_fakes_are_independent() {
    let a = FakeDiscordHttp::start().await.unwrap();
    let b = FakeDiscordHttp::start().await.unwrap();
    assert_ne!(a.addr().port(), 0);
    assert_ne!(b.addr().port(), 0);
    assert_ne!(a.addr().port(), b.addr().port());
    assert_eq!(a.url(), format!("http://127.0.0.1:{}", a.addr().port()));
    let app_a = app(&a, Some("t"), None);
    app_a.http().gateway().authed().await.unwrap();
    assert_eq!(a.requests().len(), 1);
    assert!(b.requests().is_empty());
}

// ---- Matrix 13 ----

async fn gateway_info(fake: &FakeDiscordHttp) -> serde_json::Value {
    let app = app(fake, Some("t"), None);
    let response = app.http().gateway().authed().await.unwrap();
    json(response.text().await.unwrap().as_bytes())
}

#[tokio::test]
async fn canned_gateway_bot_defaults() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = app(&fake, Some("t"), None);
    let info = app
        .http()
        .gateway()
        .authed()
        .await
        .unwrap()
        .model()
        .await
        .unwrap();
    assert_eq!(info.shards, 1);
    let limit = info.session_start_limit;
    assert_eq!(
        (
            limit.total,
            limit.remaining,
            limit.reset_after,
            limit.max_concurrency
        ),
        (1000, 1000, 14_400_000, 1)
    );
}

#[tokio::test]
async fn canned_gateway_bot_scripted() {
    let fake = FakeDiscordHttp::start_with(GatewayBot {
        shards: 4,
        session_start_limit: SessionStartLimit {
            max_concurrency: 16,
            remaining: 0,
            reset_after_ms: 1000,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let app = app(&fake, Some("t"), None);
    let info = app
        .http()
        .gateway()
        .authed()
        .await
        .unwrap()
        .model()
        .await
        .unwrap();
    assert_eq!(info.shards, 4);
    assert_eq!(info.session_start_limit.max_concurrency, 16);
    assert_eq!(info.session_start_limit.remaining, 0);
    assert_eq!(info.session_start_limit.reset_after, 1000);

    fake.set_gateway_bot(GatewayBot {
        shards: 7,
        ..GatewayBot::default()
    });
    let info = app
        .http()
        .gateway()
        .authed()
        .await
        .unwrap()
        .model()
        .await
        .unwrap();
    assert_eq!(info.shards, 7);
}

#[tokio::test]
async fn canned_gateway_bot_extremes_verbatim() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    for shards in [0, u32::MAX] {
        fake.set_gateway_bot(GatewayBot {
            shards,
            ..GatewayBot::default()
        });
        assert_eq!(gateway_info(&fake).await["shards"], shards);
        let app = app(&fake, Some("t"), None);
        let info = app
            .http()
            .gateway()
            .authed()
            .await
            .unwrap()
            .model()
            .await
            .unwrap();
        assert_eq!(info.shards, shards);
    }
    for max_concurrency in [0, 65_535] {
        fake.set_gateway_bot(GatewayBot {
            session_start_limit: SessionStartLimit {
                max_concurrency,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        });
        let app = app(&fake, Some("t"), None);
        let info = app
            .http()
            .gateway()
            .authed()
            .await
            .unwrap()
            .model()
            .await
            .unwrap();
        assert_eq!(
            u32::from(info.session_start_limit.max_concurrency),
            max_concurrency
        );
    }
    fake.set_gateway_bot(GatewayBot {
        session_start_limit: SessionStartLimit {
            max_concurrency: 65_536,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    });
    fake.clear_requests();
    let app = app(&fake, Some("t"), None);
    let response = app.http().gateway().authed().await.unwrap();
    assert!(response.model().await.is_err());
    assert_eq!(fake.requests().len(), 1);
}

// ---- Matrix 14 ----

#[tokio::test]
async fn per_test_route_matching() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/channels/1/messages",
        FakeResponse::json(StatusCode::OK, &serde_json::json!([])),
    );
    let app = app(&fake, Some("t"), None);

    let response = app.http().channel_messages(id(1)).await.unwrap();
    assert_eq!(response.status().get(), 200);
    assert_eq!(response.text().await.unwrap(), "[]");

    fake.route(
        Method::GET,
        "channels/1/messages",
        FakeResponse::json(StatusCode::OK, &serde_json::json!([])),
    );
    let response = app.http().channel_messages(id(1)).limit(5).await.unwrap();
    assert_eq!(response.status().get(), 200);
    let last = fake.requests().pop().unwrap();
    assert_eq!(last.query.as_deref(), Some("limit=5"));
    assert_eq!(last.route_path(), Some("/channels/1/messages"));

    let err = app
        .http()
        .create_message(id(1))
        .content("x")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("status code 404"), "{err}");

    let (status, body) = raw_request(
        fake.addr(),
        "GET /api/v10/channels/1/messages/ HTTP/1.1\r\n",
        b"",
    )
    .await;
    assert_eq!(status, 404);
    assert_eq!(
        json(&body),
        serde_json::json!({ "message": "404: Not Found", "code": 0 })
    );
}

// ---- Matrix 15 ----

#[tokio::test]
async fn override_and_last_registration_wins() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/gateway/bot",
        FakeResponse::empty(StatusCode::UNAUTHORIZED),
    );
    let app = app(&fake, Some("t"), None);
    assert!(app.http().gateway().authed().await.is_err());
    assert_eq!(fake.requests().len(), 1);

    fake.route(
        Method::DELETE,
        "/channels/1",
        FakeResponse::empty(StatusCode::INTERNAL_SERVER_ERROR),
    )
    .route(
        Method::DELETE,
        "/channels/1",
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let (status, _) =
        raw_request(fake.addr(), "DELETE /api/v10/channels/1 HTTP/1.1\r\n", b"").await;
    assert_eq!(status, 204);
}

// ---- Matrix 16 ----

#[tokio::test]
async fn unknown_and_outside_api() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    for path in ["/api/v10/nope", "/gateway/bot", "/api/v9/gateway/bot"] {
        let (status, body) =
            raw_request(fake.addr(), &format!("GET {path} HTTP/1.1\r\n"), b"").await;
        assert_eq!(status, 404, "{path}");
        assert_eq!(
            json(&body),
            serde_json::json!({ "message": "404: Not Found", "code": 0 })
        );
    }
    let requests = fake.requests();
    let paths: Vec<_> = requests.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        ["/api/v10/nope", "/gateway/bot", "/api/v9/gateway/bot"]
    );
    let routes: Vec<_> = requests.iter().map(|r| r.route_path()).collect();
    assert_eq!(routes, [Some("/nope"), None, None]);
}

// ---- Matrix 17 ----

#[tokio::test]
async fn raw_path_is_not_decoded() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/guilds/1/emojis/%F0%9F%99%82",
        FakeResponse::new(StatusCode::OK, "emoji"),
    );
    let (status, body) = raw_request(
        fake.addr(),
        "GET /api/v10/guilds/1/emojis/%F0%9F%99%82 HTTP/1.1\r\n",
        b"",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, b"emoji");
}

// ---- Matrix 18 ----

#[tokio::test]
async fn body_limit() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route_with(Method::POST, "/upload", |req| {
        FakeResponse::new(StatusCode::OK, req.body.len().to_string())
    });

    let body = vec![b'x'; MAX_BODY_BYTES];
    let (status, response) = raw_request(
        fake.addr(),
        &format!("POST /api/v10/upload HTTP/1.1\r\ncontent-length: {MAX_BODY_BYTES}\r\n"),
        &body,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(response, MAX_BODY_BYTES.to_string().as_bytes());
    assert_eq!(fake.requests()[0].body.len(), MAX_BODY_BYTES);
    drop(body);
    fake.clear_requests();

    let (status, response) = raw_request(
        fake.addr(),
        &format!(
            "POST /api/v10/upload HTTP/1.1\r\ncontent-length: {}\r\nexpect: 100-continue\r\n",
            MAX_BODY_BYTES + 1
        ),
        b"",
    )
    .await;
    assert_eq!(status, 413);
    assert_eq!(
        json(&response),
        serde_json::json!({ "message": "Request entity too large", "code": 40005 })
    );
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].body.is_empty());
    fake.clear_requests();

    let (status, response) = raw_request(
        fake.addr(),
        "POST /api/v10/upload HTTP/1.1\r\ncontent-length: 0\r\n",
        b"",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(response, b"0");
    assert!(fake.requests()[0].body.is_empty());
}

// ---- Matrix 19 ----

#[tokio::test]
async fn route_with_echoes_body() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route_with(Method::POST, "/channels/1/messages", |req| {
        FakeResponse::new(StatusCode::OK, req.body.clone())
    });
    let app = app(&fake, Some("t"), None);
    let response = app
        .http()
        .create_message(id(1))
        .content("hello")
        .await
        .unwrap();
    let text = response.text().await.unwrap();
    let sent = fake.requests().pop().unwrap();
    assert_eq!(text.as_bytes(), &sent.body[..]);
    assert_eq!(sent.json().unwrap()["content"], "hello");
}

// ---- Matrix 20 ----

#[tokio::test]
async fn delayed_route_times_out() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/gateway/bot",
        FakeResponse::empty(StatusCode::OK).delay(Duration::from_secs(2)),
    );
    let app = app(&fake, Some("t"), Some(Duration::from_millis(100)));
    let err = app.http().gateway().authed().await.unwrap_err();
    assert_eq!(err.to_string(), "request timed out");
    assert_eq!(fake.requests().len(), 1);
}

// ---- Matrix 21 ----

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = app(&fake, Some("t"), None);
    let mut tasks = JoinSet::new();
    for _ in 0..16 {
        let http = Arc::clone(app.http());
        tasks.spawn(async move { http.gateway().authed().await.map(|r| r.status().get()) });
    }
    let mut answered = 0;
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap().unwrap(), 200);
        answered += 1;
    }
    assert_eq!(answered, 16);
    assert_eq!(fake.requests().len(), 16);
}

// ---- Matrix 22 ----

#[tokio::test]
async fn log_order_and_clear() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    for path in ["/api/v10/a", "/api/v10/b", "/api/v10/c"] {
        raw_request(fake.addr(), &format!("GET {path} HTTP/1.1\r\n"), b"").await;
    }
    let paths: Vec<_> = fake.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/api/v10/a", "/api/v10/b", "/api/v10/c"]);
    fake.clear_requests();
    assert!(fake.requests().is_empty());
}

// ---- Matrix 23 ----

#[tokio::test]
async fn authorization_header() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/channels/1/messages",
        FakeResponse::json(StatusCode::OK, &serde_json::json!([])),
    );
    let with = app(&fake, Some("fake-token"), None);
    with.http().channel_messages(id(1)).await.unwrap();
    let without = app(&fake, None, None);
    without.http().channel_messages(id(1)).await.unwrap();
    let requests = fake.requests();
    assert_eq!(
        requests[0].headers.get("authorization").unwrap(),
        "Bot fake-token"
    );
    assert!(requests[1].headers.get("authorization").is_none());
}

// ---- Matrix 24 ----

#[tokio::test]
async fn dropped_fake_refuses() {
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = app(&fake, Some("t"), Some(Duration::from_secs(2)));
    drop(fake);
    tokio::task::yield_now().await;
    assert!(app.http().gateway().authed().await.is_err());
}
