//! The Discord gateway fake: a local `tokio-websockets` server speaking the
//! gateway protocol (HELLO, IDENTIFY/READY, RESUME/RESUMED, heartbeats,
//! dispatches and close codes), with a frame log.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_websockets::{CloseCode, Message, ServerBuilder, WebSocketStream};

/// The pause after a failed `accept` (e.g. out of file descriptors).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(10);
/// How long a fake-initiated close waits for the client's close frame.
const CLOSE_DRAIN: Duration = Duration::from_secs(2);
/// The longest close reason a close frame carries, in bytes.
const MAX_CLOSE_REASON: usize = 123;
/// The close code a scripted close with an unsendable code uses instead.
const FALLBACK_CLOSE: u16 = 1011;

/// Close codes of protocol errors the fake answers with.
mod code {
    /// An unknown opcode.
    pub(super) const UNKNOWN_OPCODE: u16 = 4001;
    /// Invalid JSON, no integer `op`, or a binary frame.
    pub(super) const DECODE_ERROR: u16 = 4002;
    /// A command that needs an authenticated session.
    pub(super) const NOT_AUTHENTICATED: u16 = 4003;
    /// A second IDENTIFY or RESUME on an authenticated connection.
    pub(super) const ALREADY_AUTHENTICATED: u16 = 4005;
}

/// How a [`FakeGateway`] behaves; [`FakeGateway::set_config`] changes it for
/// frames handled afterwards.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_testkit::fake::{FakeGatewayConfig, IdentifyReply};
///
/// let config = FakeGatewayConfig {
///     heartbeat_interval: Duration::from_millis(100),
///     identify: IdentifyReply::Close(4004),
///     ..FakeGatewayConfig::default()
/// };
/// assert!(config.heartbeat_ack);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeGatewayConfig {
    /// The interval HELLO announces, sent in whole milliseconds, at least 1
    /// (default 41 250 ms).
    pub heartbeat_interval: Duration,
    /// Whether each heartbeat is answered with an op 11 ACK (default `true`).
    pub heartbeat_ack: bool,
    /// The answer to IDENTIFY (default [`IdentifyReply::Ready`]).
    pub identify: IdentifyReply,
    /// The answer to RESUME (default [`ResumeReply::Auto`]).
    pub resume: ResumeReply,
    /// The ids READY carries.
    pub ready: ReadyData,
}

impl Default for FakeGatewayConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(41_250),
            heartbeat_ack: true,
            identify: IdentifyReply::Ready,
            resume: ResumeReply::Auto,
            ready: ReadyData::default(),
        }
    }
}

/// The ids a READY dispatch carries. Values are not validated, so `0` is sent
/// verbatim (and twilight then fails to deserialize READY).
///
/// ```
/// let ready = noctis_testkit::fake::ReadyData::default();
/// assert_eq!((ready.user_id, ready.application_id), (1, 1));
/// assert!(ready.guild_ids.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyData {
    /// The bot user's id (default 1).
    pub user_id: u64,
    /// The application's id (default 1).
    pub application_id: u64,
    /// The guilds, each sent as unavailable (default none).
    pub guild_ids: Vec<u64>,
}

impl Default for ReadyData {
    fn default() -> Self {
        Self {
            user_id: 1,
            application_id: 1,
            guild_ids: Vec::new(),
        }
    }
}

/// How a [`FakeGateway`] answers IDENTIFY.
///
/// ```
/// use noctis_testkit::fake::IdentifyReply;
///
/// let reply = IdentifyReply::InvalidSession { resumable: false };
/// assert_ne!(reply, IdentifyReply::Ready);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifyReply {
    /// Create session `fake-session-{n}` (`n` counts every IDENTIFY received
    /// on an unauthenticated connection, whatever the reply) and send READY
    /// (seq 1).
    Ready,
    /// Send op 9 `INVALID_SESSION` with `d: resumable`.
    InvalidSession {
        /// The op 9 `d` value.
        resumable: bool,
    },
    /// Close the connection with this code (1011 when the code cannot be
    /// sent).
    Close(u16),
    /// Send nothing.
    Ignore,
}

