//! The `[dispatch_latency]` table of `benches/budgets.toml`, the run
//! validity rules and the report the bench prints against it (NFR-3,
//! AD-25, AD-28). Pure except [`load`].

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The fixed rate every run pushes at, in items per second.
pub(crate) const RATE: u64 = 10_000;
/// The runtime's worker threads ("4 cores").
pub(crate) const WORKERS: usize = 4;
/// The producer tasks of a run (one per worker).
pub(crate) const PRODUCERS: usize = 4;
/// Measured runs in full mode; the gated value is the median of their p99s.
pub(crate) const RUNS: usize = 3;
/// Warm-up and measured seconds of a full-mode run.
pub(crate) const WARM_UP_SECS: u64 = 1;
pub(crate) const MEASURED_SECS: u64 = 5;
/// Measured milliseconds of the smoke run (one run, no warm-up).
pub(crate) const SMOKE_MEASURED_MS: u64 = 200;
/// The lowest achieved push rate of a valid run: 95 % of [`RATE`].
pub(crate) const MIN_RATE: u64 = 9_500;

/// Why the budgets cannot be read.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BudgetError {
    /// The budgets file cannot be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The budgets file is not valid TOML or breaks the schema.
    #[error("cannot parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    /// A budget of 0 µs can never pass.
    #[error("[dispatch_latency.{metric}] `max` must be at least 1 µs")]
    ZeroMax { metric: &'static str },
}

/// Why a measured run is not a valid NFR-3 measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RunError {
    /// No run was measured.
    #[error("no run was measured")]
    NoRuns,
    /// A gated p99 has fewer than 100 samples (`run` counts from 1).
    #[error("run {run}: fewer than 100 samples for {metric}")]
    TooFewSamples { metric: &'static str, run: usize },
    /// The run pushed below [`MIN_RATE`] (`run` counts from 1).
    #[error("run {run}: achieved {achieved} events/s, below the {MIN_RATE}/s a valid run needs")]
    RateTooLow { achieved: u64, run: usize },
}

/// The whole budgets file: only `[dispatch_latency]` is read, every other
/// top-level table (another bench's) is ignored.
#[derive(Debug, Deserialize)]
struct BudgetsFile {
    dispatch_latency: LatencyBudgets,
}

/// The `[dispatch_latency]` table.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LatencyBudgets {
    /// The requirement id (`NFR-3`).
    pub(crate) nfr: String,
    /// Whether an over-budget metric fails the run.
    pub(crate) gate: bool,
    /// Event arrival to handler start, p99 of every handled event.
    pub(crate) event_p99_us: MetricBudget,
    /// Event arrival to handler start, p99 of every interaction.
    pub(crate) interaction_p99_us: MetricBudget,
}

/// One metric's budget, in microseconds.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetricBudget {
    /// The budget; `None` reports the value only.
    pub(crate) max: Option<u64>,
    /// Shown in the report's notes.
    pub(crate) note: String,
}

/// Reads the budgets file at `path`.
///
/// # Errors
///
/// [`BudgetError::Read`], or [`parse`]'s errors.
pub(crate) fn load(path: &Path) -> Result<LatencyBudgets, BudgetError> {
    let text = std::fs::read_to_string(path).map_err(|source| BudgetError::Read {
        path: path.to_owned(),
        source,
    })?;
    parse(&text, path)
}

/// Parses the budgets file `text` (read from `path`).
///
/// # Errors
///
/// [`BudgetError::Parse`] for invalid TOML or schema,
/// [`BudgetError::ZeroMax`] for a `max = 0`.
pub(crate) fn parse(text: &str, path: &Path) -> Result<LatencyBudgets, BudgetError> {
    let file: BudgetsFile = toml::from_str(text).map_err(|source| BudgetError::Parse {
        path: path.to_owned(),
        source,
    })?;
    let budgets = file.dispatch_latency;
    for (metric, budget) in [
        ("event_p99_us", &budgets.event_p99_us),
        ("interaction_p99_us", &budgets.interaction_p99_us),
    ] {
        if budget.max == Some(0) {
            return Err(BudgetError::ZeroMax { metric });
        }
    }
    Ok(budgets)
}

/// Whether the bench runs in full mode: `cargo bench` passes `--bench`;
/// `cargo test` (no argument, `--test` or anything else) runs the smoke.
pub(crate) fn full_mode(args: impl Iterator<Item = String>) -> bool {
    let mut args = args;
    args.any(|arg| arg == "--bench")
}

/// The median of three values (the middle one after sorting).
pub(crate) fn median3(values: [u64; 3]) -> u64 {
    let mut values = values;
    values.sort_unstable();
    values[1]
}

