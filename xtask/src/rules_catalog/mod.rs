//! `rules-catalog`: checks that every rule row of `docs/discord-rules.md` has
//! a test at its enforcement point (FR-75, FR-79, NFR-10, AD-4).
//!
//! # Classification
//!
//! Each rule row's Enforcement cell (rendered by pulldown-cmark, code spans kept
//! in backticks and opaque) is one of:
//!
//! - **documentation-only**: the whole cell is one parenthesis group
//!   (`(permitted; no check)`); no requirement;
//! - **alias**: `see <ID>` or `as <ID>`; covered by the target's tests, which
//!   must be a requirement-bearing row; no requirement of its own;
//! - **tiers** split at ` / ` (paren depth 0, outside code), each `<Head>` or
//!   `<Head> (<inner>)`. A `Compile-time` tier requires `by-construction`
//!   evidence when the text of `<inner>` before its first `;` contains
//!   "by construction", else `diagnosed` evidence. `App-build`, `Runtime`,
//!   `Startup` and `App-build or Runtime` tiers require `unit` evidence, also
//!   `Runtime (by construction)`. Anything else is a parse error.
//!
//! # Evidence
//!
//! - `diagnosed`: a fixture `crates/noctis/tests/ui[-<feature>]/<area>/<ID>_<slug>.rs`
//!   whose `.stderr` contains `[<ID>]`;
//! - `by-construction`: such a fixture, or a `compile_fail` doctest (`///` or
//!   `//!` in `crates/*/src`), with a line carrying `// [<ID>]` after code;
//! - `unit`: a `#[test]` / `#[tokio::test]` function in `crates/*/src` or
//!   `crates/*/tests` (not the UI suites) whose name has the id's snake token
//!   (`cv2_03`).
//!
//! A requirement listed in `xtask/rules-pending.toml` is pending; a pending
//! requirement that already has evidence fails, so the list only shrinks. Since
//! Story 14.4 every entry (`[[rule]]` and `[[check]]`) must carry `p2 = true`
//! (a P2-feature requirement); an otherwise valid entry without it is reported
//! and still kept pending. The `custom_id` constants table is compared with the
//! `pub const`s of `crates/noctis-codec/src` unless the `custom-id-constants`
//! check is pending.
//!
//! # v1 rules audit
//!
//! The catalog's `## v1 rules audit` section lists the by-construction,
//! runtime-only and documentation-only/alias rows. It is not checked by the
//! CLI; the unit test `audit::tests::real_catalog_audit_is_complete` keeps those
//! lists equal to this classification.
//!
//! # Output
//!
//! Every problem is one stderr line `rules-catalog: <subject>: <message>`,
//! sorted and deduplicated, and the exit code is 1. Success prints
//! `rules-catalog: ok (<R> rules: <C> covered, <P> pending, <D> documentation-only,
//! <A> aliases)`, where `covered` counts requirement-bearing rows with every
//! requirement met and none pending, and `pending` counts the pending rule
//! entries (one row can have several).

#[cfg(test)]
mod audit;
pub(crate) mod catalog;
pub(crate) mod pending;
mod scan;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;

use catalog::{CATALOG, Catalog, Class};
use pending::Pending;

/// The kind of evidence a requirement needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Evidence {
    Diagnosed,
    ByConstruction,
    Unit,
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Diagnosed => "diagnosed",
            Self::ByConstruction => "by-construction",
            Self::Unit => "unit",
        })
    }
}

/// `^[A-Z][A-Z0-9]{1,3}-[0-9]{2}$`.
pub(crate) fn is_rule_id(id: &str) -> bool {
    let Some((area, num)) = id.split_once('-') else {
        return false;
    };
    (2..=4).contains(&area.len())
        && area.starts_with(|c: char| c.is_ascii_uppercase())
        && area
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && num.len() == 2
        && num.bytes().all(|b| b.is_ascii_digit())
}

