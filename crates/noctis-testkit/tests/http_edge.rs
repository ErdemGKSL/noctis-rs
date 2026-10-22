//! The Discord HTTP edge end to end against `FakeDiscordHttp` (Story 5.9,
//! Matrix 10, 12, 14-15, 17-19, 21-23, 25-31, 33-40). Its own test binary,
//! because it installs a global capturing tracing subscriber and a global
//! metrics recorder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_core::__testing::{CallbackAck, CallbackError, interaction_callback, keep_warm};
use noctis_core::{
    AckDeadlineMissed, App, AppBuilder, Error, HttpConfig, Runtime, StartError, Token,
};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, HeaderMap, HeaderName, HeaderValue, HttpProtocol,
    Method, RecordedRequest, StatusCode,
};
use serde_json::json;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The callback route of interaction 1.
const CB: &str = "/interactions/1/tok-1.x_Y/callback";
const TOKEN: &str = "tok-1.x_Y";

// ---- globals ----

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn mark(&self) -> usize {
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

fn globals() -> (&'static Capture, &'static Snapshotter) {
    static GLOBALS: OnceLock<(Capture, Snapshotter)> = OnceLock::new();
    let (capture, snapshotter) = GLOBALS.get_or_init(|| {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        recorder.install().unwrap();
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        (capture, snapshotter)
    });
    (capture, snapshotter)
}

/// Serializes the tests: they share the log capture and the metrics.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

/// Counter values and histogram sample counts since the last snapshot.
#[derive(Default)]
struct Metrics(Vec<Sample>);

/// A metric name, its labels and its value.
type Sample = (String, Vec<(String, String)>, u64);

impl Metrics {
    fn take(snapshotter: &Snapshotter) -> Self {
        Self(
            snapshotter
                .snapshot()
                .into_vec()
                .into_iter()
                .map(|(key, _, _, value)| {
                    let labels = key
                        .key()
                        .labels()
                        .map(|l| (l.key().to_owned(), l.value().to_owned()))
                        .collect();
                    let n = match value {
                        DebugValue::Counter(c) => c,
                        DebugValue::Histogram(v) => v.len() as u64,
                        DebugValue::Gauge(_) => 0,
                    };
                    (key.key().name().to_owned(), labels, n)
                })
                .collect(),
        )
    }

    fn get(&self, name: &str, label: Option<(&str, &str)>) -> u64 {
        self.0
            .iter()
            .filter(|(n, labels, _)| {
                n == name
                    && label.is_none_or(|(k, v)| labels.iter().any(|(lk, lv)| lk == k && lv == v))
            })
            .map(|(.., c)| *c)
            .sum()
    }
}

/// The growth of a counter between two snapshots: the debugging recorder
/// resets counters (and drains histograms) at each snapshot, so `after`
/// holds only what was recorded since `before` was taken.
fn delta(_before: &Metrics, after: &Metrics, name: &str, label: Option<(&str, &str)>) -> u64 {
    after.get(name, label)
}

// ---- helpers ----

fn id<T: From<NonZeroU64>>(n: u64) -> T {
    NonZeroU64::new(n).unwrap().into()
}

/// A builder on `Runtime::for_test()` attached to `fake` (h2c).
fn builder(fake: &FakeDiscordHttp) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap());
    fake.attach(&mut builder);
    builder
}

/// `HttpConfig` with h2c and fast PINGs (Matrix 21-22).
fn fast_pings() -> HttpConfig {
    let mut config = HttpConfig::default();
    config.upstream_h2c = true;
    config.ping_interval = Duration::from_millis(100);
    config.ping_timeout = Duration::from_secs(1);
    config
}

/// The deferred-ack callback body.
fn deferred() -> Bytes {
    Bytes::from(serde_json::to_vec(&json!({ "type": 5 })).unwrap())
}

/// `cb(d)`: a callback for interaction 1 received `ago` in the past.
async fn cb(app: &App, ago: Duration) -> Result<CallbackAck, CallbackError> {
    cb_with(app, 1, TOKEN, ago).await
}

async fn cb_with(
    app: &App,
    id: u64,
    token: &str,
    ago: Duration,
) -> Result<CallbackAck, CallbackError> {
    let received_at = Instant::now() - ago;
    tokio::time::timeout(
        WAIT,
        interaction_callback(app, id, token, deferred(), false, received_at),
    )
    .await
    .expect("callback returned in time")
}

