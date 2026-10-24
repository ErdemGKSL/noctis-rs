//! The reference card bench (NFR-3, AD-25): p50/p99 render latency of a
//! 934x282 PNG profile card with warm caches on a saturated render pool,
//! reported against `crates/noctis-core/benches/budgets.toml`.
//!
//! - `cargo bench -p noctis-image --features render --bench reference_card`
//!   (full mode, `--bench` in the arguments): 100 untimed warm-up renders,
//!   then `T` closed-loop clients (`T` = the pool's threads) take 1000 timed
//!   samples, each with distinct data so no render hits the render cache or
//!   joins a single flight. A sample is the wall time of
//!   `Renderer::render(job).await` alone.
//! - `cargo test` (smoke mode): the percentile and budget parser
//!   self-checks, the real `budgets.toml`, and three distinct renders.
//!
//! Over budget the bench prints a report-only line and exits 0, unless the
//! table sets `gate = true` (then it exits 1).

#![allow(clippy::print_stdout, reason = "a bench reports on stdout")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a bench panics on a broken setup or a failed render"
)]

#[path = "reference_card/card.rs"]
mod card;

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use noctis_core::App;
use noctis_image::ImageConfig;
use noctis_ports::{OutputFormat, RenderJob, Renderer};
use tokio::task::JoinSet;

/// The budgets file, shared by every Noctis bench.
const BUDGETS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../noctis-core/benches/budgets.toml"
);
/// The bench's table in [`BUDGETS`].
const TABLE: &str = "reference_card";
const WARMUP: u32 = 100;
const SAMPLES: u32 = 1000;
const WARMUP_XP: u32 = 1_000;
const TIMED_XP: u32 = 2_000;

/// The `[reference_card]` budgets, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Budgets {
    p50_ms: f64,
    p99_ms: f64,
    gate: bool,
}

/// The nearest-rank percentile `pct` (1..=100) of `sorted`:
/// `sorted[ceil(pct/100 * n) - 1]`.
fn percentile(sorted: &[Duration], pct: u64) -> Result<Duration, String> {
    let n = sorted.len() as u64;
    if n == 0 {
        return Err("no samples".to_owned());
    }
    let rank = (pct * n).div_ceil(100).max(1);
    let index = usize::try_from(rank - 1).map_err(|e| e.to_string())?;
    Ok(sorted[index])
}

/// Parses the `[reference_card]` table of `source` (read from `file`).
fn parse_budgets(source: &str, file: &str) -> Result<Budgets, String> {
    let root: toml::Table = source
        .parse()
        .map_err(|e| format!("{file}: not valid TOML: {e}"))?;
    let table = root
        .get(TABLE)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{file}: no [{TABLE}] table"))?;
    if let Some(key) = table
        .keys()
        .find(|key| !matches!(key.as_str(), "p50_ms" | "p99_ms" | "gate"))
    {
        return Err(format!("{file}: [{TABLE}] has the unknown key `{key}`"));
    }
    let millis = |key: &str| -> Result<f64, String> {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a budget in whole milliseconds is far below 2^52"
        )]
        let value = match table.get(key) {
            None => return Err(format!("{file}: [{TABLE}] is missing `{key}`")),
            Some(toml::Value::Integer(n)) => *n as f64,
            Some(toml::Value::Float(x)) => *x,
            Some(_) => return Err(format!("{file}: [{TABLE}] `{key}` is not a number")),
        };
        if !value.is_finite() || value <= 0.0 {
            return Err(format!(
                "{file}: [{TABLE}] `{key}` must be finite and > 0, got {value}"
            ));
        }
        Ok(value)
    };
    let p50_ms = millis("p50_ms")?;
    let p99_ms = millis("p99_ms")?;
    if p50_ms > p99_ms {
        return Err(format!(
            "{file}: [{TABLE}] `p50_ms` ({p50_ms}) is over `p99_ms` ({p99_ms})"
        ));
    }
    let gate = match table.get("gate") {
        None => false,
        Some(toml::Value::Boolean(gate)) => *gate,
        Some(_) => return Err(format!("{file}: [{TABLE}] `gate` is not a bool")),
    };
    Ok(Budgets {
        p50_ms,
        p99_ms,
        gate,
    })
}

/// Reads and parses the budgets file at `path`.
fn load_budgets(path: &str) -> Result<Budgets, String> {
    let source = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    parse_budgets(&source, path)
}

