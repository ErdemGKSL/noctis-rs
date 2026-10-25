//! One timed run: P producer tasks push pre-built items into the App's
//! lanes, saturating or at a fixed rate, and the recorder's instants become
//! throughput, push cost and latency samples.

use std::time::Duration;

use noctis_core::__testing::{Inbound, InboundSink as _, LaneSender};
use tokio::time::Instant;
use twilight_model::gateway::ShardId;

use crate::fixtures::{Fixtures, Kind, SLOW_GUILD};
use crate::recorder::{Failures, MAX_ITEMS, recorder};

/// How long a run may wait for its expected handler starts once every
/// producer has finished.
const RUN_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the run end is polled.
const POLL: Duration = Duration::from_millis(1);
/// The fixed-rate schedule starts this long after the producers are spawned.
const LEAD: Duration = Duration::from_millis(5);

/// How items are split over the producers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Partition {
    /// Item `seq` goes to producer `seq mod P`.
    RoundRobin,
    /// A guild's items all go to one producer (one shard per guild):
    /// guild `g` to producer `(g − 1) mod P`, the slow guild to guild 1's.
    ByGuild,
}

/// What one run pushes.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// The kind of item `seq`.
    pub(crate) kinds: Vec<Kind>,
    /// Handlers each `MESSAGE_DELETE` starts.
    pub(crate) delete_handlers: usize,
    pub(crate) partition: Partition,
    /// `None` saturates; `Some(rate)` pushes item `seq` at
    /// `t0 + seq / rate`.
    pub(crate) rate: Option<u64>,
    /// Items before this seq are warm-up: pushed and checked, not measured.
    pub(crate) measured_from: usize,
}

impl Plan {
    fn starts(&self, kind: Kind) -> usize {
        match kind {
            Kind::Create => 0,
            Kind::Delete { .. } => self.delete_handlers,
            Kind::Tap | Kind::Seq => 1,
        }
    }

    fn producer(&self, seq: usize, kind: Kind, producers: usize) -> usize {
        match (self.partition, kind) {
            (Partition::ByGuild, Kind::Delete { guild }) => {
                let guild = if guild == SLOW_GUILD { 1 } else { guild };
                usize::try_from(guild - 1).unwrap_or(0) % producers
            }
            _ => seq % producers,
        }
    }
}

/// What one run measured.
#[derive(Debug, Default)]
pub(crate) struct RunResult {
    /// Measured items (handled or not).
    pub(crate) items: u64,
    /// First measured push to the last measured handler start.
    pub(crate) span_ns: u64,
    /// First measured push to the last push.
    pub(crate) push_span_ns: u64,
    /// Producer time inside the push loops, summed.
    pub(crate) push_loop_ns: u64,
    /// Items pushed (warm-up included).
    pub(crate) pushed: u64,
    /// Push → start of every measured event handler start.
    pub(crate) event_ns: Vec<u64>,
    /// Push → start of every measured interaction handler start.
    pub(crate) interaction_ns: Vec<u64>,
    /// The fixed-rate schedule's largest lag.
    pub(crate) max_lag_ns: u64,
}

/// What one producer measured.
struct ProducerStats {
    loop_ns: u64,
    max_lag_ns: u64,
}

/// Runs `plan` with `producers` producer tasks pushing into `sender`.
///
/// # Errors
///
/// A fixture that cannot be built, or the run's failures (unexpected,
/// duplicate or missing starts, order violations).
pub(crate) async fn run(
    sender: &LaneSender,
    fixtures: &Fixtures,
    plan: &Plan,
    producers: usize,
) -> Result<RunResult, String> {
    let items = plan.kinds.len();
    if items == 0 || items > MAX_ITEMS || producers == 0 {
        return Err(format!("a run needs 1..={MAX_ITEMS} items and a producer"));
    }
    let mut kinds = plan.kinds.clone();
    // Every producer ends on a handled MESSAGE_DELETE (the sentinel), so the
    // run end covers its trailing unhandled items.
    let mut last = vec![None; producers];
    for (seq, kind) in kinds.iter().enumerate() {
        last[plan.producer(seq, *kind, producers)] = Some(seq);
    }
    for seq in last.into_iter().flatten() {
        if plan.starts(kinds[seq]) == 0 {
            kinds[seq] = Kind::DELETE;
        }
    }
    let total = u32::try_from(producers).map_err(|error| error.to_string())?;
    let mut queues: Vec<Vec<(usize, Inbound)>> = (0..producers).map(|_| Vec::new()).collect();
    for (seq, kind) in kinds.iter().enumerate() {
        let producer = plan.producer(seq, *kind, producers);
        let shard = ShardId::new(u32::try_from(producer).unwrap_or(0), total);
        let id = u64::try_from(seq).map_err(|error| error.to_string())?;
        queues[producer].push((seq, fixtures.inbound(id, *kind, shard)?));
    }
    let expected = kinds.iter().map(|kind| plan.starts(*kind)).sum();
    recorder().reset(items, expected);

    let pacing = plan
        .rate
        .map(|rate| (Instant::now() + LEAD, 1_000_000_000 / rate.max(1)));
    let tasks: Vec<_> = queues
        .into_iter()
        .filter(|queue| !queue.is_empty())
        .map(|queue| tokio::spawn(produce(sender.clone(), queue, pacing)))
        .collect();
    let mut push_loop_ns = 0;
    let mut max_lag_ns = 0;
    for task in tasks {
        let stats = task
            .await
            .map_err(|error| format!("a producer failed: {error}"))?;
        push_loop_ns += stats.loop_ns;
        max_lag_ns = max_lag_ns.max(stats.max_lag_ns);
    }
    let deadline = Instant::now() + RUN_TIMEOUT;
    while recorder().remaining() > 0 && Instant::now() < deadline {
        tokio::time::sleep(POLL).await;
    }
    let failures = recorder().failures();
    if failures.any() {
        return Err(describe(failures));
    }
    Ok(collect(plan, &kinds, push_loop_ns, max_lag_ns))
}

