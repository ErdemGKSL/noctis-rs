---
title: 'Story 6.1: `#[command]` with compile-time Command Path and Pattern validation'
type: 'feature'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: '0dfc51968c5fb2ca3cd21949d68895b37e68cb70'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Bot authors cannot declare slash Commands yet. Epic 1 built the rule functions (`noctis_rules::command::{expand, validate_name, validate_tree}`), but no `#[command]` macro calls them, so illegal Command Paths and Patterns cannot fail `cargo check`, and a handler cannot learn which pattern combination ran (FR-7, FR-8, FR-75, FR-77, AD-8, AD-26).

**Approach:** Add the `#[command("path or pattern", description = "…", max_expansions = n)]` attribute (`noctis-macros`). At expansion it checks the literal's spacing, expands the pattern with `expand`, and runs `validate_tree` on the expansions. Every violation becomes a `compile_error!` that names its rule id. When the checks pass, it generates a typed `<Fn>Path` enum (only when the pattern has a group) and turns the fn into a descriptor constructor `fn name() -> CommandHandler`, which `Module::add` accepts. `noctis-core` gains `CommandHandler`, a minimal `CommandCtx`, the `CommandPath` trait and `UnknownCommandPath`. `noctis-rules` gains `PathKind` and `validate_pattern_spacing`. App-build merging (6.4), options (6.2), metadata (6.3), dispatch and Responder (6.5) are out of scope.

## Boundaries & Constraints

**Always:**
- The macro calls `noctis-rules` functions directly and never copies a constant (AD-3). It uses `Limits::default()`; `max_expansions = n` sets only `pattern_expansions` on a mutated default (`Limits` is `#[non_exhaustive]`: `let mut l = Limits::default(); l.pattern_expansions = n;`).
- The macro runs these steps in order, and reports **every** violation from steps 3-4 at once as one combined `syn::Error`:
  1. parse the attribute arguments (fail fast);
  2. parse and check the fn signature shape (fail fast);
  3. `validate_pattern_spacing(pattern, &defaults)`, then `expand(pattern, &cap_limits)`;
  4. if `expand` succeeded, `validate_tree(&decls, &defaults)`, with one `CommandDecl::handler("", path)` per expansion carrying `.description(LocalizedText::new(desc))` when a description is given;
  5. derive the variant names (a collision is an error);
  6. check the parameter count and the path parameter.
- Each violation's message is the `Violation`'s `Display` (the `[RULE-ID] path: limit …, actual …` and `= note:`/`= help:` lines), spanned on the pattern literal. CMD-04 violations are spanned on the `description` literal when one is given.
- Help lines the macro adds, only when the `Violation` has none: CMD-11 and CMD-12, when the pattern contains `?`, get ``remove the `?`: "<pattern with every `?` removed>"``. A CMD-04 with the note `missing description` gets ``add `description = "…"` with 1 to 100 characters (doc comments are rustdoc only)``. A CMD-15 over the cap (`Bound::AtMost`) gets ``raise the cap with `max_expansions = <actual>` or split the pattern``.
- The source of every `CommandDecl` is `""`, so the notes carry no `(from …)` suffix.
- Descriptions come only from the literal `description = "…"` (AD-16). Doc comments stay on the generated descriptor fn as rustdoc and never become the description. No locale file is read, with or without the `i18n` feature.
- Generated code names runtime items only through `::noctis::__private::core` (and `::core`/`::std`) (AD-2).
- Subcommand and group nodes come only from the pattern. Neither the descriptor nor the macro has any `required` knob for them (OPT-14, by construction; the existing `OptionType::SubCommand` `compile_fail` doctest stays its evidence).
- Each public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill), re-blesses `xtask/public-api.txt` and updates the Skill's `API hash` line in the same commit.
- Every rule test file and fixture name cites its rule id. The `rules-pending.toml` entries for CMD-01, CMD-02, CMD-03, CMD-04, CMD-05, CMD-10, CMD-11, CMD-12, CMD-13, CMD-15 and CMD-16 are removed.

