//! `deps-check`: checks every workspace dependency edge against the spine's
//! Invariants graph (AD-1, AD-2) without building anything.
//!
//! The workspace is read with `cargo metadata --no-deps`. Every violation is
//! reported at once on stderr as
//! `deps-check: <from> -> <to> (<kind>): <reason> [<rule-id>]`
//! (crate-level rules omit `-> <to> (<kind>)`), sorted and deduplicated.
//!
//! The graph labels the `noctis-macros -> noctis-image` edge "tw module only,
//! render feature off". The `tw` module includes `tw::theme` (the Theme file
//! parser the image macros run at expansion). Only the "render feature off"
//! half is enforced here (`render-on`); "tw module only" is a source-level
//! property that dependency metadata cannot see, so deps-check does not check
//! it.
//!
//! The `takumi-gated` rule (AD-15) keeps takumi out of default builds: every
//! `noctis-image` dependency whose name starts with `takumi` must be an
//! optional normal dependency that only the `render` feature turns on.
//!
//! The `http-client` rule (AD-24) keeps one Discord HTTP client per App: only
//! `noctis-core`, `noctis-gateway` and the facade may depend on
//! `twilight-http`, and only `noctis-core` may construct a client, so the
//! sources of `noctis-gateway` and the facade (`src/**/*.rs`) must not contain
//! `Client::new(`, `Client::builder(` or `ClientBuilder`.
//!
//! The `session-store` rule (AD-21) keeps `noctis-gateway` the only reader and
//! writer of shard sessions: the sources (`src/**/*.rs`) of every workspace
//! member other than `noctis-gateway` and `noctis-ports` (which defines the
//! port) must not call `get`, `put` or `delete` on a `session_store` field
//! or through `SessionStore::`. Files named `tests.rs` and files under a
//! `tests` directory are exempt.
//!
//! The `asset-http` rule (AD-15) keeps the `AssetFetcher` the only
//! non-Discord HTTP client of the workspace: no member depends, in any kind,
//! on another HTTP client crate (`reqwest`, `ureq`, `isahc`, `surf`,
//! `attohttpc`, `curl`, `awc`, `minreq`, `hyper-tls`); only `noctis-image`
//! depends on `hyper-rustls`, as an optional normal dependency that only the
//! `render` feature turns on; and among the `noctis-image` sources
//! (`src/**/*.rs`, test files exempt) only `src/remote/fetch.rs` names the
//! hyper-util legacy client.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};

/// The facade crate; the only internal crate bot code depends on.
const FACADE: &str = "noctis";
const TESTKIT: &str = "noctis-testkit";
const IMAGE: &str = "noctis-image";
const MACROS: &str = "noctis-macros";
const RENDER: &str = "render";
/// The facade feature that reaches `noctis-testkit`.
const TESTKIT_FEATURE: &str = "testkit";

/// The prefix of the takumi renderer crates, gated behind `noctis-image/render`.
const TAKUMI_PREFIX: &str = "takumi";

/// Whether `name` is a takumi crate (`takumi`, `takumi-core`, ...).
fn is_takumi(name: &str) -> bool {
    name.starts_with(TAKUMI_PREFIX)
}

/// The Discord HTTP client crate (AD-24).
const TWILIGHT_HTTP: &str = "twilight-http";
/// The crates that may depend on `twilight-http`.
const HTTP_CLIENT_DEPENDENTS: [&str; 3] = ["noctis-core", "noctis-gateway", FACADE];
/// The crates whose sources are scanned for client construction; only
/// `noctis-core` builds the client.
const HTTP_CLIENT_SCANNED: [&str; 2] = ["noctis-gateway", FACADE];
/// Source text that constructs a twilight HTTP client.
const CLIENT_CONSTRUCTORS: [&str; 3] = ["Client::new(", "Client::builder(", "ClientBuilder"];

/// The TLS connector of the `AssetFetcher` (AD-15).
const HYPER_RUSTLS: &str = "hyper-rustls";
/// HTTP client crates no member may depend on: the `AssetFetcher` is the
/// only non-Discord HTTP (AD-15).
const BANNED_HTTP: [&str; 9] = [
    "reqwest",
    "ureq",
    "isahc",
    "surf",
    "attohttpc",
    "curl",
    "awc",
    "minreq",
    "hyper-tls",
];
/// The one `noctis-image` source that may name the legacy client.
const ASSET_FETCHER: &str = "src/remote/fetch.rs";
/// The hyper-util legacy client, split so this file never holds the
/// whole name.
const LEGACY_CLIENT: [&str; 2] = ["legacy::", "Client"];

/// Whether `name` is a crate the `asset-http` rule tracks.
fn is_asset_http(name: &str) -> bool {
    name == HYPER_RUSTLS || BANNED_HTTP.contains(&name)
}

/// Whether `line` names the hyper-util legacy client.
fn names_legacy_client(line: &str) -> bool {
    line.contains(&LEGACY_CLIENT.concat())
}

/// The crates that may call the `SessionStore` (AD-21): the gateway, and the
/// ports crate that defines it.
const SESSION_STORE_CALLERS: [&str; 2] = ["noctis-gateway", "noctis-ports"];
/// The receivers of a `SessionStore` call; joined with [`SESSION_STORE_OPS`]
/// at scan time, so this file never holds a whole pattern.
const SESSION_STORE_RECEIVERS: [&str; 2] = ["session_store.", "SessionStore::"];
/// The `SessionStore` ops.
const SESSION_STORE_OPS: [&str; 3] = ["get(", "put(", "delete("];

/// Whether `line` calls the `SessionStore`.
fn calls_session_store(line: &str) -> bool {
    SESSION_STORE_RECEIVERS.iter().any(|receiver| {
        line.match_indices(receiver).any(|(at, _)| {
            let rest = &line[at + receiver.len()..];
            SESSION_STORE_OPS.iter().any(|op| rest.starts_with(op))
        })
    })
}

/// The internal crates the spine's Invariants graph draws.
const CRATES: [&str; 12] = [
    "noctis",
    "noctis-rules",
    "noctis-codec",
    "noctis-ui",
    "noctis-ports",
    "noctis-gateway",
    "noctis-core",
    "noctis-macros",
    "noctis-i18n",
    "noctis-image",
    "noctis-prefix",
    "noctis-testkit",
];

