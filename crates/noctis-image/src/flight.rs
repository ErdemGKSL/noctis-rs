//! Process-local single-flight deduplication of renders (FR-36).
//!
//! Every in-flight render is one [`FlightFuture`] (a [`Shared`] future),
//! held weakly in [`Flights`] under its [`RenderKey`]. A request whose key
//! has a live flight joins it instead of rendering again. The last caller
//! to drop its future drops the flight: a queued job is skipped and
//! pending fetches are aborted. A [`FlightGuard`] in the flight removes its
//! map entry when the flight completes or is abandoned.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures_util::future::{FutureExt, Shared, WeakShared};
use noctis_ports::{BoxFuture, RenderError, RenderOutput};

use crate::cache::RenderKey;

/// A flight's result; the error is shared by every waiter.
pub(crate) type FlightResult = Result<RenderOutput, Arc<RenderError>>;
/// The future a flight runs.
type FlightBody = BoxFuture<'static, FlightResult>;
/// One in-flight render, shared by every caller of its key.
pub(crate) type FlightFuture = Shared<FlightBody>;

/// The in-flight renders by key, with the generation that started each.
type Map = HashMap<RenderKey, (u64, WeakShared<FlightBody>)>;

/// The in-flight renders of one Renderer. The map holds only in-flight
/// keys, weakly; its lock is never held across an `.await`, nor while a
/// flight is dropped.
#[derive(Default)]
pub(crate) struct Flights {
    map: Arc<Mutex<Map>>,
    next: AtomicU64,
}

/// Locks `map`; nothing panics while holding it, so a poisoned map is
/// still consistent.
fn lock(map: &Mutex<Map>) -> MutexGuard<'_, Map> {
    map.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Flights {
    /// The live flight of `key`, joined; or a new one, whose body `start`
    /// builds around the [`FlightGuard`] it must own.
    pub(crate) fn join(
        &self,
        key: RenderKey,
        start: impl FnOnce(FlightGuard) -> FlightBody,
    ) -> FlightFuture {
        if let Some(flight) = self.live(key) {
            return flight;
        }
        let generation = self.next.fetch_add(1, Ordering::Relaxed);
        let guard = FlightGuard {
            map: Arc::clone(&self.map),
            key,
            generation,
        };
        let flight = start(guard).shared();
        let raced = {
            let mut map = lock(&self.map);
            let theirs = map.get(&key).and_then(|(_, weak)| weak.upgrade());
            if theirs.is_none()
                && let Some(weak) = flight.downgrade()
            {
                map.insert(key, (generation, weak));
            }
            theirs
        };
        // Another caller started the same key meanwhile: join theirs and
        // drop ours outside the lock (its guard locks the map).
        raced.unwrap_or(flight)
    }

    /// The live flight of `key`, if any; the clone leaves the lock first.
    fn live(&self, key: RenderKey) -> Option<FlightFuture> {
        let map = lock(&self.map);
        map.get(&key).and_then(|(_, weak)| weak.upgrade())
    }

    /// The number of keys in flight.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        lock(&self.map).len()
    }
}

impl std::fmt::Debug for Flights {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flights")
            .field("in_flight", &lock(&self.map).len())
            .finish_non_exhaustive()
    }
}

/// Owned by a flight's body: removes the flight's map entry when the body
/// is dropped (completed or abandoned), unless a newer flight of the same
/// key replaced it.
pub(crate) struct FlightGuard {
    map: Arc<Mutex<Map>>,
    key: RenderKey,
    generation: u64,
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        let mut map = lock(&self.map);
        if map
            .get(&self.key)
            .is_some_and(|(generation, _)| *generation == self.generation)
        {
            map.remove(&self.key);
        }
    }
}

impl std::fmt::Debug for FlightGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlightGuard")
            .field("key", &self.key)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// A waiter's own result: the error itself when this waiter holds its last
/// reference, else a private copy ([`share_error`]).
pub(crate) fn unshare(result: FlightResult) -> Result<RenderOutput, RenderError> {
    result.map_err(|err| Arc::try_unwrap(err).unwrap_or_else(|err| share_error(&err)))
}

/// A copy of `err` for another waiter: the same variant and fields, except
/// that `Failed` (whose source cannot be cloned) and any unknown variant
/// become `Failed` with the same message.
pub(crate) fn share_error(err: &RenderError) -> RenderError {
    match err {
        RenderError::UnknownTemplate { id, version } => RenderError::UnknownTemplate {
            id: id.clone(),
            version: *version,
        },
        RenderError::Unsupported { id, why } => RenderError::Unsupported {
            id: id.clone(),
            why: why.clone(),
        },
        RenderError::OverBudget { budget, smallest } => RenderError::OverBudget {
            budget: *budget,
            smallest: *smallest,
        },
        RenderError::OverCap {
            id,
            cap,
            limit,
            actual,
        } => RenderError::OverCap {
            id: id.clone(),
            cap: *cap,
            limit: *limit,
            actual: *actual,
        },
        RenderError::SizeMismatch {
            id,
            frame,
            expected,
            declared,
        } => RenderError::SizeMismatch {
            id: id.clone(),
            frame: *frame,
            expected: *expected,
            declared: *declared,
        },
        RenderError::QueueFull => RenderError::QueueFull,
        RenderError::NoRenderer => RenderError::NoRenderer,
        RenderError::Panicked { message } => RenderError::Panicked {
            message: message.clone(),
        },
        RenderError::NoFont => RenderError::NoFont,
        RenderError::UnknownImage { id, image } => RenderError::UnknownImage {
            id: id.clone(),
            image: image.clone(),
        },
        RenderError::RemoteImage { id, image, reason } => RenderError::RemoteImage {
            id: id.clone(),
            image: image.clone(),
            reason: reason.clone(),
        },
        RenderError::UndeclaredImage { id, image } => RenderError::UndeclaredImage {
            id: id.clone(),
            image: image.clone(),
        },
        RenderError::Failed(source) => RenderError::failed(source.to_string()),
        other => RenderError::failed(other.to_string()),
    }
}

#[cfg(test)]
mod tests;
