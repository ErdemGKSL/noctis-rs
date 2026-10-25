//! `dispatch_latency` (NFR-3, AD-25, AD-28): the NFR-3 gate. Event arrival
//! (a pre-parsed `Inbound` pushed on the App's lane sender) to handler
//! start, for the AD-25 mix at a fixed 10 000 events/s from 4 producers on
//! a 4-worker runtime, reported against the `[dispatch_latency]` table of
//! `benches/budgets.toml`.
//!
//! It reuses the `pipeline` bench's fixtures, recorder, run and statistics
//! modules (`benches/pipeline/`), so both measure the same path. Full mode
//! (`cargo bench -p noctis-core --bench dispatch_latency`, `--bench` in the
//! arguments) makes 3 runs of 5 measured seconds after a 1 s warm-up each
//! and gates the median of the runs' event and interaction p99. `cargo
//! test` runs a 0.2 s smoke that prints the report and never gates.
//!
//! Exit 0 after a report (within budget, gate off, or smoke), 1 when gating
//! is on and a metric is over budget, 2 when nothing valid was measured
//! (budgets unreadable or invalid, a failed run, too few samples, or an
//! achieved rate below 9 500 events/s).

mod budget;
#[path = "../pipeline/fixtures.rs"]
#[allow(
    dead_code,
    reason = "shared with the pipeline bench, which uses the rest"
)]
mod fixtures;
#[path = "../pipeline/recorder.rs"]
#[allow(
    dead_code,
    reason = "shared with the pipeline bench, which uses the rest"
)]
mod recorder;
#[path = "../pipeline/run.rs"]
#[allow(
    dead_code,
    reason = "shared with the pipeline bench, which uses the rest"
)]
mod run;
#[path = "../pipeline/stats.rs"]
#[allow(
    dead_code,
    reason = "shared with the pipeline bench, which uses the rest"
)]
mod stats;

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use noctis::prelude::*;
use noctis::testkit::TestApp;
use tokio::runtime::Runtime;

use budget::{
    MEASURED_SECS, Measurement, PRODUCERS, RATE, RUNS, RunStats, SMOKE_MEASURED_MS, WARM_UP_SECS,
    WORKERS,
};
use fixtures::{Delete0, Fixtures, Kind, Seq, Tap};
use run::{Partition, Plan, RunResult};

/// The shared budgets file.
const BUDGETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/budgets.toml");

fn main() -> ExitCode {
    let full = budget::full_mode(std::env::args().skip(1));
    match bench(!full) {
        Ok(report) if report.failed() => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("dispatch_latency: {message}");
            ExitCode::from(2)
        }
    }
}

/// Measures, prints and returns the report; an error is the message of why
/// nothing valid was measured.
fn bench(smoke: bool) -> Result<budget::Report, String> {
    stats::self_check()?;
    let budgets = budget::load(Path::new(BUDGETS)).map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKERS)
        .enable_all()
        .build()
        .map_err(|error| format!("cannot build the tokio runtime: {error}"))?;
    let measured = measure(&runtime, smoke)?;
    drop(runtime);
    let report = budget::evaluate(&budgets, &measured, smoke).map_err(|error| error.to_string())?;
    let text = report.markdown();
    print_report(&text);
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&summary)
            .and_then(|mut file| file.write_all(text.as_bytes()));
        if let Err(error) = appended {
            eprintln!(
                "dispatch_latency: warning: cannot append the report to {}: {error}",
                Path::new(&summary).display()
            );
        }
    }
    Ok(report)
}

#[expect(clippy::print_stdout, reason = "the bench reports on stdout")]
fn print_report(text: &str) {
    print!("{text}");
}

/// Item `seq` of the AD-25 mix: per 20 items 12 `MESSAGE_CREATE` (no
/// handler), 5 handled `MESSAGE_DELETE`, 2 `Tap` clicks (inline Payload)
/// and 1 `/seq` command, interleaved (the `pipeline` bench's `mix`).
fn mix(seq: usize) -> Kind {
    match seq % 20 {
        0..12 => Kind::Create,
        12..17 => Kind::DELETE,
        17..19 => Kind::Tap,
        _ => Kind::Seq,
    }
}

/// Items pushed at [`RATE`] over `time`.
fn count(time: Duration) -> usize {
    usize::try_from(u128::from(RATE) * time.as_millis() / 1000).unwrap_or(usize::MAX)
}

/// Starts the App, checks each item kind once, then makes the runs.
fn measure(runtime: &Runtime, smoke: bool) -> Result<Measurement, String> {
    let fixtures = Fixtures::new()?;
    let (app, sender) = runtime.block_on(async {
        let mut builder = App::builder();
        builder.module(
            Module::new("bench")
                .add::<Delete0>()
                .add::<Tap>()
                .add::<Seq>(),
        );
        let app = TestApp::build(&mut builder)
            .await
            .map_err(|error| format!("the App did not start: {error}"))?;
        let sender = app
            .lane_sender()
            .ok_or_else(|| "the App exposes no lane sender".to_owned())?;
        Ok::<_, String>((app, sender))
    })?;
    let probe = Plan {
        kinds: vec![Kind::Create, Kind::DELETE, Kind::Tap, Kind::Seq],
        delete_handlers: 1,
        partition: Partition::RoundRobin,
        rate: None,
        measured_from: 0,
    };
    runtime
        .block_on(run::run(&sender, &fixtures, &probe, 1))
        .map_err(|error| format!("setup check failed: {error}"))?;
    app.http_fake().clear_requests();

    let (runs, warm_up, measured) = if smoke {
        (1, Duration::ZERO, Duration::from_millis(SMOKE_MEASURED_MS))
    } else {
        (
            RUNS,
            Duration::from_secs(WARM_UP_SECS),
            Duration::from_secs(MEASURED_SECS),
        )
    };
    let measured_from = count(warm_up);
    let items = measured_from + count(measured);
    let plan = Plan {
        kinds: (0..items).map(mix).collect(),
        delete_handlers: 1,
        partition: Partition::RoundRobin,
        rate: Some(RATE),
        measured_from,
    };
    let mut measurement = Measurement::default();
    for index in 0..runs {
        let result = runtime
            .block_on(run::run(&sender, &fixtures, &plan, PRODUCERS))
            .map_err(|error| format!("run {} of {runs}: {error}", index + 1))?;
        app.http_fake().clear_requests();
        measurement.runs.push(run_stats(result));
    }
    drop(sender);
    runtime
        .block_on(app.stop())
        .map_err(|error| format!("the App did not stop: {error}"))?;
    Ok(measurement)
}

/// The percentiles, achieved rate and lag of one run.
fn run_stats(mut result: RunResult) -> RunStats {
    let events = stats::percentiles(&mut result.event_ns);
    let interactions = stats::percentiles(&mut result.interaction_ns);
    RunStats {
        event_p50_ns: events[0],
        event_p99_ns: events[1],
        event_p999_ns: events[2],
        interaction_p50_ns: interactions[0],
        interaction_p99_ns: interactions[1],
        interaction_p999_ns: interactions[2],
        achieved_per_s: stats::per_second(result.items, result.push_span_ns),
        max_lag_ns: result.max_lag_ns,
    }
}
