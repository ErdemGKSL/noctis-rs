//! A guild's queue (AD-30): a Noctis-owned state machine over track
//! descriptions. The driver only ever holds the current track; pending
//! tracks are [`TrackInfo`]s whose inputs are created when they become
//! current. Every track that becomes current gets a token, and a driver
//! report whose token is not the current one is ignored, so each such track
//! yields exactly one terminal outcome.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use futures_util::future::BoxFuture;
use noctis_core::lifecycle::Emitter;
use tokio::task::AbortHandle;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::error::VoiceError;
use crate::events::{EndReason, QueueEmpty, TrackEnd, TrackError, TrackStart};
use crate::track::TrackInfo;

/// The counter of the tracks that became current, by outcome.
const TRACKS_COUNTER: &str = "noctis_voice_tracks_total";

/// The volume range.
const VOLUME: std::ops::RangeInclusive<f32> = 0.0..=2.0;

/// What plays a queue's current track: the Songbird driver, the recording
/// driver (feature `testkit`) or a test fake. Every method is synchronous and
/// quick (it may spawn); it runs under the queue's lock, so it must never
/// call back into the queue.
pub(crate) trait Player: Send + Sync {
    /// Makes `track` (token `token`) the driver's only track, at `volume`,
    /// playing.
    fn play(&self, token: u64, track: &TrackInfo, volume: f32);
    /// Pauses the track of `token`.
    fn pause(&self, token: u64);
    /// Resumes the track of `token`.
    fn resume(&self, token: u64);
    /// Stops the track of `token` (its input is dropped).
    fn stop(&self, token: u64);
    /// Sets the volume of the track of `token`.
    fn set_volume(&self, token: u64, volume: f32);
}

/// A player command, executed after the state changed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cmd {
    Play(u64, TrackInfo, f32),
    Pause(u64),
    Resume(u64),
    Stop(u64),
    SetVolume(u64, f32),
}

/// An event to emit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Note {
    Start(TrackInfo),
    End(TrackInfo, EndReason),
    Error(TrackInfo, String),
    Empty,
}

/// What one operation asks for: player commands, events, counted results.
#[derive(Debug, Default)]
pub(crate) struct Out {
    pub(crate) cmds: Vec<Cmd>,
    pub(crate) notes: Vec<Note>,
    pub(crate) counted: Vec<&'static str>,
}

/// The current track.
#[derive(Debug)]
struct Current {
    info: TrackInfo,
    token: u64,
    paused: bool,
    /// Whether `TrackStart` fired (the player reported it playable).
    started: bool,
}

/// The pure queue state.
#[derive(Debug)]
pub(crate) struct Queue {
    current: Option<Current>,
    pending: VecDeque<TrackInfo>,
    volume: f32,
    max: usize,
    next_token: u64,
}

