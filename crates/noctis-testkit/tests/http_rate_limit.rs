//! Story 12.4: shared HTTP rate limiting through a proxy (FR-52, AD-24,
//! INT-13, RL-01..04), end to end with `FakeDiscordHttp` as the proxy
//! (Matrix E1-E10, E12). Its own test binary, because it installs a global
//! capturing tracing subscriber and a global metrics recorder; the tests are
//! serialized.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis::http::{Method as RawMethod, RateLimitScope, RateLimited, RawHttp, RawHttpError};
use noctis::prelude::*;
use noctis::publish::{PublishError, PublishScope};
use noctis::twilight::model::id::Id;
use noctis::ui::{Components, TextDisplay, V2Message};
use noctis::webhook::WebhookError;
use noctis::{AppBuilder, Error, ResponderError, Runtime, StartError, Token};
use noctis_core::__testing::{CallbackError, interaction_callback};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, HeaderName, HeaderValue, Method,
    RecordedRequest, StatusCode,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
const CALLBACK: &str = "/interactions/100/tok/callback";
const ORIGINAL: &str = "/webhooks/7/tok/messages/@original";
const FOLLOWUP: &str = "/webhooks/7/tok";
const OWNED_HOOK: &str = "/webhooks/5/wtok";
const COMMANDS: &str = "/applications/7/commands";
const RATE_LIMITED: &str = "noctis_http_rate_limited_total";
const INVALID: &str = "noctis_http_invalid_requests_total";

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

/// Serializes the tests: they share the log capture, the metrics and the
/// handler results.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let guard = LOCK.lock().await;
    let (_, snapshotter) = globals();
    // Start every test from a fresh metrics window.
    let _ = Metrics::take(snapshotter);
    results().lock().unwrap().clear();
    guard
}

/// One snapshotted metric: its name, labels and count.
type Sample = (String, Vec<(String, String)>, u64);

/// Counter values since the last snapshot.
struct Metrics(Vec<Sample>);

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

/// What the test handlers observed, in order.
fn results() -> &'static Mutex<Vec<String>> {
    static RESULTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    &RESULTS
}

fn record(entry: impl Into<String>) {
    results().lock().unwrap().push(entry.into());
}

/// Polls until the handlers recorded `n` results.
async fn wait_results(n: usize) -> Vec<String> {
    let deadline = Instant::now() + WAIT;
    loop {
        let seen = results().lock().unwrap().clone();
        if seen.len() >= n {
            return seen;
        }
        assert!(
            Instant::now() < deadline,
            "handler results missing: {seen:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---- wire helpers ----

fn text(content: &str) -> Components {
    Components::new().push(TextDisplay::new(content.to_owned()))
}

fn h(name: &'static str, value: &'static str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    )
}

/// A 429 with `headers` and a JSON or raw `body`.
fn too_many(headers: &[(&'static str, &'static str)], body: Bytes) -> FakeResponse {
    let mut response = FakeResponse::new(StatusCode::TOO_MANY_REQUESTS, body);
    for (name, value) in headers {
        let (name, value) = h(name, value);
        response = response.header(name, value);
    }
    response
}

/// Discord's shared-resource 429 (code 20028, wait 600 s).
fn shared_429() -> FakeResponse {
    too_many(
        &[
            ("x-ratelimit-scope", "shared"),
            ("x-ratelimit-bucket", "abcd"),
            ("retry-after", "600"),
            ("content-type", "application/json"),
        ],
        Bytes::from_static(
            br#"{"message":"You are being rate limited.","retry_after":600.0,"global":false,"code":20028}"#,
        ),
    )
}

fn is_shared(limited: &RateLimited) -> bool {
    limited.scope == RateLimitScope::Shared
        && limited.retry_after == Some(Duration::from_mins(10))
        && limited.discord_code == Some(20028)
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// A complete message 77 in channel 55.
fn message() -> Value {
    json!({
        "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "components": [],
    })
}

/// The requests of `fake` to `method` `path` (relative to `/api/v10`).
fn to(fake: &FakeDiscordHttp, method: &Method, path: &str) -> Vec<RecordedRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.method == *method && r.route_path() == Some(path))
        .collect()
}

/// No bot token and no `with_components` (INT-13).
fn assert_interaction_request(request: &RecordedRequest) {
    assert!(
        request.headers.get("authorization").is_none(),
        "{request:?}"
    );
    assert!(
        !request
            .query
            .as_deref()
            .unwrap_or("")
            .contains("with_components"),
        "{request:?}"
    );
}

// ---- the commands ----

/// E1: replies, follows up and fetches the reply.
#[derive(SlashCommand)]
#[command("chain", description = "Reply, follow up, fetch")]
struct Chain;

impl Chain {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.reply(text("first")).await?;
        let _ = acked.follow_up(text("second"), Visibility::Public).await?;
        let _ = acked.fetch_reply().await?;
        record("chain done");
        Ok(acked.done())
    }
}

