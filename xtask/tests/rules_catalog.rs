//! CLI tests for `xtask rules-catalog` over generated repositories. They cover
//! the F, P, K and X rows of Story 1.7's I/O matrix, and R9-R11 and C2-C6
//! through the binary; the classifier and scanners have unit tests in the crate.

#![allow(
    clippy::expect_used,
    reason = "fixture helpers panic on I/O failure; clippy.toml only covers #[test] bodies"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CATALOG: &str = "\
# Catalog

| Tier | When |
|---|---|
| **Compile-time** | `cargo check` |

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **CV2-05** Placement | x | CR | Compile-time | traits |
| **MSG-03** Mentions | x | MSG | Compile-time (by construction) | enum |
| **CV2-03** Total | 40 | CR | Compile-time (literal) / Runtime (all trees) | count |
| **INT-01** Deadline | 3 s | RR | Runtime | timer |
| **CMD-14** Mixing | x | AC | (permitted; no check) | doc |
| **LOC-05** Budget | x | AC | see CV2-03 | |

| Constant (`noctis-codec`) | Value |
|---|---|
| `REF_LEN` | 11 |
| `TAG_LEN` | 6 |
";

const UI_RS: &str = r#"#[cfg(not(any(feature = "image")))]
#[test]
fn ui() {
    trybuild::TestCases::new().compile_fail("tests/ui/**/*.rs");
}
"#;

const OK: &str =
    "rules-catalog: ok (6 rules: 4 covered, 0 pending, 1 documentation-only, 1 aliases)";

/// A generated repository: repo-relative path -> contents.
struct Repo {
    test: &'static str,
    files: BTreeMap<String, Vec<u8>>,
}

/// A repository where every requirement is met and the constants check is pending.
fn repo(test: &'static str) -> Repo {
    Repo {
        test,
        files: BTreeMap::new(),
    }
    .with("docs/discord-rules.md", CATALOG)
    .with(
        "crates/noctis/Cargo.toml",
        "[package]\nname = \"noctis\"\n\n[features]\ndefault = []\nimage = []\n",
    )
    .with("crates/noctis/tests/ui.rs", UI_RS)
    .with(
        "crates/noctis/tests/ui/cv2/CV2-05_container_in_container.rs",
        "fn main() {}\n",
    )
    .with(
        "crates/noctis/tests/ui/cv2/CV2-05_container_in_container.stderr",
        "error: [CV2-05] a Container is not a ContainerChild\n",
    )
    .with(
        "crates/noctis/tests/ui/cv2/CV2-03_forty_one.rs",
        "fn main() {}\n",
    )
    .with(
        "crates/noctis/tests/ui/cv2/CV2-03_forty_one.stderr",
        "error: [CV2-03] 41 components\n",
    )
    .with(
        "crates/noctis/tests/ui/msg/MSG-03_all_with_ids.rs",
        "fn main() {\n    let _ = MentionIds::All(&[1_u64]); // [MSG-03]\n}\n",
    )
    .with(
        "crates/noctis/tests/ui/msg/MSG-03_all_with_ids.stderr",
        "error[E0618]: expected function\n",
    )
    .with(
        "crates/x/src/lib.rs",
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn cv2_03_runtime_count() {}\n\n    #[tokio::test]\n    async fn int_01_deadline() {}\n}\n",
    )
    .with(
        "xtask/rules-pending.toml",
        "[[check]]\nname = \"custom-id-constants\"\nstory = \"2.2\"\n",
    )
}

impl Repo {
    fn with(mut self, path: &str, contents: &str) -> Self {
        self.files
            .insert(path.to_owned(), contents.as_bytes().to_vec());
        self
    }

    fn with_bytes(mut self, path: &str, contents: &[u8]) -> Self {
        self.files.insert(path.to_owned(), contents.to_vec());
        self
    }

    fn without(mut self, path: &str) -> Self {
        assert!(self.files.remove(path).is_some(), "no {path}");
        self
    }

    /// Appends to the pending file.
    fn pending(self, toml: &str) -> Self {
        let mut text = String::from_utf8(self.files["xtask/rules-pending.toml"].clone())
            .expect("utf-8 pending file");
        text.push('\n');
        text.push_str(toml);
        self.with("xtask/rules-pending.toml", &text)
    }