**Never:**
- No options or `derive(Options)` (6.2); no `ephemeral`, `opens_modal`, `permissions`, `contexts`, `integration_types`, `nsfw`, `guilds`, `rate_limit`, `flags`, `apps` or `locales` arguments (6.3, 13.x, 11.4); no `#[user_command]`/`#[message_command]` (6.3).
- No App-build folding, merging or `validate_merge` of commands; the Registry is unchanged (6.4). No dispatch, interaction parsing, `Responder` or auto-defer (6.5, 6.6).
- `CommandCtx` gets no accessors beyond `command_path()`; 6.5 extends it.
- No change to `expand`'s whitespace-collapsing semantics or to existing rule-function outputs.
- No `unwrap`/`expect`/`panic` in macro or library code paths reachable from user input. No debug builds: every cargo command uses `--release`.
- No trybuild `pass` suites; boundary passes live in `tests/boundary/`.

## I/O & Edge-Case Matrix

The default description in each row is `description = "d"` unless the row says otherwise. "Error" means a compile error whose text contains the quoted parts.

| # | Scenario | Input | Expected | Evidence |
|---|----------|-------|----------|----------|
| 1 | 1-word path | `#[command("ping")]` on `async fn ping(ctx: CommandCtx) -> HandlerResult<()>` | `fn ping() -> CommandHandler`, `paths() == ["ping"]`, `PathKind::of("ping") == Some(Command)`, no enum generated | commands.rs |
| 2 | 2 words | `"setup start"` | `paths() == ["setup start"]`, kind `Subcommand` | commands.rs |
| 3 | 3 words | `"settings logs channel"` | kind `GroupSubcommand`; compiles | boundary/cmd.rs |
| 4 | Pattern AC | `"config (set\|get) (prefix\|welcome)"` | enum `ConfigPath { SetPrefix, SetWelcome, GetPrefix, GetWelcome }` in this order; `ALL` in that order; `as_str(SetPrefix) == "config set prefix"`; `paths()` equals the `expand` output | commands.rs |
| 5 | Handler gets the variant | `__testing::call_command(&config(), "config get welcome")` | the handler receives `ConfigPath::GetWelcome`; returns `Ok(())` | commands.rs |
| 6 | Unknown path at call | `call_command(&config(), "config nope")`, and `call_command(&ping(), "pong")` | `Err(HandlerError)` whose `as_error()` downcasts to `UnknownCommandPath`, Display ``"`config nope` is not a path of the Command `<module>::config`"``; handler not run; no panic | commands.rs |
| 7 | `from_path` | `<ConfigPath as CommandPath>::from_path("config set prefix")`, `""`, `"config  set prefix"`, `"Config set prefix"` | `Some(SetPrefix)`; then `None` for the other three (exact byte match) | commands.rs |
| 8 | Optional group, all absent is legal | `"(a\|b)? c"` | paths `a c`, `b c`, `c`; variants `A`, `B`, `Bare` | commands.rs |
| 9 | CMD-11 AC | `"config (set\|get) (prefix\|welcome)?"` | 2 errors: `[CMD-11] config set: … actual `set``, notes `` `config set` makes `set` a subcommand `` and `` `config set prefix` makes `set` a subcommand group ``, `help: remove the `?`: "config (set\|get) (prefix\|welcome)"`; same for `get` | ui/cmd/CMD-11_optional_group |
| 10 | CMD-12 | `"config (set)?"` | `[CMD-12] config`, help `remove the `?`: "config (set)"` | ui/cmd/CMD-12_optional_single_group |
| 11 | No `?` help without `?` | CMD-11 or CMD-12 for a pattern that has no `?` (unit-test the help helper with a `Violation` and the pattern `"config set"`) | no `remove the `?`` help is added | macro unit |
| 12 | CMD-01 ASCII `'` | `"it's"` | `[CMD-01] it's`, note `` first invalid character `'` (U+0027) ``, help `` use `ʼ` (U+02BC) instead of `'` `` | ui/cmd/CMD-01_ascii_apostrophe |
| 13 | CMD-01 non-space whitespace | `"config\tset"`, `"config\u{a0}set"`, `"a\nb"` | `[CMD-01]` from `validate_pattern_spacing`: actual `` `\t` `` (escaped), note `at byte 6`, help `separate words with exactly one ASCII space` | ui/cmd/CMD-01_tab_between_words + rules unit |
| 14 | CMD-02 33 chars | 33 × `a` | `[CMD-02] aaa…: limit between 1 and 32, actual 33` | ui/cmd/CMD-02_word_33 |
| 15 | CMD-02 boundary | 32 × `a` | compiles | boundary/cmd.rs |
| 16 | CMD-02 empty word | `"config  set"`, `" ping"`, `"ping "` | one `[CMD-02]` per pattern (first empty word), path = pattern, `limit between 1 and 32, actual 0`, note `empty word at byte N` (`7`, `0`, `5`), same help as row 13 | ui/cmd/CMD-02_empty_word + rules unit |
| 17 | CMD-02 unicode length | 32 × `ğ` (scalars 32, UTF-16 32) / 33 × `ğ` | compiles / `[CMD-02] … actual 33` (an emoji word fails CMD-01, so it is not used here) | boundary/cmd.rs + macro unit |
| 18 | CMD-03 | `"Config"` | `[CMD-03] Config`, help `` use `config` `` | ui/cmd/CMD-03_uppercase |
| 19 | CMD-04 missing | no `description`, fn has a `/// doc` comment | `[CMD-04] ping: limit between 1 and 100, actual 0`, note `missing description`, macro help (see Always) | ui/cmd/CMD-04_missing_description |
| 20 | CMD-04 empty | `description = ""` | `[CMD-04] … actual 0`, note `in the default description` | macro unit |
| 21 | CMD-04 101 | 101 × `a` | `[CMD-04] … actual 101`, spanned on the description literal | ui/cmd/CMD-04_description_101 |
| 22 | CMD-04 boundary | 100 × `a`; 50 × `😀` (UTF-16 100) | compile | boundary/cmd.rs |
| 23 | CMD-04 unicode over | 51 × `😀` (UTF-16 102) | `[CMD-04] … actual 102` | macro unit |
| 24 | CMD-04 per expansion | pattern with 4 expansions, no description | one CMD-04 per handler path (4) | macro unit |
| 25 | CMD-05 | `max_expansions = 64`, `"b (<4 alts of 32 chars>) (<16 alts of 32 chars>)"`, 100-char description | `[CMD-05] b: limit at most 8000, actual 8577`, note `longest variant per field` (1 + 4×32 + 64×(32+100) = 8577) | ui/cmd/CMD-05_budget_over_8000 |
| 26 | CMD-10 | `"a b c d"` | `[CMD-10] a b c d: limit at most 3, actual 4` | ui/cmd/CMD-10_four_words |
| 27 | CMD-10 empty | `""` | `[CMD-10]` (between 1 and 3, actual 0) only; spacing check reports nothing for `""` | macro unit |
| 28 | CMD-13 | `max_expansions = 26`, `"b (a1\|…\|a26)"` | `[CMD-13] b: limit at most 25, actual 26` | ui/cmd/CMD-13_26_children |
| 29 | CMD-13 boundary | `"b (a1\|…\|a25)"` (default cap) | compiles; 25 variants `A1`..`A25` | boundary/cmd.rs |
| 30 | CMD-14 | `"(a\|b) (c\|d)"` (two bases) and `"mod (ban\|kick)"` | compile | boundary/cmd.rs |
| 31 | CMD-15 duplicate | `"a (b\|b)"` | `[CMD-15] a (b\|b): limit no duplicate expansions, actual `a b``, note `produced 2 times` | ui/cmd/CMD-15_duplicate_expansion |
| 32 | CMD-15 over default cap | `"b (a1\|…\|a26)"` | `[CMD-15] … limit at most 25, actual 26`, help `raise the cap with `max_expansions = 26` or split the pattern`; no tree errors | ui/cmd/CMD-15_over_cap |
| 33 | CMD-15 custom cap | `max_expansions = 3`, `"a (b\|c) (d\|e)"` | `[CMD-15] … at most 3, actual 4` | ui/cmd/CMD-15_custom_cap |
| 34 | Raised cap compiles | `max_expansions = 26`, `"(a\|b) (c1\|…\|c13)"` | compiles, 26 variants | boundary/cmd.rs |
| 35 | CMD-16 | `"a (b"` | `[CMD-16] a (b: limit unclosed group, actual `(``, note `at byte 2` | ui/cmd/CMD-16_unclosed_group |
| 36 | Several rules at once | `"Config  it's"` | CMD-02 (spacing) then the tree's CMD-03 and CMD-01: all reported in one compile | macro unit |
| 37 | `max_expansions` invalid | `= 0`; `= -1`; `= "3"`; `= N` (a const); `= 3.0` | `` `max_expansions` must be at least 1 ``; otherwise `` `max_expansions` takes an integer literal, such as `max_expansions = 30` `` | macro unit |
| 38 | Argument errors | no args; `#[command(PATTERN)]`; `#[command(description = "d")]` (pattern missing); `b"x"` | `` `#[command]` needs a Command Path or Pattern as its first argument: `#[command("ping", description = "…")]` `` (non-literal: ``…must be a string literal; check runtime-built patterns with `noctis::rules::command::{validate_pattern_spacing, expand, validate_tree}` ``) | macro unit |
| 39 | Duplicate, unknown or planned argument | `description` twice; `foo = 1`; `ephemeral`; `permissions = X`; `description = concat!("a")` | `` duplicate `#[command]` argument `description` ``; `` unknown `#[command]` argument `foo`: expected `description` or `max_expansions` ``; `` `#[command]` argument `ephemeral` is planned and not supported yet ``; `` `description` takes a string literal `` | macro unit |
| 40 | Raw string | `r"ping"`, `description = r#"a "b""#` | accepted | macro unit |
| 41 | Signature shape | not `async`; `const`/`unsafe`/`extern`; generic or `where`; `self` receiver; variadic | same messages as `#[event]` with `#[command]` in place of `#[event]` (`needs an async fn`, `must be a plain async fn`, `cannot be generic`) | macro unit |
| 42 | Parameter count | path without groups: 0 or 2+ params; pattern with a group: 1 or 3+ params | `` `#[command("ping")]` handlers take `(ctx: CommandCtx)` ``; `` `#[command("<pattern>")]` handlers take `(ctx: CommandCtx, path: ConfigPath)` `` | macro unit |
| 43 | Wrong path type | pattern with a group, 2nd param `path: String` | `` the second parameter must be `ConfigPath`, the enum `#[command]` generates for this pattern ``, spanned on the type | macro unit |
| 44 | Wrong return or ctx type | `-> HandlerResult<u8>`, or `ctx: EventCtx` | rustc type error at the generated `__new` call (return-type span) | tests/types/command_handler_returns_value |
| 45 | Variant naming | words `set-prefix`→`SetPrefix`, `a_b`→`AB`, `ğüncelle`→`Ğüncelle`, `straße`→`Straße`, `ßa`→`SSa`, `2fa`→`_2fa`, `self`→`Self_`, `汉字`→`汉字`, `ʼa`→`ʼa` | as listed; the enum carries `#[allow(non_camel_case_types, clippy::enum_variant_names)]` | macro unit |
| 46 | Variant collision | `"x (a-b\|a_b)"` | `` `#[command]` path variants collide: `x a-b` and `x a_b` both name `AB`; rename one alternative `` | macro unit |
| 47 | Enum name | fn `config`→`ConfigPath`, `set_prefix`→`SetPrefixPath`, `r#type`→`TypePath`, `grüße`→`GrüßePath`, `_1`→`_1Path` | as listed; same visibility and `#[cfg]` attributes as the fn | macro unit |
| 48 | Visibility and docs | `pub(crate) async fn` with `/// doc` | descriptor fn keeps the visibility and doc attrs; enum gets the same visibility; every generated item is documented (no `missing_docs` warning) | commands.rs (`#![deny(missing_docs)]` module) |
| 49 | `Module::add` | `Module::new("cfg").add(config()).add(ping())` | compiles; `Debug` lists the command names | commands.rs |
| 50 | `PathKind::of` | `"a"`, `"a b"`, `"a b c"`, `""`, `"a b c d"`, `"a  b"`, `" a"` | `Command`, `Subcommand`, `GroupSubcommand`, then `None` ×4; `words()` 1/2/3 | rules unit |
| 51 | `validate_pattern_spacing` canonical | `""`, `"a"`, `"config (set\|get)? x"`, `"ğ ü"` | `[]` | rules unit |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/command/pattern.rs` -- `expand` (L158), `tokenize`, CMD-16 constants. Add `validate_pattern_spacing` here.
- `crates/noctis-rules/src/command/mod.rs` -- module docs (grammar §, order), re-exports (L96-109), `CommandDecl::handler`/`.description`, `LocalizedText::new`. Add `PathKind` and its re-export, plus the spacing-rule paragraph in the docs.
- `crates/noctis-rules/src/command/tree.rs` -- `validate_tree` (L279). CMD-11/12 notes come from `leaf_with_children` (L147). CMD-04 has the note `missing description` or `in the default description` (L103-144). Read-only.
- `crates/noctis-rules/src/command/name.rs` -- `validate_name` (CMD-02, CMD-01 + `'` help, CMD-03 help) and the `CHARSET` text. Read-only.
- `crates/noctis-rules/src/rule.rs` -- `Violation` (L226, `with_note`, `with_help`, pub `help`), Display (L269), `Bound`/`Actual` Display. Read-only.
- `crates/noctis-rules/src/limits.rs` -- `Limits` (`#[non_exhaustive]`): `pattern_expansions`, `command_name_len`, `command_description_len`, `command_options`, `command_size`, `command_path_words`.
- `crates/noctis-macros/src/event.rs` -- the template to mirror: argument splitting (`parse_args`), `check` (signature), `plain_name`, inner fn `__noctis_event`, `quote_spanned` on the output span, unit-test style (`errors()` table).
- `crates/noctis-macros/src/common.rs` -- `error`, `combine`. `crates/noctis-macros/src/lib.rs` -- the entry points and the crate doc ("Implemented so far").
- `crates/noctis-core/src/handler.rs` -- `EventHandler::__new` (L105), its erased `Arc` closure, `BoxFuture`, `HandlerError`/`HandlerResult`. Mirror these for `CommandHandler`.
- `crates/noctis-core/src/module.rs` -- `sealed::Sealed::register(self, &mut Vec<EventHandler>)`, `Module { handlers, children }`, `Debug`. Extend for commands.
- `crates/noctis-core/src/lib.rs` -- crate docs, `pub use`, the `#[doc(hidden)] pub mod __testing` (L115) where the test seam goes.
- `crates/noctis-core/src/registry.rs` -- read-only; it stays unaware of commands (6.4).
- `crates/noctis/src/lib.rs` -- root `pub use noctis_core::{…}` (L295), `pub use noctis_macros::event` with docs (L325-343), `prelude` (L686-721), `__private` (L725), crate doc module list (L6-34), `rules` docs (L1233+).
- `crates/noctis/tests/ui.rs` -- the featureless trybuild suite (`tests/ui/**/*.rs`, `tests/types/**/*.rs`). Fixtures follow `tests/ui/<area>/<RULE-ID>_<slug>.rs` plus a blessed `.stderr`. Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`.
- `crates/noctis/tests/boundary.rs` + `tests/boundary/{msg,cid,cv2,modal}.rs` -- the boundary-pass layout; add `cmd`.
- `crates/noctis/tests/events.rs` -- style reference for the facade integration test.
- `xtask/rules-pending.toml` -- remove the 11 entries with `story = "6.1"`.
- `docs/discord-rules.md` -- CMD-01 row (L78), CMD-02 row (L79), §3 "Command Pattern expansion" (L440-485, step 2 says "whitespace is collapsed").
- `examples/project-template/.claude/skills/noctis/references/commands.md` -- §1/§2 (planned enum naming, attribute table, rule-function table); `SKILL.md` L21 `API hash`.
- `examples/project-template/src/modules/{setup,dashboard,profile}.rs` -- `#[command]` sites (setup L99, dashboard L75, profile L47). `locales/en-US.yaml` L10-22 holds the `cmd.*.description` values. The template is written against the planned API and is not compiled by the gates.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/command/pattern.rs` -- add `pub fn validate_pattern_spacing(pattern: &str, limits: &Limits) -> Vec<Violation>`:
  - `""` → `[]`.
  - If `pattern.split(' ')` has an empty segment: one CMD-02, path = pattern, `Bound::Between{1, limits.command_name_len}`, `Actual::Count(0)`, note `empty word at byte N` (byte offset of the first empty segment), help `separate words with exactly one ASCII space`.
  - Then, for the first `char::is_whitespace` char other than U+0020: one CMD-01, path = pattern, `Bound::Describe("words separated by single ASCII spaces")`, `Actual::Text(c.to_string())`, note `at byte N`, same help.
  - Doc it: the macro and App-build call it before `expand`, because `expand` collapses whitespace. Add unit tests `cmd_02_*`/`cmd_01_*` for Matrix 13, 16, 51.
- `crates/noctis-rules/src/command/mod.rs` -- add `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum PathKind { Command, Subcommand, GroupSubcommand }` with `pub fn of(path: &str) -> Option<Self>` (split on `' '`; `None` when empty, any word is empty, or more than 3 words) and `pub const fn words(self) -> usize`, with docs and doctests. Re-export `validate_pattern_spacing` and `PathKind`, and update the module docs (spacing rule, `PathKind`). Unit tests for Matrix 50 (`cmd_10_path_kind_*`).
- `crates/noctis-core/src/command.rs` (new; `mod command;` in lib.rs) --
  - `pub trait CommandPath: Copy + Send + Sync + 'static { const PATHS: &'static [&'static str]; fn from_path(path: &str) -> Option<Self>; fn as_str(self) -> &'static str; }`, documented as "implemented by `#[command]` for its path enum".
  - `#[derive(Clone)] pub struct CommandCtx` holding `path: &'static str` (behind an `Arc` inner so 6.5 can grow it), with `pub fn command_path(&self) -> &'static str`, `pub(crate) fn new(path)` and a `Debug` impl.
  - `#[derive(Clone)] pub struct CommandHandler`: `name`, `pattern`, `description: &'static str`, `paths: &'static [&'static str]` and `call: Arc<dyn Fn(CommandCtx) -> BoxFuture<'static, HandlerResult<()>> + Send + Sync>`.
    - `#[doc(hidden)] pub fn __new<P: CommandPath, F, Fut>(name, pattern, description, f)` where `F: Fn(CommandCtx, P) -> Fut + Send + Sync + 'static` and `Fut: Future<Output = HandlerResult<()>> + Send + 'static`. Its paths are `P::PATHS`; the call maps `ctx.command_path()` through `from_path`, and `None` gives `Err(UnknownCommandPath)`.
    - `#[doc(hidden)] pub fn __new_plain<F, Fut>(name, path, description, f)` for `F: Fn(CommandCtx) -> Fut`. Its paths are a one-element static slice (take `&'static [&'static str]` from the macro); a mismatching ctx path gives `Err(UnknownCommandPath)`.
    - Public accessors `name()`, `pattern()`, `description()`, `paths()`, a `pub(crate) fn call(&self, ctx)` and a `Debug` impl (name, pattern, paths).
  - `#[derive(Debug, thiserror::Error)] #[error("`{path}` is not a path of the Command `{command}`")] pub struct UnknownCommandPath { path: Box<str>, command: &'static str }` with getters `path()` and `command()`.
  - Unit tests: call with a known path, an unknown path and a plain-path mismatch.
- `crates/noctis-core/src/module.rs` -- change `Sealed::register` to `fn register(self, module: &mut Module)`. Add `commands: Vec<CommandHandler>` to `Module`, impl `Sealed` + `Descriptor` for `CommandHandler`, add `pub(crate) fn commands()` (with `#[cfg_attr(not(test), expect(dead_code, reason = "Story 6.4 folds commands"))]` if unused), and list the command names in `Debug`. Update the docs: `add` takes `#[event]` handlers and `#[command]` descriptors.
- `crates/noctis-core/src/lib.rs` -- `pub use command::{CommandCtx, CommandHandler, CommandPath, UnknownCommandPath};`. Add a "# Commands" doc paragraph and `__testing::call_command(handler: &CommandHandler, path: &'static str) -> BoxFuture<'static, HandlerResult<()>>`.
- `Cargo.toml` (workspace) -- add `unicode-ident = "1.0"` (1.0.27 already in Cargo.lock, verified on crates.io 2026-10-20). `crates/noctis-macros/Cargo.toml` -- add `unicode-ident.workspace = true`.
- `crates/noctis-macros/src/command.rs` (new) + `lib.rs` `#[proc_macro_attribute] pub fn command` -- implement the Always pipeline and the generated shape in Design Notes, and the naming rules. Unit tests cover Matrix 11, 20, 23, 24, 27, 36-43 and 45-47 (exact message strings, like `event.rs`'s `errors()`). Update the crate doc.
- `crates/noctis/src/lib.rs` -- root: `pub use noctis_core::{CommandCtx, CommandHandler, CommandPath, UnknownCommandPath}` and a documented `pub use noctis_macros::command;` (arguments, the enum naming, a doctest with a pattern and a `match`). Prelude: `command`, `CommandCtx`. Update the crate and prelude doc lists.
- `crates/noctis/tests/commands.rs` (new) -- Matrix 1, 2, 4-8, 48, 49 (`#[tokio::test]` for the calls via `noctis::__private::core::__testing::call_command`; record the received variant in a `static Mutex<Vec<ConfigPath>>`).
- `crates/noctis/tests/boundary/cmd.rs` (new) + `boundary.rs` `#[path] mod cmd;` -- Matrix 3, 15, 17 (32 × `ğ`), 22, 29, 30, 34; test names cite rule ids (`cmd_02_word_32`, …).
- `crates/noctis/tests/ui/cmd/*.rs` + blessed `.stderr` (new) -- CMD-01_ascii_apostrophe, CMD-01_tab_between_words, CMD-02_word_33, CMD-02_empty_word, CMD-03_uppercase, CMD-04_missing_description, CMD-04_description_101, CMD-05_budget_over_8000, CMD-10_four_words, CMD-11_optional_group, CMD-12_optional_single_group, CMD-13_26_children, CMD-15_duplicate_expansion, CMD-15_over_cap, CMD-15_custom_cap, CMD-16_unclosed_group. Each is a minimal `use noctis::prelude::*;` file with `fn main() {}`. Also `tests/types/command_handler_returns_value.rs` (Matrix 44).
- `xtask/rules-pending.toml` -- delete the CMD-01/02/03/04/05/10/11/12/13/15/16 entries.
- `docs/discord-rules.md` -- CMD-01 row: whitespace other than U+0020 between words is CMD-01. CMD-02 row: leading, trailing or doubled spaces are empty words, checked by `validate_pattern_spacing`. §3 step 2: `expand` collapses whitespace, but `#[command]` and App-build first run `validate_pattern_spacing`. §3 example: match the real message shape (`[CMD-11] config set: limit …`) and the macro's help.
- `examples/project-template/.claude/skills/noctis/references/commands.md` --
  - §1: the handler forms that work now (`(ctx: CommandCtx)` / `(ctx: CommandCtx, path: XPath)` returning `HandlerResult<()>`); View returns, options and extractors stay [planned] for 6.2/6.5.
  - The attribute table: `description` and `max_expansions` landed, the rest stay [planned].
  - §2: the enum rules (name, variants, `Bare`, `_`/`Self_`, collision error, `ALL`, `as_str`, `CommandPath`), `CommandHandler` accessors, `PathKind`, `validate_pattern_spacing` (add it to the dynamic-pattern snippet), `UnknownCommandPath`, the real error format with the `?` help.
  - §8: `command_path()`.
  - `SKILL.md`: the `API hash` line.
- `examples/project-template/src/modules/{setup,dashboard,profile}.rs` -- add `description = "…"` equal to `cmd.<path>.description` in `locales/en-US.yaml` to each `#[command]`, and mark the unsupported arguments `// [planned]`, like the Route lines. `examples/project-template/AGENTS.md` -- add one convention bullet on `#[command]`: literal description that equals the en-US file, patterns and the path enum, `validate_pattern_spacing` + `expand` + `validate_tree` for dynamic patterns.
- `xtask/public-api.txt` -- re-bless (`cargo run --release -p xtask -- api-snapshot --bless`).
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the planner already appended the `(decision)` lines; add implementation decisions there if any.

**Acceptance Criteria:**
- Given `#[command("settings logs channel", description = "…")]` on an `async fn`, when it compiles, then the descriptor's single path maps to `PathKind::GroupSubcommand` (1 word `Command`, 2 `Subcommand`), the description is the literal, a doc comment on the fn never becomes it, and the API has no way to set `required` on a subcommand or group.
- Given `#[command("config (set|get) (prefix|welcome)", description = "…")]`, when it compiles and the descriptor is called with `config get welcome`, then the handler receives `ConfigPath::GetWelcome`, `ConfigPath::ALL`/`paths()` follow `expand`'s order, and `max_expansions = n` changes the cap (26 expansions compile with `= 26` and fail with the default).
- Given the trybuild suite, when `cargo test --release -p noctis --test ui` runs, then every fixture under `tests/ui/cmd/` fails with its `[RULE-ID]` in the `.stderr`. The CMD-11 fixture names `set`, both contributing paths and `help: remove the `?``. The boundary tests (32-char word, 100-char description, 25 children) pass.
- Given `cargo run --release -p xtask -- rules-catalog`, when it runs, then it passes with the 11 pending entries removed.
- Given the new public API, when the story is done, then `references/commands.md` documents `#[command]`, `description = "…"`, patterns and the variant enum, the template's commands carry literal descriptions, and `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

Generated code for `#[command("config (set|get) (prefix|welcome)", description = "Configure")] pub async fn config(ctx: CommandCtx, path: ConfigPath) -> HandlerResult<()> { … }`:

```rust
/// The Command Paths of [`config`]: `config (set|get) (prefix|welcome)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types, clippy::enum_variant_names)]
pub enum ConfigPath { /** `/config set prefix` */ SetPrefix, /* … */ }
impl ConfigPath {
    /// Every path, in expansion order.
    pub const ALL: [Self; 4] = [Self::SetPrefix, /* … */];
    /// The Command Path, words joined by one space.
    #[must_use] pub const fn as_str(self) -> &'static str { match self { Self::SetPrefix => "config set prefix", /* … */ } }
}
impl ::noctis::__private::core::CommandPath for ConfigPath { const PATHS: … = &[…]; fn from_path(p: &str) -> Option<Self> { match p { "config set prefix" => Some(Self::SetPrefix), …, _ => None } } fn as_str(self) -> &'static str { Self::as_str(self) } }
/// (the fn's own attrs/docs)
pub fn config() -> ::noctis::__private::core::CommandHandler {
    async fn __noctis_command(ctx: CommandCtx, path: ConfigPath) -> HandlerResult<()> { … }
    ::noctis::__private::core::CommandHandler::__new::<ConfigPath, _, _>(
        ::core::concat!(::core::module_path!(), "::", "config"), "config (set|get) (prefix|welcome)", "Configure", __noctis_command)
}
```

A pattern without any group generates no enum and calls `__new_plain(name, &["ping"], "d", __noctis_command)`. The enum is generated iff the pattern contains `(`; after `expand` succeeds, `(` only occurs in groups.

**Naming (decision).** The enum name is the fn name (with `r#` stripped) split on `_`: each non-empty segment gets its first char uppercased (`char::to_uppercase`, the full mapping), the segments are concatenated, and `Path` is appended.

A variant name joins, in order, the chosen alternatives of the expansion, skipping absent optional groups. Each word is split on `-` and `_`, and each segment is converted the same way. Chars that are not `unicode_ident::is_xid_continue` are then removed; an empty result becomes `Bare`. A result whose first char is not `is_xid_start` gets a `_` prefix, and `Self` becomes `Self_`. The same rules apply to the enum name. Two equal variant names are a compile error (Matrix 46): there is no positional fallback, so names stay predictable.

The skill already promised `<Fn>Path` and concatenated UpperCamelCase alternatives, and Unicode is kept for Turkish-speaking authors. The derives and allows keep `-D warnings` user builds clean. `uncommon_codepoints` warnings from exotic scripts are accepted (rustc only warns).

**Empty-word check (decision).** `expand` collapses whitespace (existing, tested), so `"config  set"` would silently compile, contradicting the CMD-02 row and FR-7. Changing `expand` would break its contract. A separate `validate_pattern_spacing` keeps `expand` intact and gives App-build (6.4) the same check.

**Minimal `CommandCtx` (decision).** Story 6.5 owns the pipeline. 6.1 needs only a ctx type that stays source-compatible: a handler signature `(ctx: CommandCtx, …) -> HandlerResult<()>` written now keeps compiling when 6.5 adds accessors and relaxes the return type. The erased call resolves the variant from `ctx.command_path()`, so 6.5 dispatch only builds the ctx with the Registry's static path.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff after formatting
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, including `noctis --test ui` (the cmd fixtures), `boundary` and `commands`
- `cargo hack test --release --workspace --each-feature` -- expected: all pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok, pending list shrunk
- `cargo run --release -p xtask -- api-snapshot` -- expected: ok after `--bless`, hash copied into SKILL.md
- `cargo run --release -p xtask -- skill-check` -- expected: ok (every new public item mentioned)
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok (MSRV)
