//! CLI tests for `xtask deps-check` over generated fixture workspaces, one per row
//! of Story 1.2's I/O matrix.

#![allow(
    clippy::expect_used,
    reason = "fixture helpers panic on I/O failure; clippy.toml only covers #[test] bodies"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// A fixture workspace: every member lives in `crates/<name>`, so path deps are
/// always `../<name>`.
struct Fixture {
    test: &'static str,
    /// Member name -> extra manifest TOML appended after `[package]`.
    members: BTreeMap<String, String>,
}

fn fixture(test: &'static str) -> Fixture {
    Fixture {
        test,
        members: CRATES
            .iter()
            .map(|c| ((*c).to_owned(), String::new()))
            .collect(),
    }
}

/// A `[dependencies]`-style line for a path dependency on `name`.
fn path_dep(name: &str) -> String {
    format!("{name} = {{ path = \"../{name}\" }}\n")
}

impl Fixture {
    /// Appends manifest TOML to `member` (adding the member if it is new).
    fn with(mut self, member: &str, toml: &str) -> Self {
        let entry = self.members.entry(member.to_owned()).or_default();
        entry.push_str(toml);
        entry.push('\n');
        self
    }

    fn without(mut self, member: &str) -> Self {
        self.members.remove(member);
        self
    }

    fn write(&self) -> PathBuf {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(self.test);
        let _ = fs::remove_dir_all(&root);
        let members: Vec<String> = self
            .members
            .keys()
            .map(|m| format!("\"crates/{m}\""))
            .collect();
        write(
            &root.join("Cargo.toml"),
            &format!(
                "[workspace]\nresolver = \"3\"\nmembers = [{}]\n",
                members.join(", ")
            ),
        );
        for (name, extra) in &self.members {
            let dir = root.join("crates").join(name);
            write(
                &dir.join("Cargo.toml"),
                &format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{extra}"
                ),
            );
            write(&dir.join("src/lib.rs"), "");
        }
        root.join("Cargo.toml")
    }

    fn run(&self) -> Output {
        run(&[
            "deps-check",
            "--manifest-path",
            self.write().to_str().expect("utf-8 path"),
        ])
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("has parent")).expect("create dir");
    fs::write(path, contents).expect("write file");
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

#[track_caller]
fn assert_exit(out: &Output, code: i32, needles: &[&str]) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(code), "stderr:\n{err}");
    for needle in needles {
        assert!(err.contains(needle), "missing `{needle}` in stderr:\n{err}");
    }
}

#[track_caller]
fn assert_ok(out: &Output) {
    assert_exit(out, 0, &["deps-check: ok ("]);
}

/// Every drawn edge, with the gated ones behind their features.
fn full_graph(test: &'static str) -> Fixture {
    let deps = |names: &[&str]| -> String {
        let mut toml = "[dependencies]\n".to_owned();
        for n in names {
            toml.push_str(&path_dep(n));
        }
        toml
    };
    let facade_deps: Vec<&str> = CRATES[1..]
        .iter()
        .copied()
        .filter(|c| *c != "noctis-testkit")
        .collect();
    fixture(test)
        .with("noctis-prefix", &deps(&["noctis-core"]))
        .with(
            "noctis-testkit",
            &deps(&["noctis-core", "noctis-gateway", "noctis-ports"]),
        )
        .with(
            "noctis-core",
            &deps(&[
                "noctis-gateway",
                "noctis-i18n",
                "noctis-ui",
                "noctis-ports",
                "noctis-codec",
                "noctis-rules",
            ]),
        )
        .with("noctis-i18n", &deps(&["noctis-ports", "noctis-rules"]))
        .with("noctis-gateway", &deps(&["noctis-ports"]))
        .with("noctis-ui", &deps(&["noctis-codec", "noctis-rules"]))
        .with("noctis-codec", &deps(&["noctis-rules"]))
        .with("noctis-macros", &deps(&["noctis-rules", "noctis-image"]))
        .with(
            "noctis-image",
            r#"[features]
default = []
render = ["dep:noctis-core", "dep:noctis-ports", "dep:noctis-ui"]

[dependencies]
noctis-core = { path = "../noctis-core", optional = true }
noctis-ports = { path = "../noctis-ports", optional = true }
noctis-ui = { path = "../noctis-ui", optional = true }
"#,
        )
        .with(
            "noctis",
            &format!(
                "[features]\ntestkit = [\"dep:noctis-testkit\"]\n\n{}noctis-testkit = {{ path = \"../noctis-testkit\", optional = true }}\n",
                deps(&facade_deps)
            ),
        )
}