/// The budget lines for the measured p50 and p99 (ms), and whether one is
/// over its budget.
fn verdict(p50_ms: f64, p99_ms: f64, budgets: Budgets) -> (Vec<String>, bool) {
    let mode = if budgets.gate { "gated" } else { "report only" };
    let mut over = false;
    let lines = [
        ("p50", p50_ms, budgets.p50_ms),
        ("p99", p99_ms, budgets.p99_ms),
    ]
    .into_iter()
    .map(|(stat, measured, budget)| {
        if measured > budget {
            over = true;
            format!("{TABLE}: {stat} {measured:.3} ms is over its {budget} ms budget ({mode})")
        } else {
            format!("{TABLE}: {stat} {measured:.3} ms is within its {budget} ms budget")
        }
    })
    .collect();
    (lines, over)
}

/// The exit status: failure only when a budget is over and gated.
fn exit_code(over: bool, budgets: Budgets) -> ExitCode {
    if over && budgets.gate {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The pool threads: half the available parallelism, 1..=256.
fn threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    (cores / 2).clamp(1, 256)
}

/// Builds the reference card App and returns it with its Renderer.
fn renderer(threads: usize) -> (App, Arc<dyn Renderer>) {
    let config = ImageConfig::new()
        .threads(threads)
        .queue_capacity((2 * threads).clamp(64, 4096));
    let mut builder = App::builder();
    card::configure_with(&mut builder, config).expect("the card theme parses");
    let app = builder.build().expect("the reference card App builds");
    let renderer = Arc::clone(
        app.renderer()
            .expect("the ImagePlugin installed a Renderer"),
    );
    (app, renderer)
}

/// Renders `count` cards with xp `first..first + count` on `clients`
/// closed-loop tasks; returns each render's wall time.
async fn drive(
    renderer: &Arc<dyn Renderer>,
    clients: usize,
    first: u32,
    count: u32,
) -> Vec<Duration> {
    let next = Arc::new(AtomicU32::new(0));
    let mut tasks = JoinSet::new();
    for _ in 0..clients {
        let renderer = Arc::clone(renderer);
        let next = Arc::clone(&next);
        tasks.spawn(async move {
            let mut samples = Vec::new();
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= count {
                    return samples;
                }
                let job = RenderJob::of(&card::sample(first + i)).expect("the card encodes");
                let start = Instant::now();
                renderer
                    .render(job)
                    .await
                    .expect("the reference card renders");
                samples.push(start.elapsed());
            }
        });
    }
    let mut samples = Vec::with_capacity(count as usize);
    while let Some(done) = tasks.join_next().await {
        samples.extend(done.expect("a client task panicked"));
    }
    samples
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

async fn full() -> ExitCode {
    let budgets = match load_budgets(BUDGETS) {
        Ok(budgets) => budgets,
        Err(error) => {
            println!("reference_card: {error}");
            return ExitCode::FAILURE;
        }
    };
    let threads = threads();
    let (_app, renderer) = renderer(threads);
    drive(&renderer, threads, WARMUP_XP, WARMUP).await;
    let mut samples = drive(&renderer, threads, TIMED_XP, SAMPLES).await;
    samples.sort_unstable();
    let p50 = percentile(&samples, 50).expect("samples were taken");
    let p99 = percentile(&samples, 99).expect("samples were taken");
    let total: Duration = samples.iter().sum();
    let mean = total / u32::try_from(samples.len()).expect("1000 samples");
    println!("reference_card: 934x282 PNG, warm caches, saturated pool");
    println!(
        "threads {threads}, clients {threads}, samples {}",
        samples.len()
    );
    println!(
        "min {:.3} ms, mean {:.3} ms, p50 {:.3} ms, p99 {:.3} ms, max {:.3} ms",
        ms(samples[0]),
        ms(mean),
        ms(p50),
        ms(p99),
        ms(samples[samples.len() - 1])
    );
    let (lines, over) = verdict(ms(p50), ms(p99), budgets);
    for line in lines {
        println!("{line}");
    }
    exit_code(over, budgets)
}

/// The Percentile rows.
fn check_percentiles() {
    let d = Duration::from_millis;
    assert_eq!(percentile(&[d(5)], 50), Ok(d(5)));
    assert_eq!(percentile(&[d(5)], 99), Ok(d(5)));
    // 1..=100, shuffled by a fixed permutation, then sorted.
    let mut hundred: Vec<Duration> = (0..100u64).map(|i| d((i * 37) % 100 + 1)).collect();
    hundred.sort_unstable();
    assert_eq!(percentile(&hundred, 50), Ok(d(50)));
    assert_eq!(percentile(&hundred, 99), Ok(d(99)));
    let thousand: Vec<Duration> = (1..=1000).map(d).collect();
    assert_eq!(percentile(&thousand, 99), Ok(d(990)));
    assert!(percentile(&[], 50).is_err());
}

/// The Budgets valid, Budgets invalid and Over budget rows.
fn check_budgets() {
    let valid = "[reference_card]\np50_ms = 10.0\np99_ms = 50.0\ngate = false\n";
    let expected = Budgets {
        p50_ms: 10.0,
        p99_ms: 50.0,
        gate: false,
    };
    assert_eq!(parse_budgets(valid, "b.toml"), Ok(expected));
    assert_eq!(
        parse_budgets(
            "[other]\nx = 1\n[reference_card]\np50_ms = 10\np99_ms = 50.0\n",
            "b.toml"
        ),
        Ok(expected)
    );
    let invalid = [
        ("[other]\np50_ms = 1.0\n", "[reference_card]"),
        ("[reference_card]\np99_ms = 50.0\n", "p50_ms"),
        ("[reference_card]\np50_ms = 0\np99_ms = 50.0\n", "p50_ms"),
        ("[reference_card]\np50_ms = -1.0\np99_ms = 50.0\n", "p50_ms"),
        ("[reference_card]\np50_ms = nan\np99_ms = 50.0\n", "p50_ms"),
        ("[reference_card]\np50_ms = 10.0\np99_ms = inf\n", "p99_ms"),
        ("[reference_card]\np50_ms = 60.0\np99_ms = 50.0\n", "p50_ms"),
        (
            "[reference_card]\np50_ms = 1.0\np99_ms = 5.0\ngate = \"no\"\n",
            "gate",
        ),
        (
            "[reference_card]\np50_ms = 1.0\np99_ms = 5.0\np95_ms = 2.0\n",
            "p95_ms",
        ),
    ];
    for (source, key) in invalid {
        let error = parse_budgets(source, "b.toml").unwrap_err();
        assert!(
            error.contains("b.toml") && error.contains(key),
            "{source:?}: {error}"
        );
    }
    let missing = load_budgets("/nonexistent/budgets.toml").unwrap_err();
    assert!(missing.contains("/nonexistent/budgets.toml"), "{missing}");

    let (lines, over) = verdict(3.0, 60.0, expected);
    assert!(over);
    assert_eq!(
        lines[1],
        "reference_card: p99 60.000 ms is over its 50 ms budget (report only)"
    );
    assert_eq!(exit_code(over, expected), ExitCode::SUCCESS);
    let gated = Budgets {
        gate: true,
        ..expected
    };
    assert_eq!(exit_code(over, gated), ExitCode::FAILURE);
    let (_, within) = verdict(3.0, 20.0, gated);
    assert_eq!(exit_code(within, gated), ExitCode::SUCCESS);
}

/// The size of a PNG.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("the render is a PNG");
    (reader.info().width, reader.info().height)
}

async fn smoke() -> ExitCode {
    check_percentiles();
    check_budgets();
    let budgets = load_budgets(BUDGETS).expect("the real budgets.toml parses");
    assert!(!budgets.gate, "budgets are report-only until Story 14.3");
    let (_app, renderer) = renderer(threads());
    let mut digests = Vec::new();
    for xp in [1, 2, 3] {
        let job = RenderJob::of(&card::sample(xp)).expect("the card encodes");
        let output = renderer
            .render(job)
            .await
            .expect("the reference card renders");
        assert_eq!(output.format, OutputFormat::Png);
        assert_eq!(png_size(&output.bytes), (934, 282));
        assert!(!digests.contains(&output.digest), "xp {xp} hit the cache");
        digests.push(output.digest);
    }
    println!("reference_card: smoke ok (run `cargo bench` for the report)");
    ExitCode::SUCCESS
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().any(|arg| arg == "--bench") {
        full().await
    } else {
        smoke().await
    }
}