    /// Replaces `from` with `to` in the catalog.
    fn catalog(self, from: &str, to: &str) -> Self {
        assert!(CATALOG.contains(from), "catalog lacks {from}");
        let text = CATALOG.replace(from, to);
        self.with("docs/discord-rules.md", &text)
    }

    fn write(&self) -> PathBuf {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("rules_catalog")
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
            "rules-catalog",
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

#[track_caller]
fn assert_exit(out: &Output, code: i32, needles: &[&str]) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(code), "stderr:\n{err}");
    for needle in needles {
        assert!(err.contains(needle), "missing `{needle}` in stderr:\n{err}");
    }
}

/// Exit 1 with exactly these problem lines (each prefixed `rules-catalog: `).
#[track_caller]
fn assert_problems(out: &Output, lines: &[&str]) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(1), "stderr:\n{err}");
    let got: Vec<&str> = err.lines().collect();
    let want: Vec<String> = lines
        .iter()
        .map(|l| format!("rules-catalog: {l}"))
        .collect();
    assert_eq!(got, want, "stderr:\n{err}");
}

const MSG03: &str = "crates/noctis/tests/ui/msg/MSG-03_all_with_ids.rs";
const CV205: &str = "crates/noctis/tests/ui/cv2/CV2-05_container_in_container";

#[test]
fn base_repo_passes() {
    assert_exit(&repo("base").run(), 0, &[OK]);
}

#[test]
fn real_repo_passes() {
    let out = run(&["rules-catalog"]);
    assert_exit(&out, 0, &[]);
    assert!(
        stderr(&out).starts_with("rules-catalog: ok (125 rules:"),
        "{}",
        stderr(&out)
    );
}

// R9-R11 through the binary.

#[test]
fn r9_alias_target_must_bear_requirements() {
    let out = repo("r9_unknown").catalog("see CV2-03", "see CV2-99").run();
    assert_problems(&out, &["LOC-05: alias target CV2-99 is not a catalog rule"]);
    let out = repo("r9_doc").catalog("see CV2-03", "as CMD-14").run();
    assert_problems(&out, &["LOC-05: alias target CMD-14 is documentation-only"]);
}

#[test]
fn r10_r11_parse_errors() {
    let out = repo("r10")
        .catalog("| Runtime | timer", "| Linktime | timer")
        .run();
    assert_problems(&out, &["INT-01: cannot parse Enforcement \"Linktime\""]);
    let out = repo("r11")
        .catalog("| Runtime | timer", "| Compile-time (x) y | timer")
        .run();
    assert_problems(
        &out,
        &["INT-01: cannot parse Enforcement \"Compile-time (x) y\""],
    );
    let out = repo("r11_empty")
        .catalog("| Runtime | timer", "|  | timer")
        .run();
    assert_problems(&out, &["INT-01: cannot parse Enforcement \"\""]);
}

// C2-C6.

#[test]
fn c2_missing_catalog() {
    let out = repo("c2").without("docs/discord-rules.md").run();
    assert_exit(
        &out,
        1,
        &["rules-catalog: docs/discord-rules.md: cannot read"],
    );
}

#[test]
fn c3_no_enforcement_column() {
    let out = repo("c3")
        .catalog("| Source | Enforcement | How |", "| Source | Tier | How |")
        .run();
    assert_exit(
        &out,
        1,
        &[
            "rules-catalog: CV2-05: table has no Enforcement column",
            "rules-catalog: INT-01: table has no Enforcement column",
        ],
    );
}

#[test]
fn c4_duplicate_row() {
    let out = repo("c4")
        .catalog("| **INT-01** Deadline", "| **CV2-05** Again")
        .run();
    assert_exit(&out, 1, &["rules-catalog: CV2-05: duplicate catalog row"]);
}

#[test]
fn c5_short_row() {
    let out = repo("c5")
        .catalog(
            "| **INT-01** Deadline | 3 s | RR | Runtime | timer |",
            "| **INT-01** Deadline | 3 s |",
        )
        .run();
    assert_problems(&out, &["INT-01: row has no Enforcement cell"]);
}