/// Ungated drawn edges (from, to), excluding the facade's edges.
const GRAPH: &[(&str, &str)] = &[
    ("noctis-prefix", "noctis-core"),
    ("noctis-testkit", "noctis-core"),
    ("noctis-testkit", "noctis-gateway"),
    ("noctis-testkit", "noctis-ports"),
    ("noctis-testkit", "noctis-ui"),
    ("noctis-testkit", "noctis-rules"),
    ("noctis-core", "noctis-gateway"),
    ("noctis-core", "noctis-i18n"),
    ("noctis-core", "noctis-ui"),
    ("noctis-core", "noctis-ports"),
    ("noctis-core", "noctis-codec"),
    ("noctis-core", "noctis-rules"),
    ("noctis-i18n", "noctis-ports"),
    ("noctis-i18n", "noctis-rules"),
    ("noctis-gateway", "noctis-ports"),
    ("noctis-ui", "noctis-codec"),
    ("noctis-ui", "noctis-rules"),
    ("noctis-codec", "noctis-rules"),
    ("noctis-macros", "noctis-rules"),
    // Labelled "tw module only, render feature off"; see the module docs.
    ("noctis-macros", "noctis-image"),
];

/// Feature-gated drawn edges (from, to, feature).
const GATED: &[(&str, &str, &str)] = &[
    ("noctis-image", "noctis-core", RENDER),
    ("noctis-image", "noctis-ports", RENDER),
    ("noctis-image", "noctis-ui", RENDER),
    (FACADE, TESTKIT, TESTKIT_FEATURE),
];

/// Whether `name` is an internal crate name (`noctis` or `noctis-*`).
fn is_internal(name: &str) -> bool {
    name == FACADE || name.starts_with("noctis-")
}

/// The feature that gates the edge `from -> to`, if it is a gated edge.
fn gate(from: &str, to: &str) -> Option<&'static str> {
    GATED
        .iter()
        .find(|&&(a, b, _)| a == from && b == to)
        .map(|&(_, _, g)| g)
}

/// Dependency kind as `cargo metadata` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Normal,
    Build,
    Dev,
}

impl From<DependencyKind> for Kind {
    fn from(kind: DependencyKind) -> Self {
        match kind {
            DependencyKind::Development => Self::Dev,
            DependencyKind::Build => Self::Build,
            // An unknown future kind is checked as strictly as a normal one.
            _ => Self::Normal,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "normal",
            Self::Build => "build",
            Self::Dev => "dev",
        })
    }
}

/// A dependency on an internal crate or another workspace member.
#[derive(Debug, Clone)]
pub(crate) struct Dep {
    /// The dependency's package name (renames ignored), used for edge matching.
    name: String,
    /// The name the depending manifest uses for it in features (rename or name).
    key: String,
    kind: Kind,
    optional: bool,
    uses_default_features: bool,
    features: Vec<String>,
}

/// A workspace member with its features and its checked dependencies.
#[derive(Debug, Clone)]
pub(crate) struct Member {
    name: String,
    features: BTreeMap<String, Vec<String>>,
    deps: Vec<Dep>,
}

impl Member {
    /// The package name behind the feature-syntax dependency name `key`.
    fn package_for(&self, key: &str) -> Option<&str> {
        self.deps
            .iter()
            .find(|d| d.key == key)
            .map(|d| d.name.as_str())
    }

    /// Everything feature `start` turns on, transitively.
    fn closure(&self, start: &str) -> Closure {
        let mut closure = Closure::default();
        let mut stack = vec![start.to_owned()];
        while let Some(feature) = stack.pop() {
            let Some(values) = self.features.get(&feature) else {
                closure.features.insert(feature);
                continue;
            };
            if !closure.features.insert(feature) {
                continue;
            }
            for value in values {
                if let Some(key) = value.strip_prefix("dep:") {
                    if let Some(package) = self.package_for(key) {
                        closure.activated.insert(package.to_owned());
                    }
                } else if let Some((dep, dep_feature)) = value.split_once('/') {
                    let (key, weak) = dep.strip_suffix('?').map_or((dep, false), |k| (k, true));
                    if let Some(package) = self.package_for(key) {
                        if !weak {
                            closure.activated.insert(package.to_owned());
                        }
                        closure
                            .dep_features
                            .insert((key.to_owned(), dep_feature.to_owned()));
                    }
                } else {
                    stack.push(value.clone());
                }
            }
        }
        closure
    }
}

/// The transitive effect of enabling one feature.
#[derive(Debug, Default)]
struct Closure {
    /// Own features turned on, including the start feature.
    features: BTreeSet<String>,
    /// Package names of optional dependencies turned on.
    activated: BTreeSet<String>,
    /// `(key, feature)` pairs requested from dependencies (`k/f`, `k?/f`), where
    /// `k` is the rename-aware dependency key.
    dep_features: BTreeSet<(String, String)>,
}

/// A source line that constructs a twilight HTTP client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceSite {
    /// The crate whose source holds the line.
    krate: String,
    /// `path:line`, relative to the workspace root.
    location: String,
}

/// The workspace members, their internal dependency edges and the client
/// constructions found in scanned sources.
#[derive(Debug, Clone)]
pub(crate) struct Workspace {
    crates: Vec<Member>,
    client_sites: Vec<SourceSite>,
    session_sites: Vec<SourceSite>,
    /// `noctis-image` source lines (other than the `AssetFetcher`'s) that
    /// name the legacy client.
    legacy_client_sites: Vec<SourceSite>,
}

