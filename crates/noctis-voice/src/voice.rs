//! [`Voice`], [`CallHandle`] and [`VoiceExt`]: joining and leaving voice
//! channels from any context.

use std::fmt;
use std::future::Future;

use futures_util::FutureExt as _;
use futures_util::future::Either;
use noctis_core::{App, CommandCtx, ComponentCtx, EventCtx, HookCtx, ModalSubmitCtx, State};
use noctis_gateway::{GatewayHandle, VoiceStateRequest};
use songbird::error::JoinError;
use songbird::id::ChannelId;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker};

use crate::call::{Entry, VoiceShared};
use crate::error::VoiceError;
use crate::queue::{Queue, QueueCell};
use crate::shard::NoctisShard;
use crate::track::{Track, TrackInfo};

/// The App's voice connections: one call per guild (VC-02), joined and left
/// through Noctis's gateway (VC-01).
///
/// Get it with [`VoiceExt::voice`] (`ctx.voice()`, `app.voice()`). Cheap to
/// clone; every clone works on the same calls. Without
/// [`AppBuilderVoiceExt::voice`](crate::AppBuilderVoiceExt::voice) every
/// operation is [`VoiceError::NotInstalled`].
///
/// ```no_run
/// use noctis_core::CommandCtx;
/// use noctis_voice::{VoiceError, VoiceExt};
/// use twilight_model::id::Id;
///
/// async fn join(ctx: &CommandCtx) -> Result<(), VoiceError> {
///     let call = ctx.voice().join(Id::new(1), Id::new(2)).await?;
///     assert_eq!(call.channel(), Some(Id::new(2)));
///     call.leave().await
/// }
/// ```
#[derive(Clone)]
pub struct Voice {
    shared: Option<State<VoiceShared>>,
    gateway: GatewayHandle,
}

impl Voice {
    fn new(shared: Option<State<VoiceShared>>, gateway: GatewayHandle) -> Self {
        Self { shared, gateway }
    }

    fn shared(&self) -> Result<&VoiceShared, VoiceError> {
        self.shared.as_deref().ok_or(VoiceError::NotInstalled)
    }

