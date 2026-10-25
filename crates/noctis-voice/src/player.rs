//! The Songbird [`Player`]: plays a queue's current track on the guild's
//! driver with `play_only`, so the driver never holds more than one track.

use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use songbird::Call;
use songbird::events::{Event, EventContext, EventData, EventHandler, TrackEvent};
use songbird::tracks::{PlayMode, Track as SongbirdTrack, TrackHandle};

use crate::queue::{Player, QueueCell};
use crate::track::TrackInfo;

/// The player's view of the current track: what it should be, and its
/// driver handle once it was handed to the driver.
struct Slot {
    token: u64,
    paused: bool,
    volume: f32,
    handle: Option<TrackHandle>,
}

/// Plays on one guild's Songbird `Call`.
pub(crate) struct SongbirdPlayer {
    call: Arc<tokio::sync::Mutex<Call>>,
    media: reqwest::Client,
    queue: Weak<QueueCell>,
    slot: Arc<Mutex<Slot>>,
}

impl SongbirdPlayer {
    pub(crate) fn new(
        call: Arc<tokio::sync::Mutex<Call>>,
        media: reqwest::Client,
        queue: Weak<QueueCell>,
    ) -> Self {
        Self {
            call,
            media,
            queue,
            slot: Arc::new(Mutex::new(Slot {
                token: 0,
                paused: false,
                volume: 1.0,
                handle: None,
            })),
        }
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        lock(&self.slot)
    }

    /// Runs `apply` on the handle of `token`'s track after updating the
    /// wanted state with `want`; a track not yet handed to the driver picks
    /// the state up when it is.
    fn with(&self, token: u64, want: impl FnOnce(&mut Slot), apply: impl FnOnce(&TrackHandle)) {
        let mut slot = self.slot();
        if slot.token != token {
            return;
        }
        want(&mut slot);
        if let Some(handle) = &slot.handle {
            apply(handle);
        }
    }
}

fn lock(slot: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Player for SongbirdPlayer {
    fn play(&self, token: u64, track: &TrackInfo, volume: f32) {
        {
            let mut slot = self.slot();
            if let Some(old) = slot.handle.take() {
                let _ = old.stop();
            }
            *slot = Slot {
                token,
                paused: false,
                volume,
                handle: None,
            };
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(target: "noctis::voice", "no tokio runtime: the track was not played");
            return;
        };
        let input = track.input(&self.media);
        let call = Arc::clone(&self.call);
        let slot = Arc::clone(&self.slot);
        let queue = self.queue.clone();
        // The Call's lock is awaited in a task of its own, so advancing the
        // queue never blocks a driver event task.
        runtime.spawn(async move {
            let mut call = call.lock().await;
            let mut slot = lock(&slot);
            if slot.token != token {
                return;
            }
            let mut track = SongbirdTrack::new(input).volume(slot.volume);
            if slot.paused {
                track = track.pause();
            }
            for event in [TrackEvent::Playable, TrackEvent::End, TrackEvent::Error] {
                track.events.add_event(
                    EventData::new(
                        Event::Track(event),
                        Report {
                            queue: queue.clone(),
                            token,
                            event,
                        },
                    ),
                    Duration::ZERO,
                );
            }
            slot.handle = Some(call.play_only(track));
        });
    }

    fn pause(&self, token: u64) {
        self.with(
            token,
            |slot| slot.paused = true,
            |handle| {
                let _ = handle.pause();
            },
        );
    }

    fn resume(&self, token: u64) {
        self.with(
            token,
            |slot| slot.paused = false,
            |handle| {
                let _ = handle.play();
            },
        );
    }

    fn stop(&self, token: u64) {
        let mut slot = self.slot();
        if slot.token != token {
            return;
        }
        // Token 0 is never issued: a play task still waiting drops its input.
        slot.token = 0;
        if let Some(handle) = slot.handle.take() {
            let _ = handle.stop();
        }
    }

    fn set_volume(&self, token: u64, volume: f32) {
        self.with(
            token,
            |slot| slot.volume = volume,
            |handle| {
                let _ = handle.set_volume(volume);
            },
        );
    }
}

/// Reports one driver event of `token`'s track to its queue.
pub(crate) struct Report {
    pub(crate) queue: Weak<QueueCell>,
    pub(crate) token: u64,
    pub(crate) event: TrackEvent,
}

impl Report {
    pub(crate) fn report(&self, ctx: &EventContext<'_>) {
        let Some(queue) = self.queue.upgrade() else {
            return;
        };
        match self.event {
            TrackEvent::Playable => queue.on_playable(self.token),
            TrackEvent::End => queue.on_end(self.token),
            TrackEvent::Error => {
                let error = match ctx {
                    EventContext::Track(tracks) => {
                        tracks.iter().find_map(|(state, _)| match &state.playing {
                            PlayMode::Errored(error) => Some(error.to_string()),
                            _ => None,
                        })
                    }
                    _ => None,
                };
                queue.on_error(
                    self.token,
                    error.unwrap_or_else(|| "the track failed".to_owned()),
                );
            }
            _ => {}
        }
    }
}

/// Written out as `#[async_trait]` expands it (no `async-trait`
/// dependency); the report is synchronous, so the future is ready at once.
impl EventHandler for Report {
    fn act<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        ctx: &'life1 EventContext<'life2>,
    ) -> Pin<Box<dyn Future<Output = Option<Event>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        self.report(ctx);
        Box::pin(ready(None))
    }
}

#[cfg(test)]
mod tests;
