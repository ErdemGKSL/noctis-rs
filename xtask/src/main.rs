//! Repository tooling for noctis-rs, run as `cargo run --release -p xtask -- <task>`.
//!
//! Tasks:
//! - `deps-check [--manifest-path <path>]`: checks every workspace dependency edge
//!   against the spine's Invariants graph (AD-1, AD-2, AD-28 CI gate).
//! - `rules-catalog [--root <path>]`: checks that every rule row of
//!   `docs/discord-rules.md` has a test at its enforcement point, or a pending
//!   entry in `xtask/rules-pending.toml` (FR-75, FR-79, NFR-10, AD-4).
//! - `api-snapshot [--bless]`: records the facade's public API with
//!   `cargo public-api` into `xtask/public-api.txt` and fails when the committed
//!   file is stale (FR-49, NFR-8, AD-27).
//! - `skill-check [--root <path>]`: checks that the Noctis Usage Skill mentions
//!   every snapshot item and carries its API hash, and lints the Project
//!   Template for UI Doctrine violations (FR-49, FR-50, AD-17, AD-28).

mod api_snapshot;
mod deps_check;
mod public_api;
mod rules_catalog;
mod skill_check;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo run --release -p xtask -- <task>

tasks:
  deps-check [--manifest-path <path>]
      check workspace dependency edges against the spine's Invariants graph
      (default manifest: the noctis-rs workspace root)
  rules-catalog [--root <path>]
      check that every rule in docs/discord-rules.md has a test at its
      enforcement point or a pending entry (default root: the noctis-rs repo)
  api-snapshot [--bless]
      check that xtask/public-api.txt matches the facade's public API
      (needs cargo-public-api 0.52.0 on nightly-2026-10-15); --bless rewrites it
  skill-check [--root <path>]
      check the Usage Skill against xtask/public-api.txt and lint the Project
      Template for UI Doctrine violations (default root: the noctis-rs repo)
  help, --help, -h
      print this message";

/// Exit code for a usage error.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        None => usage(ExitCode::SUCCESS),
        Some((task, _)) if matches!(task.as_str(), "help" | "--help" | "-h") => {
            usage(ExitCode::SUCCESS)
        }
        Some((task, rest)) if task == "deps-check" => match deps_check_manifest(rest) {
            Ok(manifest_path) => deps_check::run(&manifest_path),
            Err(message) => {
                eprintln!("deps-check: {message}");
                usage(ExitCode::from(USAGE_ERROR))
            }
        },
        Some((task, rest)) if task == "rules-catalog" => match root_arg(rest) {
            Ok(root) => rules_catalog::run(&root),
            Err(message) => {
                eprintln!("rules-catalog: {message}");
                usage(ExitCode::from(USAGE_ERROR))
            }
        },
        Some((task, rest)) if task == "api-snapshot" => match rest {
            [] => api_snapshot::run(false),
            [flag] if flag == "--bless" => api_snapshot::run(true),
            [flag, extra, ..] if flag == "--bless" => {
                eprintln!("api-snapshot: unexpected argument: {extra}");
                usage(ExitCode::from(USAGE_ERROR))
            }
            [other, ..] => {
                eprintln!("api-snapshot: unexpected argument: {other}");
                usage(ExitCode::from(USAGE_ERROR))
            }
        },
        Some((task, rest)) if task == "skill-check" => match root_arg(rest) {
            Ok(root) => skill_check::run(&root),
            Err(message) => {
                eprintln!("skill-check: {message}");
                usage(ExitCode::from(USAGE_ERROR))
            }
        },
        Some((task, _)) => {
            eprintln!("unknown task: {task}");
            usage(ExitCode::from(USAGE_ERROR))
        }
    }
}

fn usage(code: ExitCode) -> ExitCode {
    eprintln!("{USAGE}");
    code
}

/// Parses `deps-check`'s arguments into the manifest path to inspect.
fn deps_check_manifest(args: &[String]) -> Result<PathBuf, String> {
    match args {
        [] => Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml")),
        [flag, path] if flag == "--manifest-path" => Ok(PathBuf::from(path)),
        [flag] if flag == "--manifest-path" => Err("`--manifest-path` needs a value".to_owned()),
        [flag, _, extra, ..] if flag == "--manifest-path" => {
            Err(format!("unexpected argument: {extra}"))
        }
        [other, ..] => Err(format!("unexpected argument: {other}")),
    }
}

/// Parses `rules-catalog`'s and `skill-check`'s arguments into the repository
/// root to check.
fn root_arg(args: &[String]) -> Result<PathBuf, String> {
    match args {
        [] => Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("..")),
        [flag, path] if flag == "--root" => Ok(PathBuf::from(path)),
        [flag] if flag == "--root" => Err("`--root` needs a value".to_owned()),
        [flag, _, extra, ..] if flag == "--root" => Err(format!("unexpected argument: {extra}")),
        [other, ..] => Err(format!("unexpected argument: {other}")),
    }
}
