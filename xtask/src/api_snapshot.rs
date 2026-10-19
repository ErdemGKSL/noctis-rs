//! `api-snapshot [--bless]`: records the facade's public API in
//! `xtask/public-api.txt` and fails when the committed file is stale (FR-49,
//! NFR-8, AD-27).
//!
//! # Tooling
//!
//! The API comes from `cargo public-api` [`CARGO_PUBLIC_API`] on [`NIGHTLY`]
//! (the only nightly use in the repo), run per package with `--all-features
//! --omit blanket-impls,auto-trait-impls` into `target/public-api`, so it never
//! touches `target/debug`. The rustdoc JSON it leaves behind must have format
//! [`RUSTDOC_FORMAT_VERSION`].
//!
//! # Assembly
//!
//! rustdoc never inlines another crate's items, so the facade's output lists
//! its re-exports of internal crates as bare `pub use` lines. Each such line
//! is replaced in place by the lines of the internal crate's own output that
//! the re-export's source owns (see [`crate::public_api`]), and every mapped
//! source path is rewritten to its facade path (whole paths only, longest
//! source first). Internal crates are expanded the same way first, and each is
//! run once. An external re-export stays a `pub use` line. The result must not
//! name any `noctis_*` path.
//!
//! # Output
//!
//! Problems are `api-snapshot: <message>` lines on stderr and exit 1; tool and
//! I/O failures stop the task. A stale snapshot prints `-`/`+` lines first.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

use serde_json::Value;

use crate::public_api::{self, Line};

/// The pinned nightly toolchain.
pub(crate) const NIGHTLY: &str = "nightly-2026-10-15";
/// The pinned `cargo-public-api` version.
pub(crate) const CARGO_PUBLIC_API: &str = "0.52.0";
/// The rustdoc JSON format [`NIGHTLY`] emits.
pub(crate) const RUSTDOC_FORMAT_VERSION: u64 = 61;
/// The snapshot's repo-relative path.
pub(crate) const SNAPSHOT: &str = "xtask/public-api.txt";
/// The facade crate.
const FACADE: &str = "noctis";
/// The target directory, relative to the repo root.
const TARGET_DIR: &str = "target/public-api";

/// One `use` item of a crate's rustdoc JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reexport {
    /// The re-export's own path; for a glob, the module it sits in.
    pub(crate) path: String,
    /// The path as written in the `use`.
    pub(crate) source: String,
    pub(crate) is_glob: bool,
}

impl Reexport {
    /// The path the `pub use` line shows.
    fn line_path(&self) -> String {
        if self.is_glob {
            format!("{}::*", self.path)
        } else {
            self.path.clone()
        }
    }

    /// The internal crate the source names, if any other than `current`.
    fn internal_crate(&self, current: &str) -> Option<&str> {
        let first = self.source.split("::").next().unwrap_or_default();
        (first.starts_with("noctis_") && first != current).then_some(first)
    }
}

/// One crate's `cargo public-api` lines and rustdoc-JSON re-exports.
#[derive(Debug, Clone, Default)]
pub(crate) struct CrateApi {
    pub(crate) lines: Vec<String>,
    pub(crate) reexports: Vec<Reexport>,
}

/// Runs the task on this repository.
pub(crate) fn run(bless: bool) -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let lines = match snapshot(&root) {
        Ok(lines) => lines,
        Err(errors) => {
            for error in errors {
                eprintln!("api-snapshot: {error}");
            }
            return ExitCode::FAILURE;
        }
    };
    let text = render(&lines);
    let hash = public_api::hash(text.as_bytes());
    let path = root.join(SNAPSHOT);
    if bless {
        return match fs::write(&path, &text) {
            Ok(()) => {
                eprintln!(
                    "api-snapshot: wrote {SNAPSHOT} ({} lines, hash {hash})",
                    lines.len()
                );
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("api-snapshot: {SNAPSHOT}: cannot write: {err}");
                ExitCode::FAILURE
            }
        };
    }
    let committed = fs::read(&path).ok();
    if committed.as_deref() == Some(text.as_bytes()) {
        eprintln!("api-snapshot: ok ({} lines, hash {hash})", lines.len());
        return ExitCode::SUCCESS;
    }
    let old = committed
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = lines.iter().map(String::as_str).collect();
    for line in diff(&old, &new) {
        eprintln!("{line}");
    }
    eprintln!(
        "api-snapshot: {SNAPSHOT} is stale; run cargo run --release -p xtask -- api-snapshot --bless, then update the Usage Skill and its API hash"
    );
    ExitCode::FAILURE
}