fn callbacks(fake: &FakeDiscordHttp) -> Vec<RecordedRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.path.ends_with("/callback"))
        .collect()
}

/// Waits until `check` holds.
async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A running App on both fakes.
struct Running {
    http: Arc<noctis::twilight::http::Client>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    run: JoinHandle<Result<(), Error>>,
}

impl Running {
    async fn start(builder: &mut AppBuilder, gw: &FakeGateway) -> Self {
        let app = builder.gateway_url(gw.url()).build().unwrap();
        let http = Arc::clone(app.http());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let run = tokio::spawn(app.run_until(async {
            let _ = stopped.await;
        }));
        let deadline = Instant::now() + WAIT;
        while gw.dispatch("TYPING_START", json!({})) < 1 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Self {
            http,
            stop: Some(stop),
            run,
        }
    }

    async fn stop(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(WAIT, &mut self.run)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

// ---- Matrix 10: request bytes ----

/// Method, path, query, headers and body.
type Comparable = (
    Method,
    String,
    Option<String>,
    Vec<(String, Vec<u8>)>,
    Vec<u8>,
);

/// The comparable parts of a request: hop-by-hop headers and `host`
/// dropped, multipart boundaries replaced.
fn comparable(request: &RecordedRequest) -> Comparable {
    let boundary = request
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split("boundary=").nth(1))
        .map(str::to_owned);
    let replace = |bytes: &[u8]| -> Vec<u8> {
        match &boundary {
            Some(b) => String::from_utf8_lossy(bytes)
                .replace(b.as_str(), "BOUNDARY")
                .into_bytes(),
            None => bytes.to_vec(),
        }
    };
    let skip = [
        "host",
        "connection",
        "keep-alive",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ];
    let headers = request
        .headers
        .iter()
        .filter(|(name, _)| !skip.contains(&name.as_str()))
        .map(|(name, value)| (name.as_str().to_owned(), replace(value.as_bytes())))
        .collect();
    (
        request.method.clone(),
        request.path.clone(),
        request.query.clone(),
        headers,
        replace(&request.body),
    )
}

/// Sends the same three calls through `client`.
async fn three_calls(client: &noctis::twilight::http::Client) {
    use noctis::twilight::http::request::AuditLogReason;
    use noctis::twilight::http::request::Request;
    use noctis::twilight::http::routing::Route;
    use noctis::twilight::model::http::attachment::Attachment;

    let _ = client.delete_message(id(1), id(2)).reason("çay ☕").await;
    let _ = client
        .create_message(id(1))
        .content("files")
        .attachments(&[
            Attachment::from_bytes("a.bin".to_owned(), vec![7; 1024 * 1024], 1),
            Attachment::from_bytes("b.bin".to_owned(), Vec::new(), 2),
        ])
        .await;
    let request = Request::builder(&Route::GetChannel { channel_id: 1 })
        .headers(
            [
                (
                    HeaderName::from_static("x-dup"),
                    HeaderValue::from_static("1"),
                ),
                (
                    HeaderName::from_static("x-other"),
                    HeaderValue::from_static("o"),
                ),
                (
                    HeaderName::from_static("x-dup"),
                    HeaderValue::from_static("2"),
                ),
            ]
            .into_iter(),
        )
        .build()
        .unwrap();
    let _ = client
        .request::<noctis::twilight::model::channel::Channel>(request)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn request_bytes_match_a_direct_client() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let control = FakeDiscordHttp::start().await.unwrap();
    let app = builder(&fake).build().unwrap();
    let direct = noctis::twilight::http::Client::builder()
        .token("fake-token".to_owned())
        .proxy(control.addr().to_string(), true)
        .ratelimiter(None)
        .build();
    three_calls(app.http()).await;
    three_calls(&direct).await;
    let through: Vec<_> = fake.requests().iter().map(comparable).collect();
    let straight: Vec<_> = control.requests().iter().map(comparable).collect();
    assert_eq!(through.len(), 3);
    assert_eq!(through, straight);
    let reason = &fake.requests()[0].headers["x-audit-log-reason"];
    assert_ne!(
        reason.as_bytes(),
        "çay ☕".as_bytes(),
        "twilight percent-encodes it"
    );
    assert!(
        fake.connections()
            .iter()
            .all(|c| c.protocol == HttpProtocol::Http2)
    );
}

// ---- Matrix 12: response bytes ----

#[tokio::test(flavor = "multi_thread")]
async fn response_bytes_pass_unchanged() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let big = vec![b'x'; 10 * 1024 * 1024];
    let mut response = FakeResponse::new(StatusCode::OK, big.clone());
    for (name, value) in [
        ("x-ratelimit-bucket", "b"),
        ("x-ratelimit-limit", "5"),
        ("x-ratelimit-remaining", "4"),
        ("x-ratelimit-reset-after", "1.5"),
    ] {
        response = response.header(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    fake.route(Method::GET, "/channels/1/messages", response);
    fake.route(
        Method::DELETE,
        "/channels/1/messages/2",
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let app = builder(&fake).build().unwrap();
    let got = app.http().channel_messages(id(1)).await.unwrap();
    assert_eq!(got.status().get(), 200);
    let headers: HeaderMap = got
        .headers()
        .map(|(n, v)| {
            (
                HeaderName::from_bytes(n.as_bytes()).unwrap(),
                HeaderValue::from_bytes(v).unwrap(),
            )
        })
        .collect();
    assert_eq!(headers["x-ratelimit-bucket"], "b");
    assert_eq!(headers["x-ratelimit-reset-after"], "1.5");
    assert_eq!(got.bytes().await.unwrap(), big);
    let deleted = app.http().delete_message(id(1), id(2)).await.unwrap();
    assert_eq!(deleted.status().get(), 204);
    let missing = app.http().channel(id(9)).await.unwrap_err();
    match missing.kind() {
        noctis::twilight::http::error::ErrorType::Response { status, body, .. } => {
            assert_eq!(status.get(), 404);
            let body: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(body, json!({ "message": "404: Not Found", "code": 0 }));
        }
        other => panic!("{other:?}"),
    }
}

// ---- Matrix 14-15: upstream failures ----

#[tokio::test(flavor = "multi_thread")]
async fn upstream_refused_is_502() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = builder(&fake).build().unwrap();
    drop(fake);
    tokio::task::yield_now().await;
    let before = Metrics::take(snapshotter);
    let started = Instant::now();
    let error = app.http().gateway().authed().await.unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    match error.kind() {
        noctis::twilight::http::error::ErrorType::Response { status, body, .. } => {
            assert_eq!(status.get(), 502);
            assert!(body.starts_with(br#"{"message":"noctis edge:"#));
        }
        other => panic!("{other:?}"),
    }
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_http_requests_total",
            Some(("status_class", "error"))
        ),
        1
    );
    let outcome = tokio::time::timeout(WAIT, app.run_until(std::future::pending()))
        .await
        .unwrap();
    assert!(
        matches!(
            outcome,
            Err(Error::Start(StartError::GatewayInfo { status: None }))
        ),
        "{outcome:?}"
    );
}

/// A real upstream 502 (a Discord body, not the edge's) keeps its status.
#[tokio::test(flavor = "multi_thread")]
async fn upstream_502_keeps_its_status() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/gateway/bot",
        FakeResponse::json(
            StatusCode::BAD_GATEWAY,
            &json!({ "message": "502: Bad Gateway", "code": 0 }),
        ),
    );
    let app = builder(&fake).build().unwrap();
    let outcome = tokio::time::timeout(WAIT, app.run_until(std::future::pending()))
        .await
        .unwrap();
    assert!(
        matches!(
            outcome,
            Err(Error::Start(StartError::GatewayInfo { status: Some(502) }))
        ),
        "{outcome:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn post_write_failure_is_not_retried() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.stall(Method::GET, "/channels/1/messages", 1);
    let app = builder(&fake).build().unwrap();
    let http = Arc::clone(app.http());
    let pending = tokio::spawn(async move { http.channel_messages(id(1)).await });
    until("the stalled request arrived", || {
        fake.requests()
            .iter()
            .any(|r| r.route_path() == Some("/channels/1/messages"))
    })
    .await;
    assert_eq!(fake.requests().len(), 1);
    drop(fake);
    let dropped = Instant::now();
    let error = tokio::time::timeout(WAIT, pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(
        dropped.elapsed() <= Duration::from_secs(1),
        "{:?}",
        dropped.elapsed()
    );
    assert!(matches!(
        error.kind(),
        noctis::twilight::http::error::ErrorType::Response { status, .. } if status.get() == 502
    ));
}

// ---- Matrix 17-19, 21, 25: the keeper ----

#[tokio::test(flavor = "multi_thread")]
async fn warm_up_before_gateway_bot() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let before = Metrics::take(snapshotter);
    let fake = FakeDiscordHttp::start().await.unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let running = Running::start(&mut builder(&fake), &gw).await;
    let connections = fake.connections();
    assert_eq!(connections[0].protocol, HttpProtocol::Http2);
    let requests = fake.requests();
    assert_eq!(requests.len(), 1, "the warm-up sent no request");
    assert_eq!(requests[0].route_path(), Some("/gateway/bot"));
    assert_eq!(requests[0].connection, 0);
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_http_connections_total",
            Some(("reason", "warmup"))
        ),
        1
    );
    running.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn warm_up_failure_is_a_warning() {
    let _serial = serial().await;
    let (capture, _) = globals();
    let mark = capture.mark();
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    let bound = app.http_config().connect_timeout + Duration::from_secs(7);
    let outcome = tokio::time::timeout(bound, app.run_until(std::future::pending()))
        .await
        .expect("run returned in time");
    assert!(
        matches!(
            outcome,
            Err(Error::Start(StartError::GatewayInfo { status: None }))
        ),
        "{outcome:?}"
    );
    let text = capture.since(mark);
    let warned = text
        .lines()
        .filter(|l| l.contains("WARN") && l.contains("could not be warmed"))
        .count();
    assert_eq!(warned, 1, "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn redial_on_close() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let running = Running::start(&mut builder(&fake), &gw).await;
    let before = Metrics::take(snapshotter);
    assert_eq!(fake.close_connections(), 1);
    let closed = Instant::now();
    until("a redial", || fake.connections().len() == 2).await;
    assert!(
        closed.elapsed() <= Duration::from_secs(1),
        "{:?}",
        closed.elapsed()
    );
    // The fake sees the connection before the pool's dial returns and
    // counts it, so poll the counter instead of reading it once; each
    // snapshot drains it, so sum the polls.
    let mut redials = 0;
    until("the redial metric", || {
        redials += delta(
            &before,
            &Metrics::take(snapshotter),
            "noctis_http_connections_total",
            Some(("reason", "redial")),
        );
        redials >= 1
    })
    .await;
    assert_eq!(redials, 1);
    let _ = running.http.current_user().await;
    let last = fake.requests().pop().unwrap();
    assert_eq!(last.route_path(), Some("/users/@me"));
    assert_eq!(last.connection, 1);
    running.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_connection_stays_warm_with_pings() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let mut b = builder(&fake);
    // Fast PINGs, but the default 5 s ACK timeout: a scheduler stall on a
    // loaded host must not turn a slow ACK into a ping-timeout redial here
    // (the black-hole tests cover the 1 s timeout).
    let mut config = fast_pings();
    config.ping_timeout = HttpConfig::default().ping_timeout;
    b.http(config);
    let running = Running::start(&mut b, &gw).await;
    let _ = Metrics::take(snapshotter);
    let requests = fake.requests().len();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let conn = &fake.connections()[0];
    assert!(conn.pings.len() >= 20, "{} PINGs", conn.pings.len());
    assert_eq!(
        fake.requests().len(),
        requests,
        "no API request kept it warm"
    );
    let middle = Metrics::take(snapshotter);
    assert!(middle.get("noctis_http_h2_ping_rtt_seconds", None) > 0);
    let _ = running.http.current_user().await;
    let after = Metrics::take(snapshotter);
    assert_eq!(fake.requests().pop().unwrap().connection, 0);
    assert_eq!(
        delta(
            &middle,
            &after,
            "noctis_http_conn_reused_total",
            Some(("reused", "true"))
        ),
        1
    );
    running.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http1_upstream_stays_http1() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let gw = FakeGateway::start().await.unwrap();
    let mut b = App::builder();
    b.runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(fake.url());
    let running = Running::start(&mut b, &gw).await;
    let connections = fake.connections();
    assert_eq!(connections[0].protocol, HttpProtocol::Http1);
    assert!(connections[0].pings.is_empty());
    let requests = fake.requests();
    assert_eq!(requests[0].route_path(), Some("/gateway/bot"));
    assert_eq!(requests[0].connection, 0);
    running.stop().await;
}

// ---- Matrix 22-23: black holes ----

#[tokio::test(flavor = "multi_thread")]
async fn black_hole_is_redialed_before_the_callback() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::POST,
        CB,
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let app = builder(&fake).http(fast_pings()).build().unwrap();
    let warm = keep_warm(&app);
    until("the warm connection", || {
        fake.connections().len() == 1 && warm.is_connected()
    })
    .await;
    let before = Metrics::take(snapshotter);
    assert_eq!(fake.black_hole(), 1);
    let holed = Instant::now();
    until("a redial", || fake.connections().len() == 2).await;
    let redialed = holed.elapsed();
    assert!(redialed <= Duration::from_millis(2100), "{redialed:?}");
    until("the redial placed", || warm.is_connected()).await;
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(&before, &after, "noctis_http_ping_timeouts_total", None),
        1
    );
    let ack = cb(&app, Duration::ZERO).await.unwrap();
    assert_eq!(ack.status, 204);
    let done = Metrics::take(snapshotter);
    assert_eq!(
        delta(&after, &done, "noctis_interaction_ack_hedges_total", None),
        0
    );
    assert_eq!(callbacks(&fake)[0].connection, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn black_hole_during_a_callback_is_hedged() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::POST,
        CB,
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let app = builder(&fake).build().unwrap();
    let warm = keep_warm(&app);
    until("the warm connection", || {
        fake.connections().len() == 1 && warm.is_connected()
    })
    .await;
    let before = Metrics::take(snapshotter);
    fake.black_hole();
    let started = Instant::now();
    let ack = cb(&app, Duration::ZERO).await.unwrap();
    assert!(started.elapsed() < Duration::from_millis(2900));
    assert_eq!(ack.status, 204);
    let hedge = &fake.connections()[1];
    let at = hedge.opened_at - started;
    assert!(
        (Duration::from_millis(900)..=Duration::from_millis(1100)).contains(&at),
        "{at:?}"
    );
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_interaction_ack_hedges_total",
            Some(("outcome", "won"))
        ),
        1
    );
}

