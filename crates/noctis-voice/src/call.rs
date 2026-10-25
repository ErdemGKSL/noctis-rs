//! The call map: one Songbird `Call` per guild with a live or pending voice
//! connection (AD-30, VC-02).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use noctis_gateway::GatewayHandle;
use songbird::id::{GuildId, UserId};
use songbird::shards::Shard;
use songbird::{Call, Config};
use tokio::sync::oneshot;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::config::VoiceConfig;
use crate::shard::NoctisShard;

/// The gauge of the live and pending calls.
const CALLS_GAUGE: &str = "noctis_voice_calls";

/// One guild's call.
pub(crate) struct Entry {
    /// Unique per entry: a `CallHandle` of another generation is stale.
    pub(crate) generation: u64,
    /// The latest join on this entry; an older join's failure leaves it be.
    pub(crate) join: u64,
    /// The channel joined or moved to, mirrored from `join` and the bot's
    /// own `VOICE_STATE_UPDATE`s.
    pub(crate) channel: Option<Id<ChannelMarker>>,
    /// The Songbird call; its lock is held only for one Songbird method.
    pub(crate) call: Arc<tokio::sync::Mutex<Call>>,
    /// The call's op 4 adapter (its refusals).
    pub(crate) shard: Arc<NoctisShard>,
}

/// What a join works with, cloned out of its entry.
pub(crate) struct Joining {
    pub(crate) generation: u64,
    pub(crate) join: u64,
    pub(crate) call: Arc<tokio::sync::Mutex<Call>>,
    pub(crate) shard: Arc<NoctisShard>,
}

/// The call map.
type CallMap = HashMap<Id<GuildMarker>, Entry>; // keyed-map: one entry per guild with a live or pending voice connection (AD-30)

/// The shutdown waiters.
type Leaving = HashMap<Id<GuildMarker>, oneshot::Sender<()>>; // keyed-map: only the guilds of the running shutdown task, cleared when it ends

/// The voice state of one App, registered as App state by `VoicePlugin`.
pub(crate) struct VoiceShared {
    pub(crate) config: VoiceConfig,
    calls: Mutex<CallMap>,
    /// The next entry generation and join number.
    counter: AtomicU64,
    /// Shutdown: the guilds whose leave waits for the bot's own
    /// `VOICE_STATE_UPDATE` with no channel.
    leaving: Mutex<Leaving>,
    /// The `app` label, bound by `AppStarted` or `App::voice`.
    app: OnceLock<Arc<str>>,
}

impl VoiceShared {
    pub(crate) fn new(config: VoiceConfig) -> Self {
        Self {
            config,
            calls: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(1),
            leaving: Mutex::new(HashMap::new()),
            app: OnceLock::new(),
        }
    }

