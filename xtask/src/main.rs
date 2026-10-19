//! Repository tooling for noctis-rs, run as `cargo run --release -p xtask -- <task>`.
//!
//! No tasks exist yet; `deps-check`, `rules-catalog` and the other gates arrive
//! in later stories.

use std::process::ExitCode;

const USAGE: &str = "usage: cargo run --release -p xtask -- <task>\n\nno tasks yet";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None | Some("help") => {
            eprintln!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(task) => {
            eprintln!("unknown task: {task}");
            ExitCode::from(2)
        }
    }
}
