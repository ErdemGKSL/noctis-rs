//! Stored gateway sessions: the only reader and writer of the
//! `SessionStore` port (FR-4, AD-21).
//!
//! A shard's entry lives under `shard:{id}:{total}` (before the App's
//! `<app>:session:` prefix) and holds `(session_id, sequence, resume_url)`
//! as a schema-1 typed value. It is read at shard start, written after a
//! requested resumable close is acknowledged, and deleted when the gateway
//! rejects it or on a fresh close. Logs never carry the session id, the
//! resume URL or the stored bytes.

use std::future::Future;
use std::time::Duration;

use noctis_ports::{PortError, SessionStore, decode_value, encode_value};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use twilight_gateway::{Session, ShardId};

use crate::inbound::ShardEvent;
use crate::shard_config::check_gateway_url;

/// The schema byte of a stored session.
const SCHEMA: u8 = 1;
/// The longest accepted session id, in bytes.
const MAX_SESSION_ID: usize = 256;
/// The longest accepted resume URL, in bytes.
const MAX_RESUME_URL: usize = 2048;
/// The bound on the start-time `get` and on a reject-time `delete`.
pub(crate) const STORE_TIMEOUT: Duration = Duration::from_secs(5);

/// The stored value (postcard, field order is the format).
#[derive(Debug, Serialize, Deserialize)]
struct StoredSession {
    session_id: String,
    sequence: u64,
    resume_url: String,
}

/// The store key of shard `id` (`shard:{id}:{total}`).
pub(crate) fn key(id: ShardId) -> String {
    format!("shard:{}:{}", id.number(), id.total())
}

/// Why a store call did not finish.
enum Failure {
    Port(PortError),
    Timeout,
}

/// Runs `call` until `deadline`.
async fn bounded<T>(
    deadline: Instant,
    call: impl Future<Output = Result<T, PortError>>,
) -> Result<T, Failure> {
    match tokio::time::timeout_at(deadline, call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(Failure::Port(error)),
        Err(_) => Err(Failure::Timeout),
    }
}

/// Decodes and checks a stored value; `None` when it is not a usable
/// session.
fn parse(raw: &[u8]) -> Option<(Session, String)> {
    let stored: StoredSession = decode_value(SCHEMA, raw).ok()??;
    let id_ok = (1..=MAX_SESSION_ID).contains(&stored.session_id.len());
    let url_ok = (1..=MAX_RESUME_URL).contains(&stored.resume_url.len())
        && check_gateway_url(&stored.resume_url).is_ok();
    (id_ok && url_ok).then(|| {
        (
            Session::new(stored.sequence, stored.session_id),
            stored.resume_url,
        )
    })
}

/// Loads the stored session under `key`: `Some((session, resume_url))`
/// when a valid entry exists. An unreadable or invalid entry is deleted; a
/// store error or a `get` slower than [`STORE_TIMEOUT`] only logs. `None`
/// means the shard identifies.
pub(crate) async fn load(store: &dyn SessionStore, key: &str) -> Option<(Session, String)> {
    let raw = match bounded(Instant::now() + STORE_TIMEOUT, store.get(key)).await {
        Ok(raw) => raw?,
        Err(Failure::Port(error)) => {
            tracing::warn!(
                target: "noctis::gateway",
                %error,
                "session store read failed; identifying"
            );
            return None;
        }
        Err(Failure::Timeout) => {
            tracing::warn!(
                target: "noctis::gateway",
                timeout = ?STORE_TIMEOUT,
                "session store read timed out; identifying"
            );
            return None;
        }
    };
    let Some((session, url)) = parse(&raw) else {
        tracing::warn!(target: "noctis::gateway", "stored session unreadable; identifying");
        clear(store, key, Instant::now() + STORE_TIMEOUT).await;
        return None;
    };
    tracing::info!(
        target: "noctis::gateway",
        sequence = session.sequence(),
        "resuming a stored session"
    );
    Some((session, url))
}

