//! Local Discord fakes for end-to-end tests (FR-51, AD-14).
//!
//! [`FakeDiscordHttp`] is a Discord REST fake: an HTTP/1.1 server on
//! `127.0.0.1` that answers per-test routes under `/api/v10`, a canned
//! `GET /gateway/bot`, and logs every request. An App reaches it through
//! `AppBuilder::http_proxy(fake.url())`.
//!
//! ```no_run
//! use noctis_testkit::fake::{FakeDiscordHttp, FakeResponse, Method, StatusCode};
//!
//! # #[tokio::main] async fn main() -> std::io::Result<()> {
//! let fake = FakeDiscordHttp::start().await?;
//! fake.route(Method::GET, "/users/@me", FakeResponse::json(StatusCode::OK, &serde_json::json!({})));
//! // Build an App with `.http_proxy(fake.url())` and send requests ...
//! assert!(fake.requests().is_empty());
//! # Ok(()) }
//! ```

mod http;

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
pub use hyper::header::{HeaderName, HeaderValue};
pub use hyper::{HeaderMap, Method, StatusCode};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The largest request body [`FakeDiscordHttp`] accepts (64 MiB); a larger one
/// is answered with `413` and Discord's code `40005`.
///
/// ```
/// assert_eq!(noctis_testkit::fake::MAX_BODY_BYTES, 64 * 1024 * 1024);
/// ```
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// The path prefix every Discord REST request starts with.
const API_PREFIX: &str = "/api/v10";

/// The canned `GET /gateway/bot` answer of a [`FakeDiscordHttp`].
///
/// Values are not validated, so a test can script ones twilight or Discord
/// would reject (`0`, `u32::MAX`).
///
/// ```
/// use noctis_testkit::fake::GatewayBot;
///
/// let bot = GatewayBot { shards: 4, ..GatewayBot::default() };
/// assert_eq!(bot.to_json()["shards"], 4);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayBot {
    /// The gateway URL (default `wss://gateway.discord.gg`).
    pub url: String,
    /// The recommended shard count (default 1).
    pub shards: u32,
    /// The session start limit.
    pub session_start_limit: SessionStartLimit,
}

impl GatewayBot {
    /// The answer in Discord's JSON shape.
    ///
    /// ```
    /// let json = noctis_testkit::fake::GatewayBot::default().to_json();
    /// assert_eq!(json["session_start_limit"]["reset_after"], 14_400_000);
    /// ```
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "url": self.url,
            "shards": self.shards,
            "session_start_limit": self.session_start_limit.to_json(),
        })
    }
}

impl Default for GatewayBot {
    fn default() -> Self {
        Self {
            url: "wss://gateway.discord.gg".to_owned(),
            shards: 1,
            session_start_limit: SessionStartLimit::default(),
        }
    }
}

/// The `session_start_limit` of a [`GatewayBot`].
///
/// Fields are `u32` on purpose (twilight's `max_concurrency` is `u16`), so a
/// test can script values twilight rejects.
///
/// ```
/// use noctis_testkit::fake::SessionStartLimit;
///
/// let limit = SessionStartLimit { remaining: 0, ..SessionStartLimit::default() };
/// assert_eq!(limit.to_json()["remaining"], 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStartLimit {
    /// Total session starts per window (default 1000).
    pub total: u32,
    /// Session starts left (default 1000).
    pub remaining: u32,
    /// Milliseconds until the window resets (default 14 400 000).
    pub reset_after_ms: u64,
    /// Identify buckets (default 1).
    pub max_concurrency: u32,
}

impl SessionStartLimit {
    /// The limit in Discord's JSON shape (`reset_after` key).
    ///
    /// ```
    /// let json = noctis_testkit::fake::SessionStartLimit::default().to_json();
    /// assert_eq!(json["max_concurrency"], 1);
    /// ```
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total": self.total,
            "remaining": self.remaining,
            "reset_after": self.reset_after_ms,
            "max_concurrency": self.max_concurrency,
        })
    }
}

