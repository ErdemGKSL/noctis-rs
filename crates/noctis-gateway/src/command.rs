//! Gateway commands sent to the owning shard (FR-53, AD-21): presence
//! updates (op 3) and Request Guild Members (op 8), behind a per-shard send
//! budget (GW-04) and correlated by a Noctis nonce (GW-07).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use twilight_gateway::{Event, Intents};
use twilight_model::gateway::OpCode;
use twilight_model::gateway::payload::incoming::MemberChunk;
use twilight_model::gateway::payload::incoming::rate_limited::RateLimitMetadata;
use twilight_model::gateway::payload::outgoing::{RequestGuildMembers, UpdatePresence};
use twilight_model::gateway::presence::Presence as MemberPresence;
use twilight_model::guild::Member;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};

use crate::Presence;
use crate::gateway::CloseMode;

/// GW-04: user commands admitted per shard per [`SEND_WINDOW`]: Discord's
/// 120 minus twilight's maximum heartbeat reservation of 10.
pub(crate) const SEND_BUDGET: usize = 110;
/// GW-04: the sliding window of [`SEND_BUDGET`].
pub(crate) const SEND_WINDOW: Duration = Duration::from_mins(1);
/// How long a member request waits for its first or next answer.
pub(crate) const MEMBER_CHUNK_TIMEOUT: Duration = Duration::from_secs(10);
/// GW-07: user ids per Request Guild Members.
const MAX_USER_IDS: usize = 100;
/// The largest `limit` of a prefix query.
const MAX_QUERY_LIMIT: u16 = 100;

/// Why a gateway command was refused or failed. Nothing is sent after an
/// error; messages never carry the token, the nonce or a payload.
///
/// ```
/// use noctis_gateway::GatewayError;
///
/// assert!(GatewayError::NoUserIds.to_string().starts_with("[GW-07]"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    /// The App is not running (before `run`, after it returned, during its
    /// shutdown, or in a detached test context).
    #[error("the gateway is not running")]
    NotRunning,
    /// The shard id is outside this process's Shard Range.
    #[error("shard {shard} is outside this process's Shard Range {range:?}")]
    ShardNotInRange {
        /// The requested shard.
        shard: u32,
        /// This process's Shard Range.
        range: Range<u32>,
    },
    /// The guild's owning shard is outside this process's Shard Range.
    #[error("guild {guild} belongs to shard {shard}, outside this process's Shard Range {range:?}")]
    GuildNotInRange {
        /// The guild.
        guild: Id<GuildMarker>,
        /// Its owning shard, `(guild_id >> 22) % total`.
        shard: u32,
        /// This process's Shard Range.
        range: Range<u32>,
    },
    /// The shard's task has ended (a fatal close code).
    #[error("shard {shard} has stopped")]
    ShardClosed {
        /// The shard.
        shard: u32,
    },
    /// The shard already sent 110 commands in the last 60 s (GW-04).
    #[error(
        "[GW-04] shard {shard} reached its gateway send budget (110 per 60 s); retry in {retry_after:?}"
    )]
    SendLimit {
        /// The shard over budget.
        shard: u32,
        /// When the oldest counted send leaves the window.
        retry_after: Duration,
    },
    /// Discord rate limited the Request Guild Members (`RATE_LIMITED`,
    /// GW-07).
    #[error(
        "[GW-07] Request Guild Members for guild {guild} was rate limited; retry in {retry_after:?}"
    )]
    RateLimited {
        /// The guild.
        guild: Id<GuildMarker>,
        /// Discord's `retry_after`.
        retry_after: Duration,
    },
    /// [`MemberRequest::user_list`] got no user id.
    #[error("[GW-07] a member request needs at least one user id")]
    NoUserIds,
    /// [`MemberRequest::user_list`] got more than 100 user ids.
    #[error("[GW-07] a member request takes at most 100 user ids, got {len}")]
    TooManyUserIds {
        /// How many ids were given.
        len: usize,
    },
    /// [`MemberRequest::prefix`] got an empty query.
    #[error(
        "[GW-07] a member prefix query must not be empty; use `MemberRequest::all` for every member"
    )]
    EmptyQuery,
    /// [`MemberRequest::prefix`] got a limit outside 1..=100.
    #[error("[GW-07] a member prefix query's limit must be within 1..=100, got {limit}")]
    InvalidLimit {
        /// The limit given.
        limit: u16,
    },
    /// The request needs an intent the App does not identify with:
    /// `"GUILD_MEMBERS"` for [`MemberRequest::all`], `"GUILD_PRESENCES"` for
    /// [`MemberRequest::presences`].
    #[error("[GW-07] this member request needs the {intent} intent")]
    MissingIntent {
        /// The missing intent's name.
        intent: &'static str,
    },
    /// No answer arrived within 10 s of the request or of its last chunk.
    #[error("no answer to the member request for guild {guild} within 10 s")]
    Timeout {
        /// The guild.
        guild: Id<GuildMarker>,
    },
}

/// What a Request Guild Members asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// These users (1..=100).
    Users(Vec<Id<UserMarker>>),
    /// Members whose name starts with `query` (`""` with limit 0: all).
    Query { query: String, limit: u16 },
}

/// A Request Guild Members (op 8) for one guild (GW-07).
///
/// Every constructor yields a valid request: [`users`](Self::users) takes
/// 1..=100 ids checked at compile time, [`user_list`](Self::user_list) and
/// [`prefix`](Self::prefix) check at run time. Send it with
/// [`GatewayHandle::request_guild_members`].
///
/// ```
/// use noctis_gateway::MemberRequest;
/// use twilight_model::id::Id;
///
/// let guild = Id::new(1);
/// let one = MemberRequest::users(guild, [Id::new(2)]);
/// let named = MemberRequest::prefix(guild, "ab", 10)?.presences(true);
/// let every = MemberRequest::all(guild);
/// assert_eq!(every.guild(), guild);
/// # let _ = (one, named);
/// # Ok::<(), noctis_gateway::GatewayError>(())
/// ```
///
/// More than 100 (or zero) ids in an array do not compile:
///
/// ```compile_fail
/// use noctis_gateway::MemberRequest;
/// use twilight_model::id::Id;
///
/// let _ = MemberRequest::users(Id::new(1), [Id::new(2); 101]); // [GW-07]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRequest {
    guild: Id<GuildMarker>,
    target: Target,
    presences: bool,
}