/// Stores `session` and `resume_url` under `key` (no TTL), until
/// `deadline`; a failure only logs.
pub(crate) async fn save(
    store: &dyn SessionStore,
    key: &str,
    session: &Session,
    resume_url: &str,
    deadline: Instant,
) {
    let stored = StoredSession {
        session_id: session.id().to_owned(),
        sequence: session.sequence(),
        resume_url: resume_url.to_owned(),
    };
    let Ok(value) = encode_value(SCHEMA, &stored) else {
        tracing::warn!(target: "noctis::gateway", "session could not be encoded; not stored");
        return;
    };
    match bounded(deadline, store.put(key, value, None)).await {
        Ok(()) => tracing::debug!(
            target: "noctis::gateway",
            sequence = session.sequence(),
            "session stored"
        ),
        Err(Failure::Port(error)) => tracing::warn!(
            target: "noctis::gateway",
            %error,
            "session store write failed; session not stored"
        ),
        Err(Failure::Timeout) => tracing::warn!(
            target: "noctis::gateway",
            "session store write timed out; session not stored"
        ),
    }
}

/// Deletes the entry under `key`, until `deadline`; a failure only logs.
pub(crate) async fn clear(store: &dyn SessionStore, key: &str, deadline: Instant) {
    match bounded(deadline, store.delete(key)).await {
        Ok(()) => tracing::debug!(target: "noctis::gateway", "stored session deleted"),
        Err(Failure::Port(error)) => tracing::warn!(
            target: "noctis::gateway",
            %error,
            "session store delete failed"
        ),
        Err(Failure::Timeout) => tracing::warn!(
            target: "noctis::gateway",
            "session store delete timed out"
        ),
    }
}

/// What happened to a stored session the shard started with.
///
/// Pending until RESUMED (kept), READY (superseded) or op 9 with `d: false`
/// (rejected); after that it reacts to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResumeState {
    pending: bool,
}

/// Why the stored session is to be deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stale {
    /// The gateway answered op 9 `d: false`.
    Rejected,
    /// A READY arrived without a rejection: a new session replaced it.
    Superseded,
}

impl ResumeState {
    /// `pending` when the shard starts with a stored session.
    pub(crate) const fn new(pending: bool) -> Self {
        Self { pending }
    }

