//! CLI tests for `xtask skill-check` over generated roots (Story 1.8's M, H,
//! D and X rows, Story 4.1's C rows, Story 14.5's Q rows) and `api-snapshot`'s
//! argument errors (S7). Line grammar,
//! assembly, matching and lint details have unit tests in the crate.

#![allow(
    clippy::expect_used,
    reason = "fixture helpers panic on I/O failure; clippy.toml only covers #[test] bodies"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
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

const HANDLERS: &str = r"
#[handler]
async fn edit(ctx: ComponentCtx, responder: ComponentResponder<Manual>) -> HandlerResult<Responded> {
    Ok(responder.show_modal(m).await?.done())
}

#[event]
async fn xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    let text = msg.content;
    Ok(())
}
";

const OK: &str = "skill-check: ok (8 public items mentioned, 0 component entries checked, 3 example files linted, 4 recipes in sync, 2 rule ids indexed)";

const RECIPES_MD: &str =
    "examples/project-template/.claude/skills/noctis/references/views-and-recipes.md";
const RECIPES_RS: &str = "examples/ui-recipes/src/lib.rs";
const REQUIRED: [&str; 4] = ["tabs", "pagination", "step-flow", "settings"];

/// The four required recipes as Markdown blocks.
fn recipes_md() -> String {
    let mut out = String::new();
    for n in REQUIRED {
        let f = n.replace('-', "_");
        let _ = write!(out, "## {n}\n\n```rust recipe={n}\nfn {f}() {{}}\n```\n\n");
    }
    out
}