impl MemberRequest {
    /// These `N` users of `guild`; `N` must be within 1..=100 (a compile
    /// error carrying `[GW-07]` otherwise).
    pub fn users<const N: usize>(guild: Id<GuildMarker>, ids: [Id<UserMarker>; N]) -> Self
    where
        UserIds<N>: UserIdCount,
    {
        Self {
            guild,
            target: Target::Users(ids.into()),
            presences: false,
        }
    }

    /// The users of `guild` in `ids`, kept as given (duplicates too).
    ///
    /// # Errors
    ///
    /// [`GatewayError::NoUserIds`] for no id,
    /// [`GatewayError::TooManyUserIds`] for more than 100.
    ///
    /// ```
    /// use noctis_gateway::{GatewayError, MemberRequest};
    /// use twilight_model::id::Id;
    ///
    /// let none = MemberRequest::user_list(Id::new(1), []);
    /// assert_eq!(none, Err(GatewayError::NoUserIds));
    /// ```
    pub fn user_list(
        guild: Id<GuildMarker>,
        ids: impl IntoIterator<Item = Id<UserMarker>>,
    ) -> Result<Self, GatewayError> {
        let ids: Vec<_> = ids.into_iter().collect();
        match ids.len() {
            0 => Err(GatewayError::NoUserIds),
            len if len > MAX_USER_IDS => Err(GatewayError::TooManyUserIds { len }),
            _ => Ok(Self {
                guild,
                target: Target::Users(ids),
                presences: false,
            }),
        }
    }

    /// Up to `limit` members of `guild` whose username or nickname starts
    /// with `query`.
    ///
    /// # Errors
    ///
    /// [`GatewayError::EmptyQuery`] for an empty `query` (use
    /// [`all`](Self::all)), [`GatewayError::InvalidLimit`] for a `limit`
    /// outside 1..=100.
    pub fn prefix(
        guild: Id<GuildMarker>,
        query: impl Into<String>,
        limit: u16,
    ) -> Result<Self, GatewayError> {
        let query = query.into();
        if query.is_empty() {
            return Err(GatewayError::EmptyQuery);
        }
        if !(1..=MAX_QUERY_LIMIT).contains(&limit) {
            return Err(GatewayError::InvalidLimit { limit });
        }
        Ok(Self {
            guild,
            target: Target::Query { query, limit },
            presences: false,
        })
    }

    /// Every member of `guild` (`query: ""`, `limit: 0`); needs the
    /// `GUILD_MEMBERS` intent.
    pub fn all(guild: Id<GuildMarker>) -> Self {
        Self {
            guild,
            target: Target::Query {
                query: String::new(),
                limit: 0,
            },
            presences: false,
        }
    }

    /// Whether the answer carries the members' presences (default `false`);
    /// `true` needs the `GUILD_PRESENCES` intent.
    #[must_use]
    pub const fn presences(mut self, presences: bool) -> Self {
        self.presences = presences;
        self
    }

    /// The guild this request is for.
    pub const fn guild(&self) -> Id<GuildMarker> {
        self.guild
    }

    /// The intent this request needs that `intents` lacks, if any (D7).
    fn missing_intent(&self, intents: Intents) -> Option<&'static str> {
        let all = matches!(&self.target, Target::Query { query, .. } if query.is_empty());
        if all && !intents.contains(Intents::GUILD_MEMBERS) {
            Some("GUILD_MEMBERS")
        } else if self.presences && !intents.contains(Intents::GUILD_PRESENCES) {
            Some("GUILD_PRESENCES")
        } else {
            None
        }
    }

    /// The op 8 frame carrying `nonce`.
    fn to_json(&self, nonce: String) -> Result<String, GatewayError> {
        let builder = RequestGuildMembers::builder(self.guild).nonce(nonce);
        let builder = if self.presences {
            builder.presences(true)
        } else {
            builder
        };
        let command = match &self.target {
            Target::Users(ids) => builder
                .user_ids(ids.clone())
                .map_err(|_| GatewayError::TooManyUserIds { len: ids.len() })?,
            Target::Query { query, limit } => builder.query(query.clone(), Some(u64::from(*limit))),
        };
        Ok(encode(&command))
    }
}

/// Serializes a twilight gateway command.
#[expect(
    clippy::expect_used,
    reason = "twilight's command types are plain data with string keys; serializing them cannot fail"
)]
fn encode(command: &impl Serialize) -> String {
    serde_json::to_string(command).expect("a gateway command serializes")
}

/// A user id count, for [`UserIdCount`].
#[derive(Debug)]
pub struct UserIds<const N: usize>;

/// Implemented exactly for `UserIds<1>` ..= `UserIds<100>`.
#[diagnostic::on_unimplemented(
    message = "[GW-07] `MemberRequest::users` takes 1..=100 user ids (one Request Guild Members)",
    label = "outside 1..=100 ids"
)]
pub trait UserIdCount: crate::sealed::Sealed {}

macro_rules! user_id_counts {
    ($($n:literal)*) => {
        $(
            impl crate::sealed::Sealed for UserIds<$n> {}
            impl UserIdCount for UserIds<$n> {}
        )*
    };
}

user_id_counts!(
    1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34
    35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65
    66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96
    97 98 99 100
);

/// The answer to a [`MemberRequest`]: every chunk's members, presences and
/// unknown ids, in chunk-index order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GuildMembers {
    /// The members found.
    pub members: Vec<Member>,
    /// Their presences (only with [`MemberRequest::presences`]).
    pub presences: Vec<MemberPresence>,
    /// Requested user ids that are not members.
    pub not_found: Vec<Id<UserMarker>>,
}