    /// Joins `channel` of `guild`, or moves the guild's call there (VC-02).
    ///
    /// Sends op 4 from the guild's owning shard (VC-01) and resolves once
    /// Discord answered and the voice driver connected, within
    /// `VoiceConfig::join_timeout`. Joining the channel the call is already
    /// connected to sends nothing and returns the same call; a second join
    /// to the same channel while the first is still pending interrupts the
    /// first ([`VoiceError::Interrupted`]) and sends op 4 again. The returned
    /// future is `Send + 'static`: it can be spawned.
    ///
    /// # Errors
    ///
    /// [`VoiceError::NotInstalled`]; [`VoiceError::Gateway`] when op 4 is
    /// refused (also [`GatewayError::NotRunning`] before `run`);
    /// [`VoiceError::NotReady`] before the first READY;
    /// [`VoiceError::Timeout`] and [`VoiceError::Driver`] (the call is left
    /// and removed); [`VoiceError::Interrupted`] when a newer join,
    /// a leave, a disconnect or a `GUILD_DELETE` replaced this join.
    pub fn join(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
    ) -> impl Future<Output = Result<CallHandle, VoiceError>> + Send + 'static {
        let voice = self.clone();
        async move { voice.join_inner(guild, channel).await }
    }

    async fn join_inner(
        self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
    ) -> Result<CallHandle, VoiceError> {
        let shared = self.shared()?;
        let Some(user) = self.gateway.current_user_id() else {
            self.gateway
                .shard_for_guild(guild)
                .map_err(VoiceError::Gateway)?;
            return Err(VoiceError::NotReady);
        };
        self.gateway
            .shard_for_guild(guild)
            .map_err(VoiceError::Gateway)?;
        let joining = shared.begin_join(guild, channel, user, &self.gateway);
        let handle = CallHandle {
            guild,
            generation: joining.generation,
            voice: self.clone(),
        };
        let target = ChannelId::from(channel.into_nonzero());
        let join = {
            let mut call = joining.call.lock().await;
            if call.current_connection().is_some() && call.current_channel() == Some(target) {
                drop(call);
                shared.connected(guild, joining.generation);
                return Ok(handle);
            }
            if call.current_channel().is_some() && call.current_connection().is_none() {
                // A join still in progress: drop it locally (its waiter
                // gets `Interrupted`) and move straight to `channel`,
                // without Songbird's op 4 leave in between (VC-02).
                call.update_state(String::new(), None::<ChannelId>);
            }
            // The recording driver joins through the gateway only: it
            // resolves on Discord's answers, without the voice driver.
            let started = if shared.config.gateway_only() {
                call.join_gateway(target)
                    .await
                    .map(|join| Either::Right(join.map(|info| info.map(drop))))
            } else {
                call.join(target).await.map(Either::Left)
            };
            match started {
                Ok(join) => join,
                Err(error) => {
                    let error = map_send_error(guild, error, &joining.shard);
                    // Drop the unsent join locally, then its entry: a
                    // refused op 4 leaves no call behind (VC-02).
                    call.update_state(String::new(), None::<ChannelId>);
                    drop(call);
                    shared.remove_join(guild, joining.generation, joining.join);
                    return Err(error);
                }
            }
        };
        let after = shared.config.get_join_timeout();
        let error = match tokio::time::timeout(after, join).await {
            Ok(Ok(())) => {
                shared.connected(guild, joining.generation);
                return Ok(handle);
            }
            Ok(Err(JoinError::Dropped)) => return Err(VoiceError::Interrupted { guild }),
            Ok(Err(JoinError::TimedOut)) | Err(_) => VoiceError::Timeout { guild, after },
            Ok(Err(error)) => VoiceError::Driver {
                guild,
                reason: error.to_string(),
            },
        };
        if let Some(entry) = shared.remove_join(guild, joining.generation, joining.join) {
            leave_entry(guild, &entry).await.unwrap_or_else(|error| {
                tracing::debug!(
                    target: "noctis::voice",
                    guild = guild.get(),
                    %error,
                    "op 4 leave after a failed join was not sent"
                );
            });
        }
        Err(error)
    }

    /// Leaves `guild`'s voice channel: removes its call and sends op 4 with
    /// no channel. Without a call it still sends op 4, which clears a
    /// channel left over from before a restart.
    ///
    /// # Errors
    ///
    /// [`VoiceError::NotInstalled`]; [`VoiceError::Gateway`] when op 4 is
    /// refused (the call is removed anyway).
    pub async fn leave(&self, guild: Id<GuildMarker>) -> Result<(), VoiceError> {
        let shared = self.shared()?;
        match shared.remove(guild) {
            Some(entry) => leave_entry(guild, &entry).await,
            None => self
                .gateway
                .update_voice_state(
                    VoiceStateRequest::leave(guild).self_deaf(shared.config.is_self_deaf()),
                )
                .map_err(VoiceError::Gateway),
        }
    }

    /// `guild`'s live or pending call, if any.
    pub fn call(&self, guild: Id<GuildMarker>) -> Option<CallHandle> {
        let (generation, _) = self.shared.as_deref()?.entry(guild)?;
        Some(CallHandle {
            guild,
            generation,
            voice: self.clone(),
        })
    }

    /// The number of live and pending calls (0 without voice).
    pub fn active_calls(&self) -> usize {
        self.shared.as_deref().map_or(0, |shared| shared.len())
    }
}

impl fmt::Debug for Voice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Voice")
            .field("installed", &self.shared.is_some())
            .field("active_calls", &self.active_calls())
            .finish_non_exhaustive()
    }
}

/// Sends op 4 leave for `guild`'s removed entry and stops its driver.
pub(crate) async fn leave_entry(guild: Id<GuildMarker>, entry: &Entry) -> Result<(), VoiceError> {
    let mut call = entry.call.lock().await;
    call.leave()
        .await
        .map_err(|error| map_send_error(guild, error, &entry.shard))
}

/// The error of a refused send stage.
fn map_send_error(guild: Id<GuildMarker>, error: JoinError, shard: &NoctisShard) -> VoiceError {
    match error {
        JoinError::NoSender => match shard.take_error() {
            Some(error) => VoiceError::Gateway(error),
            None => VoiceError::Driver {
                guild,
                reason: error.to_string(),
            },
        },
        JoinError::Dropped => VoiceError::Interrupted { guild },
        other => VoiceError::Driver {
            guild,
            reason: other.to_string(),
        },
    }
}