/// The failure message of a run.
fn describe(failures: Failures) -> String {
    let Failures {
        unexpected,
        duplicate,
        missing,
        order_violations,
    } = failures;
    format!(
        "run failed: unexpected = {unexpected}, duplicate = {duplicate}, missing = {missing}, \
         order_violations = {order_violations}"
    )
}

/// Turns the recorder's instants into a [`RunResult`].
fn collect(plan: &Plan, kinds: &[Kind], push_loop_ns: u64, max_lag_ns: u64) -> RunResult {
    let rec = recorder();
    let from = plan.measured_from.min(kinds.len());
    let first_push = (from..kinds.len())
        .map(|seq| rec.pushed(seq))
        .min()
        .unwrap_or(0);
    let last_push = (0..kinds.len())
        .map(|seq| rec.pushed(seq))
        .max()
        .unwrap_or(0);
    let mut result = RunResult {
        items: u64::try_from(kinds.len() - from).unwrap_or(0),
        push_span_ns: last_push.saturating_sub(first_push),
        push_loop_ns,
        pushed: u64::try_from(kinds.len()).unwrap_or(0),
        max_lag_ns,
        ..RunResult::default()
    };
    let mut last_start = first_push;
    for (seq, kind) in kinds.iter().enumerate().skip(from) {
        let pushed = rec.pushed(seq);
        for handler in 0..plan.starts(*kind) {
            let started = rec.started(seq, handler);
            last_start = last_start.max(started);
            if kind.is_slow() {
                continue;
            }
            let latency = started.saturating_sub(pushed);
            if kind.is_interaction() {
                result.interaction_ns.push(latency);
            } else {
                result.event_ns.push(latency);
            }
        }
    }
    result.span_ns = last_start.saturating_sub(first_push);
    result
}

/// Pushes `queue` in order, each item stamped with its push instant; with
/// `pacing = (t0, period)` item `seq` waits for `t0 + seq · period` (every
/// overdue item is pushed at once).
async fn produce(
    sender: LaneSender,
    queue: Vec<(usize, Inbound)>,
    pacing: Option<(Instant, u64)>,
) -> ProducerStats {
    let rec = recorder();
    let mut max_lag_ns = 0;
    let begin = Instant::now();
    for (seq, mut inbound) in queue {
        if let Some((t0, period)) = pacing {
            let offset = u64::try_from(seq)
                .unwrap_or(u64::MAX)
                .saturating_mul(period);
            let deadline = t0 + Duration::from_nanos(offset);
            if Instant::now() < deadline {
                tokio::time::sleep_until(deadline).await;
            }
            let lag = Instant::now().saturating_duration_since(deadline);
            max_lag_ns = max_lag_ns.max(u64::try_from(lag.as_nanos()).unwrap_or(u64::MAX));
        }
        let now = Instant::now();
        inbound.received_at = now;
        rec.push(seq, now);
        sender.push(inbound);
    }
    ProducerStats {
        loop_ns: u64::try_from(begin.elapsed().as_nanos()).unwrap_or(u64::MAX),
        max_lag_ns,
    }
}

/// Several runs of one scenario × P, summed.
#[derive(Debug, Default)]
pub(crate) struct Summary {
    pub(crate) runs: usize,
    pub(crate) items: u64,
    pub(crate) span_ns: u64,
    pub(crate) push_span_ns: u64,
    pub(crate) push_loop_ns: u64,
    pub(crate) pushed: u64,
    pub(crate) event_ns: Vec<u64>,
    pub(crate) interaction_ns: Vec<u64>,
    pub(crate) max_lag_ns: u64,
}

impl Summary {
    pub(crate) fn add(&mut self, run: RunResult) {
        self.runs += 1;
        self.items += run.items;
        self.span_ns += run.span_ns;
        self.push_span_ns += run.push_span_ns;
        self.push_loop_ns += run.push_loop_ns;
        self.pushed += run.pushed;
        self.event_ns.extend(run.event_ns);
        self.interaction_ns.extend(run.interaction_ns);
        self.max_lag_ns = self.max_lag_ns.max(run.max_lag_ns);
    }
}