/// The snapshot file's contents: LF lines, one trailing newline.
pub(crate) fn render(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// `-` lines only in `old`, then `+` lines only in `new`, each in order.
pub(crate) fn diff(old: &[&str], new: &[&str]) -> Vec<String> {
    let old_set: BTreeSet<&str> = old.iter().copied().collect();
    let new_set: BTreeSet<&str> = new.iter().copied().collect();
    let removed = old
        .iter()
        .filter(|l| !new_set.contains(*l))
        .map(|l| format!("-{l}"));
    let added = new
        .iter()
        .filter(|l| !old_set.contains(*l))
        .map(|l| format!("+{l}"));
    removed.chain(added).collect()
}

/// Runs the tool on the facade and every internal crate it reaches, then
/// assembles the snapshot.
fn snapshot(root: &Path) -> Result<Vec<String>, Vec<String>> {
    check_tool(root).map_err(|e| vec![e])?;
    let mut crates: BTreeMap<String, CrateApi> = BTreeMap::new();
    let mut queue = vec![FACADE.to_owned()];
    while let Some(name) = queue.pop() {
        if crates.contains_key(&name) {
            continue;
        }
        let api = run_crate(root, &name).map_err(|e| vec![e])?;
        for reexport in &api.reexports {
            if let Some(internal) = reexport.internal_crate(&name) {
                queue.push(internal.to_owned());
            }
        }
        crates.insert(name, api);
    }
    assemble(FACADE, &crates)
}

/// `cargo +NIGHTLY …` with the parent's toolchain overrides removed.
fn cargo(root: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg(format!("+{NIGHTLY}"))
        .env("CARGO_TARGET_DIR", root.join(TARGET_DIR))
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO")
        .env_remove("RUSTC")
        .env_remove("RUSTDOC");
    cmd
}

/// S5: the pinned tool on the pinned nightly.
fn check_tool(root: &Path) -> Result<(), String> {
    let want = format!("cargo-public-api {CARGO_PUBLIC_API}");
    let found = match cargo(root).args(["public-api", "--version"]).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if stdout == want {
                return Ok(());
            }
            stdout
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            format!("{}{}", stdout.trim(), stderr.trim())
                .lines()
                .next()
                .unwrap_or("no output")
                .to_owned()
        }
        Err(err) => err.to_string(),
    };
    Err(format!(
        "{want} on {NIGHTLY} is required (found: {found}); install: rustup toolchain install {NIGHTLY} --profile minimal && cargo install cargo-public-api --version {CARGO_PUBLIC_API} --locked"
    ))
}

/// Runs the tool on crate `name` and reads its rustdoc JSON.
fn run_crate(root: &Path, name: &str) -> Result<CrateApi, String> {
    let pkg = name.replace('_', "-");
    let out = cargo(root)
        .arg("public-api")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .args(["-p", &pkg, "--all-features"])
        .args([
            "--omit",
            "blanket-impls,auto-trait-impls",
            "--color",
            "never",
        ])
        .output()
        .map_err(|err| format!("cargo public-api -p {pkg} failed ({err})"))?;
    if !out.status.success() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        return Err(format!("cargo public-api -p {pkg} failed ({})", out.status));
    }
    let stdout = String::from_utf8(out.stdout)
        .map_err(|_| format!("cargo public-api -p {pkg} printed non-UTF-8 output"))?;
    let lines = stdout.lines().map(str::to_owned).collect();
    let json_rel = format!("{TARGET_DIR}/doc/{name}.json");
    let json = fs::read_to_string(root.join(&json_rel))
        .map_err(|err| format!("{json_rel}: cannot read: {err}"))?;
    let reexports =
        reexports_from_rustdoc_json(&json, name).map_err(|err| format!("{json_rel}: {err}"))?;
    Ok(CrateApi { lines, reexports })
}