/// The four required recipes as marked regions of one file.
fn recipes_rs() -> String {
    let mut out = String::from("//! Recipes.\n");
    for n in REQUIRED {
        let f = n.replace('-', "_");
        let _ = write!(out, "\n// recipe: {n}\nfn {f}() {{}}\n// recipe-end: {n}\n");
    }
    out
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The reference index every `SKILL.md` of a generated root carries.
const REFERENCE_INDEX: &str = "\
| File | Read when |
| --- | --- |
| `references/components-v2.md` | components |
| `references/concept-map.md` | other frameworks |
| `references/limits-cheatsheet.md` | limits |
| `references/modals.md` | modals |
| `references/patterns.md` | patterns |
| `references/troubleshooting.md` | errors |
| `references/views-and-recipes.md` | recipes |
";

fn skill_md(snapshot: &str) -> String {
    format!(
        "# Noctis\n\n> **API hash:** `{}` (blake3)\n\nUse `noctis::rules::Limits`, `truncates`, `button_label_len`, `RuleId` and `discord_len`.\n\n{REFERENCE_INDEX}",
        hash(snapshot.as_bytes())
    )
}

const CATALOG: &str = "docs/discord-rules.md";
const CHEATSHEET: &str =
    "examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md";
const TROUBLESHOOTING: &str =
    "examples/project-template/.claude/skills/noctis/references/troubleshooting.md";
const PATTERNS: &str = "examples/project-template/.claude/skills/noctis/references/patterns.md";
const CONCEPT_MAP: &str =
    "examples/project-template/.claude/skills/noctis/references/concept-map.md";

/// The rule id index of the base root's two catalog rules.
const INDEX_ROWS: &str = "\
| CMD-01 | name charset | `references/commands.md` |
| **REG-06** | permission overwrites | `commands.md` §6 |
";

fn troubleshooting(rows: &str) -> String {
    format!(
        "# Troubleshooting\n\n## Rule id index\n\n| Rule | Meaning | Fix |\n| --- | --- | --- |\n{rows}"
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
    .with(&format!("{SKILL_DIR}/SKILL.md"), &skill_md(SNAPSHOT))
    .with(
        CATALOG,
        "# Rules\n\n| Rule | Text |\n| --- | --- |\n| **CMD-01** Name charset | x |\n| **REG-06** Overwrites | y |\n",
    )
    .with(
        CHEATSHEET,
        "| CMD-01 | name charset |\n| REG-06 | permission overwrites |\n",
    )
    .with(TROUBLESHOOTING, &troubleshooting(INDEX_ROWS))
    .with(
        PATTERNS,
        "# Patterns\n\n## Patterns\n\n## Anti-patterns\n\n## Common mistakes\n",
    )
    .with(CONCEPT_MAP, "# Map\n\n## tescord / dbi → Noctis\n")
    .with(COMPONENTS_MD, "# Components\n")
    .with(MODALS_MD, "# Modals\n")
    .with("examples/project-template/src/lib.rs", HANDLERS)
    .with(
        "examples/project-template/tests/flows.rs",
        "#[test]\nfn flow() { let v: Vec<u8> = it.collect(); }\n",
    )
    .with("examples/project-template/src/notes.txt", "embed")
    .with(RECIPES_MD, &recipes_md())
    .with(RECIPES_RS, &recipes_rs())
    .with(
        "examples/ui-recipes/target/release/gen.rs",
        "fn f() -> EmbedBuilder { x }\n",
    )
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

    /// Removes every file below `dir`.
    fn without_dir(mut self, dir: &str) -> Self {
        let prefix = format!("{dir}/");
        self.files.retain(|path, _| !path.starts_with(&prefix));
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
    assert!(stderr(&out).contains(" component entries checked, "));
    assert!(
        stderr(&out).ends_with(" example files linted, 6 recipes in sync, 135 rule ids indexed)\n"),
        "{}",
        stderr(&out)
    );
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
    let snapshot =
        format!("{SNAPSHOT}pub const noctis::rules::RuleId::CV2_01: noctis::rules::RuleId\n");
    let out = root("m3")
        .snapshot(&snapshot)
        .with(CHEATSHEET, "| CMD-01 | REG-06 |\n| CV2-011 | XCV2-01 |\n")
        .run();
    assert_problems(
        &out,
        &["noctis::rules::RuleId::CV2_01: public item not mentioned in the Usage Skill"],
    );
}

#[test]
fn m8_skill_missing_or_bad() {
    let out = root("m8_missing").without_dir(SKILL_DIR).run();
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
            &format!("Use noctis::rules::Limits, truncates, button_label_len, RuleId and discord_len.\n{REFERENCE_INDEX}"),
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

// C rows: component entries.

const COMPONENTS_MD: &str =
    "examples/project-template/.claude/skills/noctis/references/components-v2.md";
const MODALS_MD: &str = "examples/project-template/.claude/skills/noctis/references/modals.md";

/// A Container entry with every label.
const CONTAINER_ENTRY: &str = "# Components\n\n## 1. Container: `Container`\n\n- **Where:** top level.\n- **Limits:** 40 total ([catalog](../docs/discord-rules.md#9-components-v2-messages)).\n- **Builder:** `Container::new()`.\n\n```rust\n# `Container` in code is not a heading\n```\n\n## 2. Other\n";

/// The base root plus a snapshot exporting `noctis::ui::Container`.
fn with_container(test: &'static str) -> Root {
    let snapshot = format!("{SNAPSHOT}pub mod noctis::ui\npub struct noctis::ui::Container\n");
    root(test).snapshot(&snapshot).with(
        &format!("{SKILL_DIR}/c.md"),
        "Use `noctis::ui` and `Container`.",
    )
}

#[test]
fn c1_no_builders() {
    let out = root("c1").with(COMPONENTS_MD, "# Nothing\n").run();
    assert_exit(&out, 0, &[OK]);
}

#[test]
fn c2_full_entry() {
    let out = with_container("c2")
        .with(COMPONENTS_MD, CONTAINER_ENTRY)
        .run();
    assert_exit(
        &out,
        0,
        &[
            "skill-check: ok (10 public items mentioned, 1 component entries checked, 3 example files linted, 4 recipes in sync, 2 rule ids indexed)",
        ],
    );
}

#[test]
fn c3_missing_entry() {
    let out = with_container("c3").run();
    assert_problems(
        &out,
        &[&format!(
            "{SKILL_DIR}: component-entries: Container has no entry"
        )],
    );
}

#[test]
fn c4_missing_label() {
    let entry = CONTAINER_ENTRY.replace("**Limits:**", "Limits:");
    let out = with_container("c4").with(COMPONENTS_MD, &entry).run();
    assert_problems(
        &out,
        &[&format!(
            "{COMPONENTS_MD}:3: component-entries: entry for Container lacks **Limits:**"
        )],
    );
    // A label under the next heading of the same level does not count.
    let entry = CONTAINER_ENTRY
        .replace("- **Builder:** `Container::new()`.\n", "")
        .replace("## 2. Other\n", "## 2. Other\n**Builder:**\n");
    let out = with_container("c4_scope").with(COMPONENTS_MD, &entry).run();
    assert_problems(
        &out,
        &[&format!(
            "{COMPONENTS_MD}:3: component-entries: entry for Container lacks **Builder:**"
        )],
    );
}

#[test]
fn c5_duplicate_entry() {
    let out = with_container("c5")
        .with(COMPONENTS_MD, CONTAINER_ENTRY)
        .with(MODALS_MD, "# Modals\n### `Container` again\n")
        .run();
    assert_problems(
        &out,
        &[
            &format!("{COMPONENTS_MD}:3: component-entries: Container has 2 entries"),
            &format!("{MODALS_MD}:2: component-entries: Container has 2 entries"),
        ],
    );
}

#[test]
fn c6_two_names_in_one_heading() {
    let entry = CONTAINER_ENTRY.replace("## 2. Other", "## 2. `Label` and `TextInput`");
    let out = with_container("c6").with(COMPONENTS_MD, &entry).run();
    assert_problems(
        &out,
        &[&format!(
            "{COMPONENTS_MD}:13: component-entries: heading names more than one component builder"
        )],
    );
}

#[test]
fn c7_entry_in_modals() {
    let out = with_container("c7").with(MODALS_MD, CONTAINER_ENTRY).run();
    assert_exit(
        &out,
        0,
        &[
            "skill-check: ok (10 public items mentioned, 1 component entries checked, 3 example files linted, 4 recipes in sync, 2 rule ids indexed)",
        ],
    );
}

#[test]
fn q18_limits_without_link() {
    let entry = CONTAINER_ENTRY.replace(
        " ([catalog](../docs/discord-rules.md#9-components-v2-messages))",
        ", see docs/discord-rules.md",
    );
    let out = with_container("q18").with(COMPONENTS_MD, &entry).run();
    assert_problems(
        &out,
        &[&format!(
            "{COMPONENTS_MD}:3: component-entries: entry for Container: **Limits:** does not link docs/discord-rules.md"
        )],
    );
}

// Q rows: completeness.

#[test]
fn q1_missing_reference() {
    let out = root("q1").without(PATTERNS).run();
    assert_problems(
        &out,
        &[
            &format!("{SKILL_DIR}/SKILL.md: lists references/patterns.md, which does not exist"),
            &format!("{PATTERNS}: required reference missing"),
        ],
    );
}

#[test]
fn q5_planned_marker() {
    let out = root("q5")
        .with(
            PATTERNS,
            "# Patterns\n\n## Patterns\n\nTabs [planned].\n\n## Anti-patterns\n\n## Common mistakes\n",
        )
        .run();
    assert_problems(
        &out,
        &[&format!(
            "{PATTERNS}:5: \"planned\" marker; document what exists or say it is not provided"
        )],
    );
}

#[test]
fn q9_index_missing_id() {
    let out = root("q9")
        .with(
            TROUBLESHOOTING,
            &troubleshooting("| CMD-01 | name charset | `references/commands.md` |\n"),
        )
        .run();
    assert_problems(
        &out,
        &[&format!(
            "{TROUBLESHOOTING}: rule id index: REG-06 has no row"
        )],
    );
}

#[test]
fn q17_catalog_missing() {
    let out = root("q17").without(CATALOG).run();
    assert_exit(
        &out,
        1,
        &["skill-check: docs/discord-rules.md: cannot read: "],
    );
    assert_eq!(stderr(&out).lines().count(), 1, "{}", stderr(&out));
}

#[test]
fn q19_listener_in_markdown_example() {
    let out = root("q19")
        .with(
            "docs/guides/events.md",
            "# Events\n\n```rust,ignore\n// app.on_listener(f) is not allowed\napp.on_listener(f);\n```\n",
        )
        .run();
    assert_problems(
        &out,
        &["docs/guides/events.md:5: [UD-7] inline listener; register a #[handler] for a Route"],
    );
}

// D rows: one combined bad template.

const BAD: &str = r#"use twilight::Embed;

#[derive(SlashCommand)] #[command("x")] struct Cmd; impl Cmd { #[handler]
async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> LegacyMessage {
    r.defer_reply().await;
    tokio::spawn(async {});
    Reply { content: "hi" }
} }

fn plain() -> InteractionResponseType {
    let c = ComponentCollector::new();
    ctx.wait_for_component();
    app.on_click(|e| e);
}

mod nested { impl H {
    #[handler]
    async fn h(self, ctx: ComponentCtx) { reply().content("x") }
} }
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
                "{path}:10: [UD-3] manual response-type selection; send the rendered message with ctx.respond/ctx.update and let Noctis pick the response"
            ),
            &format!("{path}:11: [UD-7] collectors are not allowed; one #[handler] per Route"),
            &format!(
                "{path}:12: [UD-7] await-component loop; state goes in Payloads, Refs or ports"
            ),
            &format!("{path}:13: [UD-7] inline listener; register a #[handler] for a Route"),
            &format!("{path}:18: [UD-1] content as handler output; return a View"),
            &format!("{path}:4: [UD-1] LegacyMessage in an interactive handler; return a View"),
            &format!(
                "{path}:4: [UD-3] take a Responder only to open a modal (CommandResponder<Manual>)"
            ),
            &format!(
                "{path}:5: [UD-3] manual response through the Responder; use ctx.respond(msg) or ctx.update(msg)"
            ),
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

// R rows: the recipe sync check and the examples-wide lint.

#[test]
fn r2_drifted_recipe() {
    let out = root("r2")
        .with(
            RECIPES_RS,
            &recipes_rs().replace("fn tabs() {}", "fn tabs() { 1 }"),
        )
        .run();
    assert_problems(
        &out,
        &[&format!(
            "{RECIPES_MD}: recipe `tabs` differs from {RECIPES_RS} (recipe line 1)"
        )],
    );
}

#[test]
fn r6_missing_recipes_dir() {
    let out = root("r6").without(RECIPES_RS).run();
    assert_problems(
        &out,
        &["examples/ui-recipes/src: cannot read: not a directory"],
    );
}

#[test]
fn r7_every_example_is_linted() {
    let path = "examples/ui-recipes/tests/flows.rs";
    let out = root("r7")
        .with(
            path,
            "impl X {\n#[handler]\nasync fn handle(self, ctx: ComponentCtx) { tokio::spawn(f()); }\n}\n",
        )
        .run();
    assert_problems(
        &out,
        &[&format!(
            "{path}:3: [UD-7] task spawned per interaction or event; use a Ref TTL or a port"
        )],
    );
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
        .snapshot(&format!(
            "{SNAPSHOT}pub fn noctis::rules::unmentioned()\npub mod noctis::ui\npub struct noctis::ui::Container\n"
        ))
        .with(
            &format!("{SKILL_DIR}/SKILL.md"),
            &skill_md(SNAPSHOT).replace("API hash:** `", "API hash:** `x"),
        )
        .with(&format!("{SKILL_DIR}/c.md"), "Use `noctis::ui` and `Container`.")
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
            "component-entries: Container has no entry",
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
