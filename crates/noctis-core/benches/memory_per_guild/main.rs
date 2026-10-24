//! `memory_per_guild` (NFR-1, AD-25, AD-28): framework memory per idle
//! guild, Discord cache memory and Renderer cache memory of one App loading
//! idle guilds through `FakeGateway`, reported against
//! `benches/budgets.toml`.
//!
//! Run with `cargo bench -p noctis-core --bench memory_per_guild`
//! (`-- --guilds N` overrides the file's guild count). Exit 0 after a
//! report, 1 when gating is on and a metric is over budget, 2 when nothing
//! could be measured.

mod budget;
mod measure;

use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

#[global_allocator]
static ALLOC: noctis::testkit::dhat::Alloc = noctis::testkit::dhat::Alloc;

/// The shared budgets file.
const BUDGETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/budgets.toml");

fn main() -> ExitCode {
    match bench() {
        Ok(report) if report.failed() => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("memory_per_guild: {message}");
            ExitCode::from(2)
        }
    }
}

/// Measures, prints and returns the report; an error is the message of why
/// nothing was measured.
fn bench() -> Result<budget::Report, String> {
    let guilds_arg = parse_args(std::env::args().skip(1))?;
    let budgets = budget::load(Path::new(BUDGETS)).map_err(|error| error.to_string())?;
    let guilds = guilds_arg.unwrap_or_else(|| budgets.guilds());
    let profiler = measure::start_profiler().map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("cannot build the tokio runtime: {error}"))?;
    let measured = runtime
        .block_on(measure::measure(guilds))
        .map_err(|error| error.to_string())?;
    drop(runtime);
    let report = budget::evaluate(&budgets, &measured).map_err(|error| error.to_string())?;
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
                "memory_per_guild: warning: cannot append the report to {}: {error}",
                Path::new(&summary).display()
            );
        }
    }
    drop(profiler);
    Ok(report)
}

#[expect(clippy::print_stdout, reason = "the bench reports on stdout")]
fn print_report(text: &str) {
    print!("{text}");
}

/// The `--guilds N` override; cargo's `--bench` and other arguments are
/// ignored.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<u32>, String> {
    let mut args = args;
    let mut guilds = None;
    while let Some(arg) = args.next() {
        let value = if arg == "--guilds" {
            args.next()
                .ok_or_else(|| "--guilds needs a guild count".to_owned())?
        } else if let Some(value) = arg.strip_prefix("--guilds=") {
            value.to_owned()
        } else {
            continue;
        };
        let count: u64 = value
            .parse()
            .map_err(|_| format!("--guilds expects an integer, got `{value}`"))?;
        budget::check_guilds(count).map_err(|error| error.to_string())?;
        guilds = Some(u32::try_from(count).map_err(|_| "--guilds is out of range".to_owned())?);
    }
    Ok(guilds)
}