/// The `use` items reachable from the crate root, with their paths (A5, A11).
pub(crate) fn reexports_from_rustdoc_json(
    json: &str,
    crate_name: &str,
) -> Result<Vec<Reexport>, String> {
    let doc: Value = serde_json::from_str(json).map_err(|err| format!("invalid JSON: {err}"))?;
    match doc.get("format_version").and_then(Value::as_u64) {
        Some(RUSTDOC_FORMAT_VERSION) => {}
        found => {
            let found = found.map_or_else(|| "missing".to_owned(), |v| v.to_string());
            return Err(format!(
                "rustdoc JSON format {found}, expected {RUSTDOC_FORMAT_VERSION}; the pinned nightly is {NIGHTLY}"
            ));
        }
    }
    let root = doc.get("root").ok_or("missing root")?;
    let index = doc
        .get("index")
        .and_then(Value::as_object)
        .ok_or("missing index")?;
    let mut out = Vec::new();
    let mut stack = vec![(id_key(root)?, crate_name.to_owned())];
    let mut seen = BTreeSet::new();
    while let Some((id, path)) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let items = index
            .get(&id)
            .and_then(|item| item.pointer("/inner/module/items"))
            .and_then(Value::as_array)
            .ok_or_else(|| format!("item {id} is not a module"))?;
        for child_id in items {
            let child_id = id_key(child_id)?;
            let Some(child) = index.get(&child_id) else {
                continue;
            };
            if child.pointer("/inner/module").is_some() {
                let name = child
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("module {child_id} has no name"))?;
                stack.push((child_id, format!("{path}::{name}")));
            } else if let Some(u) = child.pointer("/inner/use") {
                let field = |key: &str| {
                    u.get(key)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| format!("use item {child_id} has no {key}"))
                };
                let is_glob = u.get("is_glob").and_then(Value::as_bool).unwrap_or(false);
                let source = field("source")?;
                let reexport_path = if is_glob {
                    path.clone()
                } else {
                    format!("{path}::{}", field("name")?)
                };
                out.push(Reexport {
                    path: reexport_path,
                    source,
                    is_glob,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.source.cmp(&b.source)));
    Ok(out)
}

/// An id value as an `index` key.
fn id_key(id: &Value) -> Result<String, String> {
    match id {
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.clone()),
        other => Err(format!("invalid id {other}")),
    }
}

/// Assembles `root`'s snapshot from the crates' outputs (A1-A10, A12).
pub(crate) fn assemble(
    root: &str,
    crates: &BTreeMap<String, CrateApi>,
) -> Result<Vec<String>, Vec<String>> {
    let mut memo = BTreeMap::new();
    let mut errors = BTreeSet::new();
    let lines = expand(root, crates, &mut memo, &mut errors);
    for line in &lines {
        if let Some(leak) = leaked_path(line) {
            errors.insert(format!(
                "{line}: names {leak}::…, which the facade does not re-export"
            ));
        }
    }
    if errors.is_empty() {
        Ok(lines)
    } else {
        Err(errors.into_iter().collect())
    }
}

