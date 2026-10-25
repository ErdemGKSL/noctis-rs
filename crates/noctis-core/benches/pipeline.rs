//! `pipeline` (Epic 16, AD-10, AD-25): event pipeline throughput, push cost
//! and latency from a shard's parsed `Inbound` to a handler start.
//!
//! A real App built by `TestApp::build` (on `Runtime::for_test()`) receives
//! pre-parsed `Inbound` values from P ∈ {1, 4, 16} producer tasks through
//! `TestApp::lane_sender()`; nothing is parsed in the timed loop. Scenarios:
//! (a) the AD-25 mix at a fixed 10 000 events/s and at saturation, (b)
//! `MESSAGE_DELETE` with 1 and 4 concurrent no-op handlers, (c) an ordered
//! handler over 1, 64 and 10 000 guilds with a slow guild sleeping 50 ms,
//! (d) a saturation flood with 1 % interactions. criterion measures the
//! saturation runs; the fixed-rate runs and every percentile are reported
//! in one markdown table at the end (`benches/PIPELINE.md` has the
//! baseline).
//!
//! Run with `cargo bench -p noctis-core --bench pipeline`; `-- --test` runs
//! every scenario once with smoke counts. Exit 0 after the table, 2 when a
//! run failed (naming the scenario and P). No gate runs this bench.

#[path = "pipeline/fixtures.rs"]
mod fixtures;
#[path = "pipeline/recorder.rs"]
mod recorder;
#[path = "pipeline/run.rs"]
mod run;
#[path = "pipeline/stats.rs"]
mod stats;

use std::fmt::Write as _;
use std::process::ExitCode;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput};
use noctis::prelude::*;
use noctis::testkit::TestApp;
use noctis_core::__testing::LaneSender;
use tokio::runtime::Runtime;

use fixtures::{
    Delete0, Delete1, Delete2, Delete3, Fixtures, Kind, OrderedDelete, SLOW_GUILD, Seq, Tap,
};
use run::{Partition, Plan, Summary};

/// The producer counts every scenario runs with.
const PRODUCERS: [usize; 3] = [1, 4, 16];
/// The fixed rate of scenarios (a) and (c).
const RATE: u64 = 10_000;
/// Items per saturation run (full and smoke).
const SATURATION_ITEMS: [usize; 2] = [20_000, 200];
/// The event lane of the saturation Apps: never drops a saturation run.
const SATURATION_LANE: (usize, usize) = (262_144, 16_384);
/// Fixed-rate warm-up and measured time (full).
const FIXED_WARM_UP: Duration = Duration::from_secs(1);
const FIXED_MEASURED: Duration = Duration::from_secs(5);
/// Fixed-rate measured time (smoke, no warm-up).
const SMOKE_MEASURED: Duration = Duration::from_millis(200);
/// criterion's settings for the saturation runs.
const SAMPLES: usize = 10;
const WARM_UP: Duration = Duration::from_secs(1);
const MEASUREMENT: Duration = Duration::from_secs(5);
/// The ordered scenario's guild counts.
const GUILD_COUNTS: [u64; 3] = [1, 64, 10_000];

fn main() -> ExitCode {
    let smoke = std::env::args().any(|arg| arg == "--test");
    match bench(smoke) {
        Ok(table) => {
            print_report(&table);
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("pipeline: {message}");
            ExitCode::from(2)
        }
    }
}

#[expect(clippy::print_stdout, reason = "the bench reports on stdout")]
fn print_report(text: &str) {
    print!("{text}");
}

/// One table row.
struct Row {
    scenario: String,
    producers: usize,
    summary: Summary,
    /// Saturation rows report push cost; fixed-rate rows the achieved rate.
    saturation: bool,
    notes: String,
}

/// The bench's shared state: the runtime, the fixtures and the rows so far.
struct Bench {
    runtime: Runtime,
    fixtures: Fixtures,
    smoke: bool,
    rows: Vec<Row>,
}

/// A running App and its tapped lane sender.
struct Running {
    app: TestApp,
    sender: LaneSender,
}