/// How a [`FakeGateway`] answers RESUME.
///
/// ```
/// use noctis_testkit::fake::ResumeReply;
///
/// assert_eq!(ResumeReply::Close(4009), ResumeReply::Close(4009));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeReply {
    /// A known session gets RESUMED (seq = its last seq + 1); an unknown one
    /// gets op 9 with `d: false`.
    Auto,
    /// Send op 9 `INVALID_SESSION` with `d: resumable`.
    InvalidSession {
        /// The op 9 `d` value.
        resumable: bool,
    },
    /// Close the connection with this code (1011 when the code cannot be
    /// sent).
    Close(u16),
}

/// One heartbeat (op 1) a [`FakeGateway`] received, passed to the
/// [`on_heartbeat`](FakeGateway::on_heartbeat) hook.
///
/// ```
/// use std::time::{Duration, Instant};
///
/// let seen = noctis_testkit::fake::HeartbeatSeen {
///     connection: 1,
///     received_at: Instant::now(),
///     since_hello: Duration::from_millis(80),
///     since_previous: None,
///     seq: None,
/// };
/// assert_eq!(seen.connection, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatSeen {
    /// The connection id (see [`RecordedConnection::id`]).
    pub connection: u64,
    /// When the heartbeat frame was received.
    pub received_at: Instant,
    /// The time since HELLO was sent on this connection.
    pub since_hello: Duration,
    /// The time since the previous heartbeat on this connection.
    pub since_previous: Option<Duration>,
    /// The heartbeat's `d` (the client's last sequence), if an integer.
    pub seq: Option<u64>,
}

/// One WebSocket connection a [`FakeGateway`] accepted.
///
/// ```
/// use std::time::Instant;
///
/// let conn = noctis_testkit::fake::RecordedConnection {
///     id: 1,
///     uri: "/?v=10&encoding=json".to_owned(),
///     opened_at: Instant::now(),
///     closed_at: None,
/// };
/// assert!(conn.uri.starts_with("/?v=10"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedConnection {
    /// The connection id, from 1 per fake.
    pub id: u64,
    /// The request's path and query.
    pub uri: String,
    /// When the upgrade completed.
    pub opened_at: Instant,
    /// When the connection ended, if it has.
    pub closed_at: Option<Instant>,
}

/// One frame a [`FakeGateway`] received.
///
/// ```
/// use std::time::Instant;
///
/// use noctis_testkit::fake::{FrameKind, RecordedFrame};
///
/// let frame = RecordedFrame {
///     connection: 1,
///     received_at: Instant::now(),
///     kind: FrameKind::Text(r#"{"op":1,"d":null}"#.to_owned()),
/// };
/// assert_eq!(frame.op(), Some(1));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedFrame {
    /// The connection id (see [`RecordedConnection::id`]).
    pub connection: u64,
    /// When the frame was received.
    pub received_at: Instant,
    /// The frame.
    pub kind: FrameKind,
}

impl RecordedFrame {
    /// The `op` of a text frame holding a JSON object with an integer `op`.
    ///
    /// ```
    /// use std::time::Instant;
    ///
    /// use noctis_testkit::fake::{FrameKind, RecordedFrame};
    ///
    /// let frame = RecordedFrame {
    ///     connection: 1,
    ///     received_at: Instant::now(),
    ///     kind: FrameKind::Text("not json".to_owned()),
    /// };
    /// assert_eq!(frame.op(), None);
    /// ```
    pub fn op(&self) -> Option<u64> {
        self.json()?.get("op")?.as_u64()
    }

    /// A text frame parsed as JSON.
    ///
    /// ```
    /// use std::time::Instant;
    ///
    /// use noctis_testkit::fake::{FrameKind, RecordedFrame};
    ///
    /// let frame = RecordedFrame {
    ///     connection: 1,
    ///     received_at: Instant::now(),
    ///     kind: FrameKind::Text(r#"{"op":2,"d":{"token":"Bot t"}}"#.to_owned()),
    /// };
    /// assert_eq!(frame.json().unwrap()["d"]["token"], "Bot t");
    /// ```
    pub fn json(&self) -> Option<Value> {
        match &self.kind {
            FrameKind::Text(text) => serde_json::from_str(text).ok(),
            _ => None,
        }
    }
}

