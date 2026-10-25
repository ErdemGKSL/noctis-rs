//! `skill-check [--root <path>]`: keeps the Noctis Usage Skill in step with the
//! public API and the tested recipes, and lints every example crate for UI
//! Doctrine violations (FR-49, FR-50, UX-DR1..9, NFR-8, AD-17, AD-27, AD-28).
//!
//! # Checks
//!
//! - **mentions** ([`mention`]): every public item of `xtask/public-api.txt`
//!   (the only input about the API; see [`crate::public_api`]) is mentioned in
//!   the skill's Markdown;
//! - **API hash** ([`hash`]): `SKILL.md`'s `**API hash:**` code span is the
//!   snapshot's blake3 hash;
//! - **component entries** ([`components`]): every component builder type
//!   the snapshot exports has exactly one entry, with `**Where:**`,
//!   `**Limits:**` and `**Builder:**`, in the skill's
//!   `references/components-v2.md` or `references/modals.md`, and its
//!   `**Limits:**` links `docs/discord-rules.md`;
//! - **UI Doctrine** ([`doctrine`]): UD-1, UD-3 and UD-7 over the `src/` and
//!   `tests/` of every crate directory under `examples/` (the Project Template
//!   and `examples/ui-recipes`);
//! - **recipes** ([`recipes`]): the ```` ```rust recipe=<name> ```` blocks of
//!   the skill's `references/views-and-recipes.md` equal the regions of
//!   `examples/ui-recipes/src`;
//! - **completeness** ([`completeness`]): the required references
//!   (`concept-map.md`, `limits-cheatsheet.md`, `patterns.md`,
//!   `troubleshooting.md`) and their headings exist, `SKILL.md` lists every
//!   reference, no `planned`/`v0`/`Epic <n>`/`Story <n>.<m>` marker remains,
//!   `troubleshooting.md`'s `## Rule id index` lists every rule id of
//!   `docs/discord-rules.md` in catalog order, `limits-cheatsheet.md` names
//!   every id, and the Rust examples in the skill's and `docs/`'s Markdown
//!   follow UD-7.
//!
//! # Output
//!
//! Every problem is one stderr line `skill-check: <subject>: <message>`, sorted
//! and deduplicated, and the exit code is 1. Success prints `skill-check: ok (<N>
//! public items mentioned, <E> component entries checked, <F> example files
//! linted, <R> recipes in sync, <I> rule ids indexed)`.

mod completeness;
mod components;
mod doctrine;
mod hash;
mod mention;
mod recipes;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::api_snapshot::SNAPSHOT;
use crate::public_api;

/// The Usage Skill's directory.
const SKILL_DIR: &str = "examples/project-template/.claude/skills/noctis";
/// The directory whose crates the doctrine lint covers.
const EXAMPLES_DIR: &str = "examples";
/// The linted directories of each example crate.
const EXAMPLE_SUBDIRS: &[&str] = &["src", "tests"];
/// The skill file whose `recipe=` blocks copy `examples/ui-recipes`.
const RECIPES_MD: &str = "references/views-and-recipes.md";

/// One problem; field order is the report's sort order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Problem {
    subject: String,
    message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "skill-check: {}: {}", self.subject, self.message)
    }
}

/// The sorted, deduplicated problem set.
#[derive(Debug, Default)]
pub(crate) struct Problems(BTreeSet<Problem>);

impl Problems {
    pub(crate) fn add(&mut self, subject: impl Into<String>, message: impl Into<String>) {
        self.0.insert(Problem {
            subject: subject.into(),
            message: message.into(),
        });
    }
}

/// Runs the check on the repository at `root`.
pub(crate) fn run(root: &Path) -> ExitCode {
    let mut problems = Problems::default();
    let summary = check(&Repo { root }, &mut problems);
    if problems.0.is_empty() {
        eprintln!("skill-check: ok ({summary})");
        ExitCode::SUCCESS
    } else {
        for problem in &problems.0 {
            eprintln!("{problem}");
        }
        ExitCode::FAILURE
    }
}

/// The repository being checked; paths in problems are relative to `root`.
struct Repo<'a> {
    root: &'a Path,
}

impl Repo<'_> {
    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Reads a UTF-8 file, reporting failures under `rel`.
    fn read(&self, rel: &str, problems: &mut Problems) -> Option<String> {
        let bytes = self.read_bytes(rel, "", problems)?;
        String::from_utf8(bytes)
            .map_err(|_| problems.add(rel, "not UTF-8"))
            .ok()
    }

    /// Reads a file; a failure is reported under `rel` with `hint` appended.
    fn read_bytes(&self, rel: &str, hint: &str, problems: &mut Problems) -> Option<Vec<u8>> {
        fs::read(self.path(rel))
            .map_err(|err| problems.add(rel, format!("cannot read: {err}{hint}")))
            .ok()
    }

    /// Every file below `rel` with extension `ext`, recursively, sorted; `None`
    /// when `rel` is not a readable directory.
    fn files(&self, rel: &str, ext: &str) -> Option<Vec<String>> {
        let entries = fs::read_dir(self.path(rel)).ok()?;
        let mut entries: Vec<(String, bool)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                Some((format!("{rel}/{name}"), e.file_type().ok()?.is_dir()))
            })
            .collect();
        entries.sort();
        let mut out = Vec::new();
        for (path, is_dir) in entries {
            if is_dir {
                out.extend(self.files(&path, ext).unwrap_or_default());
            } else if Path::new(&path).extension().is_some_and(|e| e == ext) {
                out.push(path);
            }
        }
        Some(out)
    }
}