impl Default for SessionStartLimit {
    fn default() -> Self {
        Self {
            total: 1000,
            remaining: 1000,
            reset_after_ms: 14_400_000,
            max_concurrency: 1,
        }
    }
}

/// A scripted response of a [`FakeDiscordHttp`] route.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_testkit::fake::{FakeResponse, HeaderName, HeaderValue, StatusCode};
///
/// let response = FakeResponse::empty(StatusCode::NO_CONTENT)
///     .header(HeaderName::from_static("x-test"), HeaderValue::from_static("1"))
///     .delay(Duration::from_millis(10));
/// ```
#[derive(Clone, Debug)]
pub struct FakeResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    delay: Option<Duration>,
}

impl FakeResponse {
    /// A response with `status` and a raw `body`.
    ///
    /// ```
    /// use noctis_testkit::fake::{FakeResponse, StatusCode};
    ///
    /// let response = FakeResponse::new(StatusCode::OK, "plain text");
    /// ```
    pub fn new(status: StatusCode, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body: body.into(),
            delay: None,
        }
    }

    /// A JSON response (`content-type: application/json`).
    ///
    /// ```
    /// use noctis_testkit::fake::{FakeResponse, StatusCode};
    ///
    /// let response = FakeResponse::json(StatusCode::OK, &serde_json::json!({ "id": "1" }));
    /// ```
    pub fn json(status: StatusCode, body: &serde_json::Value) -> Self {
        Self::new(status, body.to_string()).header(
            hyper::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
    }

    /// A response without a body.
    ///
    /// ```
    /// use noctis_testkit::fake::{FakeResponse, StatusCode};
    ///
    /// let response = FakeResponse::empty(StatusCode::NO_CONTENT);
    /// ```
    pub fn empty(status: StatusCode) -> Self {
        Self::new(status, Bytes::new())
    }

    /// Adds a header (several values of one name are all sent).
    ///
    /// ```
    /// use noctis_testkit::fake::{FakeResponse, HeaderName, HeaderValue, StatusCode};
    ///
    /// let response = FakeResponse::empty(StatusCode::TOO_MANY_REQUESTS)
    ///     .header(HeaderName::from_static("retry-after"), HeaderValue::from_static("1"));
    /// ```
    #[must_use]
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.append(name, value);
        self
    }

    /// Waits `delay` (after logging the request) before responding.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use noctis_testkit::fake::{FakeResponse, StatusCode};
    ///
    /// let slow = FakeResponse::empty(StatusCode::OK).delay(Duration::from_secs(2));
    /// ```
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// A Discord-shaped JSON error.
    fn discord_error(status: StatusCode, message: &str, code: u32) -> Self {
        Self::json(
            status,
            &serde_json::json!({ "message": message, "code": code }),
        )
    }
}

/// A request a [`FakeDiscordHttp`] received.
///
/// ```no_run
/// # #[tokio::main] async fn main() -> std::io::Result<()> {
/// let fake = noctis_testkit::fake::FakeDiscordHttp::start().await?;
/// // ... send requests ...
/// for request in fake.requests() {
///     println!("{} {} {:?}", request.method, request.path, request.route_path());
/// }
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    /// The request method.
    pub method: Method,
    /// The full, undecoded path, e.g. `/api/v10/gateway/bot`.
    pub path: String,
    /// The raw query string, without `?`.
    pub query: Option<String>,
    /// The request headers.
    pub headers: HeaderMap,
    /// The request body (empty when it was rejected as too large).
    pub body: Bytes,
}