const IMAGE_RENDER: &str = "[features]\nrender = []\n";

#[test]
fn clean_workspace_passes() {
    assert_exit(
        &fixture("clean").run(),
        0,
        &["deps-check: ok (12 internal crates, 0 internal edges)"],
    );
}

#[test]
fn full_graph_passes() {
    assert_exit(
        &full_graph("full_graph").run(),
        0,
        &["deps-check: ok (12 internal crates, 32 internal edges)"],
    );
}

#[test]
fn undrawn_edge_fails() {
    let out = fixture("undrawn_edge")
        .with(
            "noctis-ui",
            &format!("[dependencies]\n{}", path_dep("noctis-core")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "deps-check: noctis-ui -> noctis-core (normal): ",
            "[undrawn-edge]",
        ],
    );
}

#[test]
fn testkit_to_image_fails() {
    let out = fixture("testkit_to_image")
        .with(
            "noctis-testkit",
            &format!("[dependencies]\n{}", path_dep("noctis-image")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &["noctis-testkit -> noctis-image (normal)", "[undrawn-edge]"],
    );
}

#[test]
fn build_dep_on_undrawn_edge_fails() {
    let out = fixture("build_dep")
        .with(
            "noctis-rules",
            &format!("[build-dependencies]\n{}", path_dep("noctis-codec")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &["noctis-rules -> noctis-codec (build)", "[undrawn-edge]"],
    );
}

#[test]
fn normal_dep_on_facade_fails() {
    let out = fixture("depends_on_facade")
        .with(
            "noctis-core",
            &format!("[dependencies]\n{}", path_dep("noctis")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &["noctis-core -> noctis (normal)", "[depends-on-facade]"],
    );
}

#[test]
fn dev_dep_on_facade_passes() {
    let out = fixture("facade_dev_dep")
        .with("noctis", "[features]\ntestkit = []\n")
        .with(
            "noctis-core",
            "[dev-dependencies]\nnoctis = { path = \"../noctis\", features = [\"testkit\"] }\n",
        )
        .run();
    assert_ok(&out);
}

#[test]
fn dev_dep_on_drawn_edge_passes() {
    let out = fixture("dev_dep_drawn")
        .with(
            "noctis-ui",
            &format!("[dev-dependencies]\n{}", path_dep("noctis-codec")),
        )
        .run();
    assert_ok(&out);
}

#[test]
fn dev_dep_on_testkit_fails() {
    let out = fixture("dev_dep_testkit")
        .with(
            "noctis-core",
            &format!("[dev-dependencies]\n{}", path_dep("noctis-testkit")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &["noctis-core -> noctis-testkit (dev)", "[undrawn-edge]"],
    );
}

#[test]
fn image_edge_not_optional_fails() {
    let out = fixture("image_not_optional")
        .with(
            "noctis-image",
            &format!(
                "{IMAGE_RENDER}\n[dependencies]\n{}",
                path_dep("noctis-core")
            ),
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis-image -> noctis-core (normal)",
            "`render`",
            "[ungated-edge]",
        ],
    );
}

#[test]
fn image_edge_implicit_feature_fails() {
    let out = fixture("image_implicit_feature")
        .with(
            "noctis-image",
            "[features]\nrender = []\n\n[dependencies]\nnoctis-core = { path = \"../noctis-core\", optional = true }\n",
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis-image -> noctis-core (normal): feature `noctis-core`",
            "[ungated-edge]",
        ],
    );
}

#[test]
fn image_edge_other_feature_fails() {
    let out = fixture("image_other_feature")
        .with(
            "noctis-image",
            "[features]\nrender = []\nextra = [\"dep:noctis-ui\"]\n\n[dependencies]\nnoctis-ui = { path = \"../noctis-ui\", optional = true }\n",
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis-image -> noctis-ui (normal): feature `extra`",
            "[ungated-edge]",
        ],
    );
}

#[test]
fn feature_implying_render_passes() {
    let out = fixture("feature_implies_render")
        .with(
            "noctis-image",
            "[features]\nall = [\"render\"]\nrender = [\"dep:noctis-core\"]\n\n[dependencies]\nnoctis-core = { path = \"../noctis-core\", optional = true }\n",
        )
        .run();
    assert_ok(&out);
}

#[test]
fn testkit_not_optional_fails() {
    let out = fixture("testkit_not_optional")
        .with(
            "noctis",
            &format!("[dependencies]\n{}", path_dep("noctis-testkit")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis -> noctis-testkit (normal)",
            "`testkit`",
            "[ungated-edge]",
        ],
    );
}

#[test]
fn testkit_via_other_feature_fails() {
    let out = fixture("testkit_other_feature")
        .with(
            "noctis",
            "[features]\nimage = [\"dep:noctis-testkit\"]\n\n[dependencies]\nnoctis-testkit = { path = \"../noctis-testkit\", optional = true }\n",
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis -> noctis-testkit (normal): feature `image`",
            "[ungated-edge]",
        ],
    );
}