/// The median of `values`: the middle value after sorting, the lower
/// middle one for an even count; `None` when empty.
pub(crate) fn median(values: &[u64]) -> Option<u64> {
    if let Ok(three) = <[u64; 3]>::try_from(values) {
        return Some(median3(three));
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len().checked_sub(1)? / 2).copied()
}

/// The statistics of one measured run, latencies in nanoseconds (`None`
/// below the rank's sample minimum: 100 for p99, 1000 for p99.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunStats {
    pub(crate) event_p50_ns: Option<u64>,
    pub(crate) event_p99_ns: Option<u64>,
    pub(crate) event_p999_ns: Option<u64>,
    pub(crate) interaction_p50_ns: Option<u64>,
    pub(crate) interaction_p99_ns: Option<u64>,
    pub(crate) interaction_p999_ns: Option<u64>,
    /// Measured items ÷ the measured push span, per second.
    pub(crate) achieved_per_s: u64,
    /// The fixed-rate schedule's largest lag.
    pub(crate) max_lag_ns: u64,
}

/// Every measured run of one bench invocation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Measurement {
    pub(crate) runs: Vec<RunStats>,
}

/// Checks that run `index` (from 0) measured the NFR-3 load: both gated
/// p99s have samples and the achieved rate is at least [`MIN_RATE`].
///
/// # Errors
///
/// [`RunError::TooFewSamples`] or [`RunError::RateTooLow`], naming the run
/// from 1.
pub(crate) fn check_run(stats: &RunStats, index: usize) -> Result<(), RunError> {
    let run = index + 1;
    if stats.event_p99_ns.is_none() {
        return Err(RunError::TooFewSamples {
            metric: "event_p99_us",
            run,
        });
    }
    if stats.interaction_p99_ns.is_none() {
        return Err(RunError::TooFewSamples {
            metric: "interaction_p99_us",
            run,
        });
    }
    if stats.achieved_per_s < MIN_RATE {
        return Err(RunError::RateTooLow {
            achieved: stats.achieved_per_s,
            run,
        });
    }
    Ok(())
}

/// How a metric compares with its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Within its `max` (equal is within).
    Ok,
    /// Above its `max`.
    Over,
    /// No `max`, or a report-only row.
    Reported,
}

/// One report row: a latency in nanoseconds against an optional budget in
/// microseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) metric: &'static str,
    pub(crate) measured_ns: Option<u64>,
    pub(crate) max_us: Option<u64>,
    pub(crate) status: Status,
}

impl Row {
    /// A gated row: `measured > max × 1000` is over, equal is within.
    fn gated(metric: &'static str, measured_ns: Option<u64>, max_us: Option<u64>) -> Self {
        let status = match (measured_ns, max_us) {
            (Some(measured), Some(max)) if measured > max.saturating_mul(1000) => Status::Over,
            (Some(_), Some(_)) => Status::Ok,
            _ => Status::Reported,
        };
        Self {
            metric,
            measured_ns,
            max_us,
            status,
        }
    }

    /// A report-only row.
    fn reported(metric: &'static str, measured_ns: Option<u64>) -> Self {
        Self {
            metric,
            measured_ns,
            max_us: None,
            status: Status::Reported,
        }
    }
}

/// The row names of the gated metrics.
pub(crate) const EVENT_P99: &str = "Event p99";
pub(crate) const INTERACTION_P99: &str = "Interaction p99";

/// The evaluated measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) nfr: String,
    pub(crate) gate: bool,
    pub(crate) smoke: bool,
    pub(crate) rows: Vec<Row>,
    /// The notes lines, without their leading `- `.
    pub(crate) notes: Vec<String>,
}

/// The median over `runs` of the value `pick` takes from each run, over the
/// runs where it has one.
fn median_of(runs: &[RunStats], pick: impl Fn(&RunStats) -> Option<u64>) -> Option<u64> {
    let values: Vec<u64> = runs.iter().filter_map(pick).collect();
    median(&values)
}

