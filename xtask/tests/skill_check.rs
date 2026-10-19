//! CLI tests for `xtask skill-check` over generated roots (Story 1.8's M, H, P,
//! D and X rows) and `api-snapshot`'s argument errors (S7). Line grammar,
//! assembly, matching and lint details have unit tests in the crate.

#![allow(
    clippy::expect_used,
    reason = "fixture helpers panic on I/O failure; clippy.toml only covers #[test] bodies"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SNAPSHOT: &str = "\
pub mod noctis
pub mod noctis::rules
#[non_exhaustive] pub struct noctis::rules::Limits
pub noctis::rules::Limits::button_label_len: usize
impl noctis::rules::Limits
pub fn noctis::rules::Limits::truncates(&self) -> bool
impl core::fmt::Debug for noctis::rules::Limits
pub fn noctis::rules::Limits::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
pub struct noctis::rules::RuleId(_)
pub const noctis::rules::RuleId::REG_06: noctis::rules::RuleId
pub fn noctis::rules::discord_len(&str) -> usize
";

const SKILL_DIR: &str = "examples/project-template/.claude/skills/noctis";
const PENDING: &str = "[[check]]\nname = \"component-entries\"\nstory = \"4.1\"\n";

const HANDLERS: &str = r"
#[handler]
async fn edit(ctx: ComponentCtx, responder: ComponentResponder<OpensModal>) -> HandlerResult<Responded> {
    Ok(responder.modal(m).await?.done())
}

#[event]
async fn xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    let text = msg.content;
    Ok(())
}
";

const OK: &str =
    "skill-check: ok (8 public items mentioned, 2 template files linted, 1 pending check)";

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn skill_md(snapshot: &str) -> String {
    format!(
        "# Noctis\n\n> **API hash:** `{}` (blake3)\n\nUse `noctis::rules::Limits`, `truncates`, `button_label_len`, `RuleId` and `discord_len`.\n",
        hash(snapshot.as_bytes())
    )
}

/// A generated root: path -> contents.
struct Root {
    test: &'static str,
    files: BTreeMap<String, Vec<u8>>,
}

/// A root where every check passes.
fn root(test: &'static str) -> Root {
    Root {
        test,
        files: BTreeMap::new(),
    }
    .with("xtask/public-api.txt", SNAPSHOT)
    .with("xtask/skill-pending.toml", PENDING)
    .with(&format!("{SKILL_DIR}/SKILL.md"), &skill_md(SNAPSHOT))
    .with(
        &format!("{SKILL_DIR}/references/limits.md"),
        "| REG-06 | permission overwrites |\n",
    )
    .with("examples/project-template/src/lib.rs", HANDLERS)
    .with(
        "examples/project-template/tests/flows.rs",
        "#[test]\nfn flow() { let v: Vec<u8> = it.collect(); }\n",
    )
    .with("examples/project-template/src/notes.txt", "embed")
}

impl Root {
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

    /// Replaces the snapshot and re-hashes the skill.
    fn snapshot(self, snapshot: &str) -> Self {
        self.with("xtask/public-api.txt", snapshot)
            .with(&format!("{SKILL_DIR}/SKILL.md"), &skill_md(snapshot))
    }