/// E5: replies, then records how a rate-limited follow-up and edit failed.
#[derive(SlashCommand)]
#[command("limited", description = "Rate-limited follow-up and edit")]
struct Limited;

fn describe(result: Result<(), ResponderError>) -> String {
    match result {
        Err(ResponderError::RateLimited(limited)) if is_shared(&limited) => "rate limited".into(),
        other => format!("{other:?}"),
    }
}

impl Limited {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.reply(text("first")).await?;
        let follow_up = acked
            .follow_up(text("second"), Visibility::Public)
            .await
            .map(drop);
        record(format!("follow_up: {}", describe(follow_up)));
        let edit = acked.edit_reply(text("edited")).await;
        record(format!("edit_reply: {}", describe(edit)));
        Ok(acked.done())
    }
}

// ---- the running App ----

/// A fake proxy answering the interaction, Publish and webhook routes.
async fn proxy() -> FakeDiscordHttp {
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    http.route(
        Method::GET,
        "/applications/@me",
        FakeResponse::json(
            StatusCode::OK,
            &json!({ "id": "7", "integration_types_config": { "0": {} } }),
        ),
    )
    .route(
        Method::GET,
        COMMANDS,
        FakeResponse::json(StatusCode::OK, &json!([])),
    )
    .route_with(Method::PUT, COMMANDS, |request| {
        FakeResponse::json(StatusCode::OK, &request.json().unwrap_or(Value::Null))
    })
    .route(
        Method::POST,
        CALLBACK,
        FakeResponse::empty(StatusCode::NO_CONTENT),
    )
    .route(
        Method::POST,
        FOLLOWUP,
        FakeResponse::json(StatusCode::OK, &json!({ "id": "56", "channel_id": "55" })),
    )
    .route(
        Method::GET,
        ORIGINAL,
        FakeResponse::json(StatusCode::OK, &message()),
    )
    .route(
        Method::PATCH,
        ORIGINAL,
        FakeResponse::json(StatusCode::OK, &message()),
    )
    .route(
        Method::POST,
        OWNED_HOOK,
        FakeResponse::json(StatusCode::OK, &json!({ "id": "60", "channel_id": "55" })),
    )
    .route(
        Method::GET,
        "/channels/1/messages",
        FakeResponse::json(StatusCode::OK, &json!([])),
    )
    .route(
        Method::GET,
        "/channels/1/pins",
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    http
}

/// A running App on one shard against `http` and a fresh gateway.
struct Running {
    gw: FakeGateway,
    http: Arc<noctis::twilight::http::Client>,
    webhooks: noctis::webhook::Webhooks,
    run: JoinHandle<Result<(), Error>>,
}

impl Running {
    async fn start(fake: &FakeDiscordHttp, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        let gw = FakeGateway::start().await.unwrap();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(fake.url())
            .gateway_url(gw.url())
            .module(Module::new("rl").add::<Chain>().add::<Limited>());
        configure(&mut builder);
        let app = builder.build().unwrap();
        let http = Arc::clone(app.http());
        let webhooks = app.webhooks();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let deadline = Instant::now() + WAIT;
        while gw.dispatch("TYPING_START", json!({})) == 0 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Self {
            gw,
            http,
            webhooks,
            run,
        }
    }

    /// Dispatches a chat-input interaction for `name`.
    fn invoke(&self, name: &str) {
        let interaction = json!({
            "id": "100",
            "application_id": "7",
            "type": 2,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "data": { "id": "9", "name": name, "type": 1 },
        });
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", interaction), 1);
    }

    fn stop(self) {
        self.run.abort();
    }
}

/// An App built against `fake` without a gateway (twilight and raw calls).
fn bare_app(fake: &FakeDiscordHttp, configure: impl FnOnce(&mut AppBuilder)) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(fake.url());
    configure(&mut builder);
    builder.build().unwrap()
}

