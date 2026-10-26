//! CLI tests for `xtask docs-check` over generated roots (Story 14.6's L, A, G,
//! R and E rows). Slugs, target classification, fence detection and URL
//! scanning have unit tests in the crate.

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
members = []

[workspace.package]
repository = \"https://github.com/o/r/\"
";

/// Lines 1-3; a test's extra lines start at line 5 (after a blank line 4).
const README: &str = "# Readme\n\n[Guides](docs/guides/README.md) and [A](docs/guides/a.md).\n";

const INDEX: &str = "# Guides\n\n- [A](a.md)\n";

const GUIDE: &str = "\
Title
=====

## Closed ##

## Çalışma — notları!

## Setup

## Setup

```
# not a heading
```

<a id=\"Custom\"></a>
";

const LIB_RS: &str = "//! X.\n";

const OK: &str =
    "docs-check: ok (3 Markdown files, 3 links checked, 1 guides linked, 1 Rust files scanned)";

/// A repository root written under the test target dir.
struct Root {
    test: &'static str,
    files: BTreeMap<String, Vec<u8>>,
}

fn root(test: &'static str) -> Root {
    Root {
        test,
        files: BTreeMap::new(),
    }
    .with("Cargo.toml", CARGO_TOML)
    .with("README.md", README)
    .with("docs/guides/README.md", INDEX)
    .with("docs/guides/a.md", GUIDE)
    .with("crates/x/src/lib.rs", LIB_RS)
}

impl Root {
    fn with(self, path: &str, contents: &str) -> Self {
        self.with_bytes(path, contents.as_bytes())
    }

    fn with_bytes(mut self, path: &str, contents: &[u8]) -> Self {
        self.files.insert(path.to_owned(), contents.to_vec());
        self
    }

    fn without(mut self, path: &str) -> Self {
        self.files.remove(path);
        self
    }

    /// `README.md` with `extra` appended after a blank line (line 5 on).
    fn readme(self, extra: &str) -> Self {
        self.with("README.md", &format!("{README}\n{extra}"))
    }