/// What a shard task forwards to a waiting member request.
#[derive(Debug, Clone)]
pub(crate) enum Answer {
    Chunk(MemberChunk),
    RateLimited { retry_after: Duration },
}

impl Answer {
    /// The answer an intercepted event carries, if any, cloned out of it.
    pub(crate) fn of(event: &Event) -> Option<Self> {
        match event {
            Event::MemberChunk(chunk) => Some(Self::Chunk(chunk.clone())),
            Event::RateLimited(limited) => Some(Self::RateLimited {
                retry_after: retry_after(limited.retry_after),
            }),
            _ => None,
        }
    }

    /// The answer an intercepted event carries, if any, moved out of it.
    pub(crate) fn from_event(event: Event) -> Option<Self> {
        match event {
            Event::MemberChunk(chunk) => Some(Self::Chunk(chunk)),
            other => Self::of(&other),
        }
    }
}

/// Discord's `retry_after` seconds; negative or NaN is zero.
fn retry_after(seconds: f32) -> Duration {
    if seconds.is_nan() || seconds <= 0.0 {
        return Duration::ZERO;
    }
    Duration::try_from_secs_f32(seconds).unwrap_or(Duration::MAX)
}

/// The nonce an intercepted event answers, if any.
pub(crate) fn answer_nonce(event: &Event) -> Option<&str> {
    match event {
        Event::MemberChunk(chunk) => chunk.nonce.as_deref(),
        Event::RateLimited(limited) => match &limited.meta {
            RateLimitMetadata::RequestGuildMembers { nonce, .. } => nonce.as_deref(),
            _ => None,
        },
        _ => None,
    }
}

/// The member requests awaiting answers, by nonce; shared by every shard
/// task. The lock is never held across an `.await`.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    waiters: Mutex<HashMap<String, mpsc::UnboundedSender<Answer>>>,
}

impl Pending {
    fn waiters(&self) -> MutexGuard<'_, HashMap<String, mpsc::UnboundedSender<Answer>>> {
        self.waiters.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The sender of the request waiting on `nonce`, if one waits; the lock
    /// is released before the caller builds or sends its answer.
    pub(crate) fn waiter(&self, nonce: &str) -> Option<mpsc::UnboundedSender<Answer>> {
        self.waiters().get(nonce).cloned()
    }

    /// Forwards `event`'s answer to the request waiting on `nonce`; `false`
    /// when none waits (a foreign nonce).
    #[cfg(test)]
    fn deliver(&self, nonce: &str, event: &Event) -> bool {
        self.waiter(nonce).is_some_and(|waiter| {
            Answer::of(event).is_some_and(|answer| waiter.send(answer).is_ok())
        })
    }

    /// Registers a waiter for `nonce`.
    pub(crate) fn register(
        self: &Arc<Self>,
        nonce: String,
    ) -> (Registration, mpsc::UnboundedReceiver<Answer>) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.waiters().insert(nonce.clone(), tx);
        (
            Registration {
                pending: Arc::clone(self),
                nonce,
            },
            rx,
        )
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.waiters().len()
    }
}

/// A pending entry; dropping it removes the entry, so a dropped request
/// future leaves nothing behind.
pub(crate) struct Registration {
    pending: Arc<Pending>,
    nonce: String,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.pending.waiters().remove(&self.nonce);
    }
}

/// Collects the chunks of one member request.
#[derive(Debug)]
struct Collector {
    guild: Id<GuildMarker>,
    count: Option<u32>,
    chunks: BTreeMap<u32, MemberChunk>,
}

impl Collector {
    const fn new(guild: Id<GuildMarker>) -> Self {
        Self {
            guild,
            count: None,
            chunks: BTreeMap::new(),
        }
    }

    /// Takes one answer; `Some` once the request is complete or failed. A
    /// repeated or out-of-range chunk index is ignored.
    fn on_answer(&mut self, answer: Answer) -> Option<Result<GuildMembers, GatewayError>> {
        let chunk = match answer {
            Answer::RateLimited { retry_after } => {
                return Some(Err(GatewayError::RateLimited {
                    guild: self.guild,
                    retry_after,
                }));
            }
            Answer::Chunk(chunk) => chunk,
        };
        let count = *self.count.get_or_insert(chunk.chunk_count);
        if chunk.chunk_index >= count {
            return None;
        }
        self.chunks.entry(chunk.chunk_index).or_insert(chunk);
        if u32::try_from(self.chunks.len()).ok() != Some(count) {
            return None;
        }
        let mut members = GuildMembers::default();
        for chunk in std::mem::take(&mut self.chunks).into_values() {
            members.members.extend(chunk.members);
            members.presences.extend(chunk.presences);
            members.not_found.extend(chunk.not_found);
        }
        Some(Ok(members))
    }
}

/// The GW-04 sliding window of one shard.
#[derive(Debug, Default)]
struct SendWindow {
    sent: VecDeque<Instant>,
}

impl SendWindow {
    /// `Ok` when one more send fits at `now`; else how long until it does.
    fn admit(&mut self, now: Instant) -> Result<(), Duration> {
        while self.sent.front().is_some_and(|&at| at + SEND_WINDOW <= now) {
            self.sent.pop_front();
        }
        match self.sent.front() {
            Some(&oldest) if self.sent.len() >= SEND_BUDGET => Err(oldest + SEND_WINDOW - now),
            _ => Ok(()),
        }
    }

    fn record(&mut self, now: Instant) {
        self.sent.push_back(now);
    }
}

/// The latest presence set at run time on one shard (its op 3 frame), which
/// the shard task sends again after a new session's READY.
pub(crate) type PresenceSlot = Arc<Mutex<Option<String>>>;

