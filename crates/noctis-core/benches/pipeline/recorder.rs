//! The process-wide recorder: push and handler-start instants per item,
//! indexed by the item's sequence number (no map keyed by a Discord id).

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tokio::time::Instant;

/// The most items one run may hold (fixed rate: 10 000/s for 6 s).
pub(crate) const MAX_ITEMS: usize = 65_536;
/// The most handlers one item may start.
pub(crate) const MAX_HANDLERS: usize = 4;
/// The most guilds the ordered scenario uses (slot 0 is the slow guild).
pub(crate) const MAX_GUILDS: usize = 10_000;

/// Push and start instants (nanoseconds since [`Recorder::epoch`] plus one,
/// so 0 means "not recorded") and the run's failure counters.
pub(crate) struct Recorder {
    epoch: Instant,
    /// The item count of the current run; a start of a larger seq is
    /// `unexpected`.
    active: AtomicUsize,
    pushes: Box<[AtomicU64]>,
    /// `seq * MAX_HANDLERS + handler`.
    starts: Box<[AtomicU64]>,
    /// Expected starts not yet recorded.
    remaining: AtomicUsize,
    unexpected: AtomicUsize,
    duplicate: AtomicUsize,
    order_violations: AtomicUsize,
    /// The last seq + 1 an ordered handler started per guild slot.
    last_by_guild: Box<[AtomicU64]>,
}

/// The failure counters of one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Failures {
    pub(crate) unexpected: usize,
    pub(crate) duplicate: usize,
    pub(crate) missing: usize,
    pub(crate) order_violations: usize,
}

impl Failures {
    pub(crate) fn any(self) -> bool {
        self != Self::default()
    }
}

fn zeroed(len: usize) -> Box<[AtomicU64]> {
    (0..len).map(|_| AtomicU64::new(0)).collect()
}

static RECORDER: LazyLock<Recorder> = LazyLock::new(|| Recorder {
    epoch: Instant::now(),
    active: AtomicUsize::new(0),
    pushes: zeroed(MAX_ITEMS),
    starts: zeroed(MAX_ITEMS * MAX_HANDLERS),
    remaining: AtomicUsize::new(0),
    unexpected: AtomicUsize::new(0),
    duplicate: AtomicUsize::new(0),
    order_violations: AtomicUsize::new(0),
    last_by_guild: zeroed(MAX_GUILDS + 1),
});

/// The process-wide recorder.
pub(crate) fn recorder() -> &'static Recorder {
    &RECORDER
}

impl Recorder {
    /// `at` as recorded nanoseconds (never 0).
    pub(crate) fn ns(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.epoch).as_nanos())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1)
    }

    /// Clears the first `items` items and expects `expected` starts.
    pub(crate) fn reset(&self, items: usize, expected: usize) {
        self.active.store(0, Ordering::SeqCst);
        for slot in &self.pushes[..items] {
            slot.store(0, Ordering::Relaxed);
        }
        for slot in &self.starts[..items * MAX_HANDLERS] {
            slot.store(0, Ordering::Relaxed);
        }
        for slot in &self.last_by_guild {
            slot.store(0, Ordering::Relaxed);
        }
        self.unexpected.store(0, Ordering::Relaxed);
        self.duplicate.store(0, Ordering::Relaxed);
        self.order_violations.store(0, Ordering::Relaxed);
        self.remaining.store(expected, Ordering::SeqCst);
        self.active.store(items, Ordering::SeqCst);
    }

    /// Records that item `seq` was pushed at `at`.
    pub(crate) fn push(&self, seq: usize, at: Instant) {
        if let Some(slot) = self.pushes.get(seq) {
            slot.store(self.ns(at), Ordering::Release);
        }
    }

    /// Records that `handler` started item `seq` at `at`.
    pub(crate) fn start(&self, seq: u64, handler: usize, at: Instant) {
        let ns = self.ns(at);
        let active = self.active.load(Ordering::Acquire);
        let slot = usize::try_from(seq)
            .ok()
            .filter(|seq| *seq < active && handler < MAX_HANDLERS)
            .and_then(|seq| self.starts.get(seq * MAX_HANDLERS + handler));
        let Some(slot) = slot else {
            self.unexpected.fetch_add(1, Ordering::AcqRel);
            return;
        };
        if slot
            .compare_exchange(0, ns, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.remaining.fetch_sub(1, Ordering::AcqRel);
        } else {
            self.duplicate.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Records that an ordered handler started item `seq` of guild slot
    /// `guild`; a seq lower than the slot's last one is a violation.
    pub(crate) fn ordered(&self, guild: usize, seq: u64) {
        let Some(last) = self.last_by_guild.get(guild) else {
            self.unexpected.fetch_add(1, Ordering::AcqRel);
            return;
        };
        let previous = last.swap(seq.saturating_add(1), Ordering::AcqRel);
        if previous > seq {
            self.order_violations.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Expected starts not yet recorded.
    pub(crate) fn remaining(&self) -> usize {
        self.remaining.load(Ordering::Acquire)
    }

    /// The recorded push of `seq` (0 when none).
    pub(crate) fn pushed(&self, seq: usize) -> u64 {
        self.pushes
            .get(seq)
            .map_or(0, |slot| slot.load(Ordering::Acquire))
    }

    /// The recorded start of `seq` by `handler` (0 when none).
    pub(crate) fn started(&self, seq: usize, handler: usize) -> u64 {
        self.starts
            .get(seq * MAX_HANDLERS + handler)
            .map_or(0, |slot| slot.load(Ordering::Acquire))
    }

    /// The run's failure counters, with `missing` expected starts.
    pub(crate) fn failures(&self) -> Failures {
        Failures {
            unexpected: self.unexpected.load(Ordering::Acquire),
            duplicate: self.duplicate.load(Ordering::Acquire),
            missing: self.remaining(),
            order_violations: self.order_violations.load(Ordering::Acquire),
        }
    }
}