// ---- Matrix 26-40: the callback path ----

#[tokio::test(flavor = "multi_thread")]
async fn callback_basic() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::POST,
        CB,
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let app = builder(&fake).build().unwrap();
    let ack = cb(&app, Duration::ZERO).await.unwrap();
    assert_eq!(
        ack,
        CallbackAck {
            status: 204,
            body: Bytes::new(),
            already_acknowledged: false
        }
    );
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.route_path(), Some(CB));
    assert!(request.headers.get("authorization").is_none());
    assert_eq!(request.headers["content-type"], "application/json");
    assert!(
        request.headers["user-agent"]
            .to_str()
            .unwrap()
            .starts_with("DiscordBot (")
    );
    let expected = serde_json::to_vec(&noctis::twilight::model::http::interaction::InteractionResponse {
        kind: noctis::twilight::model::http::interaction::InteractionResponseType::DeferredChannelMessageWithSource,
        data: None,
    })
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request.body).unwrap(),
        serde_json::from_slice::<serde_json::Value>(&expected).unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn callback_with_response() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let body = json!({ "interaction": { "id": "1" } });
    fake.route(Method::POST, CB, FakeResponse::json(StatusCode::OK, &body));
    let app = builder(&fake).build().unwrap();
    let ack = tokio::time::timeout(
        WAIT,
        interaction_callback(&app, 1, TOKEN, deferred(), true, Instant::now()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(ack.status, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&ack.body).unwrap(),
        body
    );
    assert_eq!(
        fake.requests()[0].query.as_deref(),
        Some("with_response=true")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn callbacks_are_not_serialized() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    for n in 1..=4 {
        fake.route(
            Method::POST,
            format!("/interactions/{n}/tok-{n}/callback"),
            FakeResponse::empty(StatusCode::NO_CONTENT).delay(Duration::from_secs(1)),
        );
    }
    let app = Arc::new(builder(&fake).build().unwrap());
    let started = Instant::now();
    let tasks: Vec<_> = (1..=4_u64)
        .map(|n| {
            let app = Arc::clone(&app);
            tokio::spawn(async move { cb_with(&app, n, &format!("tok-{n}"), Duration::ZERO).await })
        })
        .collect();
    for task in tasks {
        assert!(task.await.unwrap().is_ok());
    }
    assert!(
        started.elapsed() <= Duration::from_millis(1200),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hedge_wins() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.stall(Method::POST, CB, 1).route(
        Method::POST,
        CB,
        FakeResponse::empty(StatusCode::NO_CONTENT),
    );
    let app = builder(&fake).build().unwrap();
    let before = Metrics::take(snapshotter);
    let started = Instant::now();
    assert!(cb(&app, Duration::ZERO).await.is_ok());
    let requests = callbacks(&fake);
    assert_eq!(requests.len(), 2);
    assert_eq!((requests[0].connection, requests[1].connection), (0, 1));
    let at = fake.connections()[1].opened_at - started;
    assert!(
        (Duration::from_millis(900)..=Duration::from_millis(1100)).contains(&at),
        "{at:?}"
    );
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_interaction_ack_hedges_total",
            Some(("outcome", "won"))
        ),
        1
    );
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_http_connections_total",
            Some(("reason", "hedge"))
        ),
        1
    );
}