fn bench(smoke: bool) -> Result<String, String> {
    stats::self_check()?;
    fixtures::self_check()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot build the tokio runtime: {error}"))?;
    let mut bench = Bench {
        runtime,
        fixtures: Fixtures::new()?,
        smoke,
        rows: Vec::new(),
    };
    let mut criterion = Criterion::default().configure_from_args();

    // (a) the AD-25 mix: per 20 items 12 MESSAGE_CREATE, 5 MESSAGE_DELETE,
    // 2 Tap and 1 Seq, interleaved.
    let all = [Kind::Create, Kind::DELETE, Kind::Tap, Kind::Seq];
    let module = Module::new("bench")
        .add::<Delete0>()
        .add::<Tap>()
        .add::<Seq>();
    let app = bench.start("a", module, Lane::Saturation, &all, 1)?;
    bench.saturation(&mut criterion, &app, "a_mix", "(a) mix, saturation", 1, mix)?;
    bench.fixed_rate(&app, "(a) mix @ 10k/s", 1, Partition::RoundRobin, mix)?;
    bench.stop(app)?;

    // (b) MESSAGE_DELETE with 1 and 4 concurrent no-op handlers.
    let delete = |_| Kind::DELETE;
    let module = Module::new("bench").add::<Delete0>();
    let app = bench.start("b1", module, Lane::Saturation, &[Kind::DELETE], 1)?;
    let label = "(b) delete, 1 handler";
    bench.saturation(&mut criterion, &app, "b_delete_1", label, 1, delete)?;
    bench.stop(app)?;
    let module = Module::new("bench")
        .add::<Delete0>()
        .add::<Delete1>()
        .add::<Delete2>()
        .add::<Delete3>();
    let app = bench.start("b4", module, Lane::Saturation, &[Kind::DELETE], 4)?;
    let label = "(b) delete, 4 handlers";
    bench.saturation(&mut criterion, &app, "b_delete_4", label, 4, delete)?;
    bench.stop(app)?;

    // (c) one ordered handler over G guilds; every 1000th item is the slow
    // guild's.
    let module = Module::new("bench").add::<OrderedDelete>();
    let probe = [Kind::DELETE, Kind::Delete { guild: SLOW_GUILD }];
    let app = bench.start("c", module, Lane::Default, &probe, 1)?;
    for guilds in GUILD_COUNTS {
        let label = format!("(c) ordered, {guilds} guilds @ 10k/s");
        let kind = move |seq: usize| ordered(seq, guilds);
        bench.fixed_rate(&app, &label, 1, Partition::ByGuild, kind)?;
    }
    bench.stop(app)?;

    // (d) a saturation flood: 70 % MESSAGE_CREATE, 29 % MESSAGE_DELETE,
    // 1 % Tap.
    let module = Module::new("bench").add::<Delete0>().add::<Tap>();
    let probe = [Kind::Create, Kind::DELETE, Kind::Tap];
    let app = bench.start("d", module, Lane::Saturation, &probe, 1)?;
    let label = "(d) flood, 1 % interactions";
    bench.saturation(&mut criterion, &app, "d_flood", label, 1, flood)?;
    bench.stop(app)?;

    criterion.final_summary();
    Ok(bench.table())
}

/// Item `seq` of the AD-25 mix.
fn mix(seq: usize) -> Kind {
    match seq % 20 {
        0..12 => Kind::Create,
        12..17 => Kind::DELETE,
        17..19 => Kind::Tap,
        _ => Kind::Seq,
    }
}

/// Item `seq` of the flood.
fn flood(seq: usize) -> Kind {
    match seq % 100 {
        0..70 => Kind::Create,
        70..99 => Kind::DELETE,
        _ => Kind::Tap,
    }
}

/// Item `seq` of the ordered scenario over `guilds` guilds.
fn ordered(seq: usize, guilds: u64) -> Kind {
    if seq % 1000 == 999 {
        Kind::Delete { guild: SLOW_GUILD }
    } else {
        let guild = u64::try_from(seq).unwrap_or(0) % guilds + 1;
        Kind::Delete { guild }
    }
}

/// Which event lane a scenario's App gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    /// The App's default lane (fixed-rate runs).
    Default,
    /// [`SATURATION_LANE`].
    Saturation,
}

impl Bench {
    /// Builds and starts the App of scenario `name` with `module`, then
    /// pushes one item of each of `probe` and waits for its handlers (a
    /// setup check before anything is timed).
    fn start(
        &self,
        name: &str,
        module: Module,
        lane: Lane,
        probe: &[Kind],
        delete_handlers: usize,
    ) -> Result<Running, String> {
        let running = self.runtime.block_on(async {
            let mut builder = App::builder();
            builder.module(module);
            if lane == Lane::Saturation {
                builder.event_lane(SATURATION_LANE.0, SATURATION_LANE.1);
            }
            let app = TestApp::build(&mut builder)
                .await
                .map_err(|error| format!("scenario {name}: the App did not start: {error}"))?;
            let sender = app
                .lane_sender()
                .ok_or_else(|| format!("scenario {name}: the App exposes no lane sender"))?;
            Ok::<_, String>(Running { app, sender })
        })?;
        let plan = Plan {
            kinds: probe.to_vec(),
            delete_handlers,
            partition: Partition::RoundRobin,
            rate: None,
            measured_from: 0,
        };
        self.runtime
            .block_on(run::run(&running.sender, &self.fixtures, &plan, 1))
            .map_err(|error| format!("scenario {name}: setup check failed: {error}"))?;
        running.app.http_fake().clear_requests();
        Ok(running)
    }

    /// Stops a scenario's App.
    fn stop(&self, running: Running) -> Result<(), String> {
        drop(running.sender);
        self.runtime
            .block_on(running.app.stop())
            .map_err(|error| format!("the App did not stop: {error}"))
    }