    /// Feeds one frame's status and op 9 `d`; `Some` exactly once, when the
    /// stored entry must be deleted now.
    pub(crate) fn on_frame(
        &mut self,
        status: Option<&ShardEvent>,
        invalid_session: Option<bool>,
    ) -> Option<Stale> {
        if !self.pending {
            return None;
        }
        let stale = match (status, invalid_session) {
            (Some(ShardEvent::Resumed { .. }), _) => None,
            (Some(ShardEvent::Ready { .. }), _) => Some(Stale::Superseded),
            (_, Some(false)) => Some(Stale::Rejected),
            _ => return None,
        };
        self.pending = false;
        stale
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use bytes::Bytes;
    use noctis_ports::BoxFuture;

    use super::*;

    /// How the stub answers `get`.
    enum Get {
        Value(Option<Bytes>),
        Fail,
        Hang,
    }

    /// A `SessionStore` recording its ops.
    struct Stub {
        get: Get,
        fail_delete: bool,
        ops: Mutex<Vec<String>>,
    }

    impl Stub {
        fn new(get: Get) -> Self {
            Self {
                get,
                fail_delete: false,
                ops: Mutex::new(Vec::new()),
            }
        }

        fn ops(&self) -> Vec<String> {
            self.ops.lock().unwrap().clone()
        }
    }

    impl SessionStore for Stub {
        fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            self.ops.lock().unwrap().push(format!("get {key}"));
            Box::pin(async move {
                match &self.get {
                    Get::Value(value) => Ok(value.clone()),
                    Get::Fail => Err(PortError::backend("down")),
                    Get::Hang => std::future::pending().await,
                }
            })
        }

        fn put<'a>(
            &'a self,
            key: &'a str,
            _: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<(), PortError>> {
            self.ops.lock().unwrap().push(format!("put {key} {ttl:?}"));
            Box::pin(async { Ok(()) })
        }

        fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
            self.ops.lock().unwrap().push(format!("delete {key}"));
            let fail = self.fail_delete;
            Box::pin(async move {
                if fail {
                    Err(PortError::backend("down"))
                } else {
                    Ok(())
                }
            })
        }
    }

    fn encoded(session_id: &str, sequence: u64, resume_url: &str) -> Bytes {
        encode_value(
            SCHEMA,
            &StoredSession {
                session_id: session_id.to_owned(),
                sequence,
                resume_url: resume_url.to_owned(),
            },
        )
        .unwrap()
    }

    async fn load_bytes(raw: Bytes) -> (Option<(Session, String)>, Vec<String>) {
        let stub = Stub::new(Get::Value(Some(raw)));
        let loaded = load(&stub, "shard:0:1").await;
        (loaded, stub.ops())
    }

    /// The stored value is accepted verbatim.
    async fn accepted(session_id: &str, sequence: u64, resume_url: &str) {
        let (loaded, ops) = load_bytes(encoded(session_id, sequence, resume_url)).await;
        let (session, url) = loaded.unwrap_or_else(|| panic!("{session_id:?} {resume_url:?}"));
        assert_eq!(session.id(), session_id);
        assert_eq!(session.sequence(), sequence);
        assert_eq!(url, resume_url);
        assert_eq!(ops, ["get shard:0:1"]);
    }

    /// The stored value is deleted and the shard identifies.
    async fn rejected(raw: Bytes) {
        let (loaded, ops) = load_bytes(raw.clone()).await;
        assert!(loaded.is_none(), "{raw:?}");
        assert_eq!(ops, ["get shard:0:1", "delete shard:0:1"], "{raw:?}");
    }

    #[tokio::test]
    async fn l1_miss_identifies_without_delete() {
        let stub = Stub::new(Get::Value(None));
        assert!(load(&stub, "shard:0:1").await.is_none());
        assert_eq!(stub.ops(), ["get shard:0:1"]);
    }

    #[tokio::test]
    async fn l2_valid_hit() {
        accepted("s1", 7, "ws://127.0.0.1:9").await;
    }

    #[test]
    fn l3_key() {
        assert_eq!(key(ShardId::new(0, 1)), "shard:0:1");
        assert_eq!(key(ShardId::new(3, 4)), "shard:3:4");
        assert_eq!(
            key(ShardId::new(4_294_967_294, 4_294_967_295)),
            "shard:4294967294:4294967295"
        );
    }

    #[tokio::test]
    async fn l4_unreadable_is_deleted() {
        let good = encoded("s1", 7, "ws://127.0.0.1:9");
        let mut trailing = good.to_vec();
        trailing.push(0);
        let mut cases = vec![
            Bytes::new(),
            Bytes::from_static(&[1, 0xff, 0xff, 0xff]),
            Bytes::from(trailing),
        ];
        for schema in [0_u8, 2, 255] {
            let mut other = good.to_vec();
            other[0] = schema;
            cases.push(Bytes::from(other));
        }
        for raw in cases {
            rejected(raw).await;
        }
    }

    #[tokio::test]
    async fn l4_delete_failure_only_warns() {
        let mut stub = Stub::new(Get::Value(Some(Bytes::new())));
        stub.fail_delete = true;
        assert!(load(&stub, "shard:0:1").await.is_none());
        assert_eq!(stub.ops(), ["get shard:0:1", "delete shard:0:1"]);
    }

    #[tokio::test]
    async fn l5_session_id_bounds() {
        let url = "ws://127.0.0.1:9";
        rejected(encoded("", 1, url)).await;
        rejected(encoded(&"a".repeat(257), 1, url)).await;
        accepted("a", 1, url).await;
        accepted(&"a".repeat(256), 1, url).await;
        assert_eq!("セッション".len(), 15);
        accepted("セッション", 1, url).await;
    }

    #[tokio::test]
    async fn l6_resume_url_accepted() {
        let long = format!("wss://h/{}", "a".repeat(2048 - 8));
        assert_eq!(long.len(), 2048);
        for url in [
            "wss://gateway-us-east1-b.discord.gg",
            "WSS://X.Y",
            "ws://127.0.0.1:3000",
            &long,
        ] {
            accepted("s1", 1, url).await;
        }
    }

    #[tokio::test]
    async fn l7_resume_url_rejected() {
        let long = format!("wss://h/{}", "a".repeat(2049 - 8));
        for url in [
            "",
            "http://x",
            "gateway.discord.gg",
            "ws://a b",
            "ws:/x",
            "wss://dïscord.gg",
            &long,
        ] {
            rejected(encoded("s1", 1, url)).await;
        }
    }

    #[tokio::test]
    async fn l8_sequence_bounds() {
        accepted("s1", 0, "ws://127.0.0.1:9").await;
        accepted("s1", u64::MAX, "ws://127.0.0.1:9").await;
    }

    #[tokio::test]
    async fn l9_store_error_identifies_without_delete() {
        let stub = Stub::new(Get::Fail);
        assert!(load(&stub, "shard:0:1").await.is_none());
        assert_eq!(stub.ops(), ["get shard:0:1"]);
    }

    #[tokio::test(start_paused = true)]
    async fn l10_hanging_store_times_out_after_five_seconds() {
        let stub = Stub::new(Get::Hang);
        let started = Instant::now();
        assert!(load(&stub, "shard:0:1").await.is_none());
        assert_eq!(started.elapsed(), STORE_TIMEOUT);
        assert_eq!(stub.ops(), ["get shard:0:1"]);
    }

    #[tokio::test]
    async fn save_round_trips_without_ttl() {
        let stub = Stub::new(Get::Value(None));
        let session = Session::new(3, "s1".to_owned());
        save(
            &stub,
            "shard:0:1",
            &session,
            "ws://x:1",
            Instant::now() + STORE_TIMEOUT,
        )
        .await;
        assert_eq!(stub.ops(), ["put shard:0:1 None"]);
        let (session, url) = parse(&encoded("s1", 3, "ws://x:1")).unwrap();
        assert_eq!(
            (session.id(), session.sequence(), &*url),
            ("s1", 3, "ws://x:1")
        );
    }

    #[test]
    fn r1_resumed_keeps_the_entry() {
        let mut state = ResumeState::new(true);
        let resumed = ShardEvent::Resumed { shard: 0 };
        assert_eq!(state.on_frame(Some(&resumed), None), None);
        // R5: nothing after a resume.
        let ready = ShardEvent::Ready { shard: 0 };
        assert_eq!(state.on_frame(Some(&ready), None), None);
        assert_eq!(state.on_frame(None, Some(false)), None);
    }

    #[test]
    fn r2_rejected_once() {
        let mut state = ResumeState::new(true);
        assert_eq!(state.on_frame(None, Some(false)), Some(Stale::Rejected));
        // R5: neither a second op 9 nor the READY after identify.
        assert_eq!(state.on_frame(None, Some(false)), None);
        let ready = ShardEvent::Ready { shard: 0 };
        assert_eq!(state.on_frame(Some(&ready), None), None);
    }

    #[test]
    fn r3_resumable_op9_keeps_pending() {
        let mut state = ResumeState::new(true);
        assert_eq!(state.on_frame(None, Some(true)), None);
        assert_eq!(state.on_frame(None, None), None);
        assert_eq!(state.on_frame(None, Some(false)), Some(Stale::Rejected));
    }

    #[test]
    fn r4_ready_while_pending_supersedes_once() {
        let mut state = ResumeState::new(true);
        let ready = ShardEvent::Ready { shard: 0 };
        assert_eq!(state.on_frame(Some(&ready), None), Some(Stale::Superseded));
        assert_eq!(state.on_frame(Some(&ready), None), None);
        assert_eq!(state.on_frame(None, Some(false)), None);
    }

    #[test]
    fn not_pending_never_deletes() {
        let mut state = ResumeState::new(false);
        let ready = ShardEvent::Ready { shard: 0 };
        assert_eq!(state.on_frame(Some(&ready), None), None);
        assert_eq!(state.on_frame(None, Some(false)), None);
    }
}