#[test]
fn c6_zero_rows() {
    let out = repo("c6")
        .with(
            "docs/discord-rules.md",
            "# Empty\n\n| Constant (`noctis-codec`) | Value |\n|---|---|\n| `REF_LEN` | 11 |\n",
        )
        .run();
    assert_exit(
        &out,
        1,
        &["rules-catalog: docs/discord-rules.md: no rule rows found"],
    );
}

// F1-F14.

#[test]
fn f2_stderr_lacks_id() {
    let out = repo("f2")
        .with(&format!("{CV205}.stderr"), "error: something else\n")
        .run();
    assert_problems(
        &out,
        &[
            "CV2-05: Compile-time row has no fixture whose .stderr contains [CV2-05]",
            &format!("{CV205}.rs: .stderr does not contain [CV2-05]"),
        ],
    );
}

#[test]
fn f4_f5_tag_alone_or_wrong_id() {
    for (test, source) in [
        ("f4", "fn main() {\n    // [MSG-03]\n    let _ = 1;\n}\n"),
        ("f5", "fn main() {\n    let _ = 1; // [CV2-05]\n}\n"),
    ] {
        let out = repo(test).with(MSG03, source).run();
        assert_problems(
            &out,
            &[
                "MSG-03: Compile-time (by construction) row has no fixture or compile_fail \
                 doctest carrying // [MSG-03]",
                &format!("{MSG03}: no line carries // [MSG-03] after code"),
            ],
        );
    }
}

#[test]
fn f6_bad_names() {
    for (test, path) in [
        ("f6_lower", "crates/noctis/tests/ui/msg/msg-03_x.rs"),
        ("f6_noslug", "crates/noctis/tests/ui/msg/MSG-03.rs"),
        ("f6_empty_slug", "crates/noctis/tests/ui/msg/MSG-03_.rs"),
        ("f6_upper_slug", "crates/noctis/tests/ui/msg/MSG-03_Bad.rs"),
        ("f6_no_area", "crates/noctis/tests/ui/MSG-03_x.rs"),
        ("f6_deep", "crates/noctis/tests/ui/a/b/MSG-03_x.rs"),
    ] {
        let stderr_path = format!("{}.stderr", path.trim_end_matches(".rs"));
        let out = repo(test)
            .with(path, "fn main() {}\n")
            .with(&stderr_path, "x\n")
            .run();
        assert_problems(
            &out,
            &[&format!(
                "{path}: fixture must be <suite>/<area>/<RULE-ID>_<slug>.rs"
            )],
        );
    }
}

#[test]
fn f7_area_mismatch() {
    let path = "crates/noctis/tests/ui/cmd/MSG-03_x.rs";
    let out = repo("f7")
        .with(path, "fn main() {\n    let _ = 1; // [MSG-03]\n}\n")
        .with("crates/noctis/tests/ui/cmd/MSG-03_x.stderr", "x\n")
        .run();
    assert_problems(&out, &[&format!("{path}: area directory must be msg")]);
}

#[test]
fn f8_unknown_id() {
    let path = "crates/noctis/tests/ui/zzz/ZZZ-01_x.rs";
    let out = repo("f8")
        .with(path, "fn main() {}\n")
        .with("crates/noctis/tests/ui/zzz/ZZZ-01_x.stderr", "x\n")
        .run();
    assert_problems(&out, &[&format!("{path}: ZZZ-01 is not a catalog rule")]);
}

#[test]
fn f9_no_compile_tier() {
    for (test, id, area, hint) in [
        ("f9_runtime", "INT-01", "int", ""),
        ("f9_alias", "LOC-05", "loc", "; use CV2-03"),
        ("f9_doc", "CMD-14", "cmd", ""),
    ] {
        let path = format!("crates/noctis/tests/ui/{area}/{id}_x.rs");
        let out = repo(test)
            .with(&path, "fn main() {}\n")
            .with(
                &format!("crates/noctis/tests/ui/{area}/{id}_x.stderr"),
                "x\n",
            )
            .run();
        assert_problems(
            &out,
            &[&format!("{path}: {id} has no Compile-time tier{hint}")],
        );
    }
}