/// One problem; field order is the report's sort order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Problem {
    subject: String,
    message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rules-catalog: {}: {}", self.subject, self.message)
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

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Runs the check on the repository at `root`.
pub(crate) fn run(root: &Path) -> ExitCode {
    let mut problems = Problems::default();
    let summary = check(root, &mut problems);
    if problems.is_empty() {
        eprintln!("rules-catalog: ok ({summary})");
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
        match fs::read(self.path(rel)) {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|_| problems.add(rel, "not UTF-8"))
                .ok(),
            Err(err) => {
                problems.add(rel, format!("cannot read: {err}"));
                None
            }
        }
    }

    /// The sorted entries of directory `rel` as `(rel path, is dir)`; empty
    /// when it does not exist.
    fn list(&self, rel: &str) -> Vec<(String, bool)> {
        let Ok(entries) = fs::read_dir(self.path(rel)) else {
            return Vec::new();
        };
        let mut out: Vec<(String, bool)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let is_dir = e.file_type().ok()?.is_dir();
                Some((format!("{rel}/{name}"), is_dir))
            })
            .collect();
        out.sort();
        out
    }

    /// Every file below `rel`, recursively, sorted.
    fn files(&self, rel: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (path, is_dir) in self.list(rel) {
            if is_dir {
                out.extend(self.files(&path));
            } else {
                out.push(path);
            }
        }
        out
    }
}

/// Collected evidence: ids per kind.
#[derive(Debug, Default)]
struct Found {
    by_kind: BTreeMap<Evidence, BTreeSet<String>>,
}

impl Found {
    fn add(&mut self, kind: Evidence, id: &str) {
        self.by_kind.entry(kind).or_default().insert(id.to_owned());
    }

    fn has(&self, kind: Evidence, id: &str) -> bool {
        self.by_kind.get(&kind).is_some_and(|ids| ids.contains(id))
    }
}

const UI_RS: &str = "crates/noctis/tests/ui.rs";
const FACADE_MANIFEST: &str = "crates/noctis/Cargo.toml";
const CODEC_SRC: &str = "crates/noctis-codec/src";

/// Runs every check, returning the success summary.
fn check(root: &Path, problems: &mut Problems) -> String {
    let repo = Repo { root };
    let Some(text) = repo.read(CATALOG, problems) else {
        return String::new();
    };
    let catalog = catalog::parse(&text, problems);
    let pending = repo
        .read(pending::PATH, problems)
        .map(|text| pending::parse(&text, &catalog, problems))
        .unwrap_or_default();
    let mut found = Found::default();
    let features = facade_features(&repo, problems);
    check_fixtures(&repo, &catalog, &features, &mut found, problems);
    check_registration(&repo, &features, problems);
    scan_sources(&repo, &catalog, &mut found, problems);
    let covered = check_requirements(&catalog, &pending, &found, problems);
    check_constants(&repo, &catalog, &pending, problems);
    let count = |f: fn(&Class) -> bool| {
        catalog
            .rows
            .iter()
            .filter(|r| r.class.as_ref().is_some_and(f))
            .count()
    };
    format!(
        "{} rules: {covered} covered, {} pending, {} documentation-only, {} aliases",
        catalog.rows.len(),
        pending.rules.len(),
        count(|c| *c == Class::DocOnly),
        count(|c| matches!(c, Class::Alias(_))),
    )
}

/// The facade's `[features]` keys.
fn facade_features(repo: &Repo<'_>, problems: &mut Problems) -> BTreeSet<String> {
    let Some(text) = repo.read(FACADE_MANIFEST, problems) else {
        return BTreeSet::new();
    };
    match toml::from_str::<toml::Table>(&text) {
        Ok(manifest) => manifest
            .get("features")
            .and_then(toml::Value::as_table)
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default(),
        Err(err) => {
            problems.add(FACADE_MANIFEST, err.message().replace('\n', " "));
            BTreeSet::new()
        }
    }
}

/// The UI suite directories (`ui`, `ui-<feature>`) of `crates/<krate>/tests`.
fn ui_suites(repo: &Repo<'_>, krate: &str) -> Vec<String> {
    repo.list(&format!("{krate}/tests"))
        .into_iter()
        .filter(|(path, is_dir)| {
            let name = path.rsplit('/').next().unwrap_or_default();
            *is_dir && (name == "ui" || name.starts_with("ui-"))
        })
        .map(|(path, _)| path)
        .collect()
}

/// Whether `path` is a Rust source (`.rs`, case-sensitive like the layout).
fn is_rs(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs")
}