/// The kind and payload of a [`RecordedFrame`].
///
/// ```
/// use noctis_testkit::fake::FrameKind;
///
/// let close = FrameKind::Close { code: Some(1000), reason: String::new() };
/// assert!(matches!(close, FrameKind::Close { code: Some(1000), .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameKind {
    /// A text frame.
    Text(String),
    /// A binary frame.
    Binary(Bytes),
    /// A close frame; `code` is `None` when it carried no status.
    Close {
        /// The close code.
        code: Option<u16>,
        /// The close reason.
        reason: String,
    },
    /// A ping frame.
    Ping(Bytes),
    /// A pong frame.
    Pong(Bytes),
}

/// Why [`FakeGateway::close`] rejected a close frame (tokio-websockets would
/// panic on it).
///
/// ```
/// use noctis_testkit::fake::CloseError;
///
/// assert!(CloseError::InvalidCode { code: 1005 }.to_string().contains("3000-4999"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CloseError {
    /// The code cannot be sent in a close frame.
    #[error("close code {code} cannot be sent: use 1000-1003, 1007-1014 or 3000-4999")]
    InvalidCode {
        /// The rejected code.
        code: u16,
    },
    /// The reason is longer than 123 bytes.
    #[error("the close reason is {len} bytes long: use at most 123 bytes")]
    ReasonTooLong {
        /// The reason's length in bytes.
        len: usize,
    },
}

/// The heartbeat hook.
type HeartbeatHook = Arc<dyn Fn(&HeartbeatSeen) + Send + Sync>;

/// A message the fake handle asks a connection task to send.
enum Outgoing {
    Text(String),
    Close { code: u16, reason: String },
}

/// A connection that is still being served.
struct Live {
    tx: mpsc::UnboundedSender<Outgoing>,
    /// The session this connection is authenticated with.
    session: Option<String>,
}

/// The state shared by the fake handle and its connections.
struct State {
    url: String,
    config: FakeGatewayConfig,
    hook: Option<HeartbeatHook>,
    frames: Vec<RecordedFrame>,
    connections: Vec<RecordedConnection>,
    live: BTreeMap<u64, Live>,
    /// Known sessions and their last sequence number.
    sessions: HashMap<String, u64>,
    /// IDENTIFYs received on unauthenticated connections, whatever the reply.
    identifies: u64,
}

/// A shared, poison-tolerant handle to [`State`], plus the frame signal.
#[derive(Clone)]
struct Shared {
    state: Arc<Mutex<State>>,
    frame_recorded: Arc<Notify>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a received frame and wakes `wait_for_frame` callers.
    fn record(&self, connection: u64, kind: FrameKind) -> Instant {
        let received_at = Instant::now();
        self.lock().frames.push(RecordedFrame {
            connection,
            received_at,
            kind,
        });
        self.frame_recorded.notify_waiters();
        received_at
    }

    /// Sends `message` to every live connection matching `filter`.
    fn broadcast(&self, filter: impl Fn(&Live) -> bool, message: impl Fn() -> Outgoing) -> usize {
        self.lock()
            .live
            .values()
            .filter(|live| filter(live))
            .filter(|live| live.tx.send(message()).is_ok())
            .count()
    }
}

/// A local Discord gateway fake: a WebSocket server on `127.0.0.1:0`.
///
/// Each connection gets HELLO, then IDENTIFY is answered with READY (session
/// `fake-session-{n}`, `n` counting every IDENTIFY received on an
/// unauthenticated connection, whatever the reply), RESUME with RESUMED,
/// heartbeats with ACKs, as
/// [`FakeGatewayConfig`] scripts. It sends uncompressed text frames only and
/// records every frame it receives (see [`frames`](Self::frames)). Protocol
/// errors close the connection with Discord's codes (4001, 4002, 4003,
/// 4005). Dropping the fake stops the server and its connections.
///
/// ```no_run
/// use noctis_testkit::fake::FakeGateway;
///
/// # #[tokio::main] async fn main() -> std::io::Result<()> {
/// let gateway = FakeGateway::start().await?;
/// // Build an App with `.gateway_url(gateway.url())` and connect a shard ...
/// gateway.dispatch("MESSAGE_DELETE", serde_json::json!({"id": "3", "channel_id": "2"}));
/// # Ok(()) }
/// ```
pub struct FakeGateway {
    addr: SocketAddr,
    shared: Shared,
    accept: JoinHandle<()>,
}