impl Workspace {
    /// Keeps workspace members and every dependency on a member, on an
    /// internal (`noctis`/`noctis-*`) package, wherever that package lives, or
    /// on `twilight-http` or a takumi crate; scans the gateway's and the facade's sources for
    /// client construction.
    pub(crate) fn from_metadata(metadata: &Metadata) -> Self {
        let packages = metadata.workspace_packages();
        let names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        let root = metadata.workspace_root.as_std_path();
        let client_sites = packages
            .iter()
            .filter(|p| HTTP_CLIENT_SCANNED.contains(&p.name.as_str()))
            .flat_map(|p| {
                let src = p
                    .manifest_path
                    .parent()
                    .map(|dir| dir.as_std_path().join("src"))
                    .unwrap_or_default();
                scan_client_sites(p.name.as_str(), &src, root)
            })
            .collect();
        let session_sites = packages
            .iter()
            .filter(|p| !SESSION_STORE_CALLERS.contains(&p.name.as_str()))
            .flat_map(|p| {
                let src = p
                    .manifest_path
                    .parent()
                    .map(|dir| dir.as_std_path().join("src"))
                    .unwrap_or_default();
                scan_session_sites(p.name.as_str(), &src, root)
            })
            .collect();
        let legacy_client_sites = packages
            .iter()
            .filter(|p| p.name.as_str() == IMAGE)
            .flat_map(|p| {
                let src = p
                    .manifest_path
                    .parent()
                    .map(|dir| dir.as_std_path().join("src"))
                    .unwrap_or_default();
                scan_legacy_client_sites(p.name.as_str(), &src, root)
            })
            .collect();
        let crates = packages
            .iter()
            .map(|p| Member {
                name: p.name.as_str().to_owned(),
                features: p.features.clone(),
                deps: p
                    .dependencies
                    .iter()
                    .filter(|d| {
                        names.contains(d.name.as_str())
                            || is_internal(&d.name)
                            || d.name == TWILIGHT_HTTP
                            || is_takumi(&d.name)
                            || is_asset_http(&d.name)
                    })
                    .map(|d| Dep {
                        name: d.name.clone(),
                        key: d.rename.clone().unwrap_or_else(|| d.name.clone()),
                        kind: d.kind.into(),
                        optional: d.optional,
                        uses_default_features: d.uses_default_features,
                        features: d.features.clone(),
                    })
                    .collect(),
            })
            .collect();
        Self {
            crates,
            client_sites,
            session_sites,
            legacy_client_sites,
        }
    }

    fn member(&self, name: &str) -> Option<&Member> {
        self.crates.iter().find(|m| m.name == name)
    }

    fn internal_crate_count(&self) -> usize {
        self.crates.iter().filter(|m| is_internal(&m.name)).count()
    }

    /// Distinct (from, to) pairs between internal crates, any kind.
    fn internal_edge_count(&self) -> usize {
        self.crates
            .iter()
            .filter(|m| is_internal(&m.name))
            .flat_map(|m| {
                m.deps
                    .iter()
                    .filter(|d| is_internal(&d.name))
                    .map(|d| (m.name.as_str(), d.name.as_str()))
            })
            .collect::<BTreeSet<_>>()
            .len()
    }
}

/// The rule a violation breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rule {
    UndrawnEdge,
    DependsOnFacade,
    UngatedEdge,
    RenderOn,
    TakumiGated,
    UnknownCrate,
    MissingCrate,
    BotDependency,
    HttpClient,
    SessionStore,
    AssetHttp,
}

impl Rule {
    /// The kebab rule id printed in brackets (never the catalog's `AREA-nn` shape).
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::UndrawnEdge => "undrawn-edge",
            Self::DependsOnFacade => "depends-on-facade",
            Self::UngatedEdge => "ungated-edge",
            Self::RenderOn => "render-on",
            Self::TakumiGated => "takumi-gated",
            Self::UnknownCrate => "unknown-crate",
            Self::MissingCrate => "missing-crate",
            Self::BotDependency => "bot-dependency",
            Self::HttpClient => "http-client",
            Self::SessionStore => "session-store",
            Self::AssetHttp => "asset-http",
        }
    }
}

/// One broken rule; field order is the sort order of the report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Violation {
    from: String,
    /// The edge's target and kind; `None` for crate-level rules.
    edge: Option<(String, Kind)>,
    rule: Rule,
    reason: String,
}

impl Violation {
    fn edge(rule: Rule, from: &str, to: &str, kind: Kind, reason: String) -> Self {
        Self {
            from: from.to_owned(),
            edge: Some((to.to_owned(), kind)),
            rule,
            reason,
        }
    }

    fn krate(rule: Rule, from: &str, reason: String) -> Self {
        Self {
            from: from.to_owned(),
            edge: None,
            rule,
            reason,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "deps-check: {}", self.from)?;
        if let Some((to, kind)) = &self.edge {
            write!(f, " -> {to} ({kind})")?;
        }
        write!(f, ": {} [{}]", self.reason, self.rule.id())
    }
}

/// Checks the workspace against the allow-list; returns violations sorted and
/// deduplicated.
pub(crate) fn check(ws: &Workspace) -> Vec<Violation> {
    let mut out = BTreeSet::new();
    for name in CRATES {
        if ws.member(name).is_none() {
            out.insert(Violation::krate(
                Rule::MissingCrate,
                name,
                "crate of the Invariants graph is not a workspace member".to_owned(),
            ));
        }
    }
    let facade = ws.member(FACADE);
    if enables_testkit(facade, "default") {
        out.insert(Violation::krate(
            Rule::UngatedEdge,
            FACADE,
            format!(
                "default features enable `{TESTKIT_FEATURE}`; it is for bot \
                 [dev-dependencies] only"
            ),
        ));
    }
    check_http_client(ws, &mut out);
    check_session_store(ws, &mut out);
    check_asset_http(ws, &mut out);
    for member in &ws.crates {
        if !is_internal(&member.name) {
            check_non_internal(member, facade, &mut out);
            continue;
        }
        if !CRATES.contains(&member.name.as_str()) {
            out.insert(Violation::krate(
                Rule::UnknownCrate,
                &member.name,
                "crate is not in the Invariants graph; add it to the spine's Invariants graph \
                 and to CRATES/GRAPH/GATED in xtask/src/deps_check.rs first"
                    .to_owned(),
            ));
            continue;
        }
        for dep in member
            .deps
            .iter()
            .filter(|d| d.name != TWILIGHT_HTTP && !is_takumi(&d.name) && !is_asset_http(&d.name))
        {
            if let Some(v) = check_edge(&member.name, dep) {
                out.insert(v);
            }
        }
        check_gate_features(member, &mut out);
    }
    check_render(ws, &mut out);
    check_takumi(ws, &mut out);
    out.into_iter().collect()
}

/// Only core, the gateway and the facade depend on `twilight-http`, and no
/// scanned source constructs a client (AD-24).
fn check_http_client(ws: &Workspace, out: &mut BTreeSet<Violation>) {
    for member in &ws.crates {
        if HTTP_CLIENT_DEPENDENTS.contains(&member.name.as_str()) {
            continue;
        }
        for dep in member.deps.iter().filter(|d| d.name == TWILIGHT_HTTP) {
            out.insert(Violation::edge(
                Rule::HttpClient,
                &member.name,
                TWILIGHT_HTTP,
                dep.kind,
                "only noctis-core, noctis-gateway and noctis may depend on twilight-http; \
                 use the App's client (AD-24)"
                    .to_owned(),
            ));
        }
    }
    for site in &ws.client_sites {
        out.insert(Violation::krate(
            Rule::HttpClient,
            &site.krate,
            format!(
                "{} constructs a twilight HTTP client; only noctis-core builds the App's one \
                 client (AD-24)",
                site.location
            ),
        ));
    }
}