/// `<RULE-ID>_<slug>.rs` → `(id, slug)`.
fn fixture_name(file: &str) -> Option<&str> {
    let stem = file.strip_suffix(".rs")?;
    let (id, slug) = stem.split_once('_')?;
    let slug_ok = !slug.is_empty()
        && slug.split('_').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    (is_rule_id(id) && slug_ok).then_some(id)
}

/// F1-F11, F13: validates every UI fixture and collects its evidence.
fn check_fixtures(
    repo: &Repo<'_>,
    catalog: &Catalog,
    features: &BTreeSet<String>,
    found: &mut Found,
    problems: &mut Problems,
) {
    for (krate, _) in repo.list("crates").into_iter().filter(|(_, d)| *d) {
        if krate == "crates/noctis" {
            continue;
        }
        for suite in ui_suites(repo, &krate) {
            problems.add(suite, "trybuild fixtures belong in crates/noctis/tests/ui");
        }
    }
    for suite in ui_suites(repo, "crates/noctis") {
        let name = suite.rsplit('/').next().unwrap_or_default();
        if let Some(feature) = name.strip_prefix("ui-")
            && (feature == "default" || !features.contains(feature))
        {
            problems.add(&suite, format!("unknown facade feature {feature}"));
        }
        let files = repo.files(&suite);
        let file_set: BTreeSet<&str> = files.iter().map(String::as_str).collect();
        for path in &files {
            if let Some(rs) = path.strip_suffix(".stderr") {
                if !file_set.contains(format!("{rs}.rs").as_str()) {
                    problems.add(path, "orphan .stderr");
                }
            } else if is_rs(path) {
                check_fixture(repo, catalog, &suite, path, &file_set, found, problems);
            } else {
                problems.add(path, "unexpected file");
            }
        }
    }
}

/// One fixture `.rs` file.
fn check_fixture(
    repo: &Repo<'_>,
    catalog: &Catalog,
    suite: &str,
    path: &str,
    files: &BTreeSet<&str>,
    found: &mut Found,
    problems: &mut Problems,
) {
    let rel = &path[suite.len() + 1..];
    let parts: Vec<&str> = rel.split('/').collect();
    let id = match parts.as_slice() {
        [_, file] => fixture_name(file),
        _ => None,
    };
    let Some(id) = id else {
        problems.add(path, "fixture must be <suite>/<area>/<RULE-ID>_<slug>.rs");
        return;
    };
    let area = id
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if parts[0] != area {
        problems.add(path, format!("area directory must be {area}"));
    }
    let Some(source) = repo.read(path, problems) else {
        return;
    };
    let tags: Vec<(usize, scan::Tag)> = source
        .lines()
        .enumerate()
        .flat_map(|(ix, line)| scan::tags(line).into_iter().map(move |t| (ix + 1, t)))
        .collect();
    report_unknown_tags(catalog, path, &tags, problems);
    let Some(row) = catalog.row(id) else {
        problems.add(path, format!("{id} is not a catalog rule"));
        return;
    };
    let kinds = row.class.as_ref().map(Class::kinds).unwrap_or_default();
    let (diag, bc) = (
        kinds.contains(&Evidence::Diagnosed),
        kinds.contains(&Evidence::ByConstruction),
    );
    if !diag && !bc {
        let hint = match &row.class {
            Some(Class::Alias(target)) => format!("; use {target}"),
            _ => String::new(),
        };
        problems.add(path, format!("{id} has no Compile-time tier{hint}"));
        return;
    }
    let stderr_path = format!("{}.stderr", path.trim_end_matches(".rs"));
    let stderr = if files.contains(stderr_path.as_str()) {
        repo.read(&stderr_path, problems)
    } else {
        problems.add(path, "missing .stderr");
        None
    };
    let stderr_ok = stderr
        .as_deref()
        .is_some_and(|s| s.contains(&format!("[{id}]")));
    let tag_ok = tags.iter().any(|(_, t)| t.id == id && t.after_code);
    if diag && stderr_ok {
        found.add(Evidence::Diagnosed, id);
    }
    if bc && tag_ok {
        found.add(Evidence::ByConstruction, id);
    }
    if stderr.is_some() && !(diag && stderr_ok) && !(bc && tag_ok) {
        if diag {
            problems.add(path, format!(".stderr does not contain [{id}]"));
        }
        if bc {
            problems.add(path, format!("no line carries // [{id}] after code"));
        }
    }
}

