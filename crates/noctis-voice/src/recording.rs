//! The recording driver (feature `testkit`): [`VoiceConfig::recording`]
//! joins through Songbird's gateway-only join and plays every queue on a
//! recorder a test reads and drives, so voice commands are testable without
//! UDP, the voice WebSocket, files, fetches or child processes.
//!
//! [`VoiceConfig::recording`]: crate::VoiceConfig::recording

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::events::{QueueEmpty, TrackEnd, TrackError, TrackStart};
use crate::queue::{Note, Player, Queue, QueueCell};
use crate::track::TrackInfo;

/// One command the queue gave the recording driver, in order
/// ([`VoiceRecorder::commands`]).
///
/// ```
/// use noctis_voice::RecordedCommand;
///
/// assert_eq!(RecordedCommand::SetVolume(0.5), RecordedCommand::SetVolume(0.5));
/// assert_ne!(RecordedCommand::Pause, RecordedCommand::Resume);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedCommand {
    /// A track became current and plays at `volume`.
    Play {
        /// The track, as the queue holds it.
        track: TrackInfo,
        /// The queue's volume when it started.
        volume: f32,
    },
    /// The current track was paused.
    Pause,
    /// The current track was resumed.
    Resume,
    /// The current track was stopped (skip, stop, a failure or a leave).
    Stop,
    /// The current track's volume changed.
    SetVolume(f32),
}

/// One track event the queue fired, in order, recorded whether or not a
/// hook takes it ([`VoiceRecorder::events`]).
///
/// ```
/// use noctis_voice::RecordedEvent;
///
/// fn is_end(event: &RecordedEvent) -> bool {
///     matches!(event, RecordedEvent::End(_))
/// }
/// # let _ = is_end;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedEvent {
    /// [`TrackStart`].
    Start(TrackStart),
    /// [`TrackEnd`].
    End(TrackEnd),
    /// [`TrackError`].
    Error(TrackError),
    /// [`QueueEmpty`].
    Empty(QueueEmpty),
}

/// What the recorder holds for one guild.
#[derive(Default)]
struct GuildRecord {
    commands: Vec<RecordedCommand>,
    events: Vec<RecordedEvent>,
    /// The token of the track the driver plays and its queue.
    current: Option<(u64, Weak<QueueCell>)>,
}

/// The recorded guilds.
type Guilds = HashMap<Id<GuildMarker>, GuildRecord>; // keyed-map: test-only (feature `testkit`); the guilds one test App joined, dropped with the recorder

/// The recording driver of a [`VoiceConfig::recording`] config: every
/// guild's queue commands and track events, and the driver reports a test
/// sends with [`start`](Self::start), [`finish`](Self::finish) and
/// [`fail`](Self::fail).
///
/// Get it with [`VoiceConfig::recorder`]. Clones share one record; two
/// configs are equal when they share it. Use one recorder per App: two Apps
/// built from the same recording config record into the same guild
/// entries. Nothing plays and nothing reports by itself: a track stays
/// current until the test finishes, fails, skips or stops it.
///
/// ```
/// use noctis_voice::VoiceConfig;
/// use twilight_model::id::Id;
///
/// let config = VoiceConfig::recording();
/// let recorder = config.recorder().expect("a recording config");
/// assert!(recorder.commands(Id::new(1)).is_empty());
/// assert_eq!(recorder.start(Id::new(1)), None); // nothing is current
/// ```
///
/// [`VoiceConfig::recording`]: crate::VoiceConfig::recording
/// [`VoiceConfig::recorder`]: crate::VoiceConfig::recorder
#[derive(Clone, Default)]
pub struct VoiceRecorder(Arc<Mutex<Guilds>>);

impl VoiceRecorder {
    /// An empty recorder.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Guilds> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `update` on `guild`'s record.
    fn with(&self, guild: Id<GuildMarker>, update: impl FnOnce(&mut GuildRecord)) {
        update(self.lock().entry(guild).or_default());
    }

    /// The commands `guild`'s queues gave the driver so far, oldest first;
    /// empty for a guild that never had a call.
    pub fn commands(&self, guild: Id<GuildMarker>) -> Vec<RecordedCommand> {
        self.lock()
            .get(&guild)
            .map(|record| record.commands.clone())
            .unwrap_or_default()
    }

    /// The track events `guild`'s queues fired so far, oldest first; empty
    /// for a guild that never had a call.
    pub fn events(&self, guild: Id<GuildMarker>) -> Vec<RecordedEvent> {
        self.lock()
            .get(&guild)
            .map(|record| record.events.clone())
            .unwrap_or_default()
    }