/// Answers the n-th call (0-based) with `responses[n]` (the last one
/// repeats).
fn sequence(fake: &FakeDiscordHttp, responses: Vec<FakeResponse>) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    fake.route_with(Method::POST, CB, move |_| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        responses[n.min(responses.len() - 1)].clone()
    });
    calls
}

#[tokio::test(flavor = "multi_thread")]
async fn hedge_loses() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    sequence(
        &fake,
        vec![
            FakeResponse::empty(StatusCode::NO_CONTENT).delay(Duration::from_millis(1300)),
            FakeResponse::empty(StatusCode::NO_CONTENT).delay(Duration::from_secs(1)),
        ],
    );
    let app = builder(&fake).build().unwrap();
    let before = Metrics::take(snapshotter);
    assert!(cb(&app, Duration::ZERO).await.is_ok());
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_interaction_ack_hedges_total",
            Some(("outcome", "lost"))
        ),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn already_acknowledged_is_success() {
    let _serial = serial().await;
    let (capture, snapshotter) = globals();
    let already = FakeResponse::json(
        StatusCode::BAD_REQUEST,
        &json!({ "message": "Interaction has already been acknowledged.", "code": 40060 }),
    );
    for stalled in [1, 0] {
        let fake = FakeDiscordHttp::start().await.unwrap();
        fake.stall(Method::POST, CB, stalled)
            .route(Method::POST, CB, already.clone());
        let app = builder(&fake).build().unwrap();
        let mark = capture.mark();
        let before = Metrics::take(snapshotter);
        let ack = cb(&app, Duration::ZERO).await.unwrap();
        assert!(ack.already_acknowledged, "stalled {stalled}");
        let after = Metrics::take(snapshotter);
        assert_eq!(
            delta(
                &before,
                &after,
                "noctis_interaction_ack_hedges_total",
                Some(("outcome", "40060"))
            ),
            1
        );
        let text = capture.since(mark);
        assert!(
            !text
                .lines()
                .any(|l| l.contains(" WARN ") || l.contains(" ERROR ")),
            "{text}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn deadline_already_passed() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = builder(&fake).build().unwrap();
    for ago in [Duration::from_millis(2900), Duration::from_secs(5)] {
        match cb(&app, ago).await {
            Err(CallbackError::DeadlineMissed(AckDeadlineMissed { elapsed, .. })) => {
                assert!(elapsed >= Duration::from_millis(2900), "{elapsed:?}");
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(fake.requests().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn both_attempts_stall() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.stall(Method::POST, CB, 2);
    let app = builder(&fake).build().unwrap();
    let started = Instant::now();
    let outcome = cb(&app, Duration::ZERO).await;
    let elapsed = started.elapsed();
    assert!(
        matches!(outcome, Err(CallbackError::DeadlineMissed(_))),
        "{outcome:?}"
    );
    assert!(
        (Duration::from_millis(2800)..=Duration::from_secs(3)).contains(&elapsed),
        "{elapsed:?}"
    );
    assert_eq!(callbacks(&fake).len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_that_fits_is_resent() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    sequence(
        &fake,
        vec![
            FakeResponse::json(
                StatusCode::TOO_MANY_REQUESTS,
                &json!({ "retry_after": 0.5 }),
            ),
            FakeResponse::empty(StatusCode::NO_CONTENT),
        ],
    );
    let app = builder(&fake).build().unwrap();
    let started = Instant::now();
    assert!(cb(&app, Duration::ZERO).await.is_ok());
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(callbacks(&fake).len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_too_long_or_unknown() {
    let _serial = serial().await;
    let _ = globals();
    for response in [
        FakeResponse::json(StatusCode::TOO_MANY_REQUESTS, &json!({ "retry_after": 5 })),
        FakeResponse::json(StatusCode::TOO_MANY_REQUESTS, &json!({ "message": "slow" })),
    ] {
        let fake = FakeDiscordHttp::start().await.unwrap();
        sequence(
            &fake,
            vec![response, FakeResponse::empty(StatusCode::NO_CONTENT)],
        );
        let app = builder(&fake).build().unwrap();
        let outcome = cb(&app, Duration::ZERO).await;
        assert!(
            matches!(outcome, Err(CallbackError::DeadlineMissed(_))),
            "{outcome:?}"
        );
        assert_eq!(callbacks(&fake).len(), 1);
    }
    let fake = FakeDiscordHttp::start().await.unwrap();
    sequence(
        &fake,
        vec![
            FakeResponse::empty(StatusCode::TOO_MANY_REQUESTS).header(
                HeaderName::from_static("retry-after"),
                HeaderValue::from_static("1"),
            ),
            FakeResponse::empty(StatusCode::NO_CONTENT),
        ],
    );
    let app = builder(&fake).build().unwrap();
    let started = Instant::now();
    assert!(cb(&app, Duration::ZERO).await.is_ok());
    assert!(started.elapsed() >= Duration::from_secs(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn other_client_errors_are_rejected() {
    let _serial = serial().await;
    let _ = globals();
    for (response, status, code) in [
        (
            FakeResponse::json(StatusCode::BAD_REQUEST, &json!({ "code": 50035 })),
            400,
            Some(50035),
        ),
        (
            FakeResponse::json(StatusCode::NOT_FOUND, &json!({ "code": 10062 })),
            404,
            Some(10062),
        ),
        (FakeResponse::empty(StatusCode::UNAUTHORIZED), 401, None),
    ] {
        let fake = FakeDiscordHttp::start().await.unwrap();
        fake.route(Method::POST, CB, response);
        let app = builder(&fake).build().unwrap();
        assert_eq!(
            cb(&app, Duration::ZERO).await,
            Err(CallbackError::Rejected { status, code })
        );
        assert_eq!(callbacks(&fake).len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn server_errors_and_transport() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let unavailable = FakeResponse::empty(StatusCode::SERVICE_UNAVAILABLE);
    let fake = FakeDiscordHttp::start().await.unwrap();
    sequence(
        &fake,
        vec![
            unavailable.clone(),
            FakeResponse::empty(StatusCode::NO_CONTENT),
        ],
    );
    let app = builder(&fake).build().unwrap();
    let before = Metrics::take(snapshotter);
    assert!(cb(&app, Duration::ZERO).await.is_ok());
    let requests = callbacks(&fake);
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[1].connection, requests[0].connection);
    let after = Metrics::take(snapshotter);
    assert_eq!(
        delta(
            &before,
            &after,
            "noctis_http_connections_total",
            Some(("reason", "hedge"))
        ),
        1
    );

    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(Method::POST, CB, unavailable);
    let app = builder(&fake).build().unwrap();
    assert_eq!(
        cb(&app, Duration::ZERO).await,
        Err(CallbackError::Rejected {
            status: 503,
            code: None
        })
    );

    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = builder(&fake).build().unwrap();
    drop(fake);
    tokio::task::yield_now().await;
    assert_eq!(
        cb(&app, Duration::ZERO).await,
        Err(CallbackError::Transport)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn token_check() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let app = builder(&fake).build().unwrap();
    let long = "a".repeat(513);
    for bad in ["", "a/b", "a?b", "a#b", "a b", "é", long.as_str()] {
        assert_eq!(
            cb_with(&app, 1, bad, Duration::ZERO).await,
            Err(CallbackError::InvalidToken),
            "{bad:?}"
        );
    }
    assert!(fake.requests().is_empty());
    let max = "a".repeat(512);
    for good in [max.as_str(), "A-z_0.9"] {
        let _ = cb_with(&app, 1, good, Duration::ZERO).await;
    }
    assert_eq!(callbacks(&fake).len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn large_error_body_is_capped() {
    let _serial = serial().await;
    let _ = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::POST,
        CB,
        FakeResponse::new(StatusCode::BAD_REQUEST, vec![b'<'; 1024 * 1024]),
    );
    let app = builder(&fake).build().unwrap();
    assert_eq!(
        cb(&app, Duration::ZERO).await,
        Err(CallbackError::Rejected {
            status: 400,
            code: None
        })
    );
}