// ---- the tests ----

/// E1 (INT-13): startup, twilight, raw REST, an owned webhook, the
/// interaction callback and the interaction webhooks all reach the proxy;
/// the twilight client has no limiter and interaction requests carry no
/// bot token and no `with_components`.
#[tokio::test(flavor = "multi_thread")]
async fn int_13_no_request_bypasses_the_proxy() {
    let _serial = serial().await;
    let fake = proxy().await;
    let running = Running::start(&fake, |b| {
        b.publisher(true);
    })
    .await;
    assert!(running.http.ratelimiter().is_none());
    running.invoke("chain");
    assert_eq!(wait_results(1).await, ["chain done"]);
    running
        .http
        .channel_messages(Id::new(1))
        .await
        .expect("channel messages");
    running
        .http
        .raw(RawMethod::Get, "channels/1/pins")
        .await
        .expect("raw pins");
    let hook = running.webhooks.owned(Id::new(5), "wtok").unwrap();
    let _ = hook
        .send(V2Message::new(text("hook")))
        .await
        .expect("webhook send");
    for (method, path) in [
        (Method::GET, "/gateway/bot"),
        (Method::GET, "/applications/@me"),
        (Method::GET, COMMANDS),
        (Method::PUT, COMMANDS),
        (Method::POST, CALLBACK),
        (Method::POST, FOLLOWUP),
        (Method::GET, ORIGINAL),
        (Method::GET, "/channels/1/messages"),
        (Method::GET, "/channels/1/pins"),
        (Method::POST, OWNED_HOOK),
    ] {
        let seen = to(&fake, &method, path);
        assert!(!seen.is_empty(), "{method} {path} did not reach the proxy");
        if [CALLBACK, FOLLOWUP, ORIGINAL].contains(&path) {
            for request in &seen {
                assert_interaction_request(request);
            }
        }
    }
    assert!(
        fake.requests().iter().all(|r| r.route_path().is_some()),
        "every request is a /api/v10 request through the edge"
    );
    running.stop();
}

/// E2 (RL-04): a proxied `shared` 429 is surfaced once as `RateLimited`,
/// counted, and not counted as invalid.
#[tokio::test(flavor = "multi_thread")]
async fn rl_04_proxied_shared_429_is_surfaced_once() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(Method::PATCH, "/channels/1", shared_429());
    let app = bare_app(&fake, |_| {});
    let error = app
        .http()
        .update_channel(Id::new(1))
        .await
        .expect_err("rate limited");
    let limited = RateLimited::from_http_error(&error).expect("a RateLimited");
    assert!(is_shared(&limited), "{limited:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(to(&fake, &Method::PATCH, "/channels/1").len(), 1);
    let metrics = Metrics::take(snapshotter);
    assert_eq!(metrics.get(RATE_LIMITED, Some(("scope", "shared"))), 1);
    assert_eq!(metrics.get(RATE_LIMITED, None), 1);
    assert_eq!(metrics.get(INVALID, None), 0);
}

/// E3, E4: a proxied `user` 429 with valid bucket headers and a scope-less
/// Cloudflare-like one are surfaced once and counted as invalid.
#[tokio::test(flavor = "multi_thread")]
async fn proxied_user_and_unknown_429_are_surfaced() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::PATCH,
        "/channels/1",
        too_many(
            &[
                ("x-ratelimit-scope", "user"),
                ("x-ratelimit-bucket", "abcd"),
                ("x-ratelimit-limit", "5"),
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset-after", "0.5"),
            ],
            Bytes::from_static(br#"{"message":"slow","retry_after":0.5,"global":false}"#),
        ),
    )
    .route(
        Method::PATCH,
        "/channels/2",
        too_many(&[], Bytes::from_static(b"error code: 1015")),
    );
    let app = bare_app(&fake, |_| {});
    for (channel, scope, retry_after) in [
        (1, RateLimitScope::User, Some(Duration::from_millis(500))),
        (2, RateLimitScope::Unknown, None),
    ] {
        let error = app
            .http()
            .update_channel(Id::new(channel))
            .await
            .expect_err("rate limited");
        let limited = RateLimited::from_http_error(&error).expect("a RateLimited");
        assert_eq!(limited.scope, scope);
        assert_eq!(limited.retry_after, retry_after);
        assert_eq!(limited.discord_code, None);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let path = format!("/channels/{channel}");
        assert_eq!(to(&fake, &Method::PATCH, &path).len(), 1, "{scope}");
        let metrics = Metrics::take(snapshotter);
        assert_eq!(metrics.get(INVALID, Some(("status", "429"))), 1, "{scope}");
        assert_eq!(
            metrics.get(RATE_LIMITED, Some(("scope", scope.as_str()))),
            1
        );
    }
}

