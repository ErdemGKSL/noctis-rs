//! CLI tests for `xtask release-check` over generated fixture workspaces
//! (Story 14.7's K, C, D, R, H and E rows). The rules have unit tests over
//! hand-built models in the crate.

#![allow(
    clippy::expect_used,
    reason = "fixture helpers panic on I/O failure; clippy.toml only covers #[test] bodies"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CARGO_TOML: &str = "\
[workspace]
resolver = \"3\"
members = [\"crates/noctis\", \"crates/noctis-a\", \"xtask\"]

[workspace.package]
version = \"0.1.0\"
license = \"MIT OR Apache-2.0\"
repository = \"https://github.com/o/r\"
";

const NOCTIS: &str = "\
[package]
name = \"noctis\"
version = \"0.1.0\"
edition = \"2024\"
description = \"Facade.\"
license.workspace = true
repository.workspace = true

[dependencies]
noctis-a = { path = \"../noctis-a\", version = \"0.1.0\" }
";

const NOCTIS_A: &str = "\
[package]
name = \"noctis-a\"
version = \"0.1.0\"
edition = \"2024\"
description = \"Sub-crate.\"
license.workspace = true
repository.workspace = true

[dev-dependencies]
noctis = { path = \"../noctis\" }
";

const XTASK: &str = "\
[package]
name = \"xtask\"
version = \"0.1.0\"
edition = \"2024\"
publish = false
";

const RELEASE_PLZ: &str = r#"[workspace]
release_always = false
changelog_update = false
semver_check = false
git_tag_enable = false
git_release_enable = false

[changelog]
trim = false
body = """
## [{{ version }}] - {{ timestamp | date(format="%Y-%m-%d") }}
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

const CHANGELOG: &str = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- Everything.\n";

const OK: &str = "release-check: ok (2 crates at 0.1.0; publish order: noctis-a, noctis)";

/// A fixture workspace written under the test target dir.
struct Root {
    test: &'static str,
    files: BTreeMap<String, String>,
}

fn root(test: &'static str) -> Root {
    Root {
        test,
        files: BTreeMap::new(),
    }
    .with("Cargo.toml", CARGO_TOML)
    .with("crates/noctis/Cargo.toml", NOCTIS)
    .with("crates/noctis/src/lib.rs", "")
    .with("crates/noctis-a/Cargo.toml", NOCTIS_A)
    .with("crates/noctis-a/src/lib.rs", "")
    .with("xtask/Cargo.toml", XTASK)
    .with("xtask/src/lib.rs", "")
    .with("release-plz.toml", RELEASE_PLZ)
    .with("CHANGELOG.md", CHANGELOG)
}

impl Root {
    fn with(mut self, path: &str, contents: &str) -> Self {
        self.files.insert(path.to_owned(), contents.to_owned());
        self
    }

    fn without(mut self, path: &str) -> Self {
        self.files.remove(path);
        self
    }

    /// Replaces `from` (which must occur) by `to` in the file `path`.
    fn edit(mut self, path: &str, from: &str, to: &str) -> Self {
        let text = self.files.get_mut(path).expect("fixture file");
        assert!(text.contains(from), "{path} lacks {from:?}");
        *text = text.replace(from, to);
        self
    }

    fn write(&self) -> PathBuf {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("release_check")
            .join(self.test);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create root");
        for (path, contents) in &self.files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().expect("has parent")).expect("create dir");
            fs::write(&path, contents).expect("write file");
        }
        root
    }

    fn run(&self) -> Output {
        run(&[
            "release-check",
            "--root",
            self.write().to_str().expect("utf-8 path"),
        ])
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .expect("run xtask")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Asserts exit 1 and exactly these problem lines.
fn assert_problems(out: &Output, expected: &[&str]) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines, expected);
}

#[test]
fn ok_line_is_exact() {
    let out = root("ok").run();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stderr(&out).trim_end(), OK);
}

/// K1 on the noctis-rs repository itself.
#[test]
fn real_repo_passes() {
    let out = run(&["release-check"]);
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "{err}");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let text = fs::read_to_string(manifest).expect("read the root Cargo.toml");
    let root: toml::Table = text.parse().expect("parse the root Cargo.toml");
    let version = root["workspace"]["package"]["version"]
        .as_str()
        .expect("[workspace.package] version");
    let line = err.trim_end();
    assert!(
        line.starts_with(&format!(
            "release-check: ok (13 crates at {version}; publish order: "
        )),
        "{line}"
    );
    assert!(line.ends_with(", noctis)"), "{line}");
}

/// K2.
#[test]
fn missing_workspace_version() {
    let out = root("k2")
        .edit("Cargo.toml", "version = \"0.1.0\"\n", "")
        .run();
    assert_problems(
        &out,
        &["release-check: Cargo.toml: no [workspace.package] version"],
    );
}

/// K3: no other check runs.
#[test]
fn metadata_failure_stops_the_check() {
    let out = root("k3")
        .with("crates/noctis-a/Cargo.toml", "[package\n")
        .without("CHANGELOG.md")
        .run();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(
        err.starts_with("release-check: Cargo.toml: cargo metadata failed: "),
        "{err}"
    );
}

/// C3.
#[test]
fn version_skew() {
    let out = root("c3")
        .edit("crates/noctis-a/Cargo.toml", "0.1.0", "0.1.1")
        .run();
    assert_problems(
        &out,
        &[
            "release-check: noctis-a: version 0.1.1 is not the workspace version 0.1.0 \
             (use version.workspace = true)",
        ],
    );
}

/// D1.
#[test]
fn path_only_published_edge() {
    let out = root("d1")
        .edit("crates/noctis/Cargo.toml", ", version = \"0.1.0\"", "")
        .run();
    assert_problems(
        &out,
        &[
            "release-check: noctis -> noctis-a (normal): path-only dependency \
           (cargo publish drops it; use noctis-a.workspace = true)",
        ],
    );
}

/// D3.
#[test]
fn cyclic_dev_dependency_keeps_a_version() {
    let out = root("d3")
        .edit(
            "crates/noctis-a/Cargo.toml",
            "path = \"../noctis\"",
            "path = \"../noctis\", version = \"0.1.0\"",
        )
        .run();
    assert_problems(
        &out,
        &[
            "release-check: noctis-a -> noctis (dev): cyclic dev-dependency keeps a version \
           (make it path-only so cargo publish strips it)",
        ],
    );
}

/// R1: missing, then invalid.
#[test]
fn missing_or_invalid_config() {
    let out = root("r1-missing").without("release-plz.toml").run();
    assert_problems(&out, &["release-check: release-plz.toml: missing"]);

    let out = root("r1-invalid")
        .with("release-plz.toml", "[workspace\n")
        .run();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(
        err.starts_with("release-check: release-plz.toml: "),
        "{err}"
    );
}

/// R2.
#[test]
fn required_setting() {
    let out = root("r2")
        .edit(
            "release-plz.toml",
            "release_always = false",
            "release_always = true",
        )
        .run();
    assert_problems(
        &out,
        &["release-check: release-plz.toml: workspace.release_always must be false (found true)"],
    );
}

/// R4.
#[test]
fn unpublished_member_not_skipped() {
    let out = root("r4")
        .edit(
            "release-plz.toml",
            "name = \"xtask\"\nrelease = false\n",
            "name = \"xtask\"\n",
        )
        .run();
    assert_problems(
        &out,
        &["release-check: release-plz.toml: xtask is publish = false but has no release = false"],
    );
}

/// R6.
#[test]
fn bad_package_entries() {
    let out = root("r6")
        .edit(
            "release-plz.toml",
            "name = \"xtask\"\nrelease = false\n",
            "name = \"xtask\"\nrelease = false\n\n[[package]]\nname = \"xtask\"\nrelease = false\n\n\
             [[package]]\nname = \"ghost\"\n\n[[package]]\nrelease = false\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "release-check: release-plz.toml: [[package]] ghost is not a workspace member",
            "release-check: release-plz.toml: [[package]] without name",
            "release-check: release-plz.toml: duplicate [[package]] xtask",
        ],
    );
}

/// H1.
#[test]
fn missing_changelog() {
    let out = root("h1").without("CHANGELOG.md").run();
    assert_problems(&out, &["release-check: CHANGELOG.md: missing"]);
}

/// H2: a committed release heading above `[Unreleased]` stops the release PR.
#[test]
fn release_heading_above_unreleased() {
    let out = root("h2")
        .edit(
            "CHANGELOG.md",
            "## [Unreleased]",
            "## [0.1.0] - 2026-10-26\n\n## [Unreleased]",
        )
        .run();
    assert_problems(
        &out,
        &["release-check: CHANGELOG.md:3: \"## [Unreleased]\" must be the first \"## \" heading"],
    );
}

/// E1.
#[test]
fn usage_errors() {
    for (args, first) in [
        (
            &["release-check", "--root"][..],
            "release-check: `--root` needs a value",
        ),
        (
            &["release-check", "x"][..],
            "release-check: unexpected argument: x",
        ),
    ] {
        let out = run(args);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{err}");
        assert_eq!(err.lines().next(), Some(first), "{err}");
        assert!(err.contains("release-check [--root <path>]"), "{err}");
    }
}
