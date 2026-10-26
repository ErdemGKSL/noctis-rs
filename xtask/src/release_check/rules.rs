//! The pure checks of `release-check` over [`Workspace`], the parsed
//! `release-plz.toml` and the text of `CHANGELOG.md`.

use std::collections::{BTreeMap, BTreeSet};

use toml::{Table, Value};

use super::Problems;
use super::model::Workspace;

/// The facade crate: the only crate with a changelog, a tag and a release.
pub(super) const FACADE: &str = "noctis";
/// The release-plz configuration file.
pub(super) const RELEASE_PLZ: &str = "release-plz.toml";
/// The changelog.
pub(super) const CHANGELOG: &str = "CHANGELOG.md";
/// The heading release-plz stamps the version under.
const UNRELEASED: &str = "## [Unreleased]";
/// The heading `[changelog] body` must render.
const VERSION_HEADING: &str = "## [{{ version }}]";

/// Whether `name` is a noctis crate (the facade or a `noctis-*` sub-crate).
fn is_noctis_crate(name: &str) -> bool {
    name == FACADE || name.starts_with("noctis-")
}

/// `None` or blank.
fn is_blank(field: Option<&String>) -> bool {
    field.is_none_or(|value| value.trim().is_empty())
}

/// The crate rows (K2, C1-C5) and the dependency rows (D1-D4).
pub(super) fn check_workspace(ws: &Workspace, problems: &mut Problems) {
    let version = ws.version.as_deref();
    if version.is_none() {
        problems.add("Cargo.toml", "no [workspace.package] version");
    }
    let repository = ws.repository.as_deref().map(|r| r.trim_end_matches('/'));
    if repository.is_none() {
        problems.add("Cargo.toml", "no [workspace.package] repository");
    }
    if !ws.is_published(FACADE) {
        problems.add(FACADE, "the facade crate is not published");
    }
    for member in ws.published() {
        let name = &member.name;
        if !is_noctis_crate(name) {
            problems.add(
                name,
                "published but not a noctis crate (set publish = false)",
            );
        }
        if let Some(version) = version
            && member.version != version
        {
            problems.add(
                name,
                format!(
                    "version {} is not the workspace version {version} \
                     (use version.workspace = true)",
                    member.version
                ),
            );
        }
        for (field, value) in [
            ("description", &member.description),
            ("license", &member.license),
            ("repository", &member.repository),
        ] {
            if is_blank(value.as_ref()) {
                problems.add(name, format!("missing {field}"));
            }
        }
        if let (Some(expected), Some(found)) = (repository, member.repository.as_deref())
            && !found.trim().is_empty()
            && found.trim_end_matches('/') != expected
        {
            problems.add(
                name,
                format!("repository {found} is not the workspace repository {expected}"),
            );
        }
    }
    check_dependencies(ws, problems);
}

/// D1-D4: every edge leaving a published member.
fn check_dependencies(ws: &Workspace, problems: &mut Problems) {
    for member in ws.published() {
        for dep in &member.deps {
            let subject = format!("{} -> {} ({})", member.name, dep.to, dep.kind);
            let to_published = ws.is_published(&dep.to);
            let message = match (dep.kind.is_published(), to_published) {
                (true, true) if dep.path_only => Some(format!(
                    "path-only dependency (cargo publish drops it; use {}.workspace = true)",
                    dep.to
                )),
                (true, false) => Some("depends on an unpublished member".to_owned()),
                (false, true) if !dep.path_only && reaches(ws, &dep.to, &member.name) => Some(
                    "cyclic dev-dependency keeps a version \
                     (make it path-only so cargo publish strips it)"
                        .to_owned(),
                ),
                (false, false) if !dep.path_only => {
                    Some("dev-dependency on an unpublished member keeps a version".to_owned())
                }
                _ => None,
            };
            if let Some(message) = message {
                problems.add(subject, message);
            }
        }
    }
}

/// Whether `from` reaches `to` over normal and build edges (optional ones
/// included) between any members.
fn reaches(ws: &Workspace, from: &str, to: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(name) = stack.pop() {
        if name == to {
            return true;
        }
        if !seen.insert(name) {
            continue;
        }
        if let Some(member) = ws.member(name) {
            stack.extend(
                member
                    .deps
                    .iter()
                    .filter(|d| d.kind.is_published())
                    .map(|d| d.to.as_str()),
            );
        }
    }
    false
}

