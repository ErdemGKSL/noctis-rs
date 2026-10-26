//! `release-check [--root <path>]`: the offline guard of the release pipeline
//! (FR-48, NFR-7, NFR-8, AD-28; Story 14.7). It never builds or publishes.
//!
//! # Model
//!
//! `cargo metadata --no-deps` of `<root>/Cargo.toml` gives the members; W and
//! U are `[workspace.package] version` and `repository` of the root manifest;
//! P is the members whose `publish` is not `false`, Q the others. A
//! dependency is *path-only* when its requirement is `*`.
//!
//! # Checks
//!
//! - **crates**: every published crate is a noctis crate at W, with a
//!   description, a license and the workspace repository; the facade is
//!   published;
//! - **dependencies**: published normal and build edges stay inside P and
//!   carry a version (`cargo publish` needs it); a dev-dependency that would
//!   form a cycle with the published graph, or that names an unpublished
//!   member, is path-only so `cargo publish` strips it;
//! - **`release-plz.toml`**: the golden settings of the runbook
//!   (`docs/guides/release.md`), `changelog_include` = P without the facade,
//!   every member of Q skipped, none of P skipped, a commits-free
//!   changelog body;
//! - **`CHANGELOG.md`**: starts with `# Changelog`, and `## [Unreleased]` is
//!   its first `## ` heading (release-plz stamps the version under it).
//!
//! # Output
//!
//! Every problem is one stderr line `release-check: <subject>: <message>`,
//! sorted and deduplicated, and the exit code is 1. Success prints
//! `release-check: ok (<n> crates at <W>; publish order: <a>, <b>, …)`.

mod model;
mod rules;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use cargo_metadata::MetadataCommand;

use model::Workspace;
use rules::{CHANGELOG, Config, RELEASE_PLZ};

/// One problem; field order is the report's sort order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Problem {
    subject: String,
    message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "release-check: {}: {}", self.subject, self.message)
    }
}

/// The sorted, deduplicated problem set (E2).
#[derive(Debug, Default)]
struct Problems(BTreeSet<Problem>);

impl Problems {
    fn add(&mut self, subject: impl Into<String>, message: impl Into<String>) {
        self.0.insert(Problem {
            subject: subject.into(),
            message: message.into(),
        });
    }

    /// The report lines, in order.
    fn lines(&self) -> Vec<String> {
        self.0.iter().map(ToString::to_string).collect()
    }
}

/// Runs the check on the repository at `root`.
pub(crate) fn run(root: &Path) -> ExitCode {
    let mut problems = Problems::default();
    let summary = check(root, &mut problems);
    match summary {
        Some(summary) if problems.0.is_empty() => {
            eprintln!("release-check: ok ({summary})");
            ExitCode::SUCCESS
        }
        _ => {
            for line in problems.lines() {
                eprintln!("{line}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Loads the inputs and runs every rule; returns the success summary, or
/// `None` when a problem stopped the check early.
fn check(root: &Path, problems: &mut Problems) -> Option<String> {
    let manifest_path = root.join("Cargo.toml");
    let Some(manifest) = read(&manifest_path) else {
        problems.add("Cargo.toml", "no [workspace.package] version");
        return None;
    };
    let metadata = match MetadataCommand::new()
        .manifest_path(&manifest_path)
        .no_deps()
        .exec()
    {
        Ok(metadata) => metadata,
        Err(err) => {
            problems.add(
                "Cargo.toml",
                format!("cargo metadata failed: {}", first_error_line(&err)),
            );
            return None;
        }
    };
    let ws = Workspace::from_metadata(&metadata, manifest.parse::<toml::Table>().ok().as_ref());

    rules::check_workspace(&ws, problems);
    let order = match rules::publish_order(&ws) {
        Ok(order) => order,
        Err(cycle) => {
            for name in cycle {
                problems.add(name, "dependency cycle among published crates");
            }
            Vec::new()
        }
    };
    let config = match read(&root.join(RELEASE_PLZ)) {
        None => Config::Missing,
        Some(text) => match text.parse::<toml::Table>() {
            Ok(table) => Config::Parsed(table),
            Err(err) => Config::Invalid(first_line(&err.to_string()).to_owned()),
        },
    };
    rules::check_release_plz(&config, &ws, problems);
    rules::check_changelog(read(&root.join(CHANGELOG)).as_deref(), problems);

    Some(format!(
        "{} crates at {}; publish order: {}",
        order.len(),
        ws.version.as_deref().unwrap_or("?"),
        order.join(", ")
    ))
}

/// The file's text; `None` when it does not exist or is unreadable.
fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// The first non-empty line of `cargo metadata`'s stderr, else of the error.
fn first_error_line(err: &cargo_metadata::Error) -> String {
    if let cargo_metadata::Error::CargoMetadata { stderr } = err
        && let Some(line) = stderr.lines().map(str::trim).find(|l| !l.is_empty())
    {
        return line.to_owned();
    }
    first_line(&err.to_string()).to_owned()
}

/// The first non-empty line of `text`, trimmed.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}