impl RecordedRequest {
    /// The path after `/api/v10` (e.g. `/gateway/bot`), or `None` when the
    /// request was outside the API.
    ///
    /// ```
    /// use noctis_testkit::fake::{HeaderMap, Method, RecordedRequest};
    ///
    /// let request = RecordedRequest {
    ///     method: Method::GET,
    ///     path: "/api/v10/gateway/bot".into(),
    ///     query: None,
    ///     headers: HeaderMap::new(),
    ///     body: Default::default(),
    /// };
    /// assert_eq!(request.route_path(), Some("/gateway/bot"));
    /// ```
    pub fn route_path(&self) -> Option<&str> {
        self.path
            .strip_prefix(API_PREFIX)
            .filter(|rest| rest.starts_with('/'))
    }

    /// The body parsed as JSON, or `None` when it is not JSON.
    ///
    /// ```
    /// use noctis_testkit::fake::{HeaderMap, Method, RecordedRequest};
    ///
    /// let request = RecordedRequest {
    ///     method: Method::POST,
    ///     path: "/api/v10/channels/1/messages".into(),
    ///     query: None,
    ///     headers: HeaderMap::new(),
    ///     body: r#"{"content":"hi"}"#.into(),
    /// };
    /// assert_eq!(request.json().unwrap()["content"], "hi");
    /// ```
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// A per-test route handler.
type Handler = Arc<dyn Fn(&RecordedRequest) -> FakeResponse + Send + Sync>;

/// What a route answers.
#[derive(Clone)]
enum Route {
    Fixed(FakeResponse),
    Dynamic(Handler),
}

impl Route {
    fn respond(&self, request: &RecordedRequest) -> FakeResponse {
        match self {
            Self::Fixed(response) => response.clone(),
            Self::Dynamic(handler) => handler(request),
        }
    }
}

/// The state shared by the fake handle and its connections.
struct State {
    gateway_bot: GatewayBot,
    routes: HashMap<(Method, String), Route>,
    requests: Vec<RecordedRequest>,
}

/// A shared, poison-tolerant handle to [`State`].
#[derive(Clone)]
struct Shared(Arc<Mutex<State>>);

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A local Discord HTTP fake: an HTTP/1.1 server on `127.0.0.1:0`.
///
/// Every request is logged (see [`requests`](Self::requests)). Under
/// `/api/v10` it answers, in order: the per-test route registered for the
/// exact method and raw path (query ignored; the last registration wins), the
/// canned `GET /gateway/bot`, or a Discord-shaped `404`. Bodies over
/// [`MAX_BODY_BYTES`] get `413`. Dropping the fake stops the server and its
/// connections.
///
/// ```no_run
/// use noctis_testkit::fake::{FakeDiscordHttp, FakeResponse, Method, StatusCode};
///
/// # #[tokio::main] async fn main() -> std::io::Result<()> {
/// let fake = FakeDiscordHttp::start().await?;
/// fake.route(Method::DELETE, "/channels/1", FakeResponse::empty(StatusCode::NO_CONTENT));
/// println!("point the App at {}", fake.url());
/// # Ok(()) }
/// ```
pub struct FakeDiscordHttp {
    addr: SocketAddr,
    shared: Shared,
    accept: JoinHandle<()>,
}

impl FakeDiscordHttp {
    /// Starts a fake with the default [`GatewayBot`].
    ///
    /// # Errors
    ///
    /// When no tokio runtime is current or the listener cannot bind.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = noctis_testkit::fake::FakeDiscordHttp::start().await?;
    /// # Ok(()) }
    /// ```
    pub async fn start() -> io::Result<Self> {
        Self::start_with(GatewayBot::default()).await
    }

    /// Starts a fake whose `GET /gateway/bot` answers `gateway_bot`.
    ///
    /// # Errors
    ///
    /// When no tokio runtime is current or the listener cannot bind.
    ///
    /// ```no_run
    /// use noctis_testkit::fake::{FakeDiscordHttp, GatewayBot};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = FakeDiscordHttp::start_with(GatewayBot { shards: 4, ..GatewayBot::default() }).await?;
    /// # Ok(()) }
    /// ```
    pub async fn start_with(gateway_bot: GatewayBot) -> io::Result<Self> {
        let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let shared = Shared(Arc::new(Mutex::new(State {
            gateway_bot,
            routes: HashMap::new(),
            requests: Vec::new(),
        })));
        let accept = runtime.spawn(http::accept_loop(listener, shared.clone()));
        Ok(Self {
            addr,
            shared,
            accept,
        })
    }