#[test]
fn f10_missing_orphan_unexpected() {
    let out = repo("f10_missing")
        .without(&format!("{CV205}.stderr"))
        .run();
    assert_problems(
        &out,
        &[
            "CV2-05: Compile-time row has no fixture whose .stderr contains [CV2-05]",
            &format!("{CV205}.rs: missing .stderr"),
        ],
    );
    let out = repo("f10_orphan")
        .with("crates/noctis/tests/ui/cv2/CV2-05_gone.stderr", "x\n")
        .with("crates/noctis/tests/ui/cv2/notes.txt", "x\n")
        .run();
    assert_problems(
        &out,
        &[
            "crates/noctis/tests/ui/cv2/CV2-05_gone.stderr: orphan .stderr",
            "crates/noctis/tests/ui/cv2/notes.txt: unexpected file",
        ],
    );
}

#[test]
fn f11_feature_suites() {
    let fixture = "crates/noctis/tests/ui-image/cv2/CV2-05_x";
    let out = repo("f11_unregistered")
        .with(&format!("{fixture}.rs"), "fn main() {}\n")
        .with(&format!("{fixture}.stderr"), "[CV2-05]\n")
        .run();
    assert_problems(
        &out,
        &["crates/noctis/tests/ui.rs: no compile_fail suite for tests/ui-image"],
    );
    let registered = format!(
        "{UI_RS}#[cfg(feature = \"image\")]\n#[test]\nfn ui_image() {{\n    \
         trybuild::TestCases::new().compile_fail(\"tests/ui-image/**/*.rs\");\n}}\n"
    );
    let out = repo("f11_ok")
        .with("crates/noctis/tests/ui.rs", &registered)
        .with(&format!("{fixture}.rs"), "fn main() {}\n")
        .with(&format!("{fixture}.stderr"), "[CV2-05]\n")
        .run();
    assert_exit(&out, 0, &[OK]);
    let out = repo("f11_unknown")
        .with(
            "crates/noctis/tests/ui-gpu/cv2/CV2-05_x.rs",
            "fn main() {}\n",
        )
        .with(
            "crates/noctis/tests/ui-gpu/cv2/CV2-05_x.stderr",
            "[CV2-05]\n",
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "rules-catalog: crates/noctis/tests/ui-gpu: unknown facade feature gpu",
            "rules-catalog: crates/noctis/tests/ui.rs: no compile_fail suite for tests/ui-gpu",
        ],
    );
}

#[test]
fn f12_registration() {
    let out = repo("f12_missing")
        .without("crates/noctis/tests/ui.rs")
        .run();
    assert_exit(
        &out,
        1,
        &["rules-catalog: crates/noctis/tests/ui.rs: cannot read"],
    );
    let out = repo("f12_bad")
        .with(
            "crates/noctis/tests/ui.rs",
            "#[test]\nfn ui() {\n    trybuild::TestCases::new().pass(\"tests/ok/*.rs\");\n}\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "crates/noctis/tests/ui.rs: no compile_fail suite for tests/ui",
            "crates/noctis/tests/ui.rs: registers a trybuild pass suite; suites are compile_fail only",
            "crates/noctis/tests/ui.rs: the featureless suite is not gated on feature = \"image\"",
        ],
    );
}

#[test]
fn f13_stray_suite() {
    let out = repo("f13")
        .with(
            "crates/noctis-macros/tests/ui/cv2/CV2-05_x.rs",
            "fn main() {}\n",
        )
        .run();
    assert_problems(
        &out,
        &["crates/noctis-macros/tests/ui: trybuild fixtures belong in crates/noctis/tests/ui"],
    );
}

#[test]
fn f14_empty_suite_is_no_error_by_itself() {
    let out = repo("f14")
        .without(&format!("{CV205}.rs"))
        .without(&format!("{CV205}.stderr"))
        .pending("[[rule]]\nid = \"CV2-05\"\nevidence = \"diagnosed\"\nstory = \"4.1\"\n")
        .run();
    assert_exit(
        &out,
        0,
        &["rules-catalog: ok (6 rules: 3 covered, 1 pending, 1 documentation-only, 1 aliases)"],
    );
}

// D1, D6 through the binary.