    fn write(&self) -> PathBuf {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("docs_check")
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
            "docs-check",
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
fn ok_summary_is_exact() {
    let out = root("ok").run();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stderr(&out).trim_end(), OK);
}

/// L3, L4, L8-L16, A1, A5-A10, A12, R1, R3: everything here passes.
#[test]
fn valid_links_pass() {
    let out = root("valid")
        .readme(
            "\
[dir](examples/tpl/) [enc](my%20file.md) [literal](lit%G1.md) [q](docs/guides/a.md?plain=1)
![img](docs/assets/a.png) [ref][r] and [ref][r]
<p align=\"center\"><img src=\"docs/assets/a.png\"></p>

Inline <a href='docs/guides/a.md#Custom'>a</a> and <a href=unquoted-nope.md>u</a>.

`[code](nope.md)` and

```text
[fenced](nope.md)
```

    [indented](nope.md)

[ext](https://discord.com/x#y) [mail](mailto:a@b.c) <someone@example.com>
[sha](https://github.com/o/r/blob/abc123/nope.md) [issue](https://github.com/o/r/issues/1)
[bare](https://github.com/o/r) [case](https://github.com/O/r/blob/main/nope.md)
[same-repo](https://github.com/o/r/blob/main/docs/guides/a.md#closed)
[tree](https://github.com/o/r/tree/main/examples/tpl)
[top](#readme) [upper](#README) [empty](#) [empty2](docs/guides/a.md#)
[unicode](docs/guides/a.md#%C3%A7al%C4%B1%C5%9Fma--notlar%C4%B1)
[dup](docs/guides/a.md#setup-1) [setext](docs/guides/a.md#title)
[rs](crates/x/src/lib.rs#L1) [dirfrag](docs/guides/#x)

[r]: docs/guides/a.md
",
        )
        .with("examples/tpl/notes.txt", "x")
        .with("my file.md", "# Mine\n")
        .with("lit%G1.md", "# Lit\n")
        .with_bytes("docs/assets/a.png", &[0x89, b'P', b'N', b'G'])
        .with(
            "crates/x/src/lib.rs",
            "//! X.\n/// [g](https://github.com/o/r/blob/main/docs/guides/a.md#setup).\n\
             /// ```no_run\n/// ```\n/// ```compile_fail,E0277\n/// ```\n\
             /// ```text\n/// ```\n/// ```should_panic\n/// ```\n",
        )
        .with("crates/x/src/target/gen.rs", "/// ```ignore\n")
        .with("docs/guides/sub/deep.md", "# Not a guide\n")
        .with(
            "examples/tpl/.claude/skills/s/SKILL.md",
            "[a](../../../notes.txt)\n",
        )
        .with("examples/tpl/target/doc.md", "[x](nope.md)\n")
        .run();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(
        err.starts_with("docs-check: ok (5 Markdown files, "),
        "{err}"
    );
    assert!(
        err.trim_end()
            .ends_with(", 1 guides linked, 1 Rust files scanned)"),
        "{err}"
    );
}

/// L2 and E4: a missing target, reported once per line.
#[test]
fn l2_missing_file() {
    let out = root("l2")
        .readme("[x](docs/nope.md) [y](docs/nope.md)\n")
        .run();
    assert_problems(
        &out,
        &["docs-check: README.md:5: broken link docs/nope.md: no such file or directory"],
    );
}

#[test]
fn l5_leaves_repository() {
    let out = root("l5").readme("[x](../../outside.md)\n").run();
    assert_problems(
        &out,
        &["docs-check: README.md:5: link leaves the repository: ../../outside.md"],
    );
}

#[test]
fn l6_root_relative() {
    let out = root("l6").readme("[x](/docs/a.md)\n").run();
    assert_problems(
        &out,
        &[
            "docs-check: README.md:5: root-relative link /docs/a.md: use a path relative to this file",
        ],
    );
}

#[test]
fn l7_empty_target() {
    let out = root("l7").readme("[x]()\n").run();
    assert_problems(&out, &["docs-check: README.md:5: empty link target"]);
}

#[test]
fn a2_missing_anchor() {
    let out = root("a2").readme("[x](#nope)\n").run();
    assert_problems(
        &out,
        &["docs-check: README.md:5: broken anchor #nope: no such heading in README.md"],
    );
}

/// A3 (missing) and A11: a heading inside a code block is no anchor.
#[test]
fn a3_missing_cross_file_anchor() {
    let out = root("a3")
        .readme("[x](docs/guides/a.md#nope)\n[y](docs/guides/a.md#not-a-heading)\n")
        .run();
    assert_problems(
        &out,
        &[
            "docs-check: README.md:5: broken anchor #nope: no such heading in docs/guides/a.md",
            "docs-check: README.md:6: broken anchor #not-a-heading: no such heading in docs/guides/a.md",
        ],
    );
}

/// G2 and G4: a guide linked only from the index, or only in a code span.
#[test]
fn g2_guide_not_linked_from_readme() {
    let out = root("g2")
        .readme("See `docs/guides/new.md`.\n")
        .with("docs/guides/new.md", "# New\n")
        .with(
            "docs/guides/README.md",
            &format!("{INDEX}- [New](new.md)\n"),
        )
        .run();
    assert_problems(
        &out,
        &["docs-check: README.md: guide not linked: docs/guides/new.md"],
    );
}

#[test]
fn g3_guide_not_in_index() {
    let out = root("g3")
        .readme("[New](https://github.com/o/r/blob/main/docs/guides/new.md#new)\n")
        .with("docs/guides/new.md", "# New\n")
        .run();
    assert_problems(
        &out,
        &["docs-check: docs/guides/README.md: guide not linked: docs/guides/new.md"],
    );
}

#[test]
fn g5_no_guides_directory() {
    let out = root("g5")
        .with("README.md", "# Readme\n")
        .without("docs/guides/README.md")
        .without("docs/guides/a.md")
        .run();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stderr(&out).trim_end(),
        "docs-check: ok (1 Markdown files, 0 links checked, 0 guides linked, 1 Rust files scanned)"
    );
}

#[test]
fn g6_missing_index() {
    let out = root("g6").without("docs/guides/README.md").run();
    assert_problems(
        &out,
        &[
            "docs-check: README.md:3: broken link docs/guides/README.md: no such file or directory",
            "docs-check: docs/guides/README.md: missing guides index",
        ],
    );
}

#[test]
fn r1_missing_same_repo_url_in_rust() {
    let out = root("r1")
        .with(
            "crates/x/src/lib.rs",
            "//! X.\n/// See the [guide](https://github.com/o/r/blob/main/docs/guides/missing.md).\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "docs-check: crates/x/src/lib.rs:2: broken link https://github.com/o/r/blob/main/docs/guides/missing.md: no such file or directory",
        ],
    );
}

#[test]
fn r2_ignore_doctest() {
    let out = root("r2")
        .with(
            "crates/x/src/inner/mod.rs",
            "//! X.\n    /// ```rust,ignore\n    /// let x = 1;\n    /// ```\n",
        )
        .run();
    assert_problems(
        &out,
        &[
            "docs-check: crates/x/src/inner/mod.rs:2: doctest marked ignore: make it run, no_run or compile_fail",
        ],
    );
}

#[test]
fn e1_not_utf8() {
    let out = root("e1")
        .with_bytes("docs/notes.md", &[b'#', b' ', 0xff, b'\n'])
        .with_bytes("crates/x/src/bad.rs", &[0xff])
        .run();
    assert_problems(
        &out,
        &[
            "docs-check: crates/x/src/bad.rs: not UTF-8",
            "docs-check: docs/notes.md: not UTF-8",
        ],
    );
}

#[test]
fn e2_no_repository() {
    for (test, manifest) in [
        ("e2-no-field", Some("[workspace]\nmembers = []\n")),
        ("e2-no-manifest", None),
    ] {
        let mut root =
            root(test).readme("[g](https://github.com/o/r/blob/main/docs/guides/missing.md)\n");
        root = match manifest {
            Some(text) => root.with("Cargo.toml", text),
            None => root.without("Cargo.toml"),
        };
        let out = root.run();
        assert_problems(
            &out,
            &["docs-check: Cargo.toml: no [workspace.package] repository"],
        );
    }
}

#[test]
fn e3_bad_arguments() {
    for (args, message) in [
        (
            &["docs-check", "--root"][..],
            "docs-check: `--root` needs a value",
        ),
        (
            &["docs-check", "x"][..],
            "docs-check: unexpected argument: x",
        ),
    ] {
        let out = run(args);
        let err = stderr(&out);
        assert_eq!(out.status.code(), Some(2), "{err}");
        assert_eq!(err.lines().next(), Some(message));
        assert!(err.contains("docs-check [--root <path>]"), "{err}");
    }
}

#[test]
fn real_repo_passes() {
    let out = run(&["docs-check"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}