/// One guild's call, from [`Voice::join`] or [`Voice::call`], and its
/// queue.
///
/// A handle never keeps the call alive: once the call is left, disconnected,
/// removed or replaced by a new call of the guild, the handle is stale
/// ([`channel`](Self::channel) is `None`, [`leave`](Self::leave) and every
/// queue mutator are [`VoiceError::Interrupted`], the queue getters read
/// `None`, empty or `1.0`).
///
/// **The queue** holds the current track plus the pending ones, at most
/// `VoiceConfig::max_queue` (default 1000). The driver only ever holds the
/// current track; a pending track's input is created when it becomes
/// current. Its outcomes reach Modules as the hook events
/// [`TrackStart`](crate::TrackStart), [`TrackEnd`](crate::TrackEnd),
/// [`TrackError`](crate::TrackError) and [`QueueEmpty`](crate::QueueEmpty).
/// A track error never stops the queue: the next track starts.
///
/// ```no_run
/// use noctis_voice::{CallHandle, Track, VoiceError};
///
/// async fn play(call: &CallHandle) -> Result<(), VoiceError> {
///     let info = call.enqueue(Track::url("https://cdn.example.com/a.ogg")?.title("A")).await?;
///     assert_eq!(call.now_playing().map(|t| t.id()), Some(info.id()));
///     call.set_volume(0.5)?;
///     call.skip()?;
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct CallHandle {
    guild: Id<GuildMarker>,
    pub(crate) generation: u64,
    voice: Voice,
}

impl CallHandle {
    /// The guild.
    pub fn guild(&self) -> Id<GuildMarker> {
        self.guild
    }

    /// The channel the call is in or joining; `None` once the handle is
    /// stale.
    pub fn channel(&self) -> Option<Id<ChannelMarker>> {
        let (generation, channel) = self.voice.shared.as_deref()?.entry(self.guild)?;
        (generation == self.generation).then_some(channel).flatten()
    }

    /// Leaves the channel: removes the call and sends op 4 with no channel.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Interrupted`] for a stale handle (nothing is sent);
    /// [`VoiceError::Gateway`] when op 4 is refused (the call is removed
    /// anyway).
    pub async fn leave(&self) -> Result<(), VoiceError> {
        let shared = self.voice.shared()?;
        match shared.remove_generation(self.guild, self.generation) {
            Some(entry) => leave_entry(self.guild, &entry).await,
            None => Err(VoiceError::Interrupted { guild: self.guild }),
        }
    }

    /// The queue of this handle's call, if it is not stale.
    fn queue(&self) -> Result<std::sync::Arc<QueueCell>, VoiceError> {
        self.voice
            .shared()?
            .queue(self.guild, self.generation)
            .ok_or(VoiceError::Interrupted { guild: self.guild })
    }

    /// Reads the queue; `None` for a stale handle.
    fn read<T>(&self, read: impl FnOnce(&Queue) -> T) -> Option<T> {
        self.queue().ok()?.read(read)
    }