/// No scanned source calls the `SessionStore` (AD-21).
fn check_session_store(ws: &Workspace, out: &mut BTreeSet<Violation>) {
    for site in &ws.session_sites {
        out.insert(Violation::krate(
            Rule::SessionStore,
            &site.krate,
            format!(
                "{} calls the SessionStore; only noctis-gateway reads and writes sessions (AD-21)",
                site.location
            ),
        ));
    }
}

/// The `AssetFetcher` is the only non-Discord HTTP client (AD-15): no other
/// client crate, `hyper-rustls` only on `noctis-image` behind `render`, and
/// the legacy client named only in its source.
fn check_asset_http(ws: &Workspace, out: &mut BTreeSet<Violation>) {
    for member in &ws.crates {
        for dep in member
            .deps
            .iter()
            .filter(|d| BANNED_HTTP.contains(&d.name.as_str()))
        {
            out.insert(Violation::edge(
                Rule::AssetHttp,
                &member.name,
                &dep.name,
                dep.kind,
                "is another HTTP client; the AssetFetcher (hyper-util + hyper-rustls in \
                 noctis-image) is the only non-Discord HTTP (AD-15)"
                    .to_owned(),
            ));
        }
        for dep in member.deps.iter().filter(|d| d.name == HYPER_RUSTLS) {
            let reason = if member.name != IMAGE {
                Some(format!(
                    "only `{IMAGE}` may depend on {HYPER_RUSTLS}, for the AssetFetcher (AD-15)"
                ))
            } else if dep.kind != Kind::Normal || !dep.optional {
                Some(format!(
                    "must be an optional normal dependency enabled only by `{RENDER}` (AD-15)"
                ))
            } else {
                member
                    .features
                    .keys()
                    .find(|feature| {
                        let closure = member.closure(feature);
                        closure.activated.contains(HYPER_RUSTLS)
                            && !closure.features.contains(RENDER)
                    })
                    .map(|feature| {
                        format!("feature `{feature}` enables it without `{RENDER}` (AD-15)")
                    })
            };
            if let Some(reason) = reason {
                out.insert(Violation::edge(
                    Rule::AssetHttp,
                    &member.name,
                    &dep.name,
                    dep.kind,
                    reason,
                ));
            }
        }
    }
    for site in &ws.legacy_client_sites {
        out.insert(Violation::krate(
            Rule::AssetHttp,
            &site.krate,
            format!(
                "{} names the hyper-util legacy client; only {ASSET_FETCHER} (the \
                 AssetFetcher) may (AD-15)",
                site.location
            ),
        ));
    }
}

/// Every line of the `noctis-image` sources under `src` that names the
/// legacy client, outside the `AssetFetcher` and test files.
fn scan_legacy_client_sites(krate: &str, src: &Path, root: &Path) -> Vec<SourceSite> {
    let mut files = Vec::new();
    collect_rs_files(src, &mut files);
    let fetcher = src.join(ASSET_FETCHER.trim_start_matches("src/"));
    files.retain(|file| {
        let relative = file.strip_prefix(src).unwrap_or(file);
        *file != fetcher
            && !relative
                .components()
                .any(|c| c.as_os_str() == "tests" || c.as_os_str() == "tests.rs")
    });
    scan_lines(krate, files, root, names_legacy_client)
}

/// Every line of `src/**/*.rs` under `src` that contains a client
/// constructor. A missing directory has no sites.
fn scan_client_sites(krate: &str, src: &Path, root: &Path) -> Vec<SourceSite> {
    let mut files = Vec::new();
    collect_rs_files(src, &mut files);
    scan_lines(krate, files, root, |line| {
        CLIENT_CONSTRUCTORS.iter().any(|c| line.contains(c))
    })
}

/// Every line of `src/**/*.rs` under `src` that calls the `SessionStore`,
/// skipping test files (named `tests.rs`, or under a `tests` directory). A
/// missing directory has no sites.
fn scan_session_sites(krate: &str, src: &Path, root: &Path) -> Vec<SourceSite> {
    let mut files = Vec::new();
    collect_rs_files(src, &mut files);
    files.retain(|file| {
        let relative = file.strip_prefix(src).unwrap_or(file);
        !relative
            .components()
            .any(|c| c.as_os_str() == "tests" || c.as_os_str() == "tests.rs")
    });
    scan_lines(krate, files, root, calls_session_store)
}

/// Every line of `files` for which `hit` holds, as `path:line` relative to
/// `root`, in path order.
fn scan_lines(
    krate: &str,
    mut files: Vec<PathBuf>,
    root: &Path,
    hit: impl Fn(&str) -> bool,
) -> Vec<SourceSite> {
    files.sort();
    let mut sites = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let shown = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        for (index, line) in text.lines().enumerate() {
            if hit(line) {
                sites.push(SourceSite {
                    krate: krate.to_owned(),
                    location: format!("{shown}:{}", index + 1),
                });
            }
        }
    }
    sites
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Checks one dependency of the graph crate `from`.
fn check_edge(from: &str, dep: &Dep) -> Option<Violation> {
    let to = dep.name.as_str();
    if let Some(feature) = gate(from, to) {
        return (dep.kind != Kind::Normal || !dep.optional).then(|| {
            Violation::edge(
                Rule::UngatedEdge,
                from,
                to,
                dep.kind,
                format!(
                    "must be an optional normal dependency enabled only by feature `{feature}`"
                ),
            )
        });
    }
    let facade_allowed = from == FACADE && CRATES.contains(&to);
    if facade_allowed || (to == FACADE && dep.kind == Kind::Dev) || GRAPH.contains(&(from, to)) {
        return None;
    }
    if to == FACADE {
        return Some(Violation::edge(
            Rule::DependsOnFacade,
            from,
            to,
            dep.kind,
            format!("only [dev-dependencies] may depend on `{FACADE}`"),
        ));
    }
    Some(Violation::edge(
        Rule::UndrawnEdge,
        from,
        to,
        dep.kind,
        "edge is not drawn in the spine's Invariants graph".to_owned(),
    ))
}