impl FakeGateway {
    /// Starts a fake with the default [`FakeGatewayConfig`].
    ///
    /// # Errors
    ///
    /// When no tokio runtime is current or the listener cannot bind.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// # Ok(()) }
    /// ```
    pub async fn start() -> io::Result<Self> {
        Self::start_with(FakeGatewayConfig::default()).await
    }

    /// Starts a fake with `config`.
    ///
    /// # Errors
    ///
    /// When no tokio runtime is current or the listener cannot bind.
    ///
    /// ```no_run
    /// use std::time::Duration;
    ///
    /// use noctis_testkit::fake::{FakeGateway, FakeGatewayConfig};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let config = FakeGatewayConfig {
    ///     heartbeat_interval: Duration::from_millis(100),
    ///     ..FakeGatewayConfig::default()
    /// };
    /// let gateway = FakeGateway::start_with(config).await?;
    /// # Ok(()) }
    /// ```
    pub async fn start_with(config: FakeGatewayConfig) -> io::Result<Self> {
        let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let shared = Shared {
            state: Arc::new(Mutex::new(State {
                url: format!("ws://127.0.0.1:{}", addr.port()),
                config,
                hook: None,
                frames: Vec::new(),
                connections: Vec::new(),
                live: BTreeMap::new(),
                sessions: HashMap::new(),
                identifies: 0,
            })),
            frame_recorded: Arc::new(Notify::new()),
        };
        let accept = runtime.spawn(accept_loop(listener, shared.clone()));
        Ok(Self {
            addr,
            shared,
            accept,
        })
    }

