//! Repository tooling for noctis-rs, run as `cargo run --release -p xtask -- <task>`.
//!
//! Tasks:
//! - `deps-check [--manifest-path <path>]`: checks every workspace dependency edge
//!   against the spine's Invariants graph (AD-1, AD-2, AD-28 CI gate).
//!
//! `rules-catalog`, `api-snapshot` and the other gates arrive in later stories.

mod deps_check;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo run --release -p xtask -- <task>

tasks:
  deps-check [--manifest-path <path>]
      check workspace dependency edges against the spine's Invariants graph
      (default manifest: the noctis-rs workspace root)";

/// Exit code for a usage error.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        None => usage(ExitCode::SUCCESS),
        Some((task, _)) if task == "help" => usage(ExitCode::SUCCESS),
        Some((task, rest)) if task == "deps-check" => match deps_check_manifest(rest) {
            Ok(manifest_path) => deps_check::run(&manifest_path),
            Err(message) => {
                eprintln!("deps-check: {message}");
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