#[test]
fn d1_doctest_is_by_construction_evidence() {
    let out = repo("d1")
        .without(MSG03)
        .without("crates/noctis/tests/ui/msg/MSG-03_all_with_ids.stderr")
        .with(
            "crates/x/src/mentions.rs",
            "//! ```compile_fail\n//! let _ = MentionIds::All(&[1_u64]); // [MSG-03]\n//! ```\n",
        )
        .run();
    assert_exit(&out, 0, &[OK]);
}

#[test]
fn d6_unknown_tag_ids() {
    let out = repo("d6")
        .with(
            "crates/x/src/bad.rs",
            "//! intro\n/// ```compile_fail\n/// x(); // [ZZZ-99]\n/// ```\n",
        )
        .with(
            MSG03,
            "fn main() {\n    let _ = MentionIds::All(&[1_u64]); // [MSG-03]\n    // [ZZZ-98]\n}\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            &format!("{MSG03}:3: ZZZ-98 is not a catalog rule"),
            "crates/x/src/bad.rs:3: ZZZ-99 is not a catalog rule",
        ],
    );
}

// U through the binary: ignored tests, UI suites and xtask are not evidence.

#[test]
fn u_ignored_and_excluded_tests_are_not_evidence() {
    let out = repo("u_excluded")
        .with(
            "crates/x/src/lib.rs",
            "#[test]\n#[ignore]\nfn int_01_deadline() {}\n#[test]\nfn cv2_03_count() {}\n",
        )
        .with("xtask/src/lib.rs", "#[test]\nfn int_01_x() {}\n")
        .with("examples/a/src/lib.rs", "#[test]\nfn int_01_x() {}\n")
        // A UI fixture is no unit evidence; reuse a valid one to hold the test fn.
        .with(
            MSG03,
            "fn main() {\n    let _ = MentionIds::All(&[1_u64]); // [MSG-03]\n}\n\
             #[test]\nfn int_01_x() {}\n",
        )
        .run();
    assert_problems(
        &out,
        &["INT-01: App-build or Runtime row has no unit test citing int_01"],
    );
}

#[test]
fn u_integration_tests_are_evidence() {
    let out = repo("u_integration")
        .with("crates/x/src/lib.rs", "#[test]\nfn cv2_03_count() {}\n")
        .with("crates/x/tests/it.rs", "#[test]\nfn int_01_deadline() {}\n")
        .run();
    assert_exit(&out, 0, &[OK]);
}

#[test]
fn u8_non_utf8_source() {
    let out = repo("u8")
        .with_bytes("crates/x/src/bin.rs", &[0xff, 0xfe, b'\n'])
        .run();
    assert_problems(&out, &["crates/x/src/bin.rs: not UTF-8"]);
}

// P1-P11.

#[test]
fn p1_p3_pending_and_missing() {
    let out = repo("p3").with("crates/x/src/lib.rs", "").run();
    assert_problems(
        &out,
        &[
            "CV2-03: App-build or Runtime row has no unit test citing cv2_03",
            "INT-01: App-build or Runtime row has no unit test citing int_01",
        ],
    );
    let out = repo("p1")
        .with("crates/x/src/lib.rs", "")
        .pending(
            "[[rule]]\nid = \"CV2-03\"\nevidence = \"unit\"\nstory = \"4.3\"\n\n\
             [[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"6.6\"\n",
        )
        .run();
    assert_exit(
        &out,
        0,
        &["rules-catalog: ok (6 rules: 2 covered, 2 pending, 1 documentation-only, 1 aliases)"],
    );
}

#[test]
fn p3_missing_by_construction() {
    let out = repo("p3_bc")
        .without(MSG03)
        .without("crates/noctis/tests/ui/msg/MSG-03_all_with_ids.stderr")
        .run();
    assert_problems(
        &out,
        &[
            "MSG-03: Compile-time (by construction) row has no fixture or compile_fail doctest \
           carrying // [MSG-03]",
        ],
    );
}

#[test]
fn p2_satisfied_pending() {
    let out = repo("p2")
        .pending("[[rule]]\nid = \"CV2-03\"\nevidence = \"diagnosed\"\nstory = \"4.4\"\n")
        .run();
    assert_problems(
        &out,
        &[
            "CV2-03: pending entry (diagnosed, Story 4.4) already has a test; remove it from \
           xtask/rules-pending.toml",
        ],
    );
}