/// Whether enabling the facade feature `feature` reaches `noctis-testkit`: it
/// is `testkit`, its closure contains `testkit`, or it activates a dependency
/// whose package is `noctis-testkit` (rename-aware). A name the facade does not
/// declare enables nothing unless it is `testkit`.
fn enables_testkit(facade: Option<&Member>, feature: &str) -> bool {
    feature == TESTKIT_FEATURE
        || facade.is_some_and(|m| {
            let closure = m.closure(feature);
            closure.features.contains(TESTKIT_FEATURE) || closure.activated.contains(TESTKIT)
        })
}

/// Non-internal members (AD-2) depend on the facade only, and enable its
/// `testkit` feature (directly or through any facade feature that reaches it)
/// from `[dev-dependencies]` only.
fn check_non_internal(member: &Member, facade: Option<&Member>, out: &mut BTreeSet<Violation>) {
    // `(key, facade feature, member feature)` for every `k/f` or `k?/f` the
    // member's own features request.
    let requested: BTreeSet<(String, String, &str)> = member
        .features
        .keys()
        .flat_map(|feature| {
            member
                .closure(feature)
                .dep_features
                .into_iter()
                .map(move |(key, dep_feature)| (key, dep_feature, feature.as_str()))
        })
        .collect();
    for dep in member.deps.iter().filter(|d| is_internal(&d.name)) {
        if dep.name != FACADE {
            out.insert(Violation::edge(
                Rule::BotDependency,
                &member.name,
                &dep.name,
                dep.kind,
                format!("non-internal members may depend on `{FACADE}` only (AD-2)"),
            ));
            continue;
        }
        if dep.kind == Kind::Dev {
            continue;
        }
        let mut report = |source: String| {
            out.insert(Violation::edge(
                Rule::BotDependency,
                &member.name,
                &dep.name,
                dep.kind,
                format!(
                    "{source} enables `{TESTKIT_FEATURE}`; `{TESTKIT_FEATURE}` is for \
                     [dev-dependencies] only"
                ),
            ));
        };
        for feature in &dep.features {
            if enables_testkit(facade, feature) {
                report(format!(
                    "facade feature `{feature}` from the dependency's `features` list"
                ));
            }
        }
        if dep.uses_default_features && enables_testkit(facade, "default") {
            report("the facade's default features (`default`)".to_owned());
        }
        for (key, dep_feature, feature) in &requested {
            if *key == dep.key && enables_testkit(facade, dep_feature) {
                report(format!(
                    "feature `{feature}` requests facade feature `{dep_feature}` \
                     (`{key}/{dep_feature}`), which"
                ));
            }
        }
    }
}

/// Every feature that turns a gated edge on must imply the gate feature.
fn check_gate_features(member: &Member, out: &mut BTreeSet<Violation>) {
    for &(_, to, gate) in GATED.iter().filter(|&&(a, _, _)| a == member.name) {
        let gated_optional = member
            .deps
            .iter()
            .any(|d| d.name == to && d.kind == Kind::Normal && d.optional);
        if !gated_optional {
            continue;
        }
        for feature in member.features.keys() {
            let closure = member.closure(feature);
            if closure.activated.contains(to) && !closure.features.contains(gate) {
                out.insert(Violation::edge(
                    Rule::UngatedEdge,
                    &member.name,
                    to,
                    Kind::Normal,
                    format!("feature `{feature}` enables the edge without `{gate}`"),
                ));
            }
        }
    }
}

/// `noctis-image`'s `render` must never be on by default or through `noctis-macros`.
fn check_render(ws: &Workspace, out: &mut BTreeSet<Violation>) {
    let image = ws.member(IMAGE);
    let enables_render = |feature: &str| {
        feature == RENDER || image.is_some_and(|m| m.closure(feature).features.contains(RENDER))
    };
    if image.is_some() && enables_render("default") {
        out.insert(Violation::krate(
            Rule::RenderOn,
            IMAGE,
            format!("default features enable `{RENDER}`"),
        ));
    }
    let Some(macros) = ws.member(MACROS) else {
        return;
    };
    for dep in macros.deps.iter().filter(|d| d.name == IMAGE) {
        let defaults = dep.uses_default_features.then_some("default");
        for feature in dep.features.iter().map(String::as_str).chain(defaults) {
            if enables_render(feature) {
                out.insert(Violation::edge(
                    Rule::RenderOn,
                    MACROS,
                    IMAGE,
                    dep.kind,
                    format!("dependency feature `{feature}` enables `{RENDER}`"),
                ));
            }
        }
    }
    for feature in macros.features.keys() {
        for (key, dep_feature) in macros.closure(feature).dep_features {
            if macros.package_for(&key) == Some(IMAGE) && enables_render(&dep_feature) {
                out.insert(Violation::edge(
                    Rule::RenderOn,
                    MACROS,
                    IMAGE,
                    Kind::Normal,
                    format!("feature `{feature}` enables `{key}/{dep_feature}`"),
                ));
            }
        }
    }
}

/// Every takumi dependency of `noctis-image` is optional, normal, and turned
/// on only by features that imply `render`; no other member depends on takumi.
fn check_takumi(ws: &Workspace, out: &mut BTreeSet<Violation>) {
    for member in ws.crates.iter().filter(|m| m.name != IMAGE) {
        for dep in member.deps.iter().filter(|d| is_takumi(&d.name)) {
            out.insert(Violation::edge(
                Rule::TakumiGated,
                &member.name,
                &dep.name,
                dep.kind,
                format!("only `{IMAGE}` may depend on takumi, behind `{RENDER}`"),
            ));
        }
    }
    let Some(image) = ws.member(IMAGE) else {
        return;
    };
    for dep in image.deps.iter().filter(|d| is_takumi(&d.name)) {
        if dep.kind != Kind::Normal || !dep.optional {
            out.insert(Violation::edge(
                Rule::TakumiGated,
                IMAGE,
                &dep.name,
                dep.kind,
                format!("must be an optional normal dependency enabled only by `{RENDER}`"),
            ));
            continue;
        }
        for feature in image.features.keys() {
            let closure = image.closure(feature);
            if closure.activated.contains(&dep.name) && !closure.features.contains(RENDER) {
                out.insert(Violation::edge(
                    Rule::TakumiGated,
                    IMAGE,
                    &dep.name,
                    dep.kind,
                    format!("feature `{feature}` enables it without `{RENDER}`"),
                ));
            }
        }
    }
}