/// E5: every Noctis surface returns the 429 typed.
#[tokio::test(flavor = "multi_thread")]
async fn rate_limited_is_typed_on_every_surface() {
    let _serial = serial().await;
    let fake = proxy().await;
    fake.route(Method::GET, "/channels/1/pins", shared_429())
        .route(Method::POST, OWNED_HOOK, shared_429())
        .route(Method::POST, FOLLOWUP, shared_429())
        .route(Method::PATCH, ORIGINAL, shared_429())
        .route(Method::PUT, COMMANDS, shared_429());
    let running = Running::start(&fake, |_| {}).await;

    let raw = running.http.raw(RawMethod::Get, "channels/1/pins").await;
    assert!(
        matches!(&raw, Err(RawHttpError::RateLimited(l)) if is_shared(l)),
        "{raw:?}"
    );
    let hook = running.webhooks.owned(Id::new(5), "wtok").unwrap();
    let sent = hook.send(V2Message::new(text("hook"))).await;
    assert!(
        matches!(&sent, Err(WebhookError::RateLimited(l)) if is_shared(l)),
        "{sent:?}"
    );
    running.invoke("limited");
    assert_eq!(
        wait_results(2).await,
        ["follow_up: rate limited", "edit_reply: rate limited"]
    );
    running.stop();

    let app = bare_app(&fake, |b| {
        b.module(Module::new("rl").add::<Chain>());
    });
    let published = app.publish(PublishScope::Global).await;
    assert!(
        matches!(
            &published,
            Err(PublishError::Rejected {
                status: 429,
                code: Some(20028),
                ..
            })
        ),
        "{published:?}"
    );

    fake.route(Method::GET, "/gateway/bot", shared_429());
    let app = bare_app(&fake, |_| {});
    let outcome = tokio::time::timeout(WAIT, app.run_until(std::future::pending()))
        .await
        .unwrap();
    assert!(
        matches!(
            outcome,
            Err(Error::Start(StartError::GatewayInfo { status: Some(429) }))
        ),
        "{outcome:?}"
    );
}

/// E6 (RL-03): upstream 401 and 403 answers, forwarded or on the callback
/// path, feed the invalid-request counter.
#[tokio::test(flavor = "multi_thread")]
async fn rl_03_unauthorized_and_forbidden_are_counted() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    let forbidden = || {
        FakeResponse::json(
            StatusCode::FORBIDDEN,
            &json!({ "message": "Missing Access", "code": 50001 }),
        )
    };
    fake.route(Method::GET, "/channels/3", forbidden())
        .route(Method::POST, "/interactions/1/tok/callback", forbidden())
        .route(
            Method::GET,
            "/channels/2",
            FakeResponse::json(
                StatusCode::UNAUTHORIZED,
                &json!({ "message": "401: Unauthorized", "code": 0 }),
            ),
        );
    let app = bare_app(&fake, |_| {});
    assert!(app.http().raw(RawMethod::Get, "channels/3").await.is_err());
    let callback = interaction_callback(
        &app,
        1,
        "tok",
        Bytes::from_static(br#"{"type":1}"#),
        false,
        Instant::now(),
    )
    .await;
    assert!(
        matches!(callback, Err(CallbackError::Rejected { status: 403, .. })),
        "{callback:?}"
    );
    // Last: twilight refuses every later request after a 401.
    assert!(app.http().raw(RawMethod::Get, "channels/2").await.is_err());
    let metrics = Metrics::take(snapshotter);
    assert_eq!(metrics.get(INVALID, Some(("status", "401"))), 1);
    assert_eq!(metrics.get(INVALID, Some(("status", "403"))), 2);
    assert_eq!(metrics.get(INVALID, None), 3);
    assert_eq!(metrics.get(RATE_LIMITED, None), 0);
    assert_eq!(
        metrics.get("noctis_http_requests_total", Some(("status_class", "4xx"))),
        3
    );
}