/// Compares `measured` with `budgets`: the gated values are the median of
/// the runs' event and interaction p99; the median p50 and p99.9, the
/// lowest achieved rate and the worst schedule lag are reported only.
///
/// In smoke mode nothing gates, and a run with too few samples or too low a
/// rate becomes a note line instead of an error.
///
/// # Errors
///
/// [`RunError::NoRuns`], or (full mode) the first [`check_run`] error.
pub(crate) fn evaluate(
    budgets: &LatencyBudgets,
    measured: &Measurement,
    smoke: bool,
) -> Result<Report, RunError> {
    let runs = &measured.runs;
    if runs.is_empty() {
        return Err(RunError::NoRuns);
    }
    let mut notes = Vec::new();
    for (index, run) in runs.iter().enumerate() {
        match check_run(run, index) {
            Ok(()) => {}
            Err(error) if smoke => notes.push(format!("smoke: {error}")),
            Err(error) => return Err(error),
        }
    }
    let rows = vec![
        Row::gated(
            EVENT_P99,
            median_of(runs, |run| run.event_p99_ns),
            budgets.event_p99_us.max,
        ),
        Row::gated(
            INTERACTION_P99,
            median_of(runs, |run| run.interaction_p99_ns),
            budgets.interaction_p99_us.max,
        ),
        Row::reported("Event p50", median_of(runs, |run| run.event_p50_ns)),
        Row::reported("Event p99.9", median_of(runs, |run| run.event_p999_ns)),
        Row::reported(
            "Interaction p50",
            median_of(runs, |run| run.interaction_p50_ns),
        ),
        Row::reported(
            "Interaction p99.9",
            median_of(runs, |run| run.interaction_p999_ns),
        ),
    ];
    let mut lines = vec![
        format!("{EVENT_P99}: {}", budgets.event_p99_us.note),
        format!("{INTERACTION_P99}: {}", budgets.interaction_p99_us.note),
        format!(
            "Per-run p99 µs: events {}; interactions {}",
            list(runs, |run| run.event_p99_ns),
            list(runs, |run| run.interaction_p99_ns)
        ),
        format!(
            "Achieved rate: lowest {} events/s (a valid run needs {MIN_RATE}); worst schedule lag {} µs",
            runs.iter().map(|run| run.achieved_per_s).min().unwrap_or(0),
            micros(runs.iter().map(|run| run.max_lag_ns).max())
        ),
    ];
    lines.append(&mut notes);
    Ok(Report {
        nfr: budgets.nfr.clone(),
        gate: budgets.gate,
        smoke,
        rows,
        notes: lines,
    })
}

/// The per-run values `pick` takes, in µs, comma-separated.
fn list(runs: &[RunStats], pick: impl Fn(&RunStats) -> Option<u64>) -> String {
    runs.iter()
        .map(|run| micros(pick(run)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Nanoseconds as microseconds with one decimal (truncated), or `n/a`.
pub(crate) fn micros(ns: Option<u64>) -> String {
    ns.map_or_else(
        || "n/a".to_owned(),
        |ns| format!("{}.{}", ns / 1000, ns % 1000 / 100),
    )
}

impl Report {
    /// Whether the run fails: full mode, gating on and a metric over budget.
    pub(crate) fn failed(&self) -> bool {
        !self.smoke && self.gate && self.rows.iter().any(|row| row.status == Status::Over)
    }

    /// The status cell of `status`.
    fn label(&self, status: Status) -> &'static str {
        match status {
            Status::Ok => "ok",
            Status::Reported => "reported",
            Status::Over if self.smoke => "over (smoke: not gated)",
            Status::Over if self.gate => "over",
            Status::Over => "over (report only)",
        }
    }

    /// The Markdown report (also the GitHub step summary); ends with one
    /// newline.
    pub(crate) fn markdown(&self) -> String {
        let mut out = String::new();
        // Writing to a String cannot fail.
        let _ = writeln!(out, "## Dispatch latency ({})\n", self.nfr);
        if self.smoke {
            let _ = writeln!(
                out,
                "smoke: not gated (AD-25 mix, {} events/s, {WORKERS} workers, {PRODUCERS} producers, 1 × {SMOKE_MEASURED_MS} ms, no warm-up)\n",
                thousands(RATE)
            );
        } else {
            let gate = if self.gate {
                "on (over-budget rows fail the run)"
            } else {
                "off (report only)"
            };
            let _ = writeln!(
                out,
                "AD-25 mix, {} events/s, {WORKERS} workers, {PRODUCERS} producers, {RUNS} × {MEASURED_SECS} s after {WARM_UP_SECS} s warm-up; gated value: median of the runs' p99; gate: {gate}\n",
                thousands(RATE)
            );
        }
        out.push_str("| Metric | Measured µs | Budget µs | Status |\n|---|---|---|---|\n");
        for row in &self.rows {
            let budget = row
                .max_us
                .map_or_else(|| "-".to_owned(), |max| format!("<= {max}"));
            let _ = writeln!(
                out,
                "| {} | {} | {budget} | {} |",
                row.metric,
                micros(row.measured_ns),
                self.label(row.status)
            );
        }
        out.push_str("\nNotes:\n");
        for note in &self.notes {
            let _ = writeln!(out, "- {note}");
        }
        out
    }
}

/// `n` with a space every three digits (`10 000`).
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(digit);
    }
    out
}