    /// The URL, `ws://127.0.0.1:{port}`; pass it to `AppBuilder::gateway_url`.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert!(gateway.url().starts_with("ws://127.0.0.1:"));
    /// # Ok(()) }
    /// ```
    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.addr.port())
    }

    /// The bound socket address.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert_ne!(gateway.addr().port(), 0);
    /// # Ok(()) }
    /// ```
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The current configuration.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert!(gateway.config().heartbeat_ack);
    /// # Ok(()) }
    /// ```
    pub fn config(&self) -> FakeGatewayConfig {
        self.shared.lock().config.clone()
    }

    /// Replaces the configuration; it applies to frames handled afterwards
    /// (a live connection keeps the heartbeat interval its HELLO announced).
    ///
    /// ```no_run
    /// use noctis_testkit::fake::{FakeGateway, FakeGatewayConfig, IdentifyReply};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = FakeGateway::start().await?;
    /// gateway.set_config(FakeGatewayConfig {
    ///     identify: IdentifyReply::Close(4014),
    ///     ..gateway.config()
    /// });
    /// # Ok(()) }
    /// ```
    pub fn set_config(&self, config: FakeGatewayConfig) {
        self.shared.lock().config = config;
    }

    /// Calls `hook` for every heartbeat received, before the ACK is sent and
    /// outside the fake's lock. Replaces the previous hook.
    ///
    /// ```no_run
    /// use std::sync::{Arc, Mutex};
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// let seen = Arc::new(Mutex::new(Vec::new()));
    /// let sink = Arc::clone(&seen);
    /// gateway.on_heartbeat(move |beat| sink.lock().unwrap().push(beat.clone()));
    /// # Ok(()) }
    /// ```
    pub fn on_heartbeat<F>(&self, hook: F)
    where
        F: Fn(&HeartbeatSeen) + Send + Sync + 'static,
    {
        self.shared.lock().hook = Some(Arc::new(hook));
    }

    /// Sends the dispatch `{"op":0,"s":<seq>,"t":event_type,"d":data}` to
    /// every live authenticated connection, each with its session's next
    /// sequence number. Neither argument is validated. Returns the number of
    /// connections it was queued for.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert_eq!(gateway.dispatch("TYPING_START", serde_json::json!({})), 0);
    /// # Ok(()) }
    /// ```
    pub fn dispatch(&self, event_type: &str, data: Value) -> usize {
        let mut frame = Map::new();
        frame.insert("op".into(), json!(0));
        frame.insert("s".into(), json!(0));
        frame.insert("t".into(), json!(event_type));
        frame.insert("d".into(), data);
        let mut frame = Value::Object(frame);
        let mut guard = self.shared.lock();
        let state = &mut *guard;
        let mut sent = 0;
        for live in state.live.values() {
            let Some(seq) = live
                .session
                .as_ref()
                .and_then(|session| state.sessions.get_mut(session))
            else {
                continue;
            };
            *seq += 1;
            frame["s"] = json!(*seq);
            if live.tx.send(Outgoing::Text(frame.to_string())).is_ok() {
                sent += 1;
            }
        }
        sent
    }

    /// Sends `text` as a text frame to every live connection, unchanged.
    /// Returns the number of connections it was queued for.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert_eq!(gateway.send_raw(r#"{"op":7,"d":null}"#), 0);
    /// # Ok(()) }
    /// ```
    pub fn send_raw(&self, text: impl Into<String>) -> usize {
        let text = text.into();
        self.shared
            .broadcast(|_| true, || Outgoing::Text(text.clone()))
    }

    /// Closes every live connection with `code` and `reason`; sessions are
    /// kept, so clients can resume. Returns the number of connections.
    ///
    /// # Errors
    ///
    /// [`CloseError`] when `code` cannot be sent (only 1000-1003, 1007-1014
    /// and 3000-4999 can) or `reason` is longer than 123 bytes.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert_eq!(gateway.close(4000, "reconnect"), Ok(0));
    /// assert!(gateway.close(1005, "").is_err());
    /// # Ok(()) }
    /// ```
    pub fn close(&self, code: u16, reason: &str) -> Result<usize, CloseError> {
        check_close(code, reason)?;
        Ok(self.shared.broadcast(
            |_| true,
            || Outgoing::Close {
                code,
                reason: reason.to_owned(),
            },
        ))
    }

    /// Every frame received so far, in arrival order.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert!(gateway.frames().is_empty());
    /// # Ok(()) }
    /// ```
    pub fn frames(&self) -> Vec<RecordedFrame> {
        self.shared.lock().frames.clone()
    }

    /// Empties the frame log (connections are kept).
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// gateway.clear_frames();
    /// # Ok(()) }
    /// ```
    pub fn clear_frames(&self) {
        self.shared.lock().frames.clear();
    }

    /// Every connection accepted so far, in order.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// assert!(gateway.connections().is_empty());
    /// # Ok(()) }
    /// ```
    pub fn connections(&self) -> Vec<RecordedConnection> {
        self.shared.lock().connections.clone()
    }

    /// The first frame in the log matching `pred`, waiting up to `timeout`
    /// for one to arrive; `None` on timeout.
    ///
    /// ```no_run
    /// use std::time::Duration;
    ///
    /// # #[tokio::main] async fn main() -> std::io::Result<()> {
    /// let gateway = noctis_testkit::fake::FakeGateway::start().await?;
    /// let identify = gateway.wait_for_frame(Duration::from_secs(5), |f| f.op() == Some(2)).await;
    /// # Ok(()) }
    /// ```
    pub async fn wait_for_frame(
        &self,
        timeout: Duration,
        pred: impl Fn(&RecordedFrame) -> bool,
    ) -> Option<RecordedFrame> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let mut notified = pin!(self.shared.frame_recorded.notified());
            notified.as_mut().enable();
            let found = self.shared.lock().frames.iter().find(|f| pred(f)).cloned();
            if found.is_some() {
                return found;
            }
            tokio::time::timeout_at(deadline, notified).await.ok()?;
        }
    }
}

impl fmt::Debug for FakeGateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeGateway")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl Drop for FakeGateway {
    fn drop(&mut self) {
        // Dropping the accept task drops its listener and its `JoinSet`,
        // which aborts every connection.
        self.accept.abort();
    }
}

/// Checks that tokio-websockets can send a close frame with `code` and
/// `reason` without panicking.
fn check_close(code: u16, reason: &str) -> Result<(), CloseError> {
    if !matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999) {
        return Err(CloseError::InvalidCode { code });
    }
    if reason.len() > MAX_CLOSE_REASON {
        return Err(CloseError::ReasonTooLong { len: reason.len() });
    }
    Ok(())
}

/// Accepts connections until the task is aborted; dropping it drops the
/// `JoinSet`, which aborts every connection.
async fn accept_loop(listener: TcpListener, shared: Shared) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                connections.spawn(serve(stream, shared.clone()));
            }
            Err(_) => tokio::time::sleep(ACCEPT_BACKOFF).await,
        }
        // Reap finished connections so the set does not grow forever.
        while connections.try_join_next().is_some() {}
    }
}