/// Runs every check; returns the success summary.
fn check(repo: &Repo<'_>, problems: &mut Problems) -> String {
    let snapshot = read_snapshot(repo, problems);

    let skill = mention::read_skill(repo, SKILL_DIR, problems);
    let mentioned = match (&snapshot, &skill) {
        (Some((_, lines)), Some(files)) => mention::check(lines, files, problems),
        _ => 0,
    };
    if let (Some((bytes, _)), Some(files)) = (&snapshot, &skill) {
        let skill_md = format!("{SKILL_DIR}/SKILL.md");
        let text = files
            .iter()
            .find(|(path, _)| *path == skill_md)
            .map_or("", |(_, text)| text.as_str());
        hash::check(text, &public_api::hash(bytes), problems);
    }

    let entries = match (&snapshot, &skill) {
        (Some((_, lines)), Some(files)) => {
            components::check(SKILL_DIR, &components::due_builders(lines), files, problems)
        }
        _ => 0,
    };

    let linted = lint_examples(repo, problems);

    let recipes_md = format!("{SKILL_DIR}/{RECIPES_MD}");
    // An unreadable skill is already reported by `mention::read_skill`.
    let mut in_sync = 0;
    if let Some(files) = &skill {
        if let Some((_, text)) = files.iter().find(|(path, _)| *path == recipes_md) {
            in_sync = recipes::check(repo, &recipes_md, text, problems);
        } else {
            problems.add(&recipes_md, "cannot read: not in the Usage Skill");
        }
    }

    let indexed = skill.as_ref().map_or(0, |files| {
        completeness::check(repo, SKILL_DIR, files, problems)
    });

    format!(
        "{mentioned} public items mentioned, {entries} component entries checked, {linted} example files linted, {in_sync} recipes in sync, {indexed} rule ids indexed"
    )
}

/// The doctrine lint over `examples/<crate>/{src,tests}` (missing subdirectories are
/// skipped, and nothing else of a crate, such as `target/`, is read); returns how
/// many files it linted.
fn lint_examples(repo: &Repo<'_>, problems: &mut Problems) -> usize {
    let Ok(entries) = fs::read_dir(repo.path(EXAMPLES_DIR)) else {
        problems.add(EXAMPLES_DIR, "cannot read: not a directory");
        return 0;
    };
    let mut crates: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    crates.sort();
    let mut linted = 0;
    for name in crates {
        for sub in EXAMPLE_SUBDIRS {
            let Some(files) = repo.files(&format!("{EXAMPLES_DIR}/{name}/{sub}"), "rs") else {
                continue;
            };
            for file in files {
                if let Some(src) = repo.read(&file, problems) {
                    linted += 1;
                    doctrine::check(&file, &src, problems);
                }
            }
        }
    }
    linted
}

/// Reads and parses the snapshot (M9, H5, L16).
fn read_snapshot(
    repo: &Repo<'_>,
    problems: &mut Problems,
) -> Option<(Vec<u8>, Vec<public_api::Line>)> {
    let bytes = repo.read_bytes(
        SNAPSHOT,
        "; run cargo run --release -p xtask -- api-snapshot --bless",
        problems,
    )?;
    let Ok(text) = std::str::from_utf8(&bytes) else {
        problems.add(SNAPSHOT, "not UTF-8");
        return None;
    };
    if text.contains('\r') {
        problems.add(SNAPSHOT, "must use LF line endings");
    }
    match public_api::parse(&text.replace('\r', "")) {
        Ok(lines) => Some((bytes, lines)),
        Err(errors) => {
            for (n, _) in errors {
                problems.add(format!("{SNAPSHOT}:{n}"), "cannot parse line");
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R7: every example crate's `src/` and `tests/` are linted, nothing else.
    #[test]
    fn r7_doctrine_covers_every_example() {
        let root = std::env::temp_dir().join(format!("xtask-skill-r7-{}", std::process::id()));
        let write = |rel: &str, src: &str| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, src).unwrap();
        };
        let spawn = "impl X {\n#[handler]\nasync fn handle(self, ctx: ComponentCtx) { tokio::spawn(f()); }\n}\n";
        write("examples/project-template/src/lib.rs", "fn ok() {}\n");
        write("examples/project-template/tests/flows.rs", "fn ok() {}\n");
        write("examples/ui-recipes/src/tabs.rs", spawn);
        write("examples/ui-recipes/target/release/build/gen.rs", spawn);
        write("examples/ui-recipes/benches/b.rs", spawn);
        let mut problems = Problems::default();
        let linted = lint_examples(&Repo { root: &root }, &mut problems);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(linted, 3);
        let got: Vec<String> = problems.0.iter().map(ToString::to_string).collect();
        assert_eq!(
            got,
            [
                "skill-check: examples/ui-recipes/src/tabs.rs:3: [UD-7] task spawned per interaction or event; use a Ref TTL or a port"
            ]
        );
    }
}