/// The publish order of P: Kahn's topological order over the normal and
/// build edges inside P, taken in rounds (every crate whose dependencies are
/// all in an earlier round), ties within a round alphabetical. `Err` holds the members left on
/// a cycle (which cargo itself rejects).
pub(super) fn publish_order(ws: &Workspace) -> Result<Vec<String>, Vec<String>> {
    let published: BTreeSet<&str> = ws.published().map(|m| m.name.as_str()).collect();
    // Remaining in-P dependencies of each crate.
    let mut pending: BTreeMap<&str, BTreeSet<&str>> = ws
        .published()
        .map(|m| {
            let deps = m
                .deps
                .iter()
                .filter(|d| d.kind.is_published() && published.contains(d.to.as_str()))
                .map(|d| d.to.as_str())
                .filter(|to| *to != m.name)
                .collect();
            (m.name.as_str(), deps)
        })
        .collect();
    let mut order = Vec::with_capacity(pending.len());
    loop {
        // One round: every crate whose dependencies are all published, by name.
        let ready: Vec<&str> = pending
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(name, _)| *name)
            .collect();
        if ready.is_empty() {
            break;
        }
        for name in &ready {
            pending.remove(name);
        }
        for deps in pending.values_mut() {
            deps.retain(|dep| !ready.contains(dep));
        }
        order.extend(ready.into_iter().map(str::to_owned));
    }
    if pending.is_empty() {
        Ok(order)
    } else {
        Err(pending.into_keys().map(str::to_owned).collect())
    }
}

/// The loaded `release-plz.toml`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Config {
    Missing,
    /// The parser's message, first line.
    Invalid(String),
    Parsed(Table),
}

/// The `[workspace]` keys R2 requires.
const WORKSPACE_KEYS: [(&str, bool); 5] = [
    ("release_always", false),
    ("changelog_update", false),
    ("semver_check", false),
    ("git_tag_enable", false),
    ("git_release_enable", false),
];

/// R1-R7.
pub(super) fn check_release_plz(config: &Config, ws: &Workspace, problems: &mut Problems) {
    let table = match config {
        Config::Missing => return problems.add(RELEASE_PLZ, "missing"),
        Config::Invalid(message) => return problems.add(RELEASE_PLZ, message.clone()),
        Config::Parsed(table) => table,
    };
    let mut report = |message: String| problems.add(RELEASE_PLZ, message);

    let empty = Table::new();
    let section = |key: &str| table.get(key).and_then(Value::as_table).unwrap_or(&empty);
    for (key, expected) in WORKSPACE_KEYS {
        require(
            &mut report,
            "workspace",
            section("workspace"),
            key,
            &Value::Boolean(expected),
        );
    }
    let changelog = section("changelog");
    require(
        &mut report,
        "changelog",
        changelog,
        "trim",
        &Value::Boolean(false),
    );
    match changelog.get("body") {
        Some(Value::String(body)) => {
            if !body.contains(VERSION_HEADING) || body.contains("commits") {
                report(format!(
                    "changelog.body must render only a \"{VERSION_HEADING}\" heading"
                ));
            }
        }
        found => report(format!(
            "changelog.body must be a string (found {})",
            shown(found)
        )),
    }

    let entries = packages(table, ws, &mut report);
    let facade = entries.get(FACADE).copied().unwrap_or(&empty);
    let facade_keys = [
        ("changelog_update", Value::Boolean(true)),
        ("changelog_path", Value::String(CHANGELOG.to_owned())),
        ("semver_check", Value::Boolean(true)),
        ("git_tag_enable", Value::Boolean(true)),
        ("git_tag_name", Value::String("v{{ version }}".to_owned())),
        ("git_release_enable", Value::Boolean(true)),
    ];
    for (key, expected) in &facade_keys {
        require(&mut report, "package noctis", facade, key, expected);
    }
    check_changelog_include(facade, ws, &mut report);

    for member in &ws.members {
        let entry = entries.get(member.name.as_str());
        let flag = |key: &str| entry.and_then(|e| e.get(key)).and_then(Value::as_bool);
        if member.published {
            if flag("release") == Some(false) || flag("publish") == Some(false) {
                report(format!(
                    "{} is a published crate but release-plz skips it",
                    member.name
                ));
            }
        } else if flag("release") != Some(false) {
            report(format!(
                "{} is publish = false but has no release = false",
                member.name
            ));
        }
    }
}