/// Runs the check on the workspace of `manifest_path` and prints the report.
pub(crate) fn run(manifest_path: &Path) -> ExitCode {
    let metadata = match MetadataCommand::new()
        .manifest_path(manifest_path)
        .no_deps()
        .exec()
    {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!("deps-check: cargo metadata failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    let ws = Workspace::from_metadata(&metadata);
    let violations = check(&ws);
    if violations.is_empty() {
        eprintln!(
            "deps-check: ok ({} internal crates, {} internal edges)",
            ws.internal_crate_count(),
            ws.internal_edge_count()
        );
        ExitCode::SUCCESS
    } else {
        for violation in &violations {
            eprintln!("{violation}");
        }
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_edges() -> impl Iterator<Item = (&'static str, &'static str)> {
        GRAPH
            .iter()
            .copied()
            .chain(GATED.iter().map(|&(a, b, _)| (a, b)))
    }

    #[test]
    fn graph_encodes_every_drawn_non_facade_edge() {
        // The facade's drawn ungated edges (core, macros, image, i18n, prefix)
        // are covered by its any-crate rule, so GRAPH holds the other 20.
        assert_eq!(GRAPH.len(), 20);
        assert_eq!(GATED.len(), 4);
    }

    #[test]
    fn every_name_is_a_graph_crate() {
        for (a, b) in all_edges() {
            assert!(CRATES.contains(&a), "{a}");
            assert!(CRATES.contains(&b), "{b}");
        }
    }

    #[test]
    fn graph_is_acyclic() {
        // Kahn's algorithm: every node must be removable.
        let mut edges: Vec<_> = all_edges().collect();
        let mut nodes: BTreeSet<&str> = CRATES.into_iter().collect();
        while let Some(&leaf) = nodes.iter().find(|n| !edges.iter().any(|&(a, _)| a == **n)) {
            nodes.remove(leaf);
            edges.retain(|&(_, b)| b != leaf);
        }
        assert!(nodes.is_empty(), "cycle among {nodes:?}");
    }

    fn dep(name: &str) -> Dep {
        Dep {
            name: name.to_owned(),
            key: name.to_owned(),
            kind: Kind::Normal,
            optional: true,
            uses_default_features: true,
            features: Vec::new(),
        }
    }

    fn member(features: &[(&str, &[&str])]) -> Member {
        Member {
            name: "m".to_owned(),
            features: features
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
                .collect(),
            deps: vec![dep("a"), dep("b"), dep("c"), dep("d")],
        }
    }

    #[test]
    fn closure_follows_dep_slash_weak_and_transitive_features() {
        let m = member(&[
            ("all", &["mid"]),
            ("mid", &["dep:a", "b/x", "c?/y", "mid"]),
            ("d", &["dep:d"]),
            ("other", &["d"]),
        ]);
        let c = m.closure("all");
        assert_eq!(
            c.features,
            BTreeSet::from(["all".to_owned(), "mid".to_owned()])
        );
        assert_eq!(
            c.activated,
            BTreeSet::from(["a".to_owned(), "b".to_owned()])
        );
        assert!(c.dep_features.contains(&("b".to_owned(), "x".to_owned())));
        assert!(c.dep_features.contains(&("c".to_owned(), "y".to_owned())));
        assert!(
            !c.activated.contains("c"),
            "weak `c?/y` must not activate c"
        );

        let implicit = m.closure("other");
        assert!(implicit.activated.contains("d"));
    }

    #[test]
    fn closure_maps_renamed_keys_to_packages() {
        let mut m = member(&[("f", &["dep:alias", "alias/x"])]);
        m.deps.push(Dep {
            key: "alias".to_owned(),
            ..dep("pkg")
        });
        let c = m.closure("f");
        assert!(c.activated.contains("pkg"));
        assert!(
            c.dep_features
                .contains(&("alias".to_owned(), "x".to_owned()))
        );
    }

    #[test]
    fn violation_line_format() {
        let v = Violation::edge(
            Rule::UndrawnEdge,
            "noctis-ui",
            "noctis-core",
            Kind::Normal,
            "r".to_owned(),
        );
        assert_eq!(
            v.to_string(),
            "deps-check: noctis-ui -> noctis-core (normal): r [undrawn-edge]"
        );
        let v = Violation::krate(Rule::MissingCrate, "noctis-prefix", "r".to_owned());
        assert_eq!(
            v.to_string(),
            "deps-check: noctis-prefix: r [missing-crate]"
        );
    }

    fn http_rule(ws: &Workspace) -> Vec<String> {
        check(ws)
            .into_iter()
            .filter(|v| v.rule == Rule::HttpClient)
            .map(|v| v.to_string())
            .collect()
    }

    fn with_twilight_http(name: &str) -> Member {
        Member {
            name: name.to_owned(),
            features: BTreeMap::new(),
            deps: vec![Dep {
                optional: false,
                ..dep(TWILIGHT_HTTP)
            }],
        }
    }

    #[test]
    fn twilight_http_outside_core_gateway_facade_fails() {
        let ws = Workspace {
            crates: ["noctis-core", "noctis-gateway", "noctis", "noctis-ui"]
                .map(with_twilight_http)
                .to_vec(),
            client_sites: Vec::new(),
            session_sites: Vec::new(),
            legacy_client_sites: Vec::new(),
        };
        assert_eq!(
            http_rule(&ws),
            [
                "deps-check: noctis-ui -> twilight-http (normal): only noctis-core, noctis-gateway \
              and noctis may depend on twilight-http; use the App's client (AD-24) [http-client]"
            ]
        );
        // twilight-http is never an undrawn internal edge.
        assert!(
            !check(&ws)
                .iter()
                .any(|v| v.rule == Rule::UndrawnEdge && v.to_string().contains("twilight"))
        );
    }

    #[test]
    fn client_construction_in_scanned_source_fails() {
        let ws = Workspace {
            crates: Vec::new(),
            client_sites: vec![SourceSite {
                krate: "noctis-gateway".to_owned(),
                location: "crates/noctis-gateway/src/lib.rs:3".to_owned(),
            }],
            session_sites: Vec::new(),
            legacy_client_sites: Vec::new(),
        };
        assert_eq!(
            http_rule(&ws),
            [
                "deps-check: noctis-gateway: crates/noctis-gateway/src/lib.rs:3 constructs a twilight \
              HTTP client; only noctis-core builds the App's one client (AD-24) [http-client]"
            ]
        );
    }

    fn takumi_rule(image: Member) -> Vec<String> {
        let ws = Workspace {
            crates: vec![image],
            client_sites: Vec::new(),
            session_sites: Vec::new(),
            legacy_client_sites: Vec::new(),
        };
        check(&ws)
            .into_iter()
            .filter(|v| v.rule == Rule::TakumiGated)
            .map(|v| v.to_string())
            .collect()
    }

    fn image_with(features: &[(&str, &[&str])], deps: Vec<Dep>) -> Member {
        Member {
            name: IMAGE.to_owned(),
            deps,
            ..member(features)
        }
    }

    #[test]
    fn takumi_on_another_member_fails() {
        let other = Member {
            name: "noctis-ui".to_owned(),
            deps: vec![dep("takumi")],
            ..member(&[])
        };
        assert_eq!(
            takumi_rule(other),
            [
                "deps-check: noctis-ui -> takumi (normal): only `noctis-image` may depend on \
              takumi, behind `render` [takumi-gated]"
            ]
        );
    }

    #[test]
    fn from_metadata_keeps_the_optional_takumi_deps() {
        let metadata = MetadataCommand::new()
            .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"))
            .no_deps()
            .exec()
            .expect("cargo metadata on the repo");
        let ws = Workspace::from_metadata(&metadata);
        let image = ws.member(IMAGE).expect("noctis-image is a member");
        for name in ["takumi", "takumi-core"] {
            let dep = image
                .deps
                .iter()
                .find(|d| d.name == name)
                .unwrap_or_else(|| panic!("noctis-image lists {name}"));
            assert!(dep.optional, "{name} must be optional");
        }
    }

    #[test]
    fn takumi_behind_render_passes() {
        let image = image_with(
            &[
                ("default", &[]),
                ("render", &["dep:takumi", "dep:takumi-core"]),
            ],
            vec![dep("takumi"), dep("takumi-core")],
        );
        assert_eq!(takumi_rule(image), Vec::<String>::new());
        let implied = image_with(
            &[("all", &["render"]), ("render", &["dep:takumi"])],
            vec![dep("takumi")],
        );
        assert_eq!(takumi_rule(implied), Vec::<String>::new());
    }

    #[test]
    fn takumi_outside_render_fails() {
        let required = image_with(
            &[("render", &[])],
            vec![Dep {
                optional: false,
                ..dep("takumi-core")
            }],
        );
        assert_eq!(
            takumi_rule(required),
            [
                "deps-check: noctis-image -> takumi-core (normal): must be an optional normal \
              dependency enabled only by `render` [takumi-gated]"
            ]
        );
        let dev = image_with(
            &[],
            vec![Dep {
                kind: Kind::Dev,
                optional: false,
                ..dep("takumi")
            }],
        );
        assert_eq!(takumi_rule(dev).len(), 1);
        let other_feature = image_with(
            &[
                ("render", &["dep:takumi"]),
                ("fast", &["dep:takumi-raster"]),
            ],
            vec![dep("takumi"), dep("takumi-raster")],
        );
        assert_eq!(
            takumi_rule(other_feature),
            [
                "deps-check: noctis-image -> takumi-raster (normal): feature `fast` enables it \
              without `render` [takumi-gated]"
            ]
        );
        let by_default = image_with(
            &[("default", &["dep:takumi"]), ("render", &["dep:takumi"])],
            vec![dep("takumi")],
        );
        assert_eq!(takumi_rule(by_default).len(), 1);
    }

    /// A `SessionStore` call `receiver` + `op`, assembled so this file never
    /// holds the pattern it tests.
    fn session_call(receiver: &str, op: &str) -> String {
        format!("{receiver}{op}(\"k\")")
    }

    /// A7: a `SessionStore` call in a non-gateway source is reported; test
    /// files are exempt.
    #[test]
    fn session_store_calls_outside_the_gateway_are_found() {
        let root = std::env::temp_dir().join(format!("deps-check-session-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(src.join("app")).unwrap();
        std::fs::create_dir_all(src.join("tests")).unwrap();
        let core_line = format!(
            "let raw = self.ports().{}.await?;",
            session_call("session_store.", "get")
        );
        std::fs::write(
            src.join("lib.rs"),
            format!(
                "fn a() {{}}\n{core_line}\nlet store = ports.session_store.clone();\n{}\n",
                session_call("SessionStore::", "delete")
            ),
        )
        .unwrap();
        let in_tests = session_call("SessionStore::", "get");
        std::fs::write(src.join("app/tests.rs"), &in_tests).unwrap();
        std::fs::write(src.join("tests/mod.rs"), &in_tests).unwrap();
        std::fs::write(
            src.join("app/put.rs"),
            format!("x.{}", session_call("session_store.", "put")),
        )
        .unwrap();
        let sites = scan_session_sites("noctis-core", &src, &root);
        let _ = std::fs::remove_dir_all(&root);
        let locations: Vec<&str> = sites.iter().map(|s| s.location.as_str()).collect();
        assert_eq!(
            locations,
            ["src/app/put.rs:1", "src/lib.rs:2", "src/lib.rs:4"]
        );

        let ws = Workspace {
            crates: Vec::new(),
            client_sites: Vec::new(),
            session_sites: vec![SourceSite {
                krate: "noctis-core".to_owned(),
                location: "crates/noctis-core/src/app.rs:7".to_owned(),
            }],
            legacy_client_sites: Vec::new(),
        };
        let found: Vec<String> = check(&ws)
            .into_iter()
            .filter(|v| v.rule == Rule::SessionStore)
            .map(|v| v.to_string())
            .collect();
        assert_eq!(
            found,
            [
                "deps-check: noctis-core: crates/noctis-core/src/app.rs:7 calls the SessionStore; \
                 only noctis-gateway reads and writes sessions (AD-21) [session-store]"
            ]
        );
    }

    #[test]
    fn session_store_call_patterns() {
        for (receiver, op) in [
            ("session_store.", "get"),
            ("session_store.", "put"),
            ("session_store.", "delete"),
            ("SessionStore::", "get"),
            ("SessionStore::", "put"),
            ("SessionStore::", "delete"),
        ] {
            assert!(
                calls_session_store(&session_call(receiver, op)),
                "{receiver}{op}"
            );
        }
        for line in [
            "session_store: SessionStore = MemorySessionStore::new();",
            "Arc::clone(&self.ports().session_store)",
            "session_store.take(\"k\")",
            "SessionStore::version(",
        ] {
            assert!(!calls_session_store(line), "{line}");
        }
    }

    fn asset_http_rule(crates: Vec<Member>, legacy_client_sites: Vec<SourceSite>) -> Vec<String> {
        let ws = Workspace {
            crates,
            client_sites: Vec::new(),
            session_sites: Vec::new(),
            legacy_client_sites,
        };
        check(&ws)
            .into_iter()
            .filter(|v| v.rule == Rule::AssetHttp)
            .map(|v| v.to_string())
            .collect()
    }

    /// `noctis-image` with `hyper-rustls` behind `render`.
    fn image_with_rustls(dep: Dep) -> Member {
        image_with(&[("render", &["dep:hyper-rustls"])], vec![dep])
    }

    #[test]
    fn asset_http_other_client_crates_fail() {
        let reqwest = Member {
            name: "noctis-ui".to_owned(),
            deps: vec![
                Dep {
                    optional: false,
                    ..dep("reqwest")
                },
                Dep {
                    kind: Kind::Dev,
                    ..dep("ureq")
                },
            ],
            ..member(&[])
        };
        assert_eq!(
            asset_http_rule(vec![reqwest], Vec::new()),
            [
                "deps-check: noctis-ui -> reqwest (normal): is another HTTP client; the \
                 AssetFetcher (hyper-util + hyper-rustls in noctis-image) is the only \
                 non-Discord HTTP (AD-15) [asset-http]",
                "deps-check: noctis-ui -> ureq (dev): is another HTTP client; the AssetFetcher \
                 (hyper-util + hyper-rustls in noctis-image) is the only non-Discord HTTP \
                 (AD-15) [asset-http]",
            ]
        );
    }

    #[test]
    fn asset_http_hyper_rustls_only_on_image_behind_render() {
        assert_eq!(
            asset_http_rule(vec![image_with_rustls(dep(HYPER_RUSTLS))], Vec::new()),
            Vec::<String>::new()
        );
        let core = Member {
            name: "noctis-core".to_owned(),
            deps: vec![dep(HYPER_RUSTLS)],
            ..member(&[])
        };
        assert_eq!(
            asset_http_rule(vec![core], Vec::new()),
            [
                "deps-check: noctis-core -> hyper-rustls (normal): only `noctis-image` may \
                 depend on hyper-rustls, for the AssetFetcher (AD-15) [asset-http]"
            ]
        );
        let required = image_with_rustls(Dep {
            optional: false,
            ..dep(HYPER_RUSTLS)
        });
        assert_eq!(
            asset_http_rule(vec![required], Vec::new()),
            [
                "deps-check: noctis-image -> hyper-rustls (normal): must be an optional normal \
                 dependency enabled only by `render` (AD-15) [asset-http]"
            ]
        );
        let ungated = image_with(
            &[("render", &[]), ("net", &["dep:hyper-rustls"])],
            vec![dep(HYPER_RUSTLS)],
        );
        assert_eq!(
            asset_http_rule(vec![ungated], Vec::new()),
            [
                "deps-check: noctis-image -> hyper-rustls (normal): feature `net` enables it \
                 without `render` (AD-15) [asset-http]"
            ]
        );
    }

    #[test]
    fn asset_http_legacy_client_outside_the_fetcher_fails() {
        let site = SourceSite {
            krate: IMAGE.to_owned(),
            location: "crates/noctis-image/src/plugin.rs:9".to_owned(),
        };
        assert_eq!(
            asset_http_rule(Vec::new(), vec![site]),
            [
                "deps-check: noctis-image: crates/noctis-image/src/plugin.rs:9 names the \
                 hyper-util legacy client; only src/remote/fetch.rs (the AssetFetcher) may \
                 (AD-15) [asset-http]"
            ]
        );
    }

    #[test]
    fn asset_http_scan_skips_the_fetcher_and_tests() {
        let root = std::env::temp_dir().join(format!("deps-check-legacy-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(src.join("remote/fetch")).unwrap();
        let line = format!("use hyper_util::client::{};\n", LEGACY_CLIENT.concat());
        std::fs::write(src.join("plugin.rs"), format!("fn a() {{}}\n{line}")).unwrap();
        std::fs::write(src.join("remote/fetch.rs"), &line).unwrap();
        std::fs::write(src.join("remote/fetch/tests.rs"), &line).unwrap();
        std::fs::write(src.join("remote/tests.rs"), &line).unwrap();
        let sites = scan_legacy_client_sites(IMAGE, &src, &root);
        let _ = std::fs::remove_dir_all(&root);
        let locations: Vec<&str> = sites.iter().map(|s| s.location.as_str()).collect();
        assert_eq!(locations, ["src/plugin.rs:2"]);
    }

    #[test]
    fn asset_http_the_workspace_passes() {
        let metadata = MetadataCommand::new()
            .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"))
            .no_deps()
            .exec()
            .expect("cargo metadata on the repo");
        let ws = Workspace::from_metadata(&metadata);
        let image = ws.member(IMAGE).expect("noctis-image is a member");
        assert!(
            image
                .deps
                .iter()
                .any(|d| d.name == HYPER_RUSTLS && d.optional),
            "noctis-image lists hyper-rustls"
        );
        assert_eq!(
            check(&ws)
                .into_iter()
                .filter(|v| v.rule == Rule::AssetHttp)
                .map(|v| v.to_string())
                .collect::<Vec<_>>(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn scan_finds_every_constructor_line() {
        let root = std::env::temp_dir().join(format!("deps-check-scan-{}", std::process::id()));
        let src = root.join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "fn a() {}\nlet c = Client::builder();\n",
        )
        .unwrap();
        std::fs::write(
            src.join("nested/mod.rs"),
            "use twilight_http::client::ClientBuilder;\nClient::new(t);\n",
        )
        .unwrap();
        std::fs::write(src.join("notes.txt"), "Client::new(").unwrap();
        let sites = scan_client_sites("noctis", &src, &root);
        let _ = std::fs::remove_dir_all(&root);
        let locations: Vec<&str> = sites.iter().map(|s| s.location.as_str()).collect();
        assert_eq!(
            locations,
            ["src/lib.rs:2", "src/nested/mod.rs:1", "src/nested/mod.rs:2"]
        );
    }
}