#[test]
fn p4_p5_unknown_or_not_required() {
    let out = repo("p45")
        .pending(
            "[[rule]]\nid = \"ZZZ-01\"\nevidence = \"unit\"\nstory = \"1.1\"\n\n\
             [[rule]]\nid = \"CMD-14\"\nevidence = \"unit\"\nstory = \"1.1\"\n\n\
             [[rule]]\nid = \"LOC-05\"\nevidence = \"diagnosed\"\nstory = \"1.1\"\n\n\
             [[rule]]\nid = \"INT-01\"\nevidence = \"diagnosed\"\nstory = \"1.1\"\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "rules-pending.toml: CMD-14 does not require unit evidence",
            "rules-pending.toml: INT-01 does not require diagnosed evidence",
            "rules-pending.toml: LOC-05 does not require diagnosed evidence",
            "rules-pending.toml: ZZZ-01 is not a catalog rule",
        ],
    );
}

#[test]
fn p6_bad_evidence_key_or_table() {
    for (test, toml) in [
        (
            "p6_evidence",
            "[[rule]]\nid = \"INT-01\"\nevidence = \"runtime\"\nstory = \"6.6\"\n",
        ),
        (
            "p6_key",
            "[[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"6.6\"\nnote = \"\"\n",
        ),
        ("p6_table", "[other]\nx = 1\n"),
    ] {
        let out = repo(test).pending(toml).run();
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(1), "{err}");
        // The whole file is rejected, so the constants check is no longer
        // pending either (P11).
        let pending_lines = err
            .lines()
            .filter(|l| l.starts_with("rules-catalog: rules-pending.toml: "))
            .count();
        assert_eq!(pending_lines, 1, "{err}");
    }
}

#[test]
fn p7_story_format() {
    for (ix, story) in ["4", "4.4.1", "0.3", "4.0", "v4.4", ""].iter().enumerate() {
        let toml = format!("[[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"{story}\"\n");
        let test = ["p7_a", "p7_b", "p7_c", "p7_d", "p7_e", "p7_f"][ix];
        let out = repo(test).pending(&toml).run();
        assert_problems(
            &out,
            &["rules-pending.toml: INT-01: story must be <epic>.<story>"],
        );
    }
}

#[test]
fn p8_duplicates() {
    let entry = "[[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"6.6\"\n";
    let out = repo("p8")
        .with("crates/x/src/lib.rs", "#[test]\nfn cv2_03_x() {}\n")
        .pending(&format!(
            "{entry}\n{entry}\n[[check]]\nname = \"custom-id-constants\"\nstory = \"2.2\"\n"
        ))
        .run();
    assert_problems(
        &out,
        &[
            "rules-pending.toml: duplicate entry INT-01 (unit)",
            "rules-pending.toml: duplicate entry check custom-id-constants",
        ],
    );
}

#[test]
fn p9_empty_or_missing() {
    let out = repo("p9_empty")
        .with("xtask/rules-pending.toml", "# nothing pending\n")
        .run();
    assert_problems(
        &out,
        &[
            "custom-id constant REF_LEN: not defined in noctis-codec",
            "custom-id constant TAG_LEN: not defined in noctis-codec",
        ],
    );
    let out = repo("p9_missing").without("xtask/rules-pending.toml").run();
    assert_exit(
        &out,
        1,
        &["rules-catalog: xtask/rules-pending.toml: cannot read"],
    );
}

#[test]
fn p10_unknown_check() {
    let out = repo("p10")
        .pending("[[check]]\nname = \"other\"\nstory = \"2.2\"\n")
        .run();
    assert_problems(&out, &["rules-pending.toml: unknown check other"]);
}

#[test]
fn p11_invalid_toml_keeps_other_checks() {
    let out = repo("p11")
        .with("xtask/rules-pending.toml", "[[rule]\n")
        .with("crates/x/src/lib.rs", "")
        .run();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("rules-catalog: rules-pending.toml: "), "{err}");
    assert!(
        err.contains("rules-catalog: INT-01: App-build or Runtime row has no unit test"),
        "{err}"
    );
    assert!(
        err.contains("custom-id constant REF_LEN: not defined"),
        "{err}"
    );
}