    fn calls(&self) -> MutexGuard<'_, CallMap> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn leaving(&self) -> MutexGuard<'_, Leaving> {
        self.leaving.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Binds the `app` label (the first binding wins) and publishes the
    /// gauge.
    pub(crate) fn bind_app(&self, app: &str) {
        if self.app.get().is_none() {
            let _ = self.app.set(Arc::from(app));
            let len = self.calls().len();
            self.publish(len);
        }
    }

    /// Sets `noctis_voice_calls` to `len`; nothing before the label is
    /// bound.
    fn publish(&self, len: usize) {
        if let Some(app) = self.app.get() {
            let len = u32::try_from(len).unwrap_or(u32::MAX);
            metrics::gauge!(CALLS_GAUGE, "app" => Arc::clone(app)).set(f64::from(len));
        }
    }

    /// The number of live and pending calls.
    pub(crate) fn len(&self) -> usize {
        self.calls().len()
    }

    /// The generation and channel of `guild`'s entry.
    pub(crate) fn entry(&self, guild: Id<GuildMarker>) -> Option<(u64, Option<Id<ChannelMarker>>)> {
        self.calls()
            .get(&guild)
            .map(|entry| (entry.generation, entry.channel))
    }

    /// The call of `guild`'s entry.
    pub(crate) fn call(&self, guild: Id<GuildMarker>) -> Option<Arc<tokio::sync::Mutex<Call>>> {
        self.calls()
            .get(&guild)
            .map(|entry| Arc::clone(&entry.call))
    }

    /// Gets `guild`'s entry or creates it, as a new join to `channel`.
    pub(crate) fn begin_join(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
        user: Id<UserMarker>,
        gateway: &GatewayHandle,
    ) -> Joining {
        let join = self.next();
        let mut calls = self.calls();
        if let Some(entry) = calls.get_mut(&guild) {
            entry.join = join;
            entry.channel = Some(channel);
            return Joining {
                generation: entry.generation,
                join,
                call: Arc::clone(&entry.call),
                shard: Arc::clone(&entry.shard),
            };
        }
        let shard = Arc::new(NoctisShard::new(
            gateway.clone(),
            self.config.is_self_deaf(),
        ));
        let call = Call::from_config(
            GuildId::from(guild.into_nonzero()),
            Shard::Generic(Arc::clone(&shard) as _),
            UserId::from(user.into_nonzero()),
            // Noctis's own join timeout covers the whole join.
            Config::default().gateway_timeout(None),
        );
        let entry = Entry {
            generation: self.next(),
            join,
            channel: Some(channel),
            call: Arc::new(tokio::sync::Mutex::new(call)),
            shard,
        };
        let joining = Joining {
            generation: entry.generation,
            join,
            call: Arc::clone(&entry.call),
            shard: Arc::clone(&entry.shard),
        };
        calls.insert(guild, entry);
        let len = calls.len();
        drop(calls);
        self.publish(len);
        joining
    }

    /// Removes `guild`'s entry when `should_remove` returns `true` for it.
    fn remove_where(
        &self,
        guild: Id<GuildMarker>,
        should_remove: impl FnOnce(&Entry) -> bool,
    ) -> Option<Entry> {
        let mut calls = self.calls();
        let removed = calls
            .get(&guild)
            .is_some_and(should_remove)
            .then(|| calls.remove(&guild))
            .flatten();
        let len = calls.len();
        drop(calls);
        if removed.is_some() {
            self.publish(len);
        }
        removed
    }

    /// Removes `guild`'s entry, whatever its generation.
    pub(crate) fn remove(&self, guild: Id<GuildMarker>) -> Option<Entry> {
        self.remove_where(guild, |_| true)
    }

    /// Removes `guild`'s entry if it is of `generation`.
    pub(crate) fn remove_generation(
        &self,
        guild: Id<GuildMarker>,
        generation: u64,
    ) -> Option<Entry> {
        self.remove_where(guild, |entry| entry.generation == generation)
    }

    /// Removes `guild`'s entry if `join` is still its latest join.
    pub(crate) fn remove_join(
        &self,
        guild: Id<GuildMarker>,
        generation: u64,
        join: u64,
    ) -> Option<Entry> {
        self.remove_where(guild, |entry| {
            entry.generation == generation && entry.join == join
        })
    }

    /// Records a move of `guild`'s call to `channel`; its call, if any.
    pub(crate) fn moved(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
    ) -> Option<Arc<tokio::sync::Mutex<Call>>> {
        self.calls().get_mut(&guild).map(|entry| {
            entry.channel = Some(channel);
            Arc::clone(&entry.call)
        })
    }

    /// Takes every entry (shutdown).
    pub(crate) fn take_all(&self) -> Vec<(Id<GuildMarker>, Entry)> {
        let taken: Vec<_> = self.calls().drain().collect();
        self.publish(0);
        taken
    }

    /// Waits for the bot's own `VOICE_STATE_UPDATE` without a channel in
    /// `guild`.
    pub(crate) fn await_left(&self, guild: Id<GuildMarker>) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.leaving().insert(guild, tx);
        rx
    }

    /// The bot left `guild`'s voice channel: wakes a waiting shutdown.
    pub(crate) fn left(&self, guild: Id<GuildMarker>) {
        if let Some(tx) = self.leaving().remove(&guild) {
            let _ = tx.send(());
        }
    }

    /// Forgets every shutdown waiter.
    pub(crate) fn clear_leaving(&self) {
        self.leaving().clear();
    }
}