/// R6: the `[[package]]` entries by name; bad entries are reported.
fn packages<'t>(
    table: &'t Table,
    ws: &Workspace,
    report: &mut impl FnMut(String),
) -> BTreeMap<&'t str, &'t Table> {
    let mut entries = BTreeMap::new();
    let list = table.get("package").and_then(Value::as_array);
    for entry in list.into_iter().flatten() {
        let Some(entry) = entry.as_table() else {
            report("[[package]] is not a table".to_owned());
            continue;
        };
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            report("[[package]] without name".to_owned());
            continue;
        };
        if ws.member(name).is_none() {
            report(format!("[[package]] {name} is not a workspace member"));
        }
        if entries.insert(name, entry).is_some() {
            report(format!("duplicate [[package]] {name}"));
        }
    }
    entries
}

/// R3: `package noctis` `changelog_include` names exactly P without the facade.
fn check_changelog_include(facade: &Table, ws: &Workspace, report: &mut impl FnMut(String)) {
    let expected: BTreeSet<&str> = ws
        .published()
        .map(|m| m.name.as_str())
        .filter(|name| *name != FACADE)
        .collect();
    let found = facade.get("changelog_include");
    let Some(names) = found.and_then(Value::as_array).and_then(|list| {
        list.iter()
            .map(Value::as_str)
            .collect::<Option<BTreeSet<&str>>>()
    }) else {
        return report(format!(
            "package noctis.changelog_include must be an array of the published crates \
             (found {})",
            shown(found)
        ));
    };
    for name in expected.difference(&names) {
        report(format!("package noctis changelog_include lacks {name}"));
    }
    for name in names.difference(&expected) {
        report(format!(
            "package noctis changelog_include names {name}, which is not a published crate"
        ));
    }
}

/// R2: `<table>.<key>` equals `expected`.
fn require(
    report: &mut impl FnMut(String),
    table_name: &str,
    table: &Table,
    key: &str,
    expected: &Value,
) {
    let found = table.get(key);
    if found != Some(expected) {
        report(format!(
            "{table_name}.{key} must be {expected} (found {})",
            shown(found)
        ));
    }
}

/// A TOML value as written, or `missing`.
fn shown(value: Option<&Value>) -> String {
    value.map_or_else(|| "missing".to_owned(), ToString::to_string)
}

