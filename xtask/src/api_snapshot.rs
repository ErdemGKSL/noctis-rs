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
//! run once. An external re-export stays a `pub use` line.
//!
//! rustdoc names another crate's item by its defining path
//! (`noctis_codec::route::Route`), so before expanding, every line is
//! rewritten with each internal crate's canonical paths: the defining path of
//! each item it re-exports from its own modules, mapped to the re-export's
//! public path (`noctis_codec::Route`), which the facade mapping then turns
//! into a facade path. Finally the [`HIDDEN`] paths, items that public
//! signatures name on purpose but the facade only reaches through its hidden
//! `__private` module, are rewritten. The result must not name any
//! `noctis_*` path.
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

use crate::public_api::{self, Kind};

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
/// Internal paths public signatures name on purpose although the facade
/// re-exports them only through its hidden `__private` module (or not at
/// all), with their snapshot spelling: the `#[handler]` marker that component
/// constructors require (FR-58), the sealed supertraits of the placement
/// traits (CV2-05), which nothing outside `noctis-ui` can name, and the sealed
/// supertraits of `EventPayload`, `Descriptor`, the Responder traits,
/// `SuggestionValue`, `NamespacedPort`, `IntoPort`, `RawHttp`, the static
/// placement traits and `Ownership`.
const HIDDEN: &[(&str, &str)] = &[
    ("noctis_ui::Handled", "noctis::__private::ui::Handled"),
    (
        "noctis_ui::placement::sealed",
        "noctis::__private::ui::sealed",
    ),
    // The sealed supertraits of the static placement traits (WH-02).
    (
        "noctis_ui::message::static_msg::sealed",
        "noctis::__private::ui::static_msg::sealed",
    ),
    // The sealed supertraits of `EventPayload` and `Descriptor` (AD-7).
    (
        "noctis_core::events::sealed",
        "noctis::__private::core::events::sealed",
    ),
    (
        "noctis_core::module::sealed",
        "noctis::__private::core::module::sealed",
    ),
    // The sealed supertraits of the Responder kinds, states and
    // `IntoResponse` (AD-11).
    (
        "noctis_core::responder::sealed",
        "noctis::__private::core::responder::sealed",
    ),
    // The sealed supertrait of `SuggestionValue` (`String`, `i64`, `f64`).
    (
        "noctis_core::autocomplete::sealed",
        "noctis::__private::core::autocomplete::sealed",
    ),
    // The sealed supertraits of `NamespacedPort` and `IntoPort` (AD-14).
    (
        "noctis_core::namespace::sealed",
        "noctis::__private::core::namespace::sealed",
    ),
    // The sealed supertrait of `Ownership` (`Owned`, `Foreign`; WH-02).
    (
        "noctis_core::webhook::sealed",
        "noctis::__private::core::webhook::sealed",
    ),
    // The sealed supertrait of `RawHttp` (AD-24: only `twilight_http::Client`).
    // The sealed supertrait of `IntoNodes` (Story 10.2).
    (
        "noctis_image::node::into_nodes::sealed",
        "noctis::__private::image::sealed",
    ),
    (
        "noctis_core::raw_http::sealed",
        "noctis::__private::core::raw_http::sealed",
    ),
];

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
    /// (defining path, public path) of each item the crate re-exports from
    /// its own modules, when they differ.
    pub(crate) canonical: Vec<(String, String)>,
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
    let path = root.join(SNAPSHOT);
    if bless {
        return match bless_to(&path, &lines) {
            Ok(hash) => {
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
    match verdict(committed.as_deref(), &lines) {
        Ok(hash) => {
            eprintln!("api-snapshot: ok ({} lines, hash {hash})", lines.len());
            ExitCode::SUCCESS
        }
        Err(report) => {
            for line in report {
                eprintln!("{line}");
            }
            ExitCode::FAILURE
        }
    }
}

/// S4: writes `render(lines)` to `path`; returns its hash.
fn bless_to(path: &Path, lines: &[String]) -> std::io::Result<String> {
    let text = render(lines);
    fs::write(path, &text)?;
    Ok(public_api::hash(text.as_bytes()))
}

/// S1-S3: the hash when `committed` (absent for a missing file) equals the
/// rendered `lines`, else the `-`/`+` lines followed by the stale message.
pub(crate) fn verdict(committed: Option<&[u8]>, lines: &[String]) -> Result<String, Vec<String>> {
    let text = render(lines);
    if committed == Some(text.as_bytes()) {
        return Ok(public_api::hash(text.as_bytes()));
    }
    let old = committed.map(String::from_utf8_lossy).unwrap_or_default();
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut report = diff(&old, &new);
    report.push(format!(
        "api-snapshot: {SNAPSHOT} is stale; run cargo run --release -p xtask -- api-snapshot --bless, then update the Usage Skill and its API hash"
    ));
    Err(report)
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
    let crates = discover(|name| run_crate(root, name)).map_err(|e| vec![e])?;
    assemble(FACADE, &crates)
}

/// Fetches the facade and every internal crate its re-exports reach, each once.
fn discover(
    mut fetch: impl FnMut(&str) -> Result<CrateApi, String>,
) -> Result<BTreeMap<String, CrateApi>, String> {
    let mut crates: BTreeMap<String, CrateApi> = BTreeMap::new();
    let mut queue = vec![FACADE.to_owned()];
    while let Some(name) = queue.pop() {
        if crates.contains_key(&name) {
            continue;
        }
        let api = fetch(&name)?;
        for reexport in &api.reexports {
            if let Some(internal) = reexport.internal_crate(&name) {
                queue.push(internal.to_owned());
            }
        }
        crates.insert(name, api);
    }
    Ok(crates)
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
    let canonical =
        canonical_from_rustdoc_json(&json, name).map_err(|err| format!("{json_rel}: {err}"))?;
    Ok(CrateApi {
        lines,
        reexports,
        canonical,
    })
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

/// (defining path, public path) of every non-glob `use` item reachable from
/// the crate root that re-exports an item of `crate_name` itself, when the
/// two differ (`noctis_codec::route::Route` -> `noctis_codec::Route`).
pub(crate) fn canonical_from_rustdoc_json(
    json: &str,
    crate_name: &str,
) -> Result<Vec<(String, String)>, String> {
    let doc: Value = serde_json::from_str(json).map_err(|err| format!("invalid JSON: {err}"))?;
    let index = doc
        .get("index")
        .and_then(Value::as_object)
        .ok_or("missing index")?;
    let reexports = reexports_from_rustdoc_json(json, crate_name)?;
    let mut out = Vec::new();
    for item in index.values() {
        let Some(u) = item.pointer("/inner/use") else {
            continue;
        };
        if u.get("is_glob").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let (Some(source), Some(id)) = (
            u.get("source").and_then(Value::as_str),
            u.get("id").filter(|id| !id.is_null()),
        ) else {
            continue;
        };
        let Some(defined) = doc
            .pointer(&format!("/paths/{}/path", id_key(id)?))
            .and_then(Value::as_array)
            .map(|segs| {
                segs.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("::")
            })
        else {
            continue;
        };
        if !defined.starts_with(&format!("{crate_name}::")) {
            continue;
        }
        for r in reexports
            .iter()
            .filter(|r| !r.is_glob && r.source == source)
        {
            if r.path != defined {
                out.push((defined.clone(), r.path.clone()));
            }
        }
    }
    out.sort();
    out.dedup();
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
    let mut canonical: Vec<(String, String)> = crates
        .values()
        .flat_map(|api| api.canonical.iter().cloned())
        .collect();
    canonical.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    let hidden: Vec<(String, String)> = HIDDEN
        .iter()
        .map(|&(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    let lines: Vec<String> = expand(root, crates, &canonical, &mut memo, &mut errors)
        .iter()
        .map(|l| rewrite(l, &hidden))
        .collect();
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
    canonical: &[(String, String)],
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
    let raw: Vec<String> = api.lines.iter().map(|l| rewrite(l, canonical)).collect();
    let parsed = match public_api::parse(&raw.join("\n")) {
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
    // Each `pub use` line consumes one re-export, in order.
    let mut used = vec![false; api.reexports.len()];
    for line in &parsed {
        if line.keyword != Some("use") {
            out.push(line.text.clone());
            continue;
        }
        let found =
            (0..used.len()).find(|&i| !used[i] && api.reexports[i].line_path() == line.path);
        let Some(reexport) = found.map(|i| {
            used[i] = true;
            &api.reexports[i]
        }) else {
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
        let source_lines = expand(source_crate, crates, canonical, memo, errors);
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
        let own = [(reexport.source.clone(), reexport.path.clone())];
        out.extend(block.iter().map(|l| rewrite(l, &own)));
        mappings.extend(own);
    }
    mappings.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    let out: Vec<String> = out.iter().map(|l| rewrite(l, &mappings)).collect();
    memo.insert(name.to_owned(), out.clone());
    out
}

/// The lines of `lines` whose owner is `source` or lies under it (a glob
/// takes only what lies under it), plus every impl whose self type lies
/// outside the `noctis_*` crates but whose header names such a path, with its
/// members.
fn owned_by(lines: &[String], source: &str, is_glob: bool) -> Vec<String> {
    let Ok(parsed) = public_api::parse(&lines.join("\n")) else {
        return Vec::new();
    };
    let under = format!("{source}::");
    let mut in_foreign_impl = false;
    let mut out = Vec::new();
    for l in parsed {
        let owned = (!is_glob && l.owner == source) || l.owner.starts_with(&under);
        in_foreign_impl = match l.kind {
            Kind::ImplHeader { .. } => {
                let first = l.owner.split("::").next().unwrap_or_default();
                !owned && !first.starts_with("noctis_") && names_path(&l.text, source, is_glob)
            }
            Kind::Member { .. } => in_foreign_impl,
            Kind::Item => false,
        };
        if owned || in_foreign_impl {
            out.push(l.text);
        }
    }
    out
}

/// Whether `text` names `source` (a glob: a path under it) as a whole path.
fn names_path(text: &str, source: &str, is_glob: bool) -> bool {
    let bytes = text.as_bytes();
    text.match_indices(source).any(|(i, _)| {
        let boundary = i == 0 || !(is_ident_byte(bytes[i - 1]) || bytes[i - 1] == b':');
        let rest = &text[i + source.len()..];
        let whole = !rest.bytes().next().is_some_and(is_ident_byte);
        boundary && whole && (!is_glob || rest.starts_with("::"))
    })
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Rewrites whole-path occurrences of each mapping's source (A6); `mappings`
/// is sorted longest source first.
pub(crate) fn rewrite(text: &str, mappings: &[(String, String)]) -> String {
    // Derive and attribute proc macros are spelled `parent::#[derive(Name)]`.
    let mut spelled = None;
    for (source, target) in mappings {
        let (Some(from), Some(to)) = (
            public_api::proc_macro_spellings(source),
            public_api::proc_macro_spellings(target),
        ) else {
            continue;
        };
        for (from, to) in from.iter().zip(&to) {
            let current: &str = spelled.as_deref().unwrap_or(text);
            if current.contains(from.as_str()) {
                spelled = Some(current.replace(from.as_str(), to));
            }
        }
    }
    let text = spelled.as_deref().unwrap_or(text);
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
            canonical: Vec::new(),
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
    fn a13_proc_macro_beside_trait() {
        // A derive and the trait it implements share `noctis::prelude::Button`.
        let facade = api(
            "pub mod noctis\npub mod noctis::prelude\npub use noctis::prelude::Button\npub use noctis::prelude::Button",
            vec![
                reexport("noctis::prelude::Button", "noctis_macros::Button"),
                reexport("noctis::prelude::Button", "noctis_rules::Button"),
            ],
        );
        let macros = api(
            "pub mod noctis_macros\npub proc macro noctis_macros::#[derive(Button)]",
            vec![],
        );
        let rules = api(
            "pub mod noctis_rules\npub trait noctis_rules::Button\npub const noctis_rules::Button::ID: u8",
            vec![],
        );
        let mut all = crates(facade, rules);
        all.insert("noctis_macros".to_owned(), macros);
        let out = assemble("noctis", &all).unwrap();
        assert_eq!(
            out,
            [
                "pub mod noctis",
                "pub mod noctis::prelude",
                "pub proc macro noctis::prelude::#[derive(Button)]",
                "pub trait noctis::prelude::Button",
                "pub const noctis::prelude::Button::ID: u8",
            ]
        );
        assert_eq!(
            rewrite(
                "pub proc macro noctis_macros::#[handler]",
                &[(
                    "noctis_macros::handler".to_owned(),
                    "noctis::handler".to_owned()
                )]
            ),
            "pub proc macro noctis::#[handler]"
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
    fn a14_canonical_and_hidden_paths() {
        let facade = api(
            "pub mod noctis\npub use noctis::codec::Route\npub use noctis::ui::Make",
            vec![
                reexport("noctis::codec::Route", "noctis_codec::Route"),
                reexport("noctis::ui::Make", "noctis_ui::Make"),
            ],
        );
        let mut codec = api(
            "pub mod noctis_codec\npub trait noctis_codec::Route",
            vec![],
        );
        codec.canonical = vec![(
            "noctis_codec::route::Route".to_owned(),
            "noctis_codec::Route".to_owned(),
        )];
        let ui = api(
            "pub mod noctis_ui\npub trait noctis_ui::Make: noctis_codec::route::Route + noctis_ui::placement::sealed::IntoTop\npub fn noctis_ui::Make::make(self) where Self: noctis_ui::Handled",
            vec![],
        );
        let crates = BTreeMap::from([
            ("noctis".to_owned(), facade),
            ("noctis_codec".to_owned(), codec),
            ("noctis_ui".to_owned(), ui),
        ]);
        assert_eq!(
            assemble("noctis", &crates).unwrap(),
            [
                "pub mod noctis",
                "pub trait noctis::codec::Route",
                "pub trait noctis::ui::Make: noctis::codec::Route + noctis::__private::ui::sealed::IntoTop",
                "pub fn noctis::ui::Make::make(self) where Self: noctis::__private::ui::Handled",
            ]
        );
    }

    #[test]
    fn a15_canonical_from_json() {
        let json = format!(
            r#"{{"format_version": {RUSTDOC_FORMAT_VERSION}, "root": 0, "index": {{
                "0": {{"name": "c", "inner": {{"module": {{"items": [1, 2, 3]}}}}}},
                "1": {{"name": null, "inner": {{"use": {{"source": "route::Route", "name": "Route", "id": 7, "is_glob": false}}}}}},
                "2": {{"name": null, "inner": {{"use": {{"source": "other::Id", "name": "Id", "id": 8, "is_glob": false}}}}}},
                "3": {{"name": null, "inner": {{"use": {{"source": "top::Top", "name": "Top", "id": 9, "is_glob": false}}}}}}
            }}, "paths": {{
                "7": {{"path": ["c", "route", "Route"]}},
                "8": {{"path": ["other", "Id"]}},
                "9": {{"path": ["c", "Top"]}}
            }}}}"#
        );
        assert_eq!(
            canonical_from_rustdoc_json(&json, "c").unwrap(),
            [("c::route::Route".to_owned(), "c::Route".to_owned())]
        );
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

    #[test]
    fn one_source_at_two_paths() {
        let facade = api(
            "pub mod noctis\npub use noctis::prelude::Limits\npub use noctis::rules::Limits",
            vec![
                reexport("noctis::prelude::Limits", "noctis_rules::Limits"),
                reexport("noctis::rules::Limits", "noctis_rules::Limits"),
            ],
        );
        let rules = api(
            "pub struct noctis_rules::Limits\npub fn noctis_rules::Limits::x(&noctis_rules::Limits)",
            vec![],
        );
        assert_eq!(
            assemble("noctis", &crates(facade, rules)).unwrap(),
            [
                "pub mod noctis",
                "pub struct noctis::prelude::Limits",
                "pub fn noctis::prelude::Limits::x(&noctis::prelude::Limits)",
                "pub struct noctis::rules::Limits",
                "pub fn noctis::rules::Limits::x(&noctis::rules::Limits)",
            ]
        );
    }

    #[test]
    fn two_globs_in_one_module() {
        let glob = |source: &str| Reexport {
            path: "noctis::x".to_owned(),
            source: source.to_owned(),
            is_glob: true,
        };
        let facade = api(
            "pub mod noctis\npub mod noctis::x\npub use noctis::x::*\npub use noctis::x::*",
            vec![glob("noctis_a"), glob("noctis_b")],
        );
        let crates = BTreeMap::from([
            ("noctis".to_owned(), facade),
            (
                "noctis_a".to_owned(),
                api("pub mod noctis_a\npub fn noctis_a::f()", vec![]),
            ),
            (
                "noctis_b".to_owned(),
                api("pub mod noctis_b\npub fn noctis_b::g()", vec![]),
            ),
        ]);
        assert_eq!(
            assemble("noctis", &crates).unwrap(),
            [
                "pub mod noctis",
                "pub mod noctis::x",
                "pub fn noctis::x::f()",
                "pub fn noctis::x::g()",
            ]
        );
    }

    #[test]
    fn foreign_impl_naming_source() {
        let facade = api(
            "pub mod noctis\npub use noctis::rules::Violation",
            vec![reexport(
                "noctis::rules::Violation",
                "noctis_rules::Violation",
            )],
        );
        let rules = api(
            "\
pub struct noctis_rules::Violation
impl core::convert::From<noctis_rules::Violation> for alloc::string::String
pub fn alloc::string::String::from(noctis_rules::Violation) -> Self
impl core::convert::From<noctis_rules::ViolationKind> for alloc::string::String
pub fn alloc::string::String::from(noctis_rules::ViolationKind) -> Self
pub fn noctis_rules::other()",
            vec![],
        );
        assert_eq!(
            assemble("noctis", &crates(facade, rules)).unwrap(),
            [
                "pub mod noctis",
                "pub struct noctis::rules::Violation",
                "impl core::convert::From<noctis::rules::Violation> for alloc::string::String",
                "pub fn alloc::string::String::from(noctis::rules::Violation) -> Self",
            ]
        );
    }

    #[test]
    fn s1_s3_verdict() {
        let lines = vec!["a".to_owned(), "b".to_owned()];
        let stale = "api-snapshot: xtask/public-api.txt is stale; run cargo run --release -p xtask -- api-snapshot --bless, then update the Usage Skill and its API hash";
        // S1.
        assert_eq!(
            verdict(Some(b"a\nb\n"), &lines),
            Ok(public_api::hash(b"a\nb\n"))
        );
        // S2, including a missing trailing newline.
        assert_eq!(
            verdict(Some(b"a\nc\n"), &lines).unwrap_err(),
            ["-c", "+b", stale]
        );
        assert_eq!(verdict(Some(b"a\nb"), &lines).unwrap_err(), [stale]);
        // S3.
        assert_eq!(verdict(None, &lines).unwrap_err(), ["+a", "+b", stale]);
    }

    #[test]
    fn s4_bless_writes_render() {
        let path = std::env::temp_dir().join(format!("xtask-bless-{}.txt", std::process::id()));
        let lines = vec!["a".to_owned(), "b".to_owned()];
        let hash = bless_to(&path, &lines).unwrap();
        let written = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(written, render(&lines).as_bytes());
        assert_eq!(hash, public_api::hash(&written));
        assert_eq!(verdict(Some(&written), &lines), Ok(hash));
    }

    #[test]
    fn discovery_fetches_each_crate_once() {
        let mut fetched = Vec::new();
        let crates = discover(|name| {
            fetched.push(name.to_owned());
            Ok(match name {
                "noctis" => api(
                    "",
                    vec![
                        reexport("noctis::View", "noctis_core::View"),
                        reexport("noctis::Other", "noctis_core::Other"),
                        reexport("noctis::Id", "twilight_model::id::Id"),
                    ],
                ),
                "noctis_core" => api("", vec![reexport("noctis_core::View", "noctis_ui::View")]),
                _ => api("", vec![reexport("noctis_ui::Me", "noctis_ui::inner::Me")]),
            })
        })
        .unwrap();
        assert_eq!(fetched, ["noctis", "noctis_core", "noctis_ui"]);
        assert_eq!(
            crates.keys().collect::<Vec<_>>(),
            ["noctis", "noctis_core", "noctis_ui"]
        );
        assert_eq!(discover(|_| Err("boom".to_owned())).unwrap_err(), "boom");
    }
}