/// D6: a tag naming an id the catalog does not have.
fn report_unknown_tags(
    catalog: &Catalog,
    path: &str,
    tags: &[(usize, scan::Tag)],
    problems: &mut Problems,
) {
    for (line, tag) in tags {
        if catalog.row(&tag.id).is_none() {
            problems.add(
                format!("{path}:{line}"),
                format!("{} is not a catalog rule", tag.id),
            );
        }
    }
}

/// F11, F12: `crates/noctis/tests/ui.rs` registers every suite, gates the
/// featureless one on every optional facade feature and has no pass suite.
fn check_registration(repo: &Repo<'_>, features: &BTreeSet<String>, problems: &mut Problems) {
    let Some(text) = repo.read(UI_RS, problems) else {
        return;
    };
    if !text.contains("compile_fail(\"tests/ui/**/*.rs\")") {
        problems.add(UI_RS, "no compile_fail suite for tests/ui");
    }
    if text.contains(".pass(") {
        problems.add(
            UI_RS,
            "registers a trybuild pass suite; suites are compile_fail only",
        );
    }
    for feature in features.iter().filter(|f| *f != "default") {
        if !text.contains(&format!("feature = \"{feature}\"")) {
            problems.add(
                UI_RS,
                format!("the featureless suite is not gated on feature = \"{feature}\""),
            );
        }
    }
    for suite in ui_suites(repo, "crates/noctis") {
        let name = suite.rsplit('/').next().unwrap_or_default();
        if name != "ui" && !text.contains(&format!("compile_fail(\"tests/{name}/**/*.rs\")")) {
            problems.add(UI_RS, format!("no compile_fail suite for tests/{name}"));
        }
    }
}

/// D1-D7, U1-U8: doctests in `crates/*/src`, test functions in `crates/*/src`
/// and `crates/*/tests` (UI suites excluded).
fn scan_sources(repo: &Repo<'_>, catalog: &Catalog, found: &mut Found, problems: &mut Problems) {
    for (krate, _) in repo.list("crates").into_iter().filter(|(_, d)| *d) {
        let suites = ui_suites(repo, &krate);
        let src = repo.files(&format!("{krate}/src"));
        let tests = repo.files(&format!("{krate}/tests"));
        for path in src.iter().chain(&tests) {
            let in_suite = suites.iter().any(|s| path.starts_with(&format!("{s}/")));
            if in_suite || !is_rs(path) {
                continue;
            }
            let Some(text) = repo.read(path, problems) else {
                continue;
            };
            if src.contains(path) {
                let tags = scan::doctest_tags(&text);
                report_unknown_tags(catalog, path, &tags, problems);
                for (_, tag) in tags.iter().filter(|(_, t)| t.after_code) {
                    found.add(Evidence::ByConstruction, &tag.id);
                }
            }
            for name in scan::test_fns(&text) {
                for id in scan::cited_ids(&name) {
                    found.add(Evidence::Unit, &id);
                }
            }
        }
    }
}

/// P2, P3: every requirement is met or pending, never both. Returns the
/// number of covered rows.
fn check_requirements(
    catalog: &Catalog,
    pending: &Pending,
    found: &Found,
    problems: &mut Problems,
) -> usize {
    let mut covered = 0;
    for row in &catalog.rows {
        let Some(Class::Required(kinds)) = &row.class else {
            continue;
        };
        let mut complete = true;
        for &kind in kinds {
            let met = found.has(kind, &row.id);
            match (met, pending.rule(&row.id, kind)) {
                (true, Some(entry)) => {
                    complete = false;
                    problems.add(
                        &row.id,
                        format!(
                            "pending entry ({kind}, Story {}) already has a test; remove it \
                             from {}",
                            entry.story,
                            pending::PATH
                        ),
                    );
                }
                (false, Some(_)) => complete = false,
                (true, None) => {}
                (false, None) => {
                    complete = false;
                    problems.add(&row.id, missing_message(&row.id, kind));
                }
            }
        }
        if complete {
            covered += 1;
        }
    }
    covered
}

