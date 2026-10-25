//! The call map: one Songbird `Call` per guild with a live or pending voice
//! connection (AD-30, VC-02).

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use noctis_core::lifecycle::Emitter;
use noctis_gateway::GatewayHandle;
use songbird::Call;
use songbird::driver::Bitrate;
use songbird::id::{GuildId, UserId};
use songbird::shards::Shard;
use tokio::sync::oneshot;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::config::VoiceConfig;
use crate::frames::{driver_config, effective_bitrate};
use crate::player::SongbirdPlayer;
use crate::queue::{Binding, IdleLeave, Player, QueueCell};
use crate::shard::NoctisShard;
use crate::track::TrackId;

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
    /// The guild's queue; shut down whenever the entry is removed.
    pub(crate) queue: Arc<QueueCell>,
    /// The encoder bitrate applied to the call: the channel's, capped by
    /// `VoiceConfig::max_bitrate`.
    pub(crate) bitrate: u32,
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

/// The voice state of one App, registered as App state by `VoicePlugin`:
/// a handle on [`VoiceInner`] (an idle timer holds it weakly).
pub(crate) struct VoiceShared(Arc<VoiceInner>);

impl Deref for VoiceShared {
    type Target = VoiceInner;

    fn deref(&self) -> &VoiceInner {
        &self.0
    }
}

/// The voice state of one App.
pub(crate) struct VoiceInner {
    pub(crate) config: VoiceConfig,
    calls: Mutex<CallMap>,
    /// The next entry generation and join number.
    counter: AtomicU64,
    /// The next track id.
    track_ids: AtomicU64,
    /// Shutdown: the guilds whose leave waits for the bot's own
    /// `VOICE_STATE_UPDATE` with no channel.
    leaving: Mutex<Leaving>,
    /// The `app` label and the `Emitter`, bound by `AppStarted` or
    /// `App::voice`.
    binding: Arc<Binding>,
    /// The App's media HTTP client (URL tracks).
    media: reqwest::Client,
    /// Tests: new queues play on a `FakePlayer` recording here.
    #[cfg(test)]
    pub(crate) fake_player: Mutex<Option<crate::queue::fake::Played>>,
}

impl VoiceShared {
    pub(crate) fn new(config: VoiceConfig, media: reqwest::Client) -> Self {
        Self(Arc::new(VoiceInner {
            config,
            calls: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(1),
            track_ids: AtomicU64::new(1),
            leaving: Mutex::new(HashMap::new()),
            binding: Arc::default(),
            media,
            #[cfg(test)]
            fake_player: Mutex::new(None),
        }))
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
        let mut call = Call::from_config(
            GuildId::from(guild.into_nonzero()),
            Shard::Generic(Arc::clone(&shard) as _),
            UserId::from(user.into_nonzero()),
            driver_config(),
        );
        // The channel's bitrate is applied once the bot's own voice state
        // names the channel.
        let bitrate = effective_bitrate(None, self.config.get_max_bitrate());
        call.set_bitrate(bits(bitrate));
        let call = Arc::new(tokio::sync::Mutex::new(call));
        let generation = self.next();
        let queue = self.new_queue(guild, generation, &call);
        let entry = Entry {
            generation,
            join,
            channel: Some(channel),
            call: Arc::clone(&call),
            shard: Arc::clone(&shard),
            queue,
            bitrate,
        };
        calls.insert(guild, entry);
        let len = calls.len();
        drop(calls);
        self.publish(len);
        Joining {
            generation,
            join,
            call,
            shard,
        }
    }

    /// A new queue for `guild`'s entry of `generation`, playing on `call`.
    fn new_queue(
        &self,
        guild: Id<GuildMarker>,
        generation: u64,
        call: &Arc<tokio::sync::Mutex<Call>>,
    ) -> Arc<QueueCell> {
        let idle = self.config.get_idle_leave().map(|after| {
            let inner = Arc::downgrade(&self.0);
            let leave: IdleLeave = Arc::new(move || {
                let inner = Weak::clone(&inner);
                Box::pin(async move {
                    if let Some(inner) = inner.upgrade() {
                        inner.idle_leave(guild, generation).await;
                    }
                })
            });
            (after, leave)
        });
        #[cfg(test)]
        let fake = self
            .fake_player
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Arc::new_cyclic(|queue: &Weak<QueueCell>| {
            #[cfg(test)]
            if let Some(played) = fake {
                return QueueCell::new(
                    guild,
                    self.config.get_max_queue(),
                    Box::new(crate::queue::fake::FakePlayer(played)),
                    Arc::clone(&self.binding),
                    idle,
                );
            }
            let player: Box<dyn Player> = Box::new(SongbirdPlayer::new(
                Arc::clone(call),
                self.media.clone(),
                Weak::clone(queue),
                Arc::clone(&self.binding),
            ));
            QueueCell::new(
                guild,
                self.config.get_max_queue(),
                player,
                Arc::clone(&self.binding),
                idle,
            )
        })
    }
}