    /// The criterion-measured saturation runs of one scenario at every P.
    fn saturation(
        &mut self,
        criterion: &mut Criterion,
        app: &Running,
        group: &str,
        label: &str,
        delete_handlers: usize,
        kind: impl Fn(usize) -> Kind,
    ) -> Result<(), String> {
        let items = SATURATION_ITEMS[usize::from(self.smoke)];
        let plan = Plan {
            kinds: (0..items).map(kind).collect(),
            delete_handlers,
            partition: Partition::RoundRobin,
            rate: None,
            measured_from: 0,
        };
        let mut criterion_group = criterion.benchmark_group(group);
        criterion_group
            .sample_size(SAMPLES)
            .warm_up_time(WARM_UP)
            .measurement_time(MEASUREMENT)
            .throughput(Throughput::Elements(u64::try_from(items).unwrap_or(0)));
        for producers in PRODUCERS {
            let mut runs = Vec::new();
            let mut failure = None;
            let id = BenchmarkId::from_parameter(format!("P{producers}"));
            criterion_group.bench_function(id, |b| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        if failure.is_some() {
                            break;
                        }
                        let begun = std::time::Instant::now();
                        let run = run::run(&app.sender, &self.fixtures, &plan, producers);
                        match self.runtime.block_on(run) {
                            Ok(result) => {
                                total += Duration::from_nanos(result.span_ns);
                                runs.push((begun, result));
                            }
                            Err(error) => failure = Some(error),
                        }
                        app.app.http_fake().clear_requests();
                    }
                    total.max(Duration::from_nanos(1))
                });
            });
            if let Some(error) = failure {
                return Err(format!("scenario {label}, P = {producers}: {error}"));
            }
            // criterion's warm-up is time-bounded from its first call: runs
            // begun inside it are not reported (unless nothing else ran).
            let cut = runs.first().map(|(begun, _)| *begun + WARM_UP);
            let keep_all = self.smoke || runs.iter().all(|(begun, _)| Some(*begun) < cut);
            let mut summary = Summary::default();
            for (begun, result) in runs {
                if keep_all || Some(begun) >= cut {
                    summary.add(result);
                }
            }
            self.rows.push(Row {
                scenario: label.to_owned(),
                producers,
                notes: format!("{} runs × {items} items", summary.runs),
                summary,
                saturation: true,
            });
        }
        criterion_group.finish();
        Ok(())
    }

    /// One fixed-rate run of [`RATE`] items/s per P: a warm-up second, then
    /// the measured seconds (smoke: 0.2 s, no warm-up).
    fn fixed_rate(
        &mut self,
        app: &Running,
        label: &str,
        delete_handlers: usize,
        partition: Partition,
        kind: impl Fn(usize) -> Kind,
    ) -> Result<(), String> {
        let (warm_up, measured) = if self.smoke {
            (Duration::ZERO, SMOKE_MEASURED)
        } else {
            (FIXED_WARM_UP, FIXED_MEASURED)
        };
        let count = |time: Duration| {
            usize::try_from(u128::from(RATE) * time.as_millis() / 1000).unwrap_or(usize::MAX)
        };
        let measured_from = count(warm_up);
        let items = measured_from + count(measured);
        let plan = Plan {
            kinds: (0..items).map(kind).collect(),
            delete_handlers,
            partition,
            rate: Some(RATE),
            measured_from,
        };
        for producers in PRODUCERS {
            let run = run::run(&app.sender, &self.fixtures, &plan, producers);
            let result = self
                .runtime
                .block_on(run)
                .map_err(|error| format!("scenario {label}, P = {producers}: {error}"))?;
            app.app.http_fake().clear_requests();
            let mut summary = Summary::default();
            summary.add(result);
            let achieved = stats::per_second(summary.items, summary.push_span_ns);
            let lag = stats::micros(Some(summary.max_lag_ns));
            self.rows.push(Row {
                scenario: label.to_owned(),
                producers,
                notes: format!("achieved {achieved}/s, max lag {lag} µs"),
                summary,
                saturation: false,
            });
        }
        Ok(())
    }

    /// The markdown table of every row.
    fn table(&mut self) -> String {
        let mut out = String::new();
        out.push_str(
            "| Scenario | P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |\n\
             | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n",
        );
        for row in &mut self.rows {
            let summary = &mut row.summary;
            let events = stats::percentiles(&mut summary.event_ns).map(stats::micros);
            let interactions = if summary.interaction_ns.is_empty() {
                ["—".to_owned(), "—".to_owned(), "—".to_owned()]
            } else {
                stats::percentiles(&mut summary.interaction_ns).map(stats::micros)
            };
            let push = if row.saturation && summary.pushed > 0 {
                (summary.push_loop_ns / summary.pushed).to_string()
            } else {
                "—".to_owned()
            };
            let rate = stats::per_second(summary.items, summary.span_ns);
            let _ = writeln!(
                out,
                "| {} | {} | {} | {rate} | {} | {} | {} | {} | {} | {push} | {} |",
                row.scenario,
                row.producers,
                summary.items,
                events[0],
                events[1],
                events[2],
                interactions[0],
                interactions[1],
                row.notes,
            );
        }
        out
    }
}