    fn write(&self) -> PathBuf {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("skill_check")
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
            "skill-check",
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

/// Exit 1 with exactly these problem lines (each prefixed `skill-check: `).
#[track_caller]
fn assert_problems(out: &Output, lines: &[&str]) {
    let err = stderr(out);
    assert_eq!(out.status.code(), Some(1), "stderr:\n{err}");
    let got: Vec<&str> = err.lines().collect();
    let want: Vec<String> = lines.iter().map(|l| format!("skill-check: {l}")).collect();
    assert_eq!(got, want, "stderr:\n{err}");
}

#[test]
fn real_repo_passes() {
    let out = run(&["skill-check"]);
    assert_exit(&out, 0, &[]);
    assert!(
        stderr(&out).starts_with("skill-check: ok ("),
        "{}",
        stderr(&out)
    );
    assert!(stderr(&out).contains(" template files linted, 1 pending check)"));
}

#[test]
fn x2_base_root_passes() {
    assert_exit(&root("base").run(), 0, &[OK]);
}

// M rows.

#[test]
fn m2_unmentioned_item() {
    let snapshot = format!("{SNAPSHOT}pub fn noctis::rules::command::validate_tree()\n");
    let out = root("m2").snapshot(&snapshot).run();
    assert_problems(
        &out,
        &["noctis::rules::command::validate_tree: public item not mentioned in the Usage Skill"],
    );
}

#[test]
fn m3_rule_id_needs_catalog_id_or_ident() {
    let out = root("m3")
        .with(
            &format!("{SKILL_DIR}/references/limits.md"),
            "| REG-061 | XREG-06 |\n",
        )
        .run();
    assert_problems(
        &out,
        &["noctis::rules::RuleId::REG_06: public item not mentioned in the Usage Skill"],
    );
}

#[test]
fn m8_skill_missing_or_bad() {
    let out = root("m8_missing")
        .without(&format!("{SKILL_DIR}/SKILL.md"))
        .without(&format!("{SKILL_DIR}/references/limits.md"))
        .run();
    assert_problems(
        &out,
        &[&format!("{SKILL_DIR}: cannot read: not a directory")],
    );
    let out = root("m8_utf8")
        .with_bytes(&format!("{SKILL_DIR}/references/bad.md"), &[0xff, 0xfe])
        .run();
    assert_problems(
        &out,
        &[&format!("{SKILL_DIR}/references/bad.md: not UTF-8")],
    );
}

#[test]
fn m9_snapshot_missing_or_bad() {
    let out = root("m9_missing").without("xtask/public-api.txt").run();
    assert_exit(
        &out,
        1,
        &[
            "skill-check: xtask/public-api.txt: cannot read: ",
            "; run cargo run --release -p xtask -- api-snapshot --bless",
        ],
    );
    let out = root("m9_bad").snapshot("pub mod noctis\nfoo\n").run();
    assert_problems(&out, &["xtask/public-api.txt:2: cannot parse line"]);
}

// H rows.

#[test]
fn h2_stale_hash() {
    let old = "b".repeat(64);
    let out = root("h2")
        .with(
            &format!("{SKILL_DIR}/SKILL.md"),
            &skill_md(SNAPSHOT).replace(&hash(SNAPSHOT.as_bytes()), &old),
        )
        .run();
    assert_problems(
        &out,
        &[&format!(
            "SKILL.md: API hash {old} does not match xtask/public-api.txt ({}); update the skill for the API change, then the hash",
            hash(SNAPSHOT.as_bytes())
        )],
    );
}

#[test]
fn h3_pending_hash() {
    let out = root("h3")
        .with(
            &format!("{SKILL_DIR}/SKILL.md"),
            &skill_md(SNAPSHOT).replace(&hash(SNAPSHOT.as_bytes()), "pending"),
        )
        .run();
    assert_problems(
        &out,
        &[&format!(
            "SKILL.md: API hash must be the snapshot's 64-hex blake3 hash ({})",
            hash(SNAPSHOT.as_bytes())
        )],
    );
}

#[test]
fn h4_no_hash_line() {
    let out = root("h4")
        .with(
            &format!("{SKILL_DIR}/SKILL.md"),
            "Use noctis::rules::Limits, truncates, button_label_len, RuleId and discord_len.\n",
        )
        .run();
    assert_problems(
        &out,
        &["SKILL.md: expected exactly one **API hash:** line, found 0"],
    );
}

#[test]
fn h5_crlf_snapshot() {
    let crlf = SNAPSHOT.replace('\n', "\r\n");
    let out = root("h5").snapshot(&crlf).run();
    assert_problems(&out, &["xtask/public-api.txt: must use LF line endings"]);
}

// P rows.

#[test]
fn p2_due() {
    let snapshot = format!("{SNAPSHOT}pub struct noctis::ui::Container\n");
    let out = root("p2")
        .snapshot(&snapshot)
        .with(&format!("{SKILL_DIR}/c.md"), "Container")
        .run();
    assert_problems(
        &out,
        &[
            "skill-pending.toml: component-entries: the snapshot exports component builder noctis::ui::Container; implement the component-entry check and remove this entry (Story 4.1)",
        ],
    );
}

#[test]
fn p3_not_pending() {
    let out = root("p3").with("xtask/skill-pending.toml", "").run();
    assert_problems(
        &out,
        &["skill-pending.toml: component-entries must stay pending until Story 4.1 implements it"],
    );
}

#[test]
fn p4_unknown_check() {
    let out = root("p4")
        .with(
            "xtask/skill-pending.toml",
            &format!("{PENDING}[[check]]\nname = \"other\"\nstory = \"4.1\"\n"),
        )
        .run();
    assert_problems(&out, &["skill-pending.toml: unknown check other"]);
}

#[test]
fn p5_story_format() {
    for (i, story) in ["4", "4.1.1", "0.1", "4.0", ""].into_iter().enumerate() {
        let test = ["p5_a", "p5_b", "p5_c", "p5_d", "p5_e"][i];
        let out = root(test)
            .with(
                "xtask/skill-pending.toml",
                &PENDING.replace("\"4.1\"", &format!("\"{story}\"")),
            )
            .run();
        assert_problems(
            &out,
            &[
                "skill-pending.toml: component-entries must stay pending until Story 4.1 implements it",
                "skill-pending.toml: component-entries: story must be <epic>.<story>",
            ],
        );
    }
}

#[test]
fn p6_duplicate_unknown_key_invalid() {
    let out = root("p6_dup")
        .with("xtask/skill-pending.toml", &format!("{PENDING}{PENDING}"))
        .run();
    assert_problems(
        &out,
        &["skill-pending.toml: duplicate entry component-entries"],
    );
    let out = root("p6_key")
        .with(
            "xtask/skill-pending.toml",
            &format!("{PENDING}note = \"\"\n"),
        )
        .run();
    assert_exit(
        &out,
        1,
        &[
            "skill-check: skill-pending.toml: TOML parse error at line 4, column 1 ",
            "unknown field `note`",
        ],
    );
    // Other checks still run.
    let out = root("p6_toml")
        .with("xtask/skill-pending.toml", "[[check]\n")
        .with(&format!("{SKILL_DIR}/SKILL.md"), "no hash")
        .run();
    assert_exit(
        &out,
        1,
        &[
            "skill-check: skill-pending.toml: ",
            "skill-check: SKILL.md: expected exactly one **API hash:** line, found 0",
        ],
    );
}

#[test]
fn p7_missing_file() {
    let out = root("p7").without("xtask/skill-pending.toml").run();
    assert_exit(
        &out,
        1,
        &["skill-check: xtask/skill-pending.toml: cannot read: "],
    );
}

// D rows: one combined bad template.

const BAD: &str = r#"use twilight::Embed;

#[command("x")]
async fn cmd(ctx: CommandCtx, r: CommandResponder) -> LegacyMessage {
    r.defer().await;
    tokio::spawn(async {});
    Reply { content: "hi" }
}

fn plain() -> InteractionResponseType {
    let c = ComponentCollector::new();
    ctx.wait_for_component();
    app.on_click(|e| e);
}

mod nested {
    #[handler]
    async fn h(ctx: ComponentCtx) { reply().content("x") }
}
"#;

#[test]
fn d_combined_bad_template() {
    let path = "examples/project-template/src/bad.rs";
    let out = root("d").with(path, BAD).run();
    assert_problems(
        &out,
        &[
            &format!("{path}:1: [UD-1] embeds are not an interactive output; build a View"),
            &format!(
                "{path}:10: [UD-3] manual response-type selection; return a View and let Noctis pick the response"
            ),
            &format!("{path}:11: [UD-7] collectors are not allowed; one #[handler] per Route"),
            &format!(
                "{path}:12: [UD-7] await-component loop; state goes in Payloads, Refs or ports"
            ),
            &format!("{path}:13: [UD-7] inline listener; register a #[handler] for a Route"),
            &format!("{path}:18: [UD-1] content as handler output; return a View"),
            &format!("{path}:4: [UD-1] LegacyMessage in an interactive handler; return a View"),
            &format!(
                "{path}:4: [UD-3] take a Responder only to open a modal (CommandResponder<OpensModal>)"
            ),
            &format!("{path}:5: [UD-3] manual response through the Responder; return a View"),
            &format!(
                "{path}:6: [UD-7] task spawned per interaction or event; use a Ref TTL or a port"
            ),
            &format!("{path}:7: [UD-1] content as handler output; return a View"),
        ],
    );
}

#[test]
fn d18_lex_error() {
    let path = "examples/project-template/tests/broken.rs";
    let out = root("d18").with(path, "fn f() { (\n").run();
    assert_exit(&out, 1, &[&format!("skill-check: {path}: cannot lex: ")]);
}

// X rows.

#[test]
fn x3_bad_args() {
    for args in [
        &["skill-check", "--root"][..],
        &["skill-check", "--root", "a", "b"],
        &["skill-check", "--bogus"],
    ] {
        assert_exit(&run(args), 2, &["skill-check: ", "usage: "]);
    }
}

#[test]
fn x4_many_problems() {
    let out = root("x4")
        .snapshot(&format!("{SNAPSHOT}pub fn noctis::rules::unmentioned()\n"))
        .with(
            &format!("{SKILL_DIR}/SKILL.md"),
            &skill_md(SNAPSHOT).replace("API hash:** `", "API hash:** `x"),
        )
        .with("xtask/skill-pending.toml", "")
        .with(
            "examples/project-template/src/bad.rs",
            "fn f() -> EmbedBuilder { x }\n",
        )
        .run();
    let err = stderr(&out);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 4, "{err}");
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted);
    assert_exit(
        &out,
        1,
        &[
            "noctis::rules::unmentioned: public item not mentioned",
            "SKILL.md: API hash must be",
            "component-entries must stay pending",
            "bad.rs:1: [UD-1]",
        ],
    );
}

#[test]
fn s7_api_snapshot_bad_args() {
    for args in [
        &["api-snapshot", "--bless", "x"][..],
        &["api-snapshot", "--root"],
        &["api-snapshot", "--bogus"],
    ] {
        assert_exit(
            &run(args),
            2,
            &["api-snapshot: unexpected argument: ", "usage: "],
        );
    }
}