impl Queue {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            current: None,
            pending: VecDeque::new(),
            volume: 1.0,
            max,
            next_token: 1,
        }
    }

    /// The current plus the pending tracks.
    pub(crate) fn len(&self) -> usize {
        usize::from(self.current.is_some()) + self.pending.len()
    }

    /// Nothing plays: no current track, or the current one is paused.
    pub(crate) fn is_idle(&self) -> bool {
        self.current.as_ref().is_none_or(|c| c.paused)
    }

    pub(crate) fn now_playing(&self) -> Option<TrackInfo> {
        self.current.as_ref().map(|c| c.info.clone())
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.current.as_ref().is_some_and(|c| c.paused)
    }

    pub(crate) fn list(&self) -> Vec<TrackInfo> {
        self.pending.iter().cloned().collect()
    }

    pub(crate) fn volume(&self) -> f32 {
        self.volume
    }

    /// Makes `info` current and asks the player to play it.
    fn start(&mut self, info: TrackInfo, out: &mut Out) {
        let token = self.next_token;
        self.next_token += 1;
        out.cmds.push(Cmd::Play(token, info.clone(), self.volume));
        self.current = Some(Current {
            info,
            token,
            paused: false,
            started: false,
        });
    }

    /// The next pending track becomes current, or the queue is empty.
    fn advance(&mut self, out: &mut Out) {
        self.current = None;
        match self.pending.pop_front() {
            Some(next) => self.start(next, out),
            None => out.notes.push(Note::Empty),
        }
    }

    /// Ends the current track with `reason`.
    fn end_current(&mut self, reason: EndReason, out: &mut Out) -> Option<TrackInfo> {
        let current = self.current.take()?;
        if reason != EndReason::Finished {
            out.cmds.push(Cmd::Stop(current.token));
        }
        out.notes.push(Note::End(current.info.clone(), reason));
        out.counted.push(reason.as_str());
        Some(current.info)
    }

    /// Whether `token` is the current track's.
    fn is_current(&self, token: u64) -> bool {
        self.current.as_ref().is_some_and(|c| c.token == token)
    }

    /// Appends `info`; it plays at once when nothing is current.
    pub(crate) fn enqueue(&mut self, info: TrackInfo, out: &mut Out) -> Result<(), usize> {
        if self.len() >= self.max {
            return Err(self.max);
        }
        if self.current.is_some() {
            self.pending.push_back(info);
        } else {
            self.start(info, out);
        }
        Ok(())
    }

    /// The player reported `token` playable: `TrackStart` (once).
    pub(crate) fn on_playable(&mut self, token: u64, out: &mut Out) {
        if let Some(current) = self.current.as_mut()
            && current.token == token
            && !current.started
        {
            current.started = true;
            out.notes.push(Note::Start(current.info.clone()));
        }
    }

    /// The player reported `token` ended.
    pub(crate) fn on_end(&mut self, token: u64, out: &mut Out) {
        if self.is_current(token) {
            self.end_current(EndReason::Finished, out);
            self.advance(out);
        }
    }

    /// The player reported `token` failed.
    pub(crate) fn on_error(&mut self, token: u64, error: String, out: &mut Out) {
        if !self.is_current(token) {
            return;
        }
        if let Some(current) = self.current.take() {
            out.cmds.push(Cmd::Stop(current.token));
            out.notes.push(Note::Error(current.info, error));
            out.counted.push("error");
        }
        self.advance(out);
    }

    pub(crate) fn skip(&mut self, out: &mut Out) -> Option<TrackInfo> {
        let skipped = self.end_current(EndReason::Skipped, out)?;
        self.advance(out);
        Some(skipped)
    }

    pub(crate) fn pause(&mut self, out: &mut Out) -> bool {
        match self.current.as_mut() {
            Some(current) if !current.paused => {
                current.paused = true;
                out.cmds.push(Cmd::Pause(current.token));
                true
            }
            _ => false,
        }
    }

    pub(crate) fn resume(&mut self, out: &mut Out) -> bool {
        match self.current.as_mut() {
            Some(current) if current.paused => {
                current.paused = false;
                out.cmds.push(Cmd::Resume(current.token));
                true
            }
            _ => false,
        }
    }

    /// Stops the current track and drops the pending ones; how many tracks
    /// the queue held.
    pub(crate) fn stop(&mut self, out: &mut Out) -> usize {
        let len = self.len();
        self.pending.clear();
        if self.end_current(EndReason::Stopped, out).is_some() {
            out.notes.push(Note::Empty);
        }
        len
    }

    /// Drops the pending tracks; how many.
    pub(crate) fn clear(&mut self) -> usize {
        let len = self.pending.len();
        self.pending.clear();
        len
    }

    pub(crate) fn set_volume(&mut self, volume: f32, out: &mut Out) -> Result<(), f32> {
        if !VOLUME.contains(&volume) {
            return Err(volume);
        }
        self.volume = volume;
        if let Some(current) = &self.current {
            out.cmds.push(Cmd::SetVolume(current.token, volume));
        }
        Ok(())
    }

    /// The call is gone: stops the current track without events (counted
    /// `stopped`) and drops the rest.
    pub(crate) fn shutdown(&mut self, out: &mut Out) {
        self.pending.clear();
        if let Some(current) = self.current.take() {
            out.cmds.push(Cmd::Stop(current.token));
            out.counted.push(EndReason::Stopped.as_str());
        }
    }
}