    /// Appends `track` to the queue and returns its [`TrackInfo`] (with a
    /// new id). With nothing current it becomes current at once and plays;
    /// [`TrackStart`](crate::TrackStart) fires when the driver reports it
    /// playable. A paused current track stays paused.
    ///
    /// # Errors
    ///
    /// [`VoiceError::QueueFull`] when the queue (current plus pending)
    /// already holds `VoiceConfig::max_queue` tracks; nothing changes.
    /// [`VoiceError::Interrupted`] for a stale handle.
    #[expect(
        clippy::unused_async,
        reason = "async by contract: room for an awaited admission step without an API break"
    )]
    pub async fn enqueue(&self, track: Track) -> Result<TrackInfo, VoiceError> {
        let queue = self.queue()?;
        let info = track.into_info(self.voice.shared()?.next_track_id());
        queue.enqueue(info)
    }

    /// Skips the current track: [`TrackEnd`](crate::TrackEnd) with
    /// [`EndReason::Skipped`](crate::EndReason::Skipped), then the next
    /// track plays (not paused), or [`QueueEmpty`](crate::QueueEmpty).
    /// Returns the skipped track; `None` (and no event) when nothing is
    /// current.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Interrupted`] for a stale handle.
    pub fn skip(&self) -> Result<Option<TrackInfo>, VoiceError> {
        self.queue()?.skip()
    }

    /// Pauses the current track; `false` when it is already paused or
    /// nothing is current.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Interrupted`] for a stale handle.
    pub fn pause(&self) -> Result<bool, VoiceError> {
        self.queue()?.pause()
    }

    /// Resumes the paused current track; `false` when it is playing or
    /// nothing is current.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Interrupted`] for a stale handle.
    pub fn resume(&self) -> Result<bool, VoiceError> {
        self.queue()?.resume()
    }

    /// Stops the current track ([`TrackEnd`](crate::TrackEnd) with
    /// [`EndReason::Stopped`](crate::EndReason::Stopped), then
    /// [`QueueEmpty`](crate::QueueEmpty)) and drops the pending ones.
    /// Returns how many tracks the queue held (0 and no event when empty).
    /// The call stays connected.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Interrupted`] for a stale handle.
    pub fn stop(&self) -> Result<usize, VoiceError> {
        self.queue()?.stop()
    }

    /// Drops the pending tracks; the current one keeps playing and no
    /// event fires. Returns how many were dropped.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Interrupted`] for a stale handle.
    pub fn clear(&self) -> Result<usize, VoiceError> {
        self.queue()?.clear()
    }

    /// Sets the volume of the current track and of every later one:
    /// `0.0..=2.0` (default `1.0`).
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidVolume`] for a volume outside `0.0..=2.0` or
    /// not finite (nothing changes); [`VoiceError::Interrupted`] for a stale
    /// handle.
    pub fn set_volume(&self, volume: f32) -> Result<(), VoiceError> {
        self.queue()?.set_volume(volume)
    }

    /// The current track (playing or paused); `None` when nothing is
    /// current or the handle is stale.
    pub fn now_playing(&self) -> Option<TrackInfo> {
        self.read(Queue::now_playing).flatten()
    }

    /// Whether the current track is paused.
    pub fn is_paused(&self) -> bool {
        self.read(Queue::is_paused).unwrap_or(false)
    }

    /// The pending tracks, in play order (the current one excluded).
    pub fn list(&self) -> Vec<TrackInfo> {
        self.read(Queue::list).unwrap_or_default()
    }

    /// The current plus the pending tracks.
    pub fn queue_len(&self) -> usize {
        self.read(Queue::len).unwrap_or(0)
    }

    /// The queue's volume (`1.0` by default and for a stale handle).
    pub fn volume(&self) -> f32 {
        self.read(Queue::volume).unwrap_or(1.0)
    }
}

impl fmt::Debug for CallHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallHandle")
            .field("guild", &self.guild)
            .field("channel", &self.channel())
            .finish_non_exhaustive()
    }
}

mod sealed {
    pub trait Sealed {}
}

/// `.voice()` on the App and on every handler context: the App's [`Voice`].
///
/// Sealed: implemented for `App`, `EventCtx`, `CommandCtx`,
/// `ComponentCtx`, `ModalSubmitCtx` and `HookCtx`.
///
/// ```
/// use noctis_core::App;
/// use noctis_voice::{AppBuilderVoiceExt, VoiceConfig, VoiceExt};
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
/// let mut builder = App::builder();
/// builder.voice(VoiceConfig::default());
/// let app = builder.build().unwrap();
/// assert_eq!(app.voice().active_calls(), 0);
/// # }
/// ```
pub trait VoiceExt: sealed::Sealed {
    /// The App's voice connections.
    fn voice(&self) -> Voice;
}

impl sealed::Sealed for App {}

impl VoiceExt for App {
    fn voice(&self) -> Voice {
        let shared = self.state::<VoiceShared>();
        if let Some(shared) = &shared {
            shared.bind_app(self.app_id().as_str(), self.emitter());
        }
        Voice::new(shared, self.gateway())
    }
}

/// `VoiceExt` for the contexts with `state` and `gateway`.
macro_rules! ctx_voice {
    ($($ctx:ty),* $(,)?) => {$(
        impl sealed::Sealed for $ctx {}

        impl VoiceExt for $ctx {
            fn voice(&self) -> Voice {
                Voice::new(self.state::<VoiceShared>(), self.gateway())
            }
        }
    )*};
}

ctx_voice!(EventCtx, CommandCtx, ComponentCtx, ModalSubmitCtx, HookCtx);