    /// The base URL, `http://127.0.0.1:{port}`; pass it to
    /// `AppBuilder::http_proxy`.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = noctis_testkit::fake::FakeDiscordHttp::start().await?;
    /// assert!(fake.url().starts_with("http://127.0.0.1:"));
    /// # Ok(()) }
    /// ```
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    /// The bound socket address.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = noctis_testkit::fake::FakeDiscordHttp::start().await?;
    /// assert_ne!(fake.addr().port(), 0);
    /// # Ok(()) }
    /// ```
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Replaces the canned `GET /gateway/bot` answer.
    ///
    /// ```no_run
    /// use noctis_testkit::fake::{FakeDiscordHttp, GatewayBot};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = FakeDiscordHttp::start().await?;
    /// fake.set_gateway_bot(GatewayBot { shards: 7, ..GatewayBot::default() });
    /// # Ok(()) }
    /// ```
    pub fn set_gateway_bot(&self, gateway_bot: GatewayBot) {
        self.shared.lock().gateway_bot = gateway_bot;
    }

    /// Answers `method` on `path` (relative to `/api/v10`; a leading `/` is
    /// added when missing; compared raw, without percent-decoding) with
    /// `response`. The last registration for a method and path wins.
    ///
    /// ```no_run
    /// use noctis_testkit::fake::{FakeDiscordHttp, FakeResponse, Method, StatusCode};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = FakeDiscordHttp::start().await?;
    /// fake.route(Method::GET, "/gateway/bot", FakeResponse::empty(StatusCode::UNAUTHORIZED))
    ///     .route(Method::GET, "channels/1", FakeResponse::empty(StatusCode::NOT_FOUND));
    /// # Ok(()) }
    /// ```
    pub fn route(&self, method: Method, path: impl Into<String>, response: FakeResponse) -> &Self {
        self.insert_route(method, path.into(), Route::Fixed(response))
    }

    /// Like [`route`](Self::route), but builds each response from the
    /// recorded request.
    ///
    /// ```no_run
    /// use noctis_testkit::fake::{FakeDiscordHttp, FakeResponse, Method, StatusCode};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = FakeDiscordHttp::start().await?;
    /// fake.route_with(Method::POST, "/echo", |req| {
    ///     FakeResponse::new(StatusCode::OK, req.body.clone())
    /// });
    /// # Ok(()) }
    /// ```
    pub fn route_with<F>(&self, method: Method, path: impl Into<String>, handler: F) -> &Self
    where
        F: Fn(&RecordedRequest) -> FakeResponse + Send + Sync + 'static,
    {
        self.insert_route(method, path.into(), Route::Dynamic(Arc::new(handler)))
    }

    /// Every request received so far, in arrival order.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = noctis_testkit::fake::FakeDiscordHttp::start().await?;
    /// assert!(fake.requests().is_empty());
    /// # Ok(()) }
    /// ```
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.shared.lock().requests.clone()
    }

    /// Empties the request log.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let fake = noctis_testkit::fake::FakeDiscordHttp::start().await?;
    /// fake.clear_requests();
    /// # Ok(()) }
    /// ```
    pub fn clear_requests(&self) {
        self.shared.lock().requests.clear();
    }

    fn insert_route(&self, method: Method, mut path: String, route: Route) -> &Self {
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        self.shared.lock().routes.insert((method, path), route);
        self
    }
}

impl fmt::Debug for FakeDiscordHttp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeDiscordHttp")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl Drop for FakeDiscordHttp {
    fn drop(&mut self) {
        // Dropping the accept task drops its listener and its `JoinSet`,
        // which aborts every connection.
        self.accept.abort();
    }
}