/// What a queue reports through: the App's `app` label and [`Emitter`],
/// bound when the App starts (or `app.voice()` is called), and the recorder
/// of a recording config (feature `testkit`), set when the App is built.
#[derive(Debug, Default)]
pub(crate) struct Binding {
    pub(crate) app: OnceLock<Arc<str>>,
    pub(crate) emitter: OnceLock<Emitter>,
    #[cfg(feature = "testkit")]
    pub(crate) recorder: Option<crate::recording::VoiceRecorder>,
}

/// The idle leave of a guild's call.
pub(crate) type IdleLeave = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

/// The queue and its bookkeeping, under one `std::sync::Mutex` that is
/// never held across an `.await`.
struct Locked {
    queue: Queue,
    /// Whether the call is connected (its join resolved).
    connected: bool,
    /// Whether the call was removed: every operation is refused.
    closed: bool,
    /// The armed idle timer.
    timer: Option<AbortHandle>,
}

impl Drop for Locked {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
    }
}

/// One guild's queue: the state, its player and where its events go.
pub(crate) struct QueueCell {
    guild: Id<GuildMarker>,
    locked: Mutex<Locked>,
    player: Box<dyn Player>,
    binding: Arc<Binding>,
    idle_after: Option<Duration>,
    idle_leave: Option<IdleLeave>,
}