    /// Reports `guild`'s current track playable, like the driver does once
    /// its input is ready: [`TrackStart`] fires (once per track). Returns
    /// the current track; `None` (and nothing changes) when nothing is
    /// current or the call is gone.
    pub fn start(&self, guild: Id<GuildMarker>) -> Option<TrackInfo> {
        self.report(guild, QueueCell::on_playable)
            .map(|(_, track)| track)
    }

    /// Reports `guild`'s current track played to its end: [`TrackEnd`] with
    /// [`EndReason::Finished`](crate::EndReason::Finished), then the next track plays or [`QueueEmpty`]
    /// fires. Returns the finished track; `None` (and nothing changes) when
    /// nothing is current or the call is gone.
    pub fn finish(&self, guild: Id<GuildMarker>) -> Option<TrackInfo> {
        let (token, finished) = self.report(guild, QueueCell::on_end)?;
        self.forget(guild, token);
        Some(finished)
    }

    /// Reports `guild`'s current track failed with `error`: [`TrackError`],
    /// then the next track plays or [`QueueEmpty`] fires. Returns the failed
    /// track; `None` (and nothing changes) when nothing is current or the
    /// call is gone.
    pub fn fail(&self, guild: Id<GuildMarker>, error: impl Into<String>) -> Option<TrackInfo> {
        let error = error.into();
        self.report(guild, move |queue, token| queue.on_error(token, error))
            .map(|(_, track)| track)
    }

    /// Sends one driver report for `guild`'s current track and returns its
    /// token and the track. The slot is cloned out first: the queue's lock
    /// is never taken while the recorder's is held.
    fn report(
        &self,
        guild: Id<GuildMarker>,
        send: impl FnOnce(&QueueCell, u64),
    ) -> Option<(u64, TrackInfo)> {
        let (token, queue) = self.lock().get(&guild)?.current.clone()?;
        let queue = queue.upgrade()?;
        let current = queue.read(Queue::now_playing).flatten()?;
        send(&queue, token);
        Some((token, current))
    }

    /// Clears `guild`'s slot after the track of `token` finished, unless the
    /// next track already took it.
    fn forget(&self, guild: Id<GuildMarker>, token: u64) {
        self.with(guild, |record| {
            if record.current.as_ref().is_some_and(|(t, _)| *t == token) {
                record.current = None;
            }
        });
    }

    /// Records `notes` as `guild`'s events.
    pub(crate) fn record_notes(&self, guild: Id<GuildMarker>, notes: &[Note]) {
        if notes.is_empty() {
            return;
        }
        self.with(guild, |record| {
            record
                .events
                .extend(notes.iter().map(|note| event_of(guild, note)));
        });
    }
}

/// The public form of a queue note.
fn event_of(guild: Id<GuildMarker>, note: &Note) -> RecordedEvent {
    match note.clone() {
        Note::Start(track) => RecordedEvent::Start(TrackStart { guild, track }),
        Note::End(track, reason) => RecordedEvent::End(TrackEnd {
            guild,
            track,
            reason,
        }),
        Note::Error(track, error) => RecordedEvent::Error(TrackError {
            guild,
            track,
            error,
        }),
        Note::Empty => RecordedEvent::Empty(QueueEmpty { guild }),
    }
}

impl PartialEq for VoiceRecorder {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for VoiceRecorder {}

impl fmt::Debug for VoiceRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceRecorder")
            .field("guilds", &self.lock().len())
            .finish()
    }
}

/// The [`Player`] of a recording config: records every command and keeps
/// the current track's token for the test's reports. Its methods run under
/// the queue's lock and take the recorder's.
pub(crate) struct RecordingPlayer {
    pub(crate) recorder: VoiceRecorder,
    pub(crate) guild: Id<GuildMarker>,
    pub(crate) queue: Weak<QueueCell>,
}

impl RecordingPlayer {
    fn push(&self, command: RecordedCommand) {
        self.recorder
            .with(self.guild, |record| record.commands.push(command));
    }
}

impl Player for RecordingPlayer {
    fn play(&self, token: u64, track: &TrackInfo, volume: f32) {
        let queue = Weak::clone(&self.queue);
        self.recorder.with(self.guild, |record| {
            record.commands.push(RecordedCommand::Play {
                track: track.clone(),
                volume,
            });
            record.current = Some((token, queue));
        });
    }

    fn pause(&self, _token: u64) {
        self.push(RecordedCommand::Pause);
    }

    fn resume(&self, _token: u64) {
        self.push(RecordedCommand::Resume);
    }

    fn stop(&self, token: u64) {
        self.recorder.with(self.guild, |record| {
            record.commands.push(RecordedCommand::Stop);
            if record.current.as_ref().is_some_and(|(t, _)| *t == token) {
                record.current = None;
            }
        });
    }

    fn set_volume(&self, _token: u64, volume: f32) {
        self.push(RecordedCommand::SetVolume(volume));
    }
}

#[cfg(test)]
mod tests;