// K1-K7.

const NOT_PENDING: &str = "# constants compared\n";

#[test]
fn k2_pending_but_defined() {
    let out = repo("k2")
        .with(
            "crates/noctis-codec/src/lib.rs",
            "pub const REF_LEN: usize = 11;\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "custom-id constant REF_LEN: noctis-codec defines it; remove the custom-id-constants \
           pending entry",
        ],
    );
}

#[test]
fn k3_k4_k5_comparison() {
    let ok = repo("k3")
        .with("xtask/rules-pending.toml", NOT_PENDING)
        .with(
            "crates/noctis-codec/src/layout.rs",
            "pub const REF_LEN: usize = 1_1;\npub const TAG_LEN: usize = 6;\n",
        )
        .run();
    assert_exit(&ok, 0, &[OK]);
    let out = repo("k45")
        .with("xtask/rules-pending.toml", NOT_PENDING)
        .with(
            "crates/noctis-codec/src/lib.rs",
            "pub const REF_LEN: usize = 12;\npub(crate) const TAG_LEN: usize = 6;\nconst TAG_LEN: usize = 6;\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "custom-id constant REF_LEN: catalog says 11, noctis-codec says 12",
            "custom-id constant TAG_LEN: not defined in noctis-codec",
        ],
    );
    let out = repo("k5_twice")
        .with("xtask/rules-pending.toml", NOT_PENDING)
        .with(
            "crates/noctis-codec/src/lib.rs",
            "pub const REF_LEN: usize = 11;\npub const TAG_LEN: usize = 6;\npub const TAG_LEN: usize = 6;\n",
        )
        .run();
    assert_problems(&out, &["custom-id constant TAG_LEN: defined 2 times"]);
}

#[test]
fn k6_broken_table() {
    for (test, from, to, message) in [
        (
            "k6_absent",
            "| Constant (`noctis-codec`) | Value |",
            "| Name | Value |",
            "not found",
        ),
        (
            "k6_value",
            "| `TAG_LEN` | 6 |",
            "| `TAG_LEN` | six |",
            "has a non-integer value \"six\" for TAG_LEN",
        ),
        (
            "k6_dup",
            "| `TAG_LEN` | 6 |",
            "| `REF_LEN` | 6 |",
            "lists REF_LEN twice",
        ),
        (
            "k6_name",
            "| `TAG_LEN` | 6 |",
            "| `tag_len` | 6 |",
            "has an invalid constant name \"`tag_len`\"",
        ),
    ] {
        let out = repo(test).catalog(from, to).run();
        assert_problems(
            &out,
            &[&format!(
                "docs/discord-rules.md: custom_id constants table {message}"
            )],
        );
    }
    let out = repo("k6_empty")
        .catalog("| `REF_LEN` | 11 |\n| `TAG_LEN` | 6 |\n", "")
        .run();
    assert_problems(
        &out,
        &["docs/discord-rules.md: custom_id constants table is empty"],
    );
}

// X1-X4.

#[test]
fn x3_bad_args() {
    for args in [
        &["rules-catalog", "--root"][..],
        &["rules-catalog", "--root", "a", "b"],
        &["rules-catalog", "--bogus"],
    ] {
        assert_exit(&run(args), 2, &["rules-catalog: ", "usage:"]);
    }
}

#[test]
fn x2_missing_inputs() {
    let out = run(&["rules-catalog", "--root", "/nonexistent/rules-catalog"]);
    assert_exit(
        &out,
        1,
        &["rules-catalog: docs/discord-rules.md: cannot read"],
    );
}

#[test]
fn x4_all_problems_sorted() {
    let out = repo("x4")
        .with("crates/x/src/lib.rs", "")
        .with(&format!("{CV205}.stderr"), "nothing\n")
        .pending("[[check]]\nname = \"other\"\nstory = \"2.2\"\n")
        .run();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    let lines: Vec<&str> = err.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(lines, sorted);
    assert_eq!(lines.len(), 5, "{err}");
}