impl QueueCell {
    pub(crate) fn new(
        guild: Id<GuildMarker>,
        max: usize,
        player: Box<dyn Player>,
        binding: Arc<Binding>,
        idle: Option<(Duration, IdleLeave)>,
    ) -> Self {
        let (idle_after, idle_leave) = idle.unzip();
        Self {
            guild,
            locked: Mutex::new(Locked {
                queue: Queue::new(max),
                connected: false,
                closed: false,
                timer: None,
            }),
            player,
            binding,
            idle_after,
            idle_leave,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Locked> {
        self.locked.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `op` on the open queue, then its commands, events, counts and
    /// the idle timer, all under the lock (so events keep their order).
    fn apply<T>(&self, op: impl FnOnce(&mut Queue, &mut Out) -> T) -> Result<T, VoiceError> {
        let mut locked = self.lock();
        if locked.closed {
            return Err(VoiceError::Interrupted { guild: self.guild });
        }
        let mut out = Out::default();
        let value = op(&mut locked.queue, &mut out);
        self.flush(out);
        self.sync_idle(&mut locked);
        Ok(value)
    }

    /// Executes `out`.
    fn flush(&self, out: Out) {
        for cmd in out.cmds {
            match cmd {
                Cmd::Play(token, info, volume) => self.player.play(token, &info, volume),
                Cmd::Pause(token) => self.player.pause(token),
                Cmd::Resume(token) => self.player.resume(token),
                Cmd::Stop(token) => self.player.stop(token),
                Cmd::SetVolume(token, volume) => self.player.set_volume(token, volume),
            }
        }
        // Recorded before the emitter check: a test sees every event, with
        // or without a hook.
        #[cfg(feature = "testkit")]
        if let Some(recorder) = &self.binding.recorder {
            recorder.record_notes(self.guild, &out.notes);
        }
        if let Some(app) = self.binding.app.get() {
            for result in out.counted {
                metrics::counter!(TRACKS_COUNTER, "app" => Arc::clone(app), "result" => result)
                    .increment(1);
            }
        }
        let Some(emitter) = self.binding.emitter.get() else {
            return;
        };
        let guild = self.guild;
        for note in out.notes {
            match note {
                Note::Start(track) => emitter.emit(TrackStart { guild, track }),
                Note::End(track, reason) => emitter.emit(TrackEnd {
                    guild,
                    track,
                    reason,
                }),
                Note::Error(track, error) => emitter.emit(TrackError {
                    guild,
                    track,
                    error,
                }),
                Note::Empty => emitter.emit(QueueEmpty { guild }),
            }
        }
    }

    /// Arms the idle timer when the connected call went idle, cancels it
    /// when something plays.
    fn sync_idle(&self, locked: &mut Locked) {
        let (Some(after), Some(leave)) = (self.idle_after, self.idle_leave.as_ref()) else {
            return;
        };
        let idle = locked.connected && !locked.closed && locked.queue.is_idle();
        if !idle {
            if let Some(timer) = locked.timer.take() {
                timer.abort();
            }
            return;
        }
        if locked.timer.is_some() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let leave = Arc::clone(leave);
        let task = runtime.spawn(async move {
            tokio::time::sleep(after).await;
            // A task of its own: removing the call aborts this timer.
            tokio::spawn(leave());
        });
        locked.timer = Some(task.abort_handle());
    }

    /// Whether the connected, open call is idle (what arms the idle timer).
    pub(crate) fn is_idle(&self) -> bool {
        let locked = self.lock();
        locked.connected && !locked.closed && locked.queue.is_idle()
    }

    /// The call connected: the idle timer may arm.
    pub(crate) fn set_connected(&self) {
        let mut locked = self.lock();
        if !locked.connected {
            locked.connected = true;
            self.sync_idle(&mut locked);
        }
    }

    /// The call was removed: stops the player (counted `stopped`, no
    /// event), drops the queue and cancels the idle timer.
    pub(crate) fn shutdown(&self) {
        let mut locked = self.lock();
        if locked.closed {
            return;
        }
        locked.closed = true;
        let mut out = Out::default();
        locked.queue.shutdown(&mut out);
        out.notes.clear();
        self.flush(out);
        if let Some(timer) = locked.timer.take() {
            timer.abort();
        }
    }

    pub(crate) fn enqueue(&self, info: TrackInfo) -> Result<TrackInfo, VoiceError> {
        let guild = self.guild;
        self.apply(|queue, out| queue.enqueue(info.clone(), out))?
            .map(|()| info)
            .map_err(|max| VoiceError::QueueFull { guild, max })
    }

    pub(crate) fn skip(&self) -> Result<Option<TrackInfo>, VoiceError> {
        self.apply(Queue::skip)
    }

    pub(crate) fn pause(&self) -> Result<bool, VoiceError> {
        self.apply(Queue::pause)
    }

    pub(crate) fn resume(&self) -> Result<bool, VoiceError> {
        self.apply(Queue::resume)
    }

    pub(crate) fn stop(&self) -> Result<usize, VoiceError> {
        self.apply(Queue::stop)
    }

    pub(crate) fn clear(&self) -> Result<usize, VoiceError> {
        self.apply(|queue, _| queue.clear())
    }

    pub(crate) fn set_volume(&self, volume: f32) -> Result<(), VoiceError> {
        self.apply(|queue, out| queue.set_volume(volume, out))?
            .map_err(|volume| VoiceError::InvalidVolume { volume })
    }

    /// Reads the open queue; `None` once closed.
    pub(crate) fn read<T>(&self, read: impl FnOnce(&Queue) -> T) -> Option<T> {
        let locked = self.lock();
        (!locked.closed).then(|| read(&locked.queue))
    }

    /// The player reported `token` playable.
    pub(crate) fn on_playable(&self, token: u64) {
        let _ = self.apply(|queue, out| queue.on_playable(token, out));
    }

    /// The player reported `token` ended.
    pub(crate) fn on_end(&self, token: u64) {
        let _ = self.apply(|queue, out| queue.on_end(token, out));
    }

    /// The player reported `token` failed with `error`.
    pub(crate) fn on_error(&self, token: u64, error: String) {
        let _ = self.apply(|queue, out| queue.on_error(token, error, out));
    }

    /// Whether the current track's `TrackStart` fired.
    #[cfg(test)]
    pub(crate) fn started(&self) -> bool {
        self.lock()
            .queue
            .current
            .as_ref()
            .is_some_and(|current| current.started)
    }

    /// Whether the idle timer is armed.
    #[cfg(test)]
    pub(crate) fn idle_armed(&self) -> bool {
        self.lock().timer.is_some()
    }
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests;
