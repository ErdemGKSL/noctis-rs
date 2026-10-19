//! `deps-check`: checks every workspace dependency edge against the spine's
//! Invariants graph (AD-1, AD-2) without building anything.
//!
//! The workspace is read with `cargo metadata --no-deps`. Every violation is
//! reported at once on stderr as
//! `deps-check: <from> -> <to> (<kind>): <reason> [<rule-id>]`
//! (crate-level rules omit `-> <to> (<kind>)`), sorted and deduplicated.
//!
//! The graph labels the `noctis-macros -> noctis-image` edge "tw module only,
//! render feature off". Only the "render feature off" half is enforced here
//! (`render-on`); "tw module only" is a source-level property that dependency
//! metadata cannot see, so deps-check does not check it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
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

/// The workspace members and their internal dependency edges.
#[derive(Debug, Clone)]
pub(crate) struct Workspace {
    crates: Vec<Member>,
}

impl Workspace {
    /// Keeps workspace members and every dependency on a member or on an
    /// internal (`noctis`/`noctis-*`) package, wherever that package lives.
    pub(crate) fn from_metadata(metadata: &Metadata) -> Self {
        let packages = metadata.workspace_packages();
        let names: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        let crates = packages
            .iter()
            .map(|p| Member {
                name: p.name.as_str().to_owned(),
                features: p.features.clone(),
                deps: p
                    .dependencies
                    .iter()
                    .filter(|d| names.contains(d.name.as_str()) || is_internal(&d.name))
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
        Self { crates }
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
    UnknownCrate,
    MissingCrate,
    BotDependency,
}

impl Rule {
    /// The kebab rule id printed in brackets (never the catalog's `AREA-nn` shape).
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::UndrawnEdge => "undrawn-edge",
            Self::DependsOnFacade => "depends-on-facade",
            Self::UngatedEdge => "ungated-edge",
            Self::RenderOn => "render-on",
            Self::UnknownCrate => "unknown-crate",
            Self::MissingCrate => "missing-crate",
            Self::BotDependency => "bot-dependency",
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
    for member in &ws.crates {
        if !is_internal(&member.name) {
            check_non_internal(member, &mut out);
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
        for dep in &member.deps {
            if let Some(v) = check_edge(&member.name, dep) {
                out.insert(v);
            }
        }
        check_gate_features(member, &mut out);
    }
    check_render(ws, &mut out);
    out.into_iter().collect()
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
    if from == FACADE || (to == FACADE && dep.kind == Kind::Dev) || GRAPH.contains(&(from, to)) {
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

/// Non-internal members (AD-2) depend on the facade only, and enable its
/// `testkit` feature from `[dev-dependencies]` only.
fn check_non_internal(member: &Member, out: &mut BTreeSet<Violation>) {
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
        if dep.features.iter().any(|f| f == TESTKIT_FEATURE) {
            out.insert(Violation::edge(
                Rule::BotDependency,
                &member.name,
                &dep.name,
                dep.kind,
                format!(
                    "the dependency's `features` list enables `{TESTKIT_FEATURE}`; \
                     `{TESTKIT_FEATURE}` is for [dev-dependencies] only"
                ),
            ));
        }
        let requested = (dep.key.clone(), TESTKIT_FEATURE.to_owned());
        for feature in member.features.keys() {
            if member.closure(feature).dep_features.contains(&requested) {
                out.insert(Violation::edge(
                    Rule::BotDependency,
                    &member.name,
                    &dep.name,
                    dep.kind,
                    format!(
                        "feature `{feature}` enables `{}/{TESTKIT_FEATURE}`; \
                         `{TESTKIT_FEATURE}` is for [dev-dependencies] only",
                        dep.key
                    ),
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
        // are covered by its any-crate rule, so GRAPH holds the other 18.
        assert_eq!(GRAPH.len(), 18);
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
}