fn missing_message(id: &str, kind: Evidence) -> String {
    match kind {
        Evidence::Diagnosed => {
            format!("Compile-time row has no fixture whose .stderr contains [{id}]")
        }
        Evidence::ByConstruction => format!(
            "Compile-time (by construction) row has no fixture or compile_fail doctest \
             carrying // [{id}]"
        ),
        Evidence::Unit => format!(
            "App-build or Runtime row has no unit test citing {}",
            scan::snake(id)
        ),
    }
}

/// `pub const NAME: <ty> = <int>;` definitions in `crates/noctis-codec/src`.
fn codec_constants(repo: &Repo<'_>, problems: &mut Problems) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for path in repo.files(CODEC_SRC) {
        if !is_rs(&path) {
            continue;
        }
        let Some(text) = repo.read(&path, problems) else {
            continue;
        };
        out.extend(text.lines().filter_map(pub_const));
    }
    out
}

/// Parses one `pub const NAME: <ty> = <int>;` line (K7: plain `pub` only).
fn pub_const(line: &str) -> Option<(String, u64)> {
    let rest = line.trim_start().strip_prefix("pub const ")?;
    let (name, rest) = rest.split_once(':')?;
    let name = name.trim();
    let (_, value) = rest.split_once('=')?;
    let value = value.trim().strip_suffix(';')?.trim();
    // An optional integer type suffix (`11usize`) is allowed.
    let end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '_'))
        .unwrap_or(value.len());
    let suffix = &value[end..];
    let suffix_ok = suffix.is_empty()
        || [
            "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
        ]
        .contains(&suffix);
    if !suffix_ok || !catalog::is_const_name(name) {
        return None;
    }
    Some((name.to_owned(), catalog::parse_int(&value[..end])?))
}

/// K1-K7: the catalog's constants match `noctis-codec`, unless pending.
fn check_constants(repo: &Repo<'_>, catalog: &Catalog, pending: &Pending, problems: &mut Problems) {
    let Some(table) = &catalog.constants else {
        return;
    };
    let defined = codec_constants(repo, problems);
    let is_pending = pending.checks.contains(pending::CUSTOM_ID_CONSTANTS);
    for (name, value) in table {
        let subject = format!("custom-id constant {name}");
        let defs: Vec<u64> = defined
            .iter()
            .filter(|(n, _)| n == name)
            .map(|&(_, v)| v)
            .collect();
        if is_pending {
            if !defs.is_empty() {
                problems.add(
                    subject,
                    "noctis-codec defines it; remove the custom-id-constants pending entry",
                );
            }
            continue;
        }
        match defs.as_slice() {
            [] => problems.add(subject, "not defined in noctis-codec"),
            [v] if v == value => {}
            [v] => problems.add(
                subject,
                format!("catalog says {value}, noctis-codec says {v}"),
            ),
            many => problems.add(subject, format!("defined {} times", many.len())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_id_shape() {
        for ok in ["CMD-01", "CV2-13", "META-05", "GW-07"] {
            assert!(is_rule_id(ok), "{ok}");
        }
        for bad in [
            "C-01", "CMDXX-01", "cmd-01", "CMD-1", "CMD-001", "2CV-01", "CMD01",
        ] {
            assert!(!is_rule_id(bad), "{bad}");
        }
    }

    #[test]
    fn k7_pub_const_lines() {
        assert_eq!(
            pub_const("pub const REF_LEN: usize = 11;"),
            Some(("REF_LEN".to_owned(), 11))
        );
        assert_eq!(
            pub_const("    pub const BIG: u64 = 1_000;"),
            Some(("BIG".to_owned(), 1000))
        );
        assert_eq!(pub_const("const REF_LEN: usize = 11;"), None);
        assert_eq!(pub_const("pub(crate) const REF_LEN: usize = 11;"), None);
        assert_eq!(pub_const("pub const X: usize = Y + 1;"), None);
    }

    #[test]
    fn fixture_names() {
        assert_eq!(fixture_name("MSG-03_all_with_ids.rs"), Some("MSG-03"));
        for bad in [
            "msg-03_x.rs",
            "MSG-03.rs",
            "MSG-03_.rs",
            "MSG-03_Bad.rs",
            "MSG-03_a__b.rs",
        ] {
            assert_eq!(fixture_name(bad), None, "{bad}");
        }
    }
}