/// What a received frame asks the connection to do.
enum Reply {
    Nothing,
    Send(String),
    Close(u16),
}

/// One served WebSocket connection.
struct Connection {
    id: u64,
    shared: Shared,
    hello_at: Instant,
    last_heartbeat: Option<Instant>,
}

/// Upgrades `stream` (a failed upgrade is answered `400` by tokio-websockets
/// and recorded nowhere) and serves it until either side closes.
async fn serve(stream: TcpStream, shared: Shared) {
    let Ok((request, mut ws)) = ServerBuilder::new().accept(stream).await else {
        return;
    };
    let uri = request
        .uri()
        .path_and_query()
        .map_or_else(|| request.uri().path().to_owned(), ToString::to_string);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (id, interval) = {
        let mut state = shared.lock();
        let id = state.connections.len() as u64 + 1;
        state.connections.push(RecordedConnection {
            id,
            uri,
            opened_at: Instant::now(),
            closed_at: None,
        });
        state.live.insert(id, Live { tx, session: None });
        (id, state.config.heartbeat_interval)
    };
    let interval = u64::try_from(interval.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let hello = json!({ "op": 10, "d": { "heartbeat_interval": interval } }).to_string();
    let mut conn = Connection {
        id,
        shared,
        hello_at: Instant::now(),
        last_heartbeat: None,
    };
    if ws.send(Message::text(hello)).await.is_ok() {
        conn.run(&mut ws, &mut rx).await;
    }
    let mut state = conn.shared.lock();
    state.live.remove(&id);
    if let Some(recorded) = state.connections.get_mut(conn.index()) {
        recorded.closed_at = Some(Instant::now());
    }
}

impl Connection {
    /// The index of this connection in `State::connections`.
    fn index(&self) -> usize {
        usize::try_from(self.id - 1).unwrap_or(usize::MAX)
    }

    /// Serves frames and outgoing messages until the connection ends.
    async fn run(
        &mut self,
        ws: &mut WebSocketStream<TcpStream>,
        rx: &mut mpsc::UnboundedReceiver<Outgoing>,
    ) {
        loop {
            tokio::select! {
                incoming = ws.next() => {
                    let Some(Ok(message)) = incoming else { return };
                    match self.receive(&message) {
                        Reply::Nothing => {}
                        Reply::Send(text) => {
                            if ws.send(Message::text(text)).await.is_err() {
                                return;
                            }
                        }
                        Reply::Close(code) => return self.close(ws, code, "").await,
                    }
                }
                Some(outgoing) = rx.recv() => match outgoing {
                    Outgoing::Text(text) => {
                        if ws.send(Message::text(text)).await.is_err() {
                            return;
                        }
                    }
                    Outgoing::Close { code, reason } => return self.close(ws, code, &reason).await,
                },
            }
        }
    }

    /// Sends a close frame (1011 when `code` or `reason` cannot be sent),
    /// then records frames until the client acknowledges or a bound elapses.
    /// The session is kept.
    async fn close(&self, ws: &mut WebSocketStream<TcpStream>, code: u16, reason: &str) {
        self.shared.lock().live.remove(&self.id);
        let (code, reason) = match check_close(code, reason) {
            Ok(()) => (code, reason),
            Err(_) => (FALLBACK_CLOSE, ""),
        };
        let Ok(code) = CloseCode::try_from(code) else {
            return;
        };
        if ws.send(Message::close(Some(code), reason)).await.is_err() {
            return;
        }
        let drain = async {
            while let Some(Ok(message)) = ws.next().await {
                self.shared.record(self.id, frame_kind(&message));
            }
        };
        let _ = tokio::time::timeout(CLOSE_DRAIN, drain).await;
    }

    /// Records `message`, then decides the answer.
    fn receive(&mut self, message: &Message) -> Reply {
        let kind = frame_kind(message);
        let received_at = self.shared.record(self.id, kind.clone());
        match kind {
            FrameKind::Text(text) => self.receive_text(&text, received_at),
            FrameKind::Binary(_) => Reply::Close(code::DECODE_ERROR),
            FrameKind::Close { code, .. } => {
                if matches!(code, Some(1000 | 1001)) {
                    self.forget_session();
                }
                Reply::Nothing
            }
            FrameKind::Ping(_) | FrameKind::Pong(_) => Reply::Nothing,
        }
    }

    /// Answers one text frame by its `op`.
    fn receive_text(&mut self, text: &str, received_at: Instant) -> Reply {
        let Ok(payload) = serde_json::from_str::<Value>(text) else {
            return Reply::Close(code::DECODE_ERROR);
        };
        let Some(op) = payload.get("op").and_then(Value::as_u64) else {
            return Reply::Close(code::DECODE_ERROR);
        };
        match op {
            1 => self.heartbeat(&payload, received_at),
            2 => self.identify(&payload),
            6 => self.resume(&payload),
            3 | 4 | 8 | 31 => {
                if self.authenticated() {
                    Reply::Nothing
                } else {
                    Reply::Close(code::NOT_AUTHENTICATED)
                }
            }
            _ => Reply::Close(code::UNKNOWN_OPCODE),
        }
    }

    /// Whether this connection holds a session.
    fn authenticated(&self) -> bool {
        self.shared
            .lock()
            .live
            .get(&self.id)
            .is_some_and(|live| live.session.is_some())
    }

    /// Drops this connection's session (a client close with 1000/1001).
    fn forget_session(&self) {
        let mut state = self.shared.lock();
        if let Some(session) = state
            .live
            .get_mut(&self.id)
            .and_then(|live| live.session.take())
        {
            state.sessions.remove(&session);
        }
    }

    /// Calls the hook, then ACKs when configured.
    fn heartbeat(&mut self, payload: &Value, received_at: Instant) -> Reply {
        let seen = HeartbeatSeen {
            connection: self.id,
            received_at,
            since_hello: received_at.saturating_duration_since(self.hello_at),
            since_previous: self
                .last_heartbeat
                .map(|previous| received_at.saturating_duration_since(previous)),
            seq: payload.get("d").and_then(Value::as_u64),
        };
        self.last_heartbeat = Some(received_at);
        let (hook, ack) = {
            let state = self.shared.lock();
            (state.hook.clone(), state.config.heartbeat_ack)
        };
        if let Some(hook) = hook {
            hook(&seen);
        }
        if ack {
            Reply::Send(json!({ "op": 11 }).to_string())
        } else {
            Reply::Nothing
        }
    }

    /// Answers IDENTIFY per `FakeGatewayConfig::identify`.
    fn identify(&self, payload: &Value) -> Reply {
        let mut guard = self.shared.lock();
        let state = &mut *guard;
        let Some(live) = state.live.get_mut(&self.id) else {
            return Reply::Nothing;
        };
        if live.session.is_some() {
            return Reply::Close(code::ALREADY_AUTHENTICATED);
        }
        // `n` of `fake-session-{n}` counts every IDENTIFY received on an
        // unauthenticated connection, whatever the reply, so a retry after
        // INVALID_SESSION gets a new number.
        state.identifies += 1;
        match state.config.identify {
            IdentifyReply::Ready => {
                let session = format!("fake-session-{}", state.identifies);
                state.sessions.insert(session.clone(), 1);
                let data = ready_data(
                    &state.config.ready,
                    &session,
                    &state.url,
                    payload.pointer("/d/shard"),
                );
                live.session = Some(session);
                Reply::Send(json!({ "op": 0, "s": 1, "t": "READY", "d": data }).to_string())
            }
            IdentifyReply::InvalidSession { resumable } => invalid_session(resumable),
            IdentifyReply::Close(code) => Reply::Close(code),
            IdentifyReply::Ignore => Reply::Nothing,
        }
    }

    /// Answers RESUME per `FakeGatewayConfig::resume`.
    fn resume(&self, payload: &Value) -> Reply {
        let mut guard = self.shared.lock();
        let state = &mut *guard;
        let Some(live) = state.live.get_mut(&self.id) else {
            return Reply::Nothing;
        };
        if live.session.is_some() {
            return Reply::Close(code::ALREADY_AUTHENTICATED);
        }
        match state.config.resume {
            ResumeReply::Auto => {
                let session = payload
                    .pointer("/d/session_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let Some(seq) = state.sessions.get_mut(session) else {
                    return invalid_session(false);
                };
                *seq += 1;
                let seq = *seq;
                live.session = Some(session.to_owned());
                Reply::Send(json!({ "op": 0, "s": seq, "t": "RESUMED", "d": null }).to_string())
            }
            ResumeReply::InvalidSession { resumable } => invalid_session(resumable),
            ResumeReply::Close(code) => Reply::Close(code),
        }
    }
}

/// An op 9 `INVALID_SESSION` frame.
fn invalid_session(resumable: bool) -> Reply {
    Reply::Send(json!({ "op": 9, "d": resumable }).to_string())
}

/// READY's `d`.
fn ready_data(ready: &ReadyData, session: &str, url: &str, shard: Option<&Value>) -> Value {
    let guilds: Vec<Value> = ready
        .guild_ids
        .iter()
        .map(|id| json!({ "id": id.to_string(), "unavailable": true }))
        .collect();
    let mut data = Map::new();
    data.insert("v".into(), json!(10));
    data.insert(
        "user".into(),
        json!({
            "id": ready.user_id.to_string(),
            "username": "fake-bot",
            "discriminator": "0000",
            "avatar": null,
            "bot": true,
            "mfa_enabled": false,
        }),
    );
    data.insert(
        "application".into(),
        json!({ "id": ready.application_id.to_string(), "flags": 0 }),
    );
    data.insert("guilds".into(), Value::Array(guilds));
    data.insert("session_id".into(), json!(session));
    data.insert("resume_gateway_url".into(), json!(url));
    if let Some(shard) = shard {
        data.insert("shard".into(), shard.clone());
    }
    Value::Object(data)
}

/// The recorded form of `message`.
fn frame_kind(message: &Message) -> FrameKind {
    let payload = Bytes::copy_from_slice(message.as_payload());
    if let Some(text) = message.as_text() {
        FrameKind::Text(text.to_owned())
    } else if let Some((code, reason)) = message.as_close() {
        FrameKind::Close {
            code: (!payload.is_empty()).then(|| code.into()),
            reason: reason.to_owned(),
        }
    } else if message.is_binary() {
        FrameKind::Binary(payload)
    } else if message.is_ping() {
        FrameKind::Ping(payload)
    } else {
        FrameKind::Pong(payload)
    }
}

#[cfg(test)]
mod tests {
    use twilight_model::gateway::payload::incoming::Ready;

    use super::*;

    #[test]
    fn ready_deserializes_into_twilight() {
        let ready = ReadyData {
            user_id: 7,
            application_id: 8,
            guild_ids: vec![1, 2],
        };
        let data = ready_data(&ready, "fake-session-1", "ws://x:1", Some(&json!([0, 1])));
        let model: Ready = serde_json::from_value(data).unwrap();
        assert_eq!(model.user.id.get(), 7);
        assert_eq!(model.application.id.get(), 8);
        assert_eq!(model.guilds.len(), 2);
        assert!(model.guilds.iter().all(|g| g.unavailable));
        assert_eq!(model.session_id, "fake-session-1");
        assert_eq!(model.resume_gateway_url, "ws://x:1");
        assert_eq!(model.shard.map(|s| (s.number(), s.total())), Some((0, 1)));
        assert_eq!(model.version, 10);
    }

    #[test]
    fn ready_without_shard() {
        let data = ready_data(&ReadyData::default(), "s", "ws://x:1", None);
        assert!(data.get("shard").is_none());
        let model: Ready = serde_json::from_value(data).unwrap();
        assert!(model.shard.is_none());
    }

    #[test]
    fn close_validation() {
        for code in [1000, 1003, 1007, 1014, 3000, 4999] {
            assert_eq!(check_close(code, ""), Ok(()), "{code}");
            // Every accepted code must also be accepted by tokio-websockets.
            let close = CloseCode::try_from(code).unwrap();
            assert!(!close.is_reserved());
        }
        for code in [999, 1004, 1005, 1006, 1015, 1016, 2999, 5000] {
            assert_eq!(check_close(code, ""), Err(CloseError::InvalidCode { code }));
        }
        assert_eq!(
            check_close(1000, &"a".repeat(124)),
            Err(CloseError::ReasonTooLong { len: 124 })
        );
    }
}