/// E7: a callback 429 keeps the INT-01 fit rule and is counted, not as
/// invalid when shared.
#[tokio::test(flavor = "multi_thread")]
async fn callback_429_keeps_the_fit_rule() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::POST,
        "/interactions/1/tok/callback",
        too_many(
            &[("x-ratelimit-scope", "shared")],
            Bytes::from_static(br#"{"message":"slow","retry_after":5,"global":false}"#),
        ),
    );
    let app = bare_app(&fake, |_| {});
    let callback = interaction_callback(
        &app,
        1,
        "tok",
        Bytes::from_static(br#"{"type":1}"#),
        false,
        Instant::now(),
    )
    .await;
    assert!(
        matches!(callback, Err(CallbackError::DeadlineMissed(_))),
        "{callback:?}"
    );
    let metrics = Metrics::take(snapshotter);
    assert_eq!(metrics.get(RATE_LIMITED, Some(("scope", "shared"))), 1);
    assert_eq!(metrics.get(INVALID, None), 0);
}

/// E8: 2xx, 404 and 500 answers record no rate-limit or invalid sample.
#[tokio::test(flavor = "multi_thread")]
async fn ordinary_statuses_are_not_counted() {
    let _serial = serial().await;
    let (_, snapshotter) = globals();
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/channels/1",
        FakeResponse::json(StatusCode::OK, &json!({})),
    )
    .route(
        Method::GET,
        "/channels/3",
        FakeResponse::json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({ "message": "boom", "code": 0 }),
        ),
    );
    let app = bare_app(&fake, |_| {});
    assert!(app.http().raw(RawMethod::Get, "channels/1").await.is_ok());
    // The fake's default answer: Discord's 404.
    assert!(app.http().raw(RawMethod::Get, "channels/2").await.is_err());
    assert!(app.http().raw(RawMethod::Get, "channels/3").await.is_err());
    let metrics = Metrics::take(snapshotter);
    assert_eq!(metrics.get(RATE_LIMITED, None), 0);
    assert_eq!(metrics.get(INVALID, None), 0);
    assert_eq!(metrics.get("noctis_http_requests_total", None), 3);
}

/// E9: `http_timeout` bounds the interaction callback too.
#[tokio::test(flavor = "multi_thread")]
async fn http_timeout_bounds_callbacks() {
    let _serial = serial().await;
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::POST,
        "/interactions/1/tok/callback",
        FakeResponse::empty(StatusCode::NO_CONTENT).delay(Duration::from_secs(2)),
    );
    let app = bare_app(&fake, |b| {
        b.http_timeout(Duration::from_millis(500));
    });
    let started = Instant::now();
    let callback = interaction_callback(
        &app,
        1,
        "tok",
        Bytes::from_static(br#"{"type":1}"#),
        false,
        started,
    )
    .await;
    let elapsed = started.elapsed();
    assert!(
        matches!(callback, Err(CallbackError::DeadlineMissed(_))),
        "{callback:?}"
    );
    assert!(
        (Duration::from_millis(500)..Duration::from_millis(1500)).contains(&elapsed),
        "{elapsed:?}"
    );
}

/// E10: `http_timeout` bounds a forwarded request.
#[tokio::test(flavor = "multi_thread")]
async fn http_timeout_bounds_forwarded_requests() {
    let _serial = serial().await;
    let fake = FakeDiscordHttp::start().await.unwrap();
    fake.route(
        Method::GET,
        "/channels/1",
        FakeResponse::json(StatusCode::OK, &json!({})).delay(Duration::from_secs(2)),
    );
    let app = bare_app(&fake, |b| {
        b.http_timeout(Duration::from_millis(300));
    });
    let started = Instant::now();
    let outcome = app.http().raw(RawMethod::Get, "channels/1").await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(matches!(outcome, Err(RawHttpError::Http(_))), "{outcome:?}");
}

/// E12 (RL-02): a Cluster process with a proxy does not warn.
#[tokio::test(flavor = "multi_thread")]
async fn rl_02_cluster_with_proxy_does_not_warn() {
    let _serial = serial().await;
    let (capture, _) = globals();
    let mark = capture.mark();
    let fake = FakeDiscordHttp::start_with(GatewayBot {
        shards: 2,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let running = Running::start(&fake, |b| {
        b.shard_total(2).shard_range(0..1);
    })
    .await;
    running.stop();
    let logged = capture.since(mark);
    assert!(!logged.contains("RL-02"), "{logged}");
    assert!(!logged.contains("without an HTTP proxy"), "{logged}");
}