/// Reads a presence slot.
pub(crate) fn last_presence(slot: &PresenceSlot) -> Option<String> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The command side of one shard.
#[derive(Debug)]
struct ShardLine {
    tx: mpsc::UnboundedSender<String>,
    window: Mutex<SendWindow>,
    presence: PresenceSlot,
}

/// The shard task's side of its command line.
#[derive(Debug)]
pub(crate) struct ShardInbox {
    /// Frames to send through `Shard::send`.
    pub(crate) commands: mpsc::UnboundedReceiver<String>,
    /// The latest runtime presence.
    pub(crate) presence: PresenceSlot,
}

/// Everything the command handles need of a running [`Gateway`](crate::Gateway).
#[derive(Debug)]
pub(crate) struct Commands {
    shards: Range<u32>,
    total: u32,
    intents: Intents,
    /// One per shard of `shards`, in order.
    lines: Vec<ShardLine>,
    pending: Arc<Pending>,
    seed: u32,
    counter: AtomicU64,
    /// The Gateway's close request: once set, every command is
    /// [`GatewayError::NotRunning`].
    close: watch::Receiver<Option<CloseMode>>,
}

impl Commands {
    /// The commands of shards `shards` of `total`, with one inbox per shard
    /// (in order) for the shard tasks.
    pub(crate) fn new(
        shards: Range<u32>,
        total: u32,
        intents: Intents,
        close: watch::Receiver<Option<CloseMode>>,
    ) -> (Self, Vec<ShardInbox>) {
        let (lines, inboxes) = shards
            .clone()
            .map(|_| {
                let (tx, commands) = mpsc::unbounded_channel();
                let presence = PresenceSlot::default();
                (
                    ShardLine {
                        tx,
                        window: Mutex::default(),
                        presence: Arc::clone(&presence),
                    },
                    ShardInbox { commands, presence },
                )
            })
            .unzip();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u32::try_from(since.as_nanos() & u128::from(u32::MAX)).ok())
            .unwrap_or_default();
        let commands = Self {
            shards,
            total,
            intents,
            lines,
            pending: Arc::default(),
            seed,
            counter: AtomicU64::new(0),
            close,
        };
        (commands, inboxes)
    }

    /// The pending member requests, for the shard tasks.
    pub(crate) fn pending(&self) -> &Arc<Pending> {
        &self.pending
    }

    fn running(&self) -> Result<(), GatewayError> {
        if self.close.borrow().is_some() {
            Err(GatewayError::NotRunning)
        } else {
            Ok(())
        }
    }

    fn line(&self, shard: u32) -> Result<&ShardLine, GatewayError> {
        shard
            .checked_sub(self.shards.start)
            .filter(|_| self.shards.contains(&shard))
            .and_then(|index| self.lines.get(usize::try_from(index).ok()?))
            .ok_or_else(|| GatewayError::ShardNotInRange {
                shard,
                range: self.shards.clone(),
            })
    }

    fn shard_for_guild(&self, guild: Id<GuildMarker>) -> Result<u32, GatewayError> {
        let owner = (guild.get() >> 22) % u64::from(self.total);
        // `owner < total`, which is a `u32`.
        let shard = u32::try_from(owner).unwrap_or(u32::MAX);
        if self.shards.contains(&shard) {
            Ok(shard)
        } else {
            Err(GatewayError::GuildNotInRange {
                guild,
                shard,
                range: self.shards.clone(),
            })
        }
    }

    /// Sends `presence` to `shards` (within range), all or none.
    fn set_presence(&self, shards: Range<u32>, presence: &Presence) -> Result<(), GatewayError> {
        self.running()?;
        let lines = shards
            .map(|shard| self.line(shard).map(|line| (shard, line)))
            .collect::<Result<Vec<_>, _>>()?;
        let json = encode(&UpdatePresence {
            d: presence.to_payload(),
            op: OpCode::PresenceUpdate,
        });
        let now = Instant::now();
        // Every window is locked in shard order (the only multi-lock), so
        // the budget check and the sends are all or none.
        let mut windows: Vec<_> = lines
            .iter()
            .map(|(_, line)| line.window.lock().unwrap_or_else(PoisonError::into_inner))
            .collect();
        for ((shard, line), window) in lines.iter().zip(&mut windows) {
            if line.tx.is_closed() {
                return Err(GatewayError::ShardClosed { shard: *shard });
            }
            window
                .admit(now)
                .map_err(|retry_after| GatewayError::SendLimit {
                    shard: *shard,
                    retry_after,
                })?;
        }
        for ((shard, line), window) in lines.iter().zip(&mut windows) {
            *line.presence.lock().unwrap_or_else(PoisonError::into_inner) = Some(json.clone());
            window.record(now);
            line.tx
                .send(json.clone())
                .map_err(|_| GatewayError::ShardClosed { shard: *shard })?;
        }
        Ok(())
    }

    /// Sends `request` to its owning shard and registers its waiter.
    fn send_member_request(
        &self,
        request: &MemberRequest,
    ) -> Result<(Registration, mpsc::UnboundedReceiver<Answer>), GatewayError> {
        self.running()?;
        if let Some(intent) = request.missing_intent(self.intents) {
            return Err(GatewayError::MissingIntent { intent });
        }
        let shard = self.shard_for_guild(request.guild)?;
        let line = self.line(shard)?;
        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let nonce = format!("{:08x}{counter:x}", self.seed);
        let json = request.to_json(nonce.clone())?;
        let now = Instant::now();
        let mut window = line.window.lock().unwrap_or_else(PoisonError::into_inner);
        if line.tx.is_closed() {
            return Err(GatewayError::ShardClosed { shard });
        }
        window
            .admit(now)
            .map_err(|retry_after| GatewayError::SendLimit { shard, retry_after })?;
        // Registered before the frame leaves, so no answer can beat it.
        let (registration, answers) = self.pending.register(nonce);
        window.record(now);
        line.tx
            .send(json)
            .map_err(|_| GatewayError::ShardClosed { shard })?;
        Ok((registration, answers))
    }
}