/// Crate `name`'s lines with its internal re-exports expanded, in its own paths.
fn expand(
    name: &str,
    crates: &BTreeMap<String, CrateApi>,
    memo: &mut BTreeMap<String, Vec<String>>,
    errors: &mut BTreeSet<String>,
) -> Vec<String> {
    if let Some(lines) = memo.get(name) {
        return lines.clone();
    }
    let Some(api) = crates.get(name) else {
        errors.insert(format!(
            "no cargo public-api output for {}",
            name.replace('_', "-")
        ));
        return Vec::new();
    };
    let parsed = match public_api::parse(&api.lines.join("\n")) {
        Ok(parsed) => parsed,
        Err(bad) => {
            for (_, line) in bad {
                errors.insert(format!("cannot parse cargo public-api line: {line}"));
            }
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    let mut mappings: Vec<(String, String)> = Vec::new();
    for line in &parsed {
        if line.keyword != Some("use") {
            out.push(line.text.clone());
            continue;
        }
        let Some(reexport) = api.reexports.iter().find(|r| r.line_path() == line.path) else {
            errors.insert(format!(
                "pub use {}: no re-export found in rustdoc JSON",
                line.path
            ));
            continue;
        };
        let Some(source_crate) = reexport.internal_crate(name) else {
            out.push(line.text.clone());
            continue;
        };
        let source_lines = expand(source_crate, crates, memo, errors);
        let block = owned_by(&source_lines, &reexport.source, reexport.is_glob);
        if block.is_empty() {
            errors.insert(format!(
                "{}: re-export of {} matches no public item of {}",
                line.path,
                reexport.source,
                source_crate.replace('_', "-")
            ));
            continue;
        }
        out.extend(block);
        mappings.push((reexport.source.clone(), reexport.path.clone()));
    }
    mappings.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    let out: Vec<String> = out.iter().map(|l| rewrite(l, &mappings)).collect();
    memo.insert(name.to_owned(), out.clone());
    out
}

/// The lines of `lines` whose owner is `source` or lies under it (a glob
/// takes only what lies under it).
fn owned_by(lines: &[String], source: &str, is_glob: bool) -> Vec<String> {
    let Ok(parsed) = public_api::parse(&lines.join("\n")) else {
        return Vec::new();
    };
    let under = format!("{source}::");
    parsed
        .into_iter()
        .filter(|l: &Line| (!is_glob && l.owner == source) || l.owner.starts_with(&under))
        .map(|l| l.text)
        .collect()
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Rewrites whole-path occurrences of each mapping's source (A6); `mappings`
/// is sorted longest source first.
pub(crate) fn rewrite(text: &str, mappings: &[(String, String)]) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        let boundary = i == 0 || !(is_ident_byte(bytes[i - 1]) || bytes[i - 1] == b':');
        if boundary
            && let Some((source, target)) = mappings.iter().find(|(source, _)| {
                bytes[i..].starts_with(source.as_bytes())
                    && !bytes
                        .get(i + source.len())
                        .copied()
                        .is_some_and(is_ident_byte)
            })
        {
            out.push_str(&text[copied..i]);
            out.push_str(target);
            i += source.len();
            copied = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// The first `noctis_<x>` path root named in `line` (A9).
fn leaked_path(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(pos) = line[from..].find("noctis_") {
        let start = from + pos;
        let end = start
            + line[start..]
                .bytes()
                .take_while(|b| is_ident_byte(*b))
                .count();
        let boundary = start == 0 || !(is_ident_byte(bytes[start - 1]) || bytes[start - 1] == b':');
        if boundary && line[end..].starts_with("::") {
            return Some(&line[start..end]);
        }
        from = end.max(start + 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reexport(path: &str, source: &str) -> Reexport {
        Reexport {
            path: path.to_owned(),
            source: source.to_owned(),
            is_glob: false,
        }
    }

    fn api(lines: &str, reexports: Vec<Reexport>) -> CrateApi {
        CrateApi {
            lines: lines.lines().map(str::to_owned).collect(),
            reexports,
        }
    }

    const RULES: &str = "\
pub mod noctis_rules
pub mod noctis_rules::command
pub fn noctis_rules::command::validate_tree(&noctis_rules::Limits) -> u8
pub mod noctis_rules::len
pub fn noctis_rules::len::discord_len(&str) -> usize
#[non_exhaustive] pub struct noctis_rules::Limits
pub noctis_rules::Limits::button_label_len: usize
impl noctis_rules::Limits
pub fn noctis_rules::Limits::truncates(&self) -> bool
impl core::clone::Clone for noctis_rules::Limits
pub fn noctis_rules::Limits::clone(&self) -> Self
pub trait noctis_rules::LimitsExt
pub fn noctis_rules::LimitsExt::x(&noctis_rules::Limits)";

    fn crates(facade: CrateApi, rules: CrateApi) -> BTreeMap<String, CrateApi> {
        BTreeMap::from([
            ("noctis".to_owned(), facade),
            ("noctis_rules".to_owned(), rules),
        ])
    }

    #[test]
    fn a2_item_reexport() {
        let facade = api(
            "pub mod noctis\npub mod noctis::rules\npub use noctis::rules::Limits",
            vec![reexport("noctis::rules::Limits", "noctis_rules::Limits")],
        );
        // `LimitsExt` shares the prefix but is not owned by `Limits`.
        let out = assemble("noctis", &crates(facade, api(RULES, vec![]))).unwrap();
        assert_eq!(out.len(), 8);
        assert!(out.iter().all(|l| !l.contains("LimitsExt")));
        let facade = api(
            "pub mod noctis\npub mod noctis::rules\npub use noctis::rules::Limits\npub use noctis::rules::LimitsExt",
            vec![
                reexport("noctis::rules::Limits", "noctis_rules::Limits"),
                reexport("noctis::rules::LimitsExt", "noctis_rules::LimitsExt"),
            ],
        );
        let out = assemble("noctis", &crates(facade, api(RULES, vec![]))).unwrap();
        assert_eq!(
            out,
            [
                "pub mod noctis",
                "pub mod noctis::rules",
                "#[non_exhaustive] pub struct noctis::rules::Limits",
                "pub noctis::rules::Limits::button_label_len: usize",
                "impl noctis::rules::Limits",
                "pub fn noctis::rules::Limits::truncates(&self) -> bool",
                "impl core::clone::Clone for noctis::rules::Limits",
                "pub fn noctis::rules::Limits::clone(&self) -> Self",
                "pub trait noctis::rules::LimitsExt",
                "pub fn noctis::rules::LimitsExt::x(&noctis::rules::Limits)",
            ]
        );
    }

    #[test]
    fn a3_a4_module_and_nested() {
        let facade = api(
            "pub mod noctis\npub use noctis::rules::command\npub use noctis::rules::discord_len\npub use noctis::rules::Limits",
            vec![
                reexport("noctis::rules::command", "noctis_rules::command"),
                reexport(
                    "noctis::rules::discord_len",
                    "noctis_rules::len::discord_len",
                ),
                reexport("noctis::rules::Limits", "noctis_rules::Limits"),
            ],
        );
        let out = assemble("noctis", &crates(facade, api(RULES, vec![]))).unwrap();
        assert_eq!(out[1], "pub mod noctis::rules::command");
        assert_eq!(
            out[2],
            "pub fn noctis::rules::command::validate_tree(&noctis::rules::Limits) -> u8"
        );
        assert_eq!(out[3], "pub fn noctis::rules::discord_len(&str) -> usize");
        assert!(!out.iter().any(|l| l.contains("len::")));
    }

    #[test]
    fn a5_glob() {
        let json = format!(
            r#"{{"format_version": {RUSTDOC_FORMAT_VERSION}, "root": 0, "index": {{
                "0": {{"name": "noctis", "inner": {{"module": {{"items": [1]}}}}}},
                "1": {{"name": "x", "inner": {{"module": {{"items": [2, 3]}}}}}},
                "2": {{"name": null, "inner": {{"use": {{"source": "noctis_rules::command", "name": "command", "id": 9, "is_glob": true}}}}}},
                "3": {{"name": null, "inner": {{"use": {{"source": "twilight_model::id::Id", "name": "Id", "id": null, "is_glob": false}}}}}}
            }}}}"#
        );
        let reexports = reexports_from_rustdoc_json(&json, "noctis").unwrap();
        assert_eq!(
            reexports,
            [
                Reexport {
                    path: "noctis::x".to_owned(),
                    source: "noctis_rules::command".to_owned(),
                    is_glob: true
                },
                reexport("noctis::x::Id", "twilight_model::id::Id"),
            ]
        );
        let facade = api(
            "pub mod noctis\npub mod noctis::x\npub use noctis::x::*\npub use noctis::x::Id",
            reexports,
        );
        let rules = api(
            "pub mod noctis_rules::command\npub fn noctis_rules::command::f()",
            vec![],
        );
        let out = assemble("noctis", &crates(facade, rules)).unwrap();
        assert_eq!(
            out,
            [
                "pub mod noctis",
                "pub mod noctis::x",
                "pub fn noctis::x::f()",
                "pub use noctis::x::Id",
            ]
        );
    }

    #[test]
    fn a6_boundary_safe_rewrite() {
        let mappings = vec![
            (
                "noctis_rules::len::discord_len".to_owned(),
                "noctis::rules::discord_len".to_owned(),
            ),
            (
                "noctis_rules::Limits".to_owned(),
                "noctis::rules::Limits".to_owned(),
            ),
        ];
        assert_eq!(
            rewrite(
                "noctis_rules::LimitsExt noctis_rules::Limits::truncates(&noctis_rules::Limits) xnoctis_rules::Limits a::noctis_rules::Limits noctis_rules::len::discord_len é",
                &mappings
            ),
            "noctis_rules::LimitsExt noctis::rules::Limits::truncates(&noctis::rules::Limits) xnoctis_rules::Limits a::noctis_rules::Limits noctis::rules::discord_len é"
        );
    }

    #[test]
    fn a7_recursion() {
        let facade = api(
            "pub mod noctis\npub use noctis::prelude::View",
            vec![reexport(
                "noctis::prelude::View",
                "noctis_core::prelude::View",
            )],
        );
        let core = api(
            "pub mod noctis_core\npub mod noctis_core::prelude\npub use noctis_core::prelude::View",
            vec![reexport("noctis_core::prelude::View", "noctis_ui::View")],
        );
        let ui = api(
            "pub mod noctis_ui\npub trait noctis_ui::View\npub fn noctis_ui::View::render(&self, &noctis_ui::View)",
            vec![],
        );
        let crates = BTreeMap::from([
            ("noctis".to_owned(), facade),
            ("noctis_core".to_owned(), core),
            ("noctis_ui".to_owned(), ui),
        ]);
        let out = assemble("noctis", &crates).unwrap();
        assert_eq!(
            out,
            [
                "pub mod noctis",
                "pub trait noctis::prelude::View",
                "pub fn noctis::prelude::View::render(&self, &noctis::prelude::View)",
            ]
        );
    }

    #[test]
    fn a8_external_reexport_kept() {
        let facade = api(
            "pub mod noctis\npub use noctis::prelude::Id",
            vec![reexport("noctis::prelude::Id", "twilight_model::id::Id")],
        );
        let crates = BTreeMap::from([("noctis".to_owned(), facade)]);
        assert_eq!(
            assemble("noctis", &crates).unwrap(),
            ["pub mod noctis", "pub use noctis::prelude::Id"]
        );
    }

    #[test]
    fn a9_leak() {
        let facade = api(
            "pub mod noctis\npub use noctis::rules::command",
            vec![reexport("noctis::rules::command", "noctis_rules::command")],
        );
        let errors = assemble("noctis", &crates(facade, api(RULES, vec![]))).unwrap_err();
        assert_eq!(
            errors,
            [
                "pub fn noctis::rules::command::validate_tree(&noctis_rules::Limits) -> u8: names noctis_rules::…, which the facade does not re-export"
            ]
        );
        assert_eq!(leaked_path("a::noctis_x::Y"), None);
        assert_eq!(leaked_path("&noctis_x::Y"), Some("noctis_x"));
        assert_eq!(leaked_path("noctis_x"), None);
    }

    #[test]
    fn a10_empty_expansion() {
        let facade = api(
            "pub mod noctis\npub use noctis::rules::Hidden",
            vec![reexport("noctis::rules::Hidden", "noctis_rules::Hidden")],
        );
        let errors = assemble("noctis", &crates(facade, api(RULES, vec![]))).unwrap_err();
        assert_eq!(
            errors,
            [
                "noctis::rules::Hidden: re-export of noctis_rules::Hidden matches no public item of noctis-rules"
            ]
        );
    }

    #[test]
    fn a11_json_errors() {
        let check = |json: &str, needle: &str| {
            let err = reexports_from_rustdoc_json(json, "noctis").unwrap_err();
            assert!(err.contains(needle), "{err}");
        };
        check("{", "invalid JSON");
        check(
            r#"{"format_version": 60, "root": 0, "index": {}}"#,
            "rustdoc JSON format 60, expected 61; the pinned nightly is nightly-2026-10-15",
        );
        check(r#"{"format_version": 61, "index": {}}"#, "missing root");
        check(r#"{"format_version": 61, "root": 0}"#, "missing index");
    }

    #[test]
    fn a12_unmapped_use() {
        let facade = api("pub mod noctis\npub use noctis::rules::Limits", vec![]);
        let errors = assemble("noctis", &crates(facade, api(RULES, vec![]))).unwrap_err();
        assert_eq!(
            errors,
            ["pub use noctis::rules::Limits: no re-export found in rustdoc JSON"]
        );
    }

    #[test]
    fn l16_garbage_in_tool_output() {
        let facade = api("pub mod noctis\ngarbage", vec![]);
        let crates = BTreeMap::from([("noctis".to_owned(), facade)]);
        assert_eq!(
            assemble("noctis", &crates).unwrap_err(),
            ["cannot parse cargo public-api line: garbage"]
        );
    }

    #[test]
    fn diff_lists_removed_then_added() {
        assert_eq!(
            diff(&["a", "b", "c"], &["a", "d", "c", "e"]),
            ["-b", "+d", "+e"]
        );
        assert_eq!(diff(&[], &["a"]), ["+a"]);
        assert_eq!(render(&["a".to_owned(), "b".to_owned()]), "a\nb\n");
    }
}