impl VoiceShared {
    /// The bitrate of `channel` changed or became known (`None`: unknown):
    /// when `guild`'s call is in `channel` and its encoder bitrate changes,
    /// the new one is applied on the call in a task of its own (never while
    /// holding the call map).
    pub(crate) fn set_bitrate(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
        channel_bitrate: Option<u32>,
    ) {
        let bitrate = effective_bitrate(channel_bitrate, self.config.get_max_bitrate());
        let call = {
            let mut calls = self.calls();
            let Some(entry) = calls.get_mut(&guild) else {
                return;
            };
            if entry.channel != Some(channel) || entry.bitrate == bitrate {
                return;
            }
            entry.bitrate = bitrate;
            Arc::clone(&entry.call)
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let inner = Arc::downgrade(&self.0);
        runtime.spawn(async move {
            let mut driver = call.lock().await;
            // The entry's latest bitrate, read under the Call lock: tasks
            // that run out of order still leave the newest one applied.
            let latest = inner.upgrade().and_then(|inner| {
                inner
                    .calls()
                    .get(&guild)
                    .filter(|entry| Arc::ptr_eq(&entry.call, &call))
                    .map(|entry| entry.bitrate)
            });
            if let Some(latest) = latest {
                driver.set_bitrate(bits(latest));
            }
        });
    }
}

impl VoiceInner {
    fn calls(&self) -> MutexGuard<'_, CallMap> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn leaving(&self) -> MutexGuard<'_, Leaving> {
        self.leaving.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Binds the `app` label and the `Emitter` (the first binding wins)
    /// and publishes the gauge.
    pub(crate) fn bind_app(&self, app: &str, emitter: Emitter) {
        let _ = self.binding.emitter.set(emitter);
        if self.binding.app.get().is_none() {
            let _ = self.binding.app.set(Arc::from(app));
            let len = self.calls().len();
            self.publish(len);
        }
    }

    /// A new track id, unique per App.
    pub(crate) fn next_track_id(&self) -> TrackId {
        TrackId::new(self.track_ids.fetch_add(1, Ordering::Relaxed))
    }

    /// Sets `noctis_voice_calls` to `len`; nothing before the label is
    /// bound.
    fn publish(&self, len: usize) {
        if let Some(app) = self.binding.app.get() {
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

    /// The encoder bitrate of `guild`'s call.
    #[cfg(test)]
    pub(crate) fn bitrate(&self, guild: Id<GuildMarker>) -> Option<u32> {
        self.calls().get(&guild).map(|entry| entry.bitrate)
    }

    /// The call of `guild`'s entry.
    pub(crate) fn call(&self, guild: Id<GuildMarker>) -> Option<Arc<tokio::sync::Mutex<Call>>> {
        self.calls()
            .get(&guild)
            .map(|entry| Arc::clone(&entry.call))
    }

    /// The queue of `guild`'s entry of `generation`, if it is still open.
    pub(crate) fn queue(&self, guild: Id<GuildMarker>, generation: u64) -> Option<Arc<QueueCell>> {
        self.calls()
            .get(&guild)
            .filter(|entry| entry.generation == generation)
            .map(|entry| Arc::clone(&entry.queue))
    }

    /// The join of `guild`'s entry of `generation` resolved: its queue may
    /// arm the idle timer.
    pub(crate) fn connected(&self, guild: Id<GuildMarker>, generation: u64) {
        if let Some(queue) = self.queue(guild, generation) {
            queue.set_connected();
        }
    }

    /// Idle leave: removes `guild`'s entry of `generation` if its queue is
    /// still idle and sends op 4 leave, like `CallHandle::leave`.
    pub(crate) async fn idle_leave(&self, guild: Id<GuildMarker>, generation: u64) {
        // Activity between the timer's sleep and this removal keeps the call.
        let Some(entry) = self.remove_where(guild, |entry| {
            entry.generation == generation && entry.queue.is_idle()
        }) else {
            return;
        };
        tracing::debug!(target: "noctis::voice", guild = guild.get(), "idle leave");
        if let Err(error) = crate::voice::leave_entry(guild, &entry).await {
            tracing::warn!(
                target: "noctis::voice",
                guild = guild.get(),
                %error,
                "op 4 leave of an idle call was not sent"
            );
        }
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
        if let Some(entry) = &removed {
            entry.queue.shutdown();
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
        for (_, entry) in &taken {
            entry.queue.shutdown();
        }
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

/// A Songbird bitrate of `bitrate` bits per second.
fn bits(bitrate: u32) -> Bitrate {
    Bitrate::Bits(i32::try_from(bitrate).unwrap_or(i32::MAX))
}