/// Sends gateway commands to the shard that owns them: presence updates
/// ([`presence`](Self::presence)) and Request Guild Members
/// ([`request_guild_members`](Self::request_guild_members)).
///
/// Cheap to clone; every clone follows the same running gateway. Core's
/// handle (`App::gateway`, `ctx.gateway()`) is live while the App runs;
/// otherwise every call is [`GatewayError::NotRunning`].
///
/// Each shard admits at most 110 commands per 60 s (GW-04): one more is
/// [`GatewayError::SendLimit`] and nothing is queued. Commands for a guild
/// or shard outside this process's Shard Range are
/// [`GatewayError::GuildNotInRange`] / [`GatewayError::ShardNotInRange`].
///
/// ```
/// use noctis_gateway::{GatewayError, GatewayHandle, Presence};
///
/// let handle = GatewayHandle::detached();
/// assert_eq!(handle.presence().set(Presence::dnd()), Err(GatewayError::NotRunning));
/// ```
#[derive(Clone, Default)]
pub struct GatewayHandle {
    slot: Arc<RwLock<Option<Arc<Commands>>>>,
}

impl GatewayHandle {
    /// A handle attached to no gateway: every call is
    /// [`GatewayError::NotRunning`].
    pub fn detached() -> Self {
        Self::default()
    }

    /// A handle on `commands`.
    pub(crate) fn live(commands: Arc<Commands>) -> Self {
        Self {
            slot: Arc::new(RwLock::new(Some(commands))),
        }
    }

    /// Points this handle and its clones at `live`'s gateway (core, at
    /// startup).
    #[doc(hidden)]
    pub fn attach(&self, live: &Self) {
        let commands = live.commands();
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = commands;
    }

    /// Detaches this handle and its clones (core, after shutdown).
    #[doc(hidden)]
    pub fn detach(&self) {
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = None;
    }

    fn commands(&self) -> Option<Arc<Commands>> {
        self.slot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn running(&self) -> Result<Arc<Commands>, GatewayError> {
        self.commands().ok_or(GatewayError::NotRunning)
    }

    /// The shard that owns `guild`: `(guild_id >> 22) % total`.
    ///
    /// # Errors
    ///
    /// [`GatewayError::NotRunning`], or [`GatewayError::GuildNotInRange`]
    /// when that shard is outside this process's Shard Range.
    pub fn shard_for_guild(&self, guild: Id<GuildMarker>) -> Result<u32, GatewayError> {
        self.running()?.shard_for_guild(guild)
    }

    /// The presence handle of this gateway.
    pub fn presence(&self) -> PresenceHandle {
        PresenceHandle {
            gateway: self.clone(),
        }
    }

    /// Sends `request` (op 8) to its guild's owning shard and collects the
    /// answer (`GUILD_MEMBERS_CHUNK` events, matched by a nonce Noctis sets).
    ///
    /// Cancel-safe: dropping the future forgets the request, and later
    /// chunks for it are treated as foreign.
    ///
    /// # Errors
    ///
    /// Checked in order before anything is sent:
    /// [`GatewayError::NotRunning`], [`GatewayError::MissingIntent`],
    /// [`GatewayError::GuildNotInRange`], [`GatewayError::ShardClosed`],
    /// [`GatewayError::SendLimit`]. Then [`GatewayError::RateLimited`] when
    /// Discord answers `RATE_LIMITED` (GW-07), or [`GatewayError::Timeout`]
    /// when no chunk arrives for 10 s.
    pub fn request_guild_members(
        &self,
        request: MemberRequest,
    ) -> impl Future<Output = Result<GuildMembers, GatewayError>> + Send + 'static {
        let commands = self.running();
        async move {
            let (registration, mut answers) = commands?.send_member_request(&request)?;
            let mut collector = Collector::new(request.guild);
            let result = loop {
                match tokio::time::timeout(MEMBER_CHUNK_TIMEOUT, answers.recv()).await {
                    Ok(Some(answer)) => {
                        if let Some(result) = collector.on_answer(answer) {
                            break result;
                        }
                    }
                    Ok(None) | Err(_) => {
                        break Err(GatewayError::Timeout {
                            guild: request.guild,
                        });
                    }
                }
            };
            drop(registration);
            result
        }
    }
}

impl fmt::Debug for GatewayHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let commands = self.commands();
        f.debug_struct("GatewayHandle")
            .field("running", &commands.is_some())
            .field("shards", &commands.as_ref().map(|c| c.shards.clone()))
            .finish()
    }
}

/// Sets the bot's presence at run time (op 3), on every shard of this
/// process or on one. Never blocks; each shard remembers the latest presence
/// and sends it again after a new session's READY.
///
/// ```
/// use noctis_gateway::{GatewayError, GatewayHandle, Presence};
///
/// let presence = GatewayHandle::detached().presence();
/// assert_eq!(presence.set_shard(0, Presence::idle()), Err(GatewayError::NotRunning));
/// ```
#[derive(Debug, Clone)]
pub struct PresenceHandle {
    gateway: GatewayHandle,
}