/// H1-H3: the changelog's header (`None` when the file is missing).
pub(super) fn check_changelog(text: Option<&str>, problems: &mut Problems) {
    let Some(text) = text else {
        return problems.add(CHANGELOG, "missing");
    };
    let mut lines = text
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    if lines.next() != Some("# Changelog") {
        problems.add(CHANGELOG, "must start with \"# Changelog\"");
    }
    let mut fenced = false;
    let mut early = Vec::new();
    let mut unreleased = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if line == UNRELEASED {
            unreleased = true;
            break;
        }
        if line.starts_with("## ") {
            early.push(index + 1);
        }
    }
    if !unreleased {
        return problems.add(CHANGELOG, format!("no \"{UNRELEASED}\" heading"));
    }
    for line in early {
        problems.add(
            format!("{CHANGELOG}:{line}"),
            format!("\"{UNRELEASED}\" must be the first \"## \" heading"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release_check::model::{Dep, Kind, Member};

    fn member(name: &str, deps: &[(&str, Kind, bool)]) -> Member {
        Member {
            name: name.to_owned(),
            version: "0.1.0".to_owned(),
            published: true,
            description: Some("d".to_owned()),
            license: Some("MIT".to_owned()),
            repository: Some("https://github.com/o/r".to_owned()),
            deps: deps
                .iter()
                .map(|(to, kind, path_only)| Dep {
                    to: (*to).to_owned(),
                    kind: *kind,
                    path_only: *path_only,
                })
                .collect(),
        }
    }

    fn unpublished(mut m: Member) -> Member {
        m.published = false;
        m
    }

    fn workspace(members: Vec<Member>) -> Workspace {
        Workspace {
            version: Some("0.1.0".to_owned()),
            repository: Some("https://github.com/o/r/".to_owned()),
            members,
        }
    }

    fn base() -> Workspace {
        workspace(vec![
            member("noctis", &[("noctis-a", Kind::Normal, false)]),
            member("noctis-a", &[("noctis", Kind::Dev, true)]),
            unpublished(member("xtask", &[])),
        ])
    }

    fn ws_problems(ws: &Workspace) -> Vec<String> {
        let mut problems = Problems::default();
        check_workspace(ws, &mut problems);
        problems.lines()
    }

    fn config(ws: &Workspace, text: &str) -> Vec<String> {
        let mut problems = Problems::default();
        let config = Config::Parsed(text.parse().expect("valid toml"));
        check_release_plz(&config, ws, &mut problems);
        problems.lines()
    }

    fn changelog(text: Option<&str>) -> Vec<String> {
        let mut problems = Problems::default();
        check_changelog(text, &mut problems);
        problems.lines()
    }

    const GOLDEN: &str = r#"
[workspace]
release_always = false
changelog_update = false
semver_check = false
git_tag_enable = false
git_release_enable = false

[changelog]
trim = false
body = """
## [{{ version }}] - {{ timestamp }}
"""

[[package]]
name = "noctis"
changelog_update = true
changelog_path = "CHANGELOG.md"
changelog_include = ["noctis-a"]
semver_check = true
git_tag_enable = true
git_tag_name = "v{{ version }}"
git_release_enable = true

[[package]]
name = "xtask"
release = false
"#;

    #[test]
    fn base_workspace_passes() {
        assert!(ws_problems(&base()).is_empty());
        assert!(config(&base(), GOLDEN).is_empty());
    }

    /// K2.
    #[test]
    fn missing_workspace_version() {
        let mut ws = base();
        ws.version = None;
        assert_eq!(
            ws_problems(&ws),
            ["release-check: Cargo.toml: no [workspace.package] version"]
        );
    }

    #[test]
    fn missing_workspace_repository() {
        let mut ws = base();
        ws.repository = None;
        assert_eq!(
            ws_problems(&ws),
            ["release-check: Cargo.toml: no [workspace.package] repository"]
        );
    }

    /// C1, C2.
    #[test]
    fn foreign_published_crate_and_unpublished_facade() {
        let mut ws = base();
        ws.members[0].published = false;
        ws.members.push(member("music-bot", &[]));
        assert_eq!(
            ws_problems(&ws),
            [
                "release-check: music-bot: published but not a noctis crate (set publish = false)",
                "release-check: noctis: the facade crate is not published",
            ]
        );
    }

    /// C3, C4, C5 (a trailing `/` is ignored on both sides).
    #[test]
    fn crate_metadata_rows() {
        let mut ws = base();
        ws.members[1].version = "0.1.1".to_owned();
        ws.members[1].description = Some("  ".to_owned());
        ws.members[1].license = None;
        ws.members[0].repository = Some("https://github.com/x/y".to_owned());
        ws.members[2].repository = Some("https://github.com/x/y".to_owned());
        assert_eq!(
            ws_problems(&ws),
            [
                "release-check: noctis: repository https://github.com/x/y is not the \
                 workspace repository https://github.com/o/r",
                "release-check: noctis-a: missing description",
                "release-check: noctis-a: missing license",
                "release-check: noctis-a: version 0.1.1 is not the workspace version 0.1.0 \
                 (use version.workspace = true)",
            ]
        );
        ws.members[0].repository = Some("https://github.com/o/r/".to_owned());
        ws.members[0].description = None;
        ws.members[1] = member("noctis-a", &[]);
        assert_eq!(
            ws_problems(&ws),
            ["release-check: noctis: missing description"]
        );
    }

    /// D1, D2, D4.
    #[test]
    fn published_edges() {
        let ws = workspace(vec![
            member(
                "noctis",
                &[
                    ("noctis-a", Kind::Normal, true),
                    ("noctis-b", Kind::Build, true),
                    ("xtask", Kind::Normal, false),
                    ("xtask", Kind::Dev, false),
                ],
            ),
            member("noctis-a", &[("xtask", Kind::Dev, true)]),
            member("noctis-b", &[]),
            unpublished(member("xtask", &[("noctis", Kind::Normal, true)])),
        ]);
        assert_eq!(
            ws_problems(&ws),
            [
                "release-check: noctis -> noctis-a (normal): path-only dependency \
                 (cargo publish drops it; use noctis-a.workspace = true)",
                "release-check: noctis -> noctis-b (build): path-only dependency \
                 (cargo publish drops it; use noctis-b.workspace = true)",
                "release-check: noctis -> xtask (dev): dev-dependency on an unpublished \
                 member keeps a version",
                "release-check: noctis -> xtask (normal): depends on an unpublished member",
            ]
        );
    }

    /// D3 over an optional, transitive cycle; D5 for an acyclic versioned dev-dep.
    #[test]
    fn cyclic_dev_dependency_must_be_path_only() {
        let ws = workspace(vec![
            member("noctis", &[("noctis-t", Kind::Normal, false)]),
            member("noctis-t", &[("noctis-core", Kind::Normal, false)]),
            member(
                "noctis-core",
                &[("noctis", Kind::Dev, false), ("noctis-x", Kind::Dev, false)],
            ),
            member("noctis-x", &[]),
        ]);
        assert_eq!(
            ws_problems(&ws),
            [
                "release-check: noctis-core -> noctis (dev): cyclic dev-dependency keeps a \
                 version (make it path-only so cargo publish strips it)"
            ]
        );
        assert!(reaches(&ws, "noctis", "noctis-core"));
        assert!(!reaches(&ws, "noctis-core", "noctis"));
    }

    /// Kahn order: dependencies first, ties alphabetical, build edges count,
    /// dev edges do not.
    #[test]
    fn publish_order_is_topological_with_alphabetical_ties() {
        let ws = workspace(vec![
            member(
                "noctis",
                &[
                    ("noctis-c", Kind::Normal, false),
                    ("noctis-a", Kind::Normal, false),
                ],
            ),
            member("noctis-a", &[("noctis-b", Kind::Build, false)]),
            member("noctis-b", &[("noctis", Kind::Dev, true)]),
            member("noctis-c", &[]),
            member("noctis-d", &[("noctis-c", Kind::Normal, false)]),
            unpublished(member("xtask", &[("noctis", Kind::Normal, true)])),
        ]);
        assert_eq!(
            publish_order(&ws).expect("acyclic"),
            ["noctis-b", "noctis-c", "noctis-a", "noctis-d", "noctis"]
        );
    }

    #[test]
    fn publish_order_reports_a_cycle() {
        let ws = workspace(vec![
            member("noctis", &[("noctis-a", Kind::Normal, false)]),
            member("noctis-a", &[("noctis", Kind::Normal, false)]),
            member("noctis-z", &[]),
        ]);
        assert_eq!(
            publish_order(&ws).expect_err("cyclic"),
            ["noctis", "noctis-a"]
        );
    }

    /// R1.
    #[test]
    fn missing_or_invalid_config() {
        let mut problems = Problems::default();
        check_release_plz(&Config::Missing, &base(), &mut problems);
        check_release_plz(&Config::Invalid("bad".to_owned()), &base(), &mut problems);
        assert_eq!(
            problems.lines(),
            [
                "release-check: release-plz.toml: bad",
                "release-check: release-plz.toml: missing",
            ]
        );
    }

    /// R2 for every table.
    #[test]
    fn required_settings() {
        let text = GOLDEN
            .replace("release_always = false", "release_always = true")
            .replace("trim = false\n", "")
            .replace("git_tag_name = \"v{{ version }}\"", "git_tag_name = \"x\"");
        assert_eq!(
            config(&base(), &text),
            [
                "release-check: release-plz.toml: changelog.trim must be false (found missing)",
                "release-check: release-plz.toml: package noctis.git_tag_name must be \
                 \"v{{ version }}\" (found \"x\")",
                "release-check: release-plz.toml: workspace.release_always must be false \
                 (found true)",
            ]
        );
    }

    /// R3.
    #[test]
    fn changelog_include_drift() {
        let text = GOLDEN.replace("[\"noctis-a\"]", "[\"noctis-b\"]");
        assert_eq!(
            config(&base(), &text),
            [
                "release-check: release-plz.toml: package noctis changelog_include lacks noctis-a",
                "release-check: release-plz.toml: package noctis changelog_include names \
                 noctis-b, which is not a published crate",
            ]
        );
    }

    #[test]
    fn malformed_values() {
        let body = GOLDEN.replace(
            "body = \"\"\"\n## [{{ version }}] - {{ timestamp }}\n\"\"\"",
            "body = 1",
        );
        assert_eq!(
            config(&base(), &body),
            ["release-check: release-plz.toml: changelog.body must be a string (found 1)"]
        );
        let no_include = GOLDEN.replace("changelog_include = [\"noctis-a\"]\n", "");
        assert_eq!(
            config(&base(), &no_include),
            [
                "release-check: release-plz.toml: package noctis.changelog_include must be an \
                 array of the published crates (found missing)"
            ]
        );
        let head = &GOLDEN[..GOLDEN.find("[[package]]").expect("a [[package]]")];
        let not_table = format!("package = [1]\n{head}");
        assert!(
            config(&base(), &not_table).contains(
                &"release-check: release-plz.toml: [[package]] is not a table".to_owned()
            )
        );
    }

    /// R4, R5.
    #[test]
    fn skipped_and_unskipped_members() {
        let text = GOLDEN.replace("name = \"xtask\"\nrelease = false", "name = \"xtask\"")
            + "\n[[package]]\nname = \"noctis-a\"\npublish = false\n";
        assert_eq!(
            config(&base(), &text),
            [
                "release-check: release-plz.toml: noctis-a is a published crate but \
                 release-plz skips it",
                "release-check: release-plz.toml: xtask is publish = false but has no \
                 release = false",
            ]
        );
    }

    /// R6.
    #[test]
    fn bad_package_entries() {
        let text = format!(
            "{GOLDEN}\n[[package]]\nrelease = false\n\n[[package]]\nname = \"ghost\"\n\n\
             [[package]]\nname = \"xtask\"\nrelease = false\n"
        );
        assert_eq!(
            config(&base(), &text),
            [
                "release-check: release-plz.toml: [[package]] ghost is not a workspace member",
                "release-check: release-plz.toml: [[package]] without name",
                "release-check: release-plz.toml: duplicate [[package]] xtask",
            ]
        );
    }

    /// R7.
    #[test]
    fn body_must_render_only_the_heading() {
        let expected = [
            "release-check: release-plz.toml: changelog.body must render only a \
             \"## [{{ version }}]\" heading",
        ];
        let commits = GOLDEN.replace(" - {{ timestamp }}", "{% for c in commits %}{% endfor %}");
        assert_eq!(config(&base(), &commits), expected);
        let no_heading = GOLDEN.replace("## [{{ version }}]", "# {{ version }}");
        assert_eq!(config(&base(), &no_heading), expected);
    }

    /// H1.
    #[test]
    fn changelog_header() {
        assert_eq!(changelog(None), ["release-check: CHANGELOG.md: missing"]);
        assert_eq!(
            changelog(Some("# Changes\n\n## [Unreleased]\n")),
            ["release-check: CHANGELOG.md: must start with \"# Changelog\""]
        );
        assert_eq!(
            changelog(Some("# Changelog\n\n## Unreleased\n")),
            ["release-check: CHANGELOG.md: no \"## [Unreleased]\" heading"]
        );
    }

    /// H2 (fenced lines ignored), H3.
    #[test]
    fn unreleased_comes_first() {
        assert_eq!(
            changelog(Some("# Changelog\n\n## [0.1.0]\n\n## [Unreleased]\n")),
            [
                "release-check: CHANGELOG.md:3: \"## [Unreleased]\" must be the first \"## \" heading"
            ]
        );
        assert!(
            changelog(Some(
                "# Changelog\r\n\r\n```md\r\n## [0.0.1]\r\n```\r\n## [Unreleased]\r\n\r\nÇalışma\r\n## [0.1.0]\r\n"
            ))
            .is_empty()
        );
    }
}