#[test]
fn facade_dev_dep_on_testkit_fails() {
    let out = fixture("facade_dev_testkit")
        .with(
            "noctis",
            &format!("[dev-dependencies]\n{}", path_dep("noctis-testkit")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &["noctis -> noctis-testkit (dev)", "[ungated-edge]"],
    );
}

#[test]
fn macros_dep_feature_render_fails() {
    let out = fixture("macros_render_dep")
        .with("noctis-image", IMAGE_RENDER)
        .with(
            "noctis-macros",
            "[dependencies]\nnoctis-image = { path = \"../noctis-image\", features = [\"render\"] }\n",
        )
        .run();
    assert_exit(
        &out,
        1,
        &["noctis-macros -> noctis-image (normal)", "[render-on]"],
    );
}

#[test]
fn macros_feature_render_fails() {
    let out = fixture("macros_render_feature")
        .with("noctis-image", IMAGE_RENDER)
        .with(
            "noctis-macros",
            &format!(
                "[features]\nimg = [\"noctis-image/render\"]\n\n[dependencies]\n{}",
                path_dep("noctis-image")
            ),
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis-macros -> noctis-image (normal): feature `img`",
            "[render-on]",
        ],
    );
}

#[test]
fn image_default_render_fails() {
    let out = fixture("image_default_render")
        .with(
            "noctis-image",
            "[features]\ndefault = [\"render\"]\nrender = []\n",
        )
        .run();
    assert_exit(&out, 1, &["deps-check: noctis-image: ", "[render-on]"]);
}

#[test]
fn unknown_crate_fails() {
    let out = fixture("unknown_crate").with("noctis-foo", "").run();
    assert_exit(&out, 1, &["deps-check: noctis-foo: ", "[unknown-crate]"]);
}

#[test]
fn missing_crate_fails() {
    let out = fixture("missing_crate").without("noctis-prefix").run();
    assert_exit(&out, 1, &["deps-check: noctis-prefix: ", "[missing-crate]"]);
}

#[test]
fn non_internal_member_on_internal_crate_fails() {
    for (test, section, kind) in [
        ("bot_dep_normal", "dependencies", "normal"),
        ("bot_dep_dev", "dev-dependencies", "dev"),
        ("bot_dep_build", "build-dependencies", "build"),
    ] {
        let out = fixture(test)
            .with(
                "xtask",
                &format!("[{section}]\n{}", path_dep("noctis-core")),
            )
            .run();
        assert_exit(
            &out,
            1,
            &[
                &format!("xtask -> noctis-core ({kind})"),
                "[bot-dependency]",
            ],
        );
    }
}

#[test]
fn non_internal_member_on_facade_passes() {
    let out = fixture("bot_facade")
        .with("bot", &format!("[dependencies]\n{}", path_dep("noctis")))
        .run();
    assert_ok(&out);
}

#[test]
fn several_violations_are_all_reported_sorted() {
    let out = fixture("several")
        .with(
            "noctis-ui",
            &format!("[dependencies]\n{}", path_dep("noctis-core")),
        )
        .with(
            "noctis-codec",
            &format!("[dependencies]\n{}", path_dep("noctis")),
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "noctis-ui -> noctis-core (normal)",
            "[undrawn-edge]",
            "noctis-codec -> noctis (normal)",
            "[depends-on-facade]",
        ],
    );
    let err = stderr(&out);
    let codec = err.find("noctis-codec ->").expect("codec line");
    let ui = err.find("noctis-ui ->").expect("ui line");
    assert!(codec < ui, "violations must be sorted:\n{err}");
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        &["deps-check", "--bogus"][..],
        &["deps-check", "--manifest-path"][..],
    ] {
        assert_exit(&run(args), 2, &["usage: "]);
    }
}

#[test]
fn metadata_failure_exits_1() {
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-dir/Cargo.toml");
    let out = run(&[
        "deps-check",
        "--manifest-path",
        missing.to_str().expect("utf-8 path"),
    ]);
    assert_exit(&out, 1, &["deps-check: cargo metadata failed: "]);
}