impl PresenceHandle {
    /// Sets `presence` on every shard of this process, all or none.
    ///
    /// # Errors
    ///
    /// [`GatewayError::NotRunning`]; [`GatewayError::ShardClosed`] or
    /// [`GatewayError::SendLimit`] (GW-04) for the lowest shard id that
    /// cannot send, in which case no shard sends.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a presence is a value the caller hands over"
    )]
    pub fn set(&self, presence: Presence) -> Result<(), GatewayError> {
        let commands = self.gateway.running()?;
        commands.set_presence(commands.shards.clone(), &presence)
    }

    /// Sets `presence` on shard `shard` only.
    ///
    /// # Errors
    ///
    /// [`GatewayError::NotRunning`]; [`GatewayError::ShardNotInRange`] when
    /// `shard` is outside this process's Shard Range;
    /// [`GatewayError::ShardClosed`]; [`GatewayError::SendLimit`] (GW-04).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a presence is a value the caller hands over"
    )]
    pub fn set_shard(&self, shard: u32, presence: Presence) -> Result<(), GatewayError> {
        let commands = self.gateway.running()?;
        commands.line(shard)?;
        commands.set_presence(shard..shard + 1, &presence)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use twilight_model::gateway::payload::incoming::RateLimited;

    use super::*;

    struct Fixture {
        handle: GatewayHandle,
        commands: Arc<Commands>,
        inboxes: Vec<ShardInbox>,
        close: watch::Sender<Option<CloseMode>>,
    }

    fn fixture(shards: Range<u32>, total: u32, intents: Intents) -> Fixture {
        let (close, close_rx) = watch::channel(None);
        let (commands, inboxes) = Commands::new(shards, total, intents, close_rx);
        let commands = Arc::new(commands);
        Fixture {
            handle: GatewayHandle::live(Arc::clone(&commands)),
            commands,
            inboxes,
            close,
        }
    }

    fn sent(inbox: &mut ShardInbox) -> Vec<Value> {
        std::iter::from_fn(|| inbox.commands.try_recv().ok())
            .map(|json| serde_json::from_str(&json).unwrap())
            .collect()
    }

    fn guild_on(shard: u64, total: u64) -> Id<GuildMarker> {
        // `(5 << 22) | 7` style ids: the timestamp part picks the shard.
        Id::new(((shard + total * 3) << 22) | 7)
    }

    fn chunk(guild: Id<GuildMarker>, nonce: &str, index: u32, count: u32) -> Event {
        Event::MemberChunk(MemberChunk {
            chunk_count: count,
            chunk_index: index,
            guild_id: guild,
            members: Vec::new(),
            nonce: Some(nonce.to_owned()),
            not_found: vec![Id::new(u64::from(index) + 100)],
            presences: Vec::new(),
        })
    }

    fn rate_limited(guild: Id<GuildMarker>, nonce: &str, seconds: f32) -> Event {
        Event::RateLimited(RateLimited {
            opcode: OpCode::RequestGuildMembers,
            retry_after: seconds,
            meta: RateLimitMetadata::RequestGuildMembers {
                guild_id: guild,
                nonce: Some(nonce.to_owned()),
            },
        })
    }

    /// M16: Discord's owning-shard formula.
    #[test]
    fn owning_shard() {
        let f = fixture(0..4, 4, Intents::GUILDS);
        assert_eq!(f.handle.shard_for_guild(Id::new((5 << 22) | 7)), Ok(1));
        assert_eq!(f.handle.shard_for_guild(Id::new(7)), Ok(0));
        let f = fixture(0..2, 4, Intents::GUILDS);
        assert_eq!(
            f.handle.shard_for_guild(Id::new(3 << 22)),
            Err(GatewayError::GuildNotInRange {
                guild: Id::new(3 << 22),
                shard: 3,
                range: 0..2
            })
        );
    }

    /// M2, M4: user id bounds.
    #[test]
    fn gw_07_user_id_bounds() {
        let guild = Id::new(1);
        let hundred = MemberRequest::users(guild, [Id::new(2); 100]);
        assert_eq!(hundred.guild(), guild);
        assert!(MemberRequest::user_list(guild, vec![Id::new(2); 100]).is_ok());
        assert_eq!(
            MemberRequest::user_list(guild, []),
            Err(GatewayError::NoUserIds)
        );
        assert_eq!(
            MemberRequest::user_list(guild, vec![Id::new(2); 101]),
            Err(GatewayError::TooManyUserIds { len: 101 })
        );
        let duplicates = MemberRequest::user_list(guild, [Id::new(2), Id::new(2)]).unwrap();
        assert_eq!(
            duplicates.target,
            Target::Users(vec![Id::new(2), Id::new(2)])
        );
    }

    /// M6: prefix bounds.
    #[test]
    fn gw_07_prefix_bounds() {
        let guild = Id::new(1);
        assert_eq!(
            MemberRequest::prefix(guild, "", 5),
            Err(GatewayError::EmptyQuery)
        );
        assert_eq!(
            MemberRequest::prefix(guild, "a", 0),
            Err(GatewayError::InvalidLimit { limit: 0 })
        );
        assert_eq!(
            MemberRequest::prefix(guild, "a", 101),
            Err(GatewayError::InvalidLimit { limit: 101 })
        );
        assert!(MemberRequest::prefix(guild, "ç", 100).is_ok());
    }

    /// M1, M5 wire forms (always an array of ids; the nonce is ours).
    #[test]
    fn member_request_wire_forms() {
        let guild = Id::new(9);
        let users: Value = serde_json::from_str(
            &MemberRequest::users(guild, [Id::new(2)])
                .to_json("n1".to_owned())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            users,
            json!({ "op": 8, "d": { "guild_id": "9", "user_ids": ["2"], "nonce": "n1" } })
        );
        let prefix: Value = serde_json::from_str(
            &MemberRequest::prefix(guild, "ab", 10)
                .unwrap()
                .presences(true)
                .to_json("n2".to_owned())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            prefix,
            json!({ "op": 8, "d": { "guild_id": "9", "query": "ab", "limit": 10, "nonce": "n2", "presences": true } })
        );
        let all: Value =
            serde_json::from_str(&MemberRequest::all(guild).to_json("n3".to_owned()).unwrap())
                .unwrap();
        assert_eq!(all["d"]["query"], json!(""));
        assert_eq!(all["d"]["limit"], json!(0));
    }

    /// M7, M8: intents are checked before anything is sent.
    #[tokio::test]
    async fn gw_07_missing_intents() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let guild = Id::new(1);
        assert_eq!(
            f.handle
                .request_guild_members(MemberRequest::all(guild))
                .await,
            Err(GatewayError::MissingIntent {
                intent: "GUILD_MEMBERS"
            })
        );
        let request = MemberRequest::users(guild, [Id::new(2)]).presences(true);
        assert_eq!(
            f.handle.request_guild_members(request).await,
            Err(GatewayError::MissingIntent {
                intent: "GUILD_PRESENCES"
            })
        );
        assert!(sent(&mut f.inboxes[0]).is_empty());
        assert_eq!(f.commands.pending.len(), 0);
    }

    /// M15 (unit): a guild outside the range sends nothing.
    #[tokio::test]
    async fn guild_outside_range_sends_nothing() {
        let mut f = fixture(0..2, 4, Intents::GUILDS);
        let guild = guild_on(3, 4);
        assert_eq!(
            f.handle
                .request_guild_members(MemberRequest::users(guild, [Id::new(2)]))
                .await,
            Err(GatewayError::GuildNotInRange {
                guild,
                shard: 3,
                range: 0..2
            })
        );
        assert!(f.inboxes.iter_mut().all(|inbox| sent(inbox).is_empty()));
    }

    /// Sends a request on shard 0 of `f` and returns its future and nonce.
    async fn start_request(
        f: &mut Fixture,
        guild: Id<GuildMarker>,
    ) -> (
        tokio::task::JoinHandle<Result<GuildMembers, GatewayError>>,
        String,
    ) {
        let future = f
            .handle
            .request_guild_members(MemberRequest::users(guild, [Id::new(2)]));
        let task = tokio::spawn(future);
        let frame = loop {
            if let Ok(frame) = f.inboxes[0].commands.try_recv() {
                break frame;
            }
            tokio::task::yield_now().await;
        };
        let frame: Value = serde_json::from_str(&frame).unwrap();
        let nonce = frame["d"]["nonce"].as_str().unwrap().to_owned();
        assert!(nonce.len() < 32, "{nonce}");
        (task, nonce)
    }

    /// M1, M9, M10: chunks arrive out of order and twice; the answer is in
    /// index order once all arrived.
    #[tokio::test]
    async fn chunks_collect_in_index_order() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let guild = Id::new(1);
        let (task, nonce) = start_request(&mut f, guild).await;
        let pending = Arc::clone(f.commands.pending());
        for index in [2, 0, 0] {
            assert!(pending.deliver(&nonce, &chunk(guild, &nonce, index, 3)));
        }
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        assert!(pending.deliver(&nonce, &chunk(guild, &nonce, 1, 3)));
        let members = task.await.unwrap().unwrap();
        assert_eq!(
            members.not_found,
            vec![Id::new(100), Id::new(101), Id::new(102)]
        );
        assert_eq!(pending.len(), 0);
        // M12: the nonce is now foreign.
        assert!(!pending.deliver(&nonce, &chunk(guild, &nonce, 0, 1)));
    }

    /// M11: `RATE_LIMITED` for the request's nonce fails it (GW-07).
    #[tokio::test]
    async fn gw_07_rate_limited_reaches_the_caller() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let guild = Id::new(1);
        let (task, nonce) = start_request(&mut f, guild).await;
        assert!(
            !f.commands
                .pending()
                .deliver("other", &rate_limited(guild, "other", 1.0))
        );
        assert!(
            f.commands
                .pending()
                .deliver(&nonce, &rate_limited(guild, &nonce, 2.5))
        );
        assert_eq!(
            task.await.unwrap(),
            Err(GatewayError::RateLimited {
                guild,
                retry_after: Duration::from_millis(2500)
            })
        );
        assert_eq!(f.commands.pending.len(), 0);
    }

    #[test]
    fn retry_after_clamps() {
        assert_eq!(retry_after(-1.0), Duration::ZERO);
        assert_eq!(retry_after(f32::NAN), Duration::ZERO);
        assert_eq!(retry_after(0.5), Duration::from_millis(500));
    }

    /// M13: no answer for 10 s is a timeout; the entry is gone.
    #[tokio::test(start_paused = true)]
    async fn silence_times_out() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let guild = Id::new(1);
        let (task, nonce) = start_request(&mut f, guild).await;
        tokio::time::sleep(Duration::from_secs(9)).await;
        assert!(
            f.commands
                .pending()
                .deliver(&nonce, &chunk(guild, &nonce, 0, 2))
        );
        // The idle timer restarts at each chunk.
        tokio::time::sleep(Duration::from_secs(9)).await;
        assert!(!task.is_finished());
        assert_eq!(task.await.unwrap(), Err(GatewayError::Timeout { guild }));
        assert_eq!(f.commands.pending.len(), 0);
    }

    /// M14: a dropped future removes its entry.
    #[tokio::test]
    async fn dropped_future_forgets_the_request() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let guild = Id::new(1);
        let (task, nonce) = start_request(&mut f, guild).await;
        assert_eq!(f.commands.pending.len(), 1);
        task.abort();
        let _ = task.await;
        assert_eq!(f.commands.pending.len(), 0);
        assert!(
            !f.commands
                .pending()
                .deliver(&nonce, &chunk(guild, &nonce, 0, 1))
        );
    }

    /// B1, B2: 110 sends pass, the 111th is refused and queues nothing.
    #[tokio::test(start_paused = true)]
    async fn gw_04_send_budget() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let presence = f.handle.presence();
        for _ in 0..55 {
            presence.set(Presence::online()).unwrap();
            tokio::time::advance(Duration::from_millis(100)).await;
        }
        for _ in 0..55 {
            presence.set_shard(0, Presence::idle()).unwrap();
        }
        assert_eq!(sent(&mut f.inboxes[0]).len(), 110);
        let Err(GatewayError::SendLimit { shard, retry_after }) = presence.set(Presence::dnd())
        else {
            panic!("the 111th send passed");
        };
        assert_eq!(shard, 0);
        assert!(retry_after > Duration::ZERO && retry_after <= SEND_WINDOW);
        assert_eq!(retry_after, Duration::from_millis(54_500));
        let request = f
            .handle
            .request_guild_members(MemberRequest::users(Id::new(1), [Id::new(2)]))
            .await;
        assert!(matches!(
            request,
            Err(GatewayError::SendLimit { shard: 0, .. })
        ));
        assert!(sent(&mut f.inboxes[0]).is_empty());
        assert_eq!(f.commands.pending.len(), 0);
    }

    /// B3: the window slides.
    #[tokio::test(start_paused = true)]
    async fn gw_04_window_slides() {
        let mut f = fixture(0..1, 1, Intents::GUILDS);
        let presence = f.handle.presence();
        presence.set(Presence::online()).unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 1..SEND_BUDGET {
            presence.set(Presence::online()).unwrap();
        }
        assert!(presence.set(Presence::online()).is_err());
        tokio::time::advance(Duration::from_secs(59)).await;
        presence.set(Presence::online()).unwrap();
        assert!(presence.set(Presence::online()).is_err());
        assert_eq!(sent(&mut f.inboxes[0]).len(), SEND_BUDGET + 1);
    }

    /// B4, B5: all or none across shards; budgets are per shard.
    #[tokio::test(start_paused = true)]
    async fn gw_04_all_or_none_and_independent() {
        let mut f = fixture(0..2, 2, Intents::GUILDS);
        let presence = f.handle.presence();
        for _ in 0..SEND_BUDGET {
            presence.set_shard(1, Presence::online()).unwrap();
        }
        assert!(matches!(
            presence.set(Presence::idle()),
            Err(GatewayError::SendLimit { shard: 1, .. })
        ));
        assert!(sent(&mut f.inboxes[0]).is_empty());
        assert_eq!(sent(&mut f.inboxes[1]).len(), SEND_BUDGET);
        // Shard 0's budget is untouched.
        for _ in 0..SEND_BUDGET {
            presence.set_shard(0, Presence::online()).unwrap();
        }
        assert!(presence.set_shard(0, Presence::online()).is_err());
    }

    /// P7, P8, P9: whole range, one shard, out of range.
    #[test]
    fn presence_targets() {
        let mut f = fixture(2..4, 4, Intents::GUILDS);
        let presence = f.handle.presence();
        presence.set(Presence::watching("x")).unwrap();
        for inbox in &mut f.inboxes {
            let frames = sent(inbox);
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0]["op"], json!(3));
            assert_eq!(frames[0]["d"]["activities"][0]["name"], json!("x"));
            assert!(last_presence(&inbox.presence).is_some());
        }
        presence.set_shard(3, Presence::dnd()).unwrap();
        assert!(sent(&mut f.inboxes[0]).is_empty());
        assert_eq!(sent(&mut f.inboxes[1])[0]["d"]["status"], json!("dnd"));
        for shard in [1, 4, u32::MAX] {
            assert_eq!(
                presence.set_shard(shard, Presence::dnd()),
                Err(GatewayError::ShardNotInRange { shard, range: 2..4 })
            );
        }
        assert!(f.inboxes.iter_mut().all(|inbox| sent(inbox).is_empty()));
    }

    /// P10: detached, after detach, and once a close was requested.
    #[tokio::test]
    async fn not_running() {
        let detached = GatewayHandle::detached();
        assert_eq!(
            detached.presence().set(Presence::online()),
            Err(GatewayError::NotRunning)
        );
        assert_eq!(
            detached.shard_for_guild(Id::new(1)),
            Err(GatewayError::NotRunning)
        );
        assert_eq!(
            detached
                .request_guild_members(MemberRequest::all(Id::new(1)))
                .await,
            Err(GatewayError::NotRunning)
        );

        let f = fixture(0..1, 1, Intents::GUILDS);
        let core = GatewayHandle::detached();
        let clone = core.clone();
        core.attach(&f.handle);
        assert!(clone.presence().set(Presence::online()).is_ok());
        assert!(format!("{clone:?}").contains("running: true"));
        f.close.send_replace(Some(CloseMode::Resumable));
        assert_eq!(
            clone.presence().set(Presence::online()),
            Err(GatewayError::NotRunning)
        );
        core.detach();
        assert_eq!(
            clone.shard_for_guild(Id::new(1)),
            Err(GatewayError::NotRunning)
        );
    }

    /// C1: a shard whose task ended.
    #[tokio::test]
    async fn ended_shard_is_closed() {
        let mut f = fixture(0..2, 2, Intents::GUILDS);
        drop(f.inboxes.remove(0));
        let presence = f.handle.presence();
        assert_eq!(
            presence.set_shard(0, Presence::online()),
            Err(GatewayError::ShardClosed { shard: 0 })
        );
        assert_eq!(
            presence.set(Presence::online()),
            Err(GatewayError::ShardClosed { shard: 0 })
        );
        assert!(sent(&mut f.inboxes[0]).is_empty());
        assert_eq!(
            f.handle
                .request_guild_members(MemberRequest::users(guild_on(0, 2), [Id::new(2)]))
                .await,
            Err(GatewayError::ShardClosed { shard: 0 })
        );
        assert_eq!(f.commands.pending.len(), 0);
    }

    #[test]
    fn messages_carry_rule_ids() {
        let guild = Id::new(1);
        for (error, id) in [
            (
                GatewayError::SendLimit {
                    shard: 0,
                    retry_after: Duration::ZERO,
                },
                "[GW-04]",
            ),
            (
                GatewayError::RateLimited {
                    guild,
                    retry_after: Duration::ZERO,
                },
                "[GW-07]",
            ),
            (GatewayError::EmptyQuery, "[GW-07]"),
            (GatewayError::InvalidLimit { limit: 0 }, "[GW-07]"),
            (GatewayError::TooManyUserIds { len: 101 }, "[GW-07]"),
            (
                GatewayError::MissingIntent {
                    intent: "GUILD_MEMBERS",
                },
                "[GW-07]",
            ),
        ] {
            assert!(error.to_string().starts_with(id), "{error}");
        }
    }
}
