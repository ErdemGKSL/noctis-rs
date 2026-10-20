---
title: 'Story 6.2: Typed options with `derive(Options)` and `derive(Choices)`'
type: 'feature'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: '7b308e1b9f4f117418268afd237dc30fd2c98baa'
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

**Problem:** A `#[command]` handler cannot take options yet. Story 1.5 built the option rule functions (`noctis_rules::command::{OptionDecl, validate_options, measure_options}`), but no derive calls them. Illegal option declarations therefore do not fail `cargo check`, and no typed, panic-free decode of an interaction's options exists (FR-10, FR-77; AD-3, AD-4, AD-16, AD-26; OPT-01..11, OPT-13, CMD-04, CMD-06).

**Approach:**
- `#[derive(Options)]` on a named struct lowers each field's `#[option(..)]` literals into `OptionDecl`s and checks them with `validate_options`.
  - Rules that do not depend on the field's Rust type become `compile_error!`s at expansion.
  - Rules that do depend on it become free `const _` items. Each one panics with the precomputed `Violation` text for the field's actual `CommandOption::KIND` (AD-4 allows "free `const _` items over concrete types").
- Core gains:
  - the `CommandOption` trait (field type → OPT-02 kind; `[OPT-02]` `on_unimplemented`) and the `Options` trait;
  - `CommandInput`: Noctis's own walk of the raw APPLICATION_COMMAND JSON;
  - the typed errors `OptionError` and `InvalidOption` (validator failure carrying a `MessageKey`);
  - options-aware `CommandHandler` constructors.
- `#[command]` accepts a trailing options parameter.
- `derive(Choices)` also implements `CommandOption` (STRING choices).
- The ephemeral notice for `InvalidOption` is Story 6.8's job (FR-10 "validator notice" is listed there); 6.2 delivers the typed error and skips the handler.

## Boundaries & Constraints

**Always:**
- Macros call `noctis-rules` functions with `Limits::default()` and never copy a constant (AD-3). The generated code names runtime items only through `::noctis::__private::{core, ui, rules}` and `::core`/`::std` (AD-2); add `pub use noctis_rules as rules;` to `noctis::__private`.
- Descriptions come only from `#[option(description = "…")]` (AD-16). Doc comments stay rustdoc. No locale file is read.
- An option's name is `rename = "…"` if given, else the field ident with `r#` stripped. Declaration order is kept: the derive never reorders (OPT-01). A field is optional iff its type's last path segment is `Option` with one generic argument; the inner type is then the option type.
- `derive(Options)` reports every expansion-time violation at once, as one combined `syn::Error`. On any expansion error it emits the errors plus the type-dependent `const _` checks, and no `impl`.
- Runtime decoding never panics, never `unwrap`s/`expect`s input, and returns `OptionError`. A decode or validator error means the handler is not run.
- Each public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill), re-blesses `xtask/public-api.txt` and updates the Skill's `API hash`, in the same commit. Fixture and test names cite their rule id.

**Never:**
- No autocomplete handling, `Partial<T>` or `#[autocomplete]` macro (6.7): `autocomplete = path` only sets the flag, runs OPT-10/OPT-11 and checks that the path exists.
- No metadata or context menus (6.3), no App-build merge or option-inclusive CMD-05/CMD-13 of a whole Command (6.4), no dispatch/Responder/notice sending (6.5, 6.8), no Publish JSON (6.9), no locale source (11.4).
- No integer or number choice values: `#[choice(.., value = <non-string>)]` stays a parse error. No integer types other than `i64` and no number types other than `f64`.
- No runtime re-check of declared `min_value`/`max_value`/`min_length`/`max_length`/`channel_types` (Discord enforces them; validators cover custom checks).
- No change to `validate_options`' outputs, no generic `Options` structs, no trybuild `pass` suites, no debug builds (`--release` everywhere).

## I/O & Edge-Case Matrix

"Error" means a compile error whose text contains the quoted parts. In compile-time rows, `T` is the field (inner) type and `d` = `description = "d"`. Messages are `Violation` `Display`s with the field's option name as path `P`; the macro adds nothing to them unless the row says so. "const error" = `E0080 evaluation panicked: <text>` from the generated `const _`, spanned on the field's `#[option]` attribute (on the field ident when there is none).

### `derive(Options)` — shape and attributes

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 1 | Happy | `struct W { member: ResolvedUser, days: Option<i64>, reason: Option<String> }`, each `#[option(d)]` | compiles; `W::OPTIONS` = 3 decls in order: `member` USER required, `days` INTEGER optional, `reason` STRING optional; empty `choices` |
| 2 | Empty struct | `struct E {}` | compiles; `OPTIONS` empty; `from_input` always `Ok` |
| 3 | Not a named struct | tuple struct, unit struct, enum, union | `` `derive(Options)` takes a struct with named fields `` |
| 4 | Generic | `struct O<T>`, lifetime, `where` | `an Options struct cannot be generic` |
| 5 | Unknown / duplicate arg | `#[option(foo = 1)]`; `description` twice; two `#[option]` attrs on one field | `` unknown `#[option]` argument `foo`: expected `description`, `rename`, `min_value`, `max_value`, `min_length`, `max_length`, `channel_types`, `autocomplete`, `file_types` or `validate` ``; `` duplicate `#[option]` argument `description` ``; `a field takes one #[option(..)] attribute` |
| 6 | `min`/`max` spelled tescord-style | `#[option(min = 1)]` | `` unknown `#[option]` argument `min`: use `min_value` `` (same for `max` → `max_value`) |
| 7 | Literal kinds | `description = concat!("a")`; `rename = 1`; `min_length = -1`; `min_length = 1u8`; `min_value = "1"`; `min_value = 1i64`; `file_types = "x"`; `channel_types = GuildText` | `` `description` takes a string literal `` (also `rename`); `` `min_length` takes an unsuffixed integer literal `` (also `max_length`); `` `min_value` takes an unsuffixed integer or float literal, optionally negated `` (also `max_value`); `` `file_types` takes an array of string literals ``; `` `channel_types` lists ChannelType variants such as `[GuildText]` `` |
| 8 | Integer literal outside `i64` | `min_value = 9223372036854775808` | `` `min_value` is outside the i64 range `` |
| 9 | Raw idents and rename | field `r#type`; `rename = "silent"` | names `type`, `silent` |

### `derive(Options)` — rules found at expansion (type-independent)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 10 | CMD-04 missing | field without a description (or without `#[option]`) | `[CMD-04] P: limit between 1 and 100, actual 0`, note `in the default description`, plus `` help: add `#[option(description = "…")]` with 1 to 100 characters (doc comments are rustdoc only) ``; spanned on the field ident |
| 11 | CMD-04 empty / 101 / 100 / UTF-16 | `""`; 101 × `a`; 100 × `a`; 50 × `😀`; 51 × `😀` | `actual 0` (no extra help); `actual 101`; compiles; compiles; `actual 102`; spanned on the literal |
| 12 | Names | `rename = "Days"`; `"it's"`; 33 × `a`; 32 × `a`; `""` | `[CMD-03]`; `[CMD-01]`; `[CMD-02] … actual 33`; compiles; `[CMD-02] options[i]: … actual 0` (the macro rewrites the single-decl `options[0]` path prefix to the field index `i`) |
| 13 | OPT-01 | `a: Option<i64>, b: String` | `[OPT-01] b: limit required options before optional ones, actual b`, note ``after optional `a` ``; spanned on `b` |
| 14 | OPT-01 several | `a: Option<i64>, b: i64, c: i64` | one OPT-01 each for `b` and `c` |
| 15 | CMD-06 | `a: i64` and `#[option(rename = "a")] b: i64` | `[CMD-06] a: limit unique option names, actual a`, note `declared at positions 0, 1`; spanned on the second field |
| 16 | CMD-13 | 26 fields; 25 fields | `[CMD-13] options: limit at most 25, actual 26` on the struct ident; 25 compiles |
| 17 | OPT-05 order | `i64`, `min_value = 10, max_value = 5` | `[OPT-05] P: limit min_value ≤ max_value, actual `10 > 5`` (as const error: order needs a numeric kind) |
| 18 | OPT-06 range | `String`, `max_length = 6001`; `min_length = 6001`; `max_length = 0`; `min_length = 0, max_length = 6000` | const errors `[OPT-06] P.max_length: limit between 1 and 6000, actual 6001` / `P.min_length … between 0 and 6000, actual 6001` / `… actual 0`; last compiles |
| 19 | OPT-13 entries/count | `ResolvedAttachment`, `file_types = ["pdf"]`; `[".tar.gz"]`; 11 entries; `["image", ".PNG", ".7z"]` | `[OPT-13] P.file_types[0]: …, actual pdf`; same for `.tar.gz`; `[OPT-13] P.file_types: limit at most 10, actual 11`; compiles |
| 20 | Multiple at once | `Days` name + missing description + OPT-01 | all three in one compile |

Rows 17-19 are listed here because they are literal problems. Whether one shows as a `compile_error!` or a const error follows the partition rule in Design Notes. The expected text is the same either way.

### Type-dependent rules (const errors) and OPT-02

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 21 | OPT-02 | field `u8`, `Vec<String>`, `Option<Option<i64>>`, `&'static str` | E0277 `` [OPT-02] `u8` is not a command option type ``, note listing the supported types |
| 22 | OPT-05 misuse | `String`, `min_value = 1` | `[OPT-05] P.min_value: limit min_value / max_value only on INTEGER or NUMBER options, actual STRING` |
| 23 | OPT-05 float on INTEGER | `i64`, `max_value = 1.5` | `[OPT-05] P.max_value: limit an integer bound on an INTEGER option, actual 1.5` |
| 24 | OPT-03 | `i64`, `min_value = -9007199254740992`; `= -9007199254740991` | `[OPT-03] P.min_value …`; compiles |
| 25 | OPT-04 | `f64`, `max_value = 1e300`; `= 9007199254740992` (2^53, an int literal on NUMBER); `= -9007199254740992.0` | `[OPT-04]`; compiles; compiles |
| 26 | OPT-06 misuse | `i64`, `min_length = 1` | `[OPT-06] P.min_length: limit min_length / max_length only on STRING options, actual INTEGER` |
| 27 | OPT-07 | `String`, `channel_types = [GuildText]`; `Id<ChannelMarker>` / `ResolvedChannel` with `[GuildText, ChannelType::GuildVoice]` | `[OPT-07] P.channel_types: limit channel_types only on CHANNEL options, actual STRING`; compiles, decl `channel_types == [0, 2]` |
| 28 | Unknown channel type | `[GuildTxt]`; `[]` | `` unknown channel type `GuildTxt`; expected one of GuildText, Private, GuildVoice, Group, GuildCategory, GuildAnnouncement, AnnouncementThread, PublicThread, PrivateThread, GuildStageVoice, GuildDirectory, GuildForum, GuildMedia `` (expansion); `` `channel_types` lists at least one channel type `` |
| 29 | OPT-11 | `bool`, `autocomplete = suggest` | `[OPT-11] P.autocomplete: limit autocomplete only on STRING, INTEGER or NUMBER options, actual BOOLEAN` |
| 30 | Autocomplete OK | `String` / `i64` / `f64`, `autocomplete = suggest` (an existing fn) | compiles; decl `autocomplete == true` |
| 31 | Autocomplete path missing | `autocomplete = nope` | rustc E0425 (the path is referenced in a generated `const _`) |
| 32 | OPT-10 | `Severity` (Choices) with `autocomplete = suggest` | `[OPT-10] P: limit choices or autocomplete, not both, actual P` |
| 33 | OPT-08 | field of a 26-variant Choices enum; 25 variants | `[OPT-08] P.choices: limit at most 25, actual more than 25`, note `` `Big` has more variants than a command option takes ``; 25 compiles |
| 34 | OPT-13 misuse | `String`, `file_types = ["image"]` | `[OPT-13] P.file_types: limit file_types only on ATTACHMENT options, actual STRING` |
| 35 | min/max on Choices | `Severity`, `min_value = 1` | OPT-05 misuse, actual `STRING` |
| 36 | Several type-dependent problems on one field | `bool`, `min_value = 1, min_length = 2` | each violation in its own `const _` item, so all are reported |

### `derive(Choices)` (shared with modals)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 37 | Implements CommandOption | `#[choice("Low")] Low, #[choice("High", value = "hi")] High` | `KIND == String`; `CHOICES == [("Low","Low"), ("High","hi")]`; `noctis::ui::Choices` impl unchanged |
| 38 | OPT-09 name | label 101 × `a`; `""`; 100 × `a`; 50 × `😀` | `[OPT-09] A: limit between 1 and 100, actual 101` + note `in the choice name` + note `the same limit applies to modal and select options (MOD-07, CV2-09)`; `… actual 0`; compiles; compiles. Spanned on the label. No `[MOD-07]` label error any more |
| 39 | OPT-09 value | `value = <101 × a>`; ident-derived value of 101 chars | `[OPT-09] A: limit at most 100, actual 101` + note `in the choice value` + the shared-limit note; spanned on the literal / ident |
| 40 | Empty value | `value = ""` | unchanged: `[MOD-07] value: limit between 1 and 100, actual 0` |
| 41 | 26 variants | 26 `#[choice]` variants | derive compiles (CV2-09 fixture keeps its E0277); used as an option → row 33 |
| 42 | Existing errors | no variants, tuple variant, generic, duplicate value, malformed `#[choice]`, description 101 | unchanged messages (`[MOD-07]` for description) |

### Runtime: `CommandInput::from_json` and decode

JSON is the interaction `d` object. "Err X" means `Err(OptionError::X{..})`. Its `Display` starts with the rule id when the row names one.

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 43 | Top-level command | `{"type":2,"data":{"name":"warn","type":1,"options":[{"name":"days","type":4,"value":3}]}}` | `path() == "warn"`, `get("days") == Some(&OptionValue::Integer(3))` |
| 44 | Subcommand | `data.name "mod"`, options `[{"type":1,"name":"warn","options":[…]}]` | path `mod warn`; leaf options are the subcommand's |
| 45 | Group | `[{"type":2,"name":"logs","options":[{"type":1,"name":"channel","options":[]}]}]` under `settings` | path `settings logs channel`; no options |
| 46 | No options / `type` absent | `data` without `options`; `data.type` absent | path = name, no options (absent `data.type` means 1) |
| 47 | Not a slash command | `"type":3`; `"type":4`; `data.type: 2` | Err `NotSlashCommand { interaction_type: 3, command_type: 1 }` / `{4, 1}` / `{2, 2}`. Display ``not a slash command: interaction type 3, command type 1`` |
| 48 | Malformed | bad JSON; missing `data`/`name`; `options` not an array; option without `name`/`type`; unknown type `12`; type 1 next to a value option; group with two children or a non-subcommand child; snowflake `"abc"`/`"0"`; STRING value `5`; BOOLEAN `"true"` | Err `Malformed { reason }`, Display `malformed command interaction: <reason>`; no panic |
| 49 | Duplicate option | `days` twice at the leaf | Err `Duplicate { option: "days" }` |
| 50 | Integer outside i64 | INTEGER `9223372036854775808`; `-9223372036854775809`; `1.5`; `1e3` | Err `OutOfRange { option, value }`, Display `` [OPT-03] option `days`: 9223372036854775808 is not an i64 `` (value = the JSON number's text) |
| 51 | i64 extremes | `9223372036854775807`, `-9223372036854775808`, `9007199254740993` | decoded as given (only the i64 fit is checked) |
| 52 | NUMBER | `2`, `2.5`, `-0.0`, `1e308` | `Number(2.0)`, `2.5`, `-0.0`, `1e308` |
| 53 | Snowflakes | USER `"80351110224678912"`; a numeric JSON snowflake `80351110224678912` | `User(Id)`; Err `Malformed` (Discord sends strings) |
| 54 | `focused` | option with `"focused": true` | accepted, ignored (6.7) |
| 55 | Resolved | `data.resolved` present / absent / malformed | parsed with twilight `InteractionDataResolved` / empty maps / Err `Malformed` |
| 56 | Unicode | STRING `"ğüşçö 😀"`, name `"gün"` | kept byte-exact |

### Runtime: `Options::from_input`, validators and the handler call (through `__testing::call_command_json`)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 57 | Parsed struct | row 1 struct, JSON with member (+resolved user and member), days 3 | handler receives `W { member: ResolvedUser{..}, days: Some(3), reason: None }` |
| 58 | Missing required | no `member` | Err (downcast `OptionError::Missing { option: "member" }`, Display ``required option `member` is missing``); handler not run |
| 59 | Optional absent | no `days` | `days: None` |
| 60 | Wrong type | `days` sent as STRING `"3"` | Err `WrongType { option: "days", expected: Integer, found: String }`, Display `` [OPT-02] option `days`: expected INTEGER, found STRING `` |
| 61 | NUMBER vs INTEGER | `f64` field gets type 4 `2` | Err `WrongType` (expected NUMBER, found INTEGER) |
| 62 | Unresolved | `ResolvedUser` field, id not in `resolved.users` (or no `resolved`); `ResolvedRole`/`ResolvedChannel`/`ResolvedAttachment` likewise | Err `Unresolved { option, id }`, Display ``option `member`: id 42 is not in the resolved data`` |
| 63 | Plain ids | `Id<UserMarker>`, `Id<ChannelMarker>`, `Id<RoleMarker>`, `Id<GenericMarker>` (MENTIONABLE), `Id<AttachmentMarker>` without `resolved` | decoded from the id alone |
| 64 | Mentionable | id in `users` (+member) / only in `roles` / in neither | `Mentionable::User` (user wins) / `Mentionable::Role` / Err `Unresolved` |
| 65 | Choices | `"low"`; `"zzz"`; INTEGER `1` for a Choices field | `Severity::Low`; Err `UnknownChoice { option, value: "zzz" }`; Err `WrongType` |
| 66 | Extra option | input has `foo` not in the struct | ignored |
| 67 | Validator passes / fails | `#[option(d, validate = positive)]` with `fn positive(v: &i64, _: &CommandCtx) -> Result<(), MessageKey>`; value 5 / -1 | handler runs / Err downcast `InvalidOption`, `option() == "days"`, `key() == MessageKey::new("warn.days.positive")`, Display ``option `days` failed validation: warn.days.positive``; handler not run |
| 68 | Validator on absent optional | `days` absent | validator not called; handler runs |
| 69 | Validator order | two failing validators | the first field's error (declaration order) |
| 70 | Validator wrong signature | `fn(i64) -> bool` | rustc E0308 at the generated coercion, spanned on the path |
| 71 | Decode before validate | missing required + a failing validator | `Missing` (validators never see a partial struct) |
| 72 | Path seam | JSON path not in `handler.paths()` | Err `UnknownCommandPath` (no decode) |
| 73 | Seam with bad JSON | `b"{"` | Err downcast `OptionError::Malformed` |
| 74 | No-options handler | `call_command_json` on `(ctx)` handler with options in JSON | handler runs; options ignored |

### `#[command]` with options

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 75 | Plain + options | `async fn warn(ctx: CommandCtx, opts: W)` | `__new_plain_with_options`; `warn().options() == W::OPTIONS` |
| 76 | Pattern + options | `(ctx, path: ModPath, opts: W)` | `__new_with_options::<ModPath, W, _, _>`; handler gets the variant and the struct |
| 77 | No options | 6.1 forms | unchanged; `options()` empty |
| 78 | Param count | plain: 0 or ≥ 3 params; pattern: 1 or ≥ 4 | `` `#[command("ping")]` handlers take `(ctx: CommandCtx)` or `(ctx: CommandCtx, options: O)` with `O: Options` `` / `` `#[command("mod (ban|kick)")]` handlers take `(ctx: CommandCtx, path: ModPath)` or `(ctx: CommandCtx, path: ModPath, options: O)` with `O: Options` `` |
| 79 | Options type without the derive | `opts: String` | E0277 `` `String` is not an options struct ``, note ``derive it with `#[derive(Options)]` `` |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/command/option.rs` -- `OptionType` (L33, `ALL`, `name()`), `NumericValue`, `ChoiceValue`, `Choice::new` (const), `OptionDecl` + const setters (L245-390), `validate_options` (L425; order and messages L440-830), `is_file_type`. Read-only except for adding the channel-type table.
- `crates/noctis-rules/src/command/mod.rs` -- re-exports; module docs (add the channel-type table).
- `crates/noctis-rules/src/key.rs` -- `MessageKey` (`Copy`, `const fn new`).
- `crates/noctis-rules/src/limits.rs` -- `option_choices` (25), `command_options`, `choice_name_len`, `choice_value_len`.
- `crates/noctis-macros/src/choices.rs` -- the existing `derive(Choices)` (MOD-07 texts, `Count = Arity<N>`, `from_value`). Extend it here.
- `crates/noctis-macros/src/command.rs` -- `check_params` (L395, messages L400-404), `expand` (L500; `__new`/`__new_plain` emission L550-560), unit tests L834-872 (parameter messages change).
- `crates/noctis-macros/src/modal.rs` L470-495 -- `channel_types = [..]` parsing to copy (last path segment), `int_arg`/`array` helpers.
- `crates/noctis-macros/src/common.rs` -- `error`, `combine`, `option`. `crates/noctis-macros/src/lib.rs` -- entry points (L95 `Choices`); add `#[proc_macro_derive(Options, attributes(option))]`.
- `crates/noctis-core/src/command.rs` -- `CommandCtx` (Arc inner), `CommandHandler` (`__new` L128, `__new_plain` L156, `call` L211, `unknown` L217), `call_command` seam (L235). `crates/noctis-core/src/lib.rs` L132 `__testing`.
- `crates/noctis-core/src/handler.rs` -- `HandlerError::new`, `as_error` (downcast in tests).
- `crates/noctis-core/Cargo.toml` -- add `noctis-rules.workspace = true`, `noctis-ui.workspace = true` (edges allowed: `xtask/src/deps_check.rs` L70, L73).
- `crates/noctis-ui/src/modal/resolved.rs` -- `ResolvedUser`/`Role`/`Channel`/`Attachment` (`#[non_exhaustive]`, so core cannot build them), `Mentionable`. `crates/noctis-ui/src/modal/form.rs` L119 `Choices` trait; L320-420 resolve helpers to refactor onto the new constructors. `crates/noctis-ui/src/modal/submit.rs` -- `SubmitError` style and the serde_json walk to mirror.
- twilight-model 0.17.1 -- `InteractionDataResolved` (no `Default`: build empty maps), `ChannelType` names/values (0-5, 10-16).
- `crates/noctis/src/lib.rs` -- root `pub use noctis_core::{…}` (L297), `command` macro docs (L381), prelude (L743-780), `__private` (L784), `rules` (L1292).
- `crates/noctis/tests/{commands.rs, boundary.rs, boundary/cmd.rs, ui.rs}` -- 6.1 style references; `tests/ui/cv2/CV2-09_derive_string_select_26_choices.rs` must keep passing as is.
- `xtask/rules-pending.toml` -- 13 entries with `story = "6.2"` (CMD-06, OPT-01..11, OPT-13).
- `docs/discord-rules.md` -- rows CMD-04 (L81), CMD-06 (L83), OPT-01..13 (L102-114), MOD-07 (L239).
- `examples/project-template/.claude/skills/noctis/references/commands.md` §3 (options table, rule list, "Choices" subsection), `references/modals.md` §1.2, `SKILL.md` L21 (API hash); `examples/project-template/src/modules/profile.rs` L42-53 (`ProfileOptions`); `locales/en-US.yaml` (`cmd.profile.opt.user.description: Whose profile to show (default you)`); `examples/project-template/AGENTS.md`.

## Tasks & Acceptance

**Execution:**

**`crates/noctis-rules/src/command/option.rs` (+ re-export in `mod.rs`)**
- Add `pub const CHANNEL_TYPES: [(&str, u8); 13]` (twilight 0.17.1 names → values: GuildText 0, Private 1, GuildVoice 2, Group 3, GuildCategory 4, GuildAnnouncement 5, AnnouncementThread 10, PublicThread 11, PrivateThread 12, GuildStageVoice 13, GuildDirectory 14, GuildForum 15, GuildMedia 16) and `pub fn channel_type(name: &str) -> Option<u8>`.
- Docs plus a unit test `opt_07_channel_type_names` that checks every pair round-trips through `twilight_model::channel::ChannelType::from(u8)` (dev-dep only if already present; otherwise assert the table literally).

**`crates/noctis-ui/src/modal/resolved.rs`**
- Add public constructors:
  - `ResolvedUser::from_resolved(&InteractionDataResolved, Id<UserMarker>) -> Option<Self>` (member from `members`);
  - `ResolvedRole::from_resolved(.., Id<RoleMarker>)`;
  - `ResolvedChannel::from_resolved(.., Id<ChannelMarker>)`;
  - `ResolvedAttachment::from_resolved(.., Id<AttachmentMarker>)`;
  - `Mentionable::from_resolved(.., Id<GenericMarker>)` (user first, then role).
- Each gets docs with a doctest. Refactor `form.rs`'s `user`/`role`/channel/attachment helpers onto them (behaviour unchanged).

**`crates/noctis-core/src/options.rs` (new; `mod options;`)**
- `#[diagnostic::on_unimplemented(message = "[OPT-02] `{Self}` is not a command option type", label = "not a command option type", note = "option fields are String, i64, f64, bool, Id<UserMarker>, Id<ChannelMarker>, Id<RoleMarker>, Id<GenericMarker>, Id<AttachmentMarker>, ResolvedUser, ResolvedChannel, ResolvedRole, Mentionable, ResolvedAttachment or a #[derive(Choices)] enum; wrap optional ones in Option<T>")]` on `pub trait CommandOption: Sized + Send + Sync + 'static`. Its items:
  - `const KIND: OptionType`;
  - `const CHOICES: &'static [Choice<'static>]`;
  - `fn decode(value: &OptionValue, input: &CommandInput, option: &str) -> Result<Self, OptionError>`.
- Implement it for the 14 types in the note:
  - String → STRING, i64 → INTEGER, f64 → NUMBER, bool → BOOLEAN;
  - USER, CHANNEL, ROLE, MENTIONABLE and ATTACHMENT for the Id and Resolved types;
  - `decode` returns `WrongType` for a mismatched value and `Unresolved` for a missing entity.
- `#[diagnostic::on_unimplemented(message = "`{Self}` is not an options struct", note = "derive it with `#[derive(Options)]`")] pub trait Options: Sized + Send + 'static`. Its items:
  - `const OPTIONS: &'static [OptionDecl<'static>]`;
  - `fn from_input(input: &CommandInput) -> Result<Self, OptionError>`;
  - `fn validate(&self, ctx: &CommandCtx) -> Result<(), InvalidOption> { Ok(()) }`.
- `#[non_exhaustive] #[derive(Clone, Debug, PartialEq)] pub enum OptionValue { String(String), Integer(i64), Number(f64), Boolean(bool), User(Id<UserMarker>), Channel(Id<ChannelMarker>), Role(Id<RoleMarker>), Mentionable(Id<GenericMarker>), Attachment(Id<AttachmentMarker>) }` with `pub fn kind(&self) -> OptionType`.
- `#[derive(Clone, Debug)] pub struct CommandInput` (path `String`, leaf options `Vec<(String, OptionValue)>`, `InteractionDataResolved`):
  - `pub fn from_json(raw: &[u8]) -> Result<Self, OptionError>` (Matrix 43-56; walk `serde_json::Value`; `resolved` through `serde_json::from_value`);
  - `pub fn path(&self) -> &str`, `pub fn get(&self, name: &str) -> Option<&OptionValue>`, `pub fn resolved(&self) -> &InteractionDataResolved`;
  - `pub(crate) fn empty(path)`.
- `#[derive(Clone, Debug, PartialEq, thiserror::Error)] #[non_exhaustive] pub enum OptionError`. Variants and the exact Displays from the Matrix:
  - `Malformed { reason: String }`, `NotSlashCommand { interaction_type: u64, command_type: u64 }`, `Duplicate { option: String }`;
  - `Missing { option: String }`, `WrongType { option: String, expected: OptionType, found: OptionType }` (uses `OptionType::name()`), `OutOfRange { option: String, value: String }`;
  - `Unresolved { option: String, id: u64 }`, `UnknownChoice { option: String, value: String }`.
- `#[derive(Debug, Clone, thiserror::Error)] #[error("option `{option}` failed validation: {key}")] pub struct InvalidOption { option: &'static str, key: MessageKey }` with `option()`, `key()` and `#[doc(hidden)] pub fn __new`.
- `#[doc(hidden)] pub mod __options`, the helpers the derives call:
  - `required::<T: CommandOption>(input, name: &'static str) -> Result<T, OptionError>` (`Missing`, then a kind check → `WrongType`, then `T::decode`);
  - `optional::<T>(…) -> Result<Option<T>, OptionError>`;
  - `choice::<C: noctis_ui::Choices>(value, option) -> Result<C, OptionError>`.
- Unit tests for Matrix 43-56 and 58-66, named after their rule where one applies (`opt_02_wrong_type`, `opt_03_integer_outside_i64`).

**`crates/noctis-core/src/command.rs`**
- `CommandCtx` inner gains `input: CommandInput` (`pub(crate) fn with_input(path, input)`; `new(path)` uses `CommandInput::empty(path)`).
- `CommandHandler` gains `options: &'static [OptionDecl<'static>]` with `pub fn options()`, empty for 6.1 forms.
- Add `#[doc(hidden)] pub fn __new_with_options<P: CommandPath, O: Options, F: Fn(CommandCtx, P, O) -> Fut, Fut>` and `__new_plain_with_options<O, F: Fn(CommandCtx, O) -> Fut, Fut>`. The call order is: path check → `O::from_input(ctx.input)` → `opts.validate(&ctx)` → handler. Each error is wrapped in `HandlerError::new`.
- Add `pub(crate) fn call_command_json`. Update `Debug` to list option names. Unit tests for Matrix 67-74.

**`crates/noctis-core/src/lib.rs`**
- `pub use options::{CommandInput, CommandOption, InvalidOption, OptionError, OptionValue, Options};` and `pub use options::__options` (doc hidden).
- `__testing::call_command_json(handler: &CommandHandler, json: &[u8]) -> BoxFuture<'static, HandlerResult<()>>`.
- Crate docs: an "Options" paragraph.

**`crates/noctis-macros/src/options.rs` (new) + `lib.rs` `#[proc_macro_derive(Options, attributes(option))]`**
- Parse (Matrix 3-9) and lower per Design Notes.
- Emit the impl, the `const _` checks and the autocomplete path reference (`const _: () = { let _ = #path; };`).
- Validators: `let f: fn(&T, &CommandCtx) -> Result<(), MessageKey> = #path;` spanned on the path.
- Unit tests for Matrix 3-20 and 28, plus the const-arm generation (exact strings, like `command.rs`'s tests). Update the crate doc.

**`crates/noctis-macros/src/choices.rs`**
- Also emit `impl ::noctis::__private::core::CommandOption` (`KIND` String, `CHOICES` built with `Choice::new(label, ChoiceValue::Str(value))`, `decode` via `__options::choice::<Self>`).
- Replace the MOD-07 label/value-length checks with OPT-09 from `validate_options` on a synthetic STRING decl named `choice` (keep only `OPT_09`). Re-path `choice.choices[j]` to variant `j`'s ident, add the shared-limit note, and span on the literal/ident.
- The empty-value MOD-07 check and everything else stay.
- Update the unit tests (Matrix 37-42).

**`crates/noctis-macros/src/command.rs`**
- `check_params`: accept the trailing options param (Matrix 75-78 messages). `expand`: emit the `*_with_options` constructors when the param is present.
- Update the unit tests.

**`crates/noctis/src/lib.rs`**
- `__private`: add `rules`.
- Root: re-export the new core items and a documented `pub use noctis_macros::Options;` (attribute table, type table, validator, doctest).
- Prelude: `Options` (trait + derive), `CommandOption`, `MessageKey`. Update the crate/prelude doc lists.

**Facade tests**
- `crates/noctis/tests/options.rs` (new) -- Matrix 1, 2, 9, 27, 30, 37, 57-74 end to end through `noctis::__private::core::__testing::call_command_json`; record the received struct in a `static Mutex`.
- `crates/noctis/tests/boundary/opt.rs` (new) + `boundary.rs` `#[path] mod opt;` -- 25 options (`cmd_13_25_options`), 32-char option name, 100-char and 50×`😀` descriptions (`cmd_04_*`), `i64` bounds ±(2^53−1) (`opt_03_*`), `f64` bounds ±2^53 (`opt_04_*`), `max_length = 6000` / `min_length = 0` (`opt_06_*`), 25-variant Choices field (`opt_08_25_choices`), 100-char choice name and value (`opt_09_*`), 10 `file_types` (`opt_13_10_entries`).
- `crates/noctis/tests/ui/opt/*.rs` + blessed `.stderr` (new), each a minimal `use noctis::prelude::*;` file with `fn main() {}`: `CMD-04_option_missing_description`, `CMD-04_option_description_101`, `CMD-06_duplicate_option_name`, `OPT-01_required_after_optional`, `OPT-02_unsupported_type`, `OPT-03_integer_bound_out_of_range`, `OPT-04_number_bound_out_of_range`, `OPT-05_bounds_on_string`, `OPT-05_min_over_max`, `OPT-06_max_length_6001`, `OPT-07_channel_types_on_string`, `OPT-08_26_choices`, `OPT-09_choice_name_and_value_101`, `OPT-10_choices_and_autocomplete`, `OPT-11_autocomplete_on_bool`, `OPT-13_invalid_file_type`. Also `tests/types/command_options_not_derived.rs` (Matrix 79).
- Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`. Check that `CV2-09_derive_string_select_26_choices.stderr` is unchanged.

**Repo docs and tracking**
- `xtask/rules-pending.toml` -- delete the 13 `story = "6.2"` entries.
- `docs/discord-rules.md` -- update these rows:
  - CMD-06: field names or `rename`.
  - OPT-01: unchanged meaning.
  - OPT-02: add "Runtime (decoding)": `OptionError::WrongType`.
  - OPT-03: runtime `OutOfRange` when the value does not fit `i64`.
  - OPT-05 / OPT-06 / OPT-07 / OPT-11 / OPT-13: `#[option(min_value, max_value, …)]` names; type-dependent checks are `const _` items over the field's `CommandOption::KIND`.
  - OPT-07: `channel_types = [GuildText, …]`.
  - OPT-08: `derive(Options)` checks the Choices enum's `CHOICES.len() ≤ 25`.
  - OPT-09: `#[choice("Name", value = "v")]`, measured by `derive(Choices)`.
  - OPT-10: Choices-typed field plus `autocomplete`.
  - MOD-07: label/value lengths are reported as OPT-09 (same limit).
- `examples/project-template/.claude/skills/noctis/references/commands.md` --
  - §1: the handler forms with options.
  - §3: rewrite to the real API: the attribute table with `min_value`/`max_value`, `channel_types = […]`, validator signature, `autocomplete` (wired in 6.7), the type table without `i32`/`u32`, `CommandOption`/`Options`/`OptionValue`/`CommandInput`/`OptionError` (variants)/`InvalidOption`, the const-error form; notices are sent from 6.8 [planned].
  - "Choices": STRING-only now, integer choices [planned], OPT-08/OPT-09.
- `references/modals.md` §1.2 -- the OPT-09 message for long labels and values; `Resolved*::from_resolved`.
- `SKILL.md` -- `API hash`.
- `examples/project-template/src/modules/profile.rs` -- `#[option(description = "Whose profile to show (default you)")]` on `user`.
- `examples/project-template/AGENTS.md` -- one bullet on `derive(Options)`: literal descriptions equal to `cmd.<path>.opt.<name>.description`, `Option<T>` last, validators return `MessageKey`.
- `xtask/public-api.txt` -- re-bless.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- add any implementation decisions (the planning decisions are already there).

**Acceptance Criteria:**
- Given `#[derive(Options)]` with fields of the supported types (incl. `Option<T>`), when the Command is invoked through `call_command_json` with a valid interaction, then the handler receives the parsed struct; a missing required option, a wrong option type and an integer outside `i64` each yield their typed `OptionError` (downcastable from the `HandlerError`), never a panic, and the handler does not run.
- Given an option with a validator, when it fails at runtime, then the call returns `InvalidOption` carrying the validator's `MessageKey` and the handler does not run (6.8 turns it into the ephemeral message).
- Given `cargo test --release -p noctis --test ui`, when it runs, then every fixture under `tests/ui/opt/` fails with its `[RULE-ID]` in the `.stderr`, the existing CV2-09 and MOD-07 fixtures are unchanged, and the boundary tests (25 options, 25 choices, 32-char name, 100-char description) pass.
- Given each option's `#[option(description = "…")]`, when compiled, then `OPTIONS[i].description.default` is that literal, a doc comment never becomes it, and a missing or 101-char one fails with CMD-04.
- Given `cargo run --release -p xtask -- rules-catalog`, when it runs, then it passes with the 13 entries removed; and given the new public API, `api-snapshot` and `skill-check` pass with commands.md documenting `derive(Options)`, `derive(Choices)` and validators.

## Spec Change Log

## Review Triage Log

## Design Notes

**Partition and generated shape (decision).** For field *i*, the macro builds `decl_i(K)` for each `K` in `OptionType::ALL`. Each decl carries:
- the option name;
- the description (`""` if missing);
- `required`;
- the bounds and lengths;
- the channel-type values;
- `autocomplete`;
- `file_types`;
- no choices.

It runs `validate_options(&[decl_i(K)])` for every `K`:
- A violation that appears with equal `Display` for all 9 kinds is type-independent and becomes a `syn::Error`.
- Each remaining violation becomes its own `const _` item: `match <T as CommandOption>::KIND { OptionType::K => panic!(<text>), _ => {} }`, listing every kind for which it occurs, with braces doubled.
- The array-level rules (CMD-13, OPT-01, CMD-06) come from one `validate_options` over all decls with kind `String`, filtered to those three ids.

OPT-08 and OPT-10 need the choices, so they get dedicated const items (`if <T>::CHOICES.len() > 25` / `if !<T>::CHOICES.is_empty()` when `autocomplete`). Their text comes from `Violation::new(..)` built in the macro (the limit from `Limits::default().option_choices`). Verified locally with rustc 1.97 (`--emit=metadata`): it prints `error[E0080]: evaluation panicked: <text>`.

```rust
impl ::noctis::__private::core::Options for W {
    const OPTIONS: &'static [OptionDecl<'static>] = &[
        OptionDecl::new("days", <i64 as CommandOption>::KIND, "How long")
            .required(false).min_value(NumericValue::Int(1))
            .choices(<i64 as CommandOption>::CHOICES), /* … */ ];
    fn from_input(i: &CommandInput) -> Result<Self, OptionError> {
        Ok(Self { days: __options::optional::<i64>(i, "days")?, /* … */ })
    }
    fn validate(&self, ctx: &CommandCtx) -> Result<(), InvalidOption> { /* per field, in order */ Ok(()) }
}
```

**Why not marker traits.** Traits per attribute (`NumericOption`, …) cannot reproduce `validate_options`' exact range messages (OPT-03 vs OPT-04 depends on the kind). The const-arm scheme keeps AD-3 (one rule implementation) and needs no syntactic type guessing.

**Placement (decision).** The option traits, `CommandInput` and the errors live in `noctis-core` (capability map: FR-10 → macros, rules, core), next to `CommandHandler`, which needs them. The core → ui and core → rules edges are allowed. The resolved entity types stay in noctis-ui, shared with modals (Story 4.6 memlog), and gain public `from_resolved` constructors because they are `#[non_exhaustive]`.

**`derive(Choices)` (decision).** A single derive serves modals and commands. A 26-variant enum stays legal for the derive itself, which keeps the CV2-09 fixture. OPT-08 fires where the enum is used as an option. Label and value length is one rule pair with identical limits (OPT-09 / MOD-07 / CV2-09), so it is reported once, as OPT-09 with a note. Command choices are STRING only in 6.2.

**Smaller decisions.**
- Attribute names follow Discord (`min_value`/`max_value`, matching the violation paths). `channel_types` reuses the modal syntax.
- Validators are sync `fn(&T, &CommandCtx) -> Result<(), MessageKey>`, called only for present values.
- Unknown incoming options are ignored (stale schema).
- The `#[command]` macro cannot see the options struct, so the option-inclusive CMD-05/CMD-13 per Command wait for 6.4, which uses `CommandHandler::options()` and `measure_options`.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis --test ui` (new `opt/` fixtures, unchanged others), `boundary`, `options`, `commands`
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok (core → ui/rules allowed)
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok, 13 pending entries gone
- `cargo run --release -p xtask -- api-snapshot` -- expected: ok after `--bless`; hash copied into SKILL.md
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok (MSRV; `#[diagnostic::on_unimplemented]` and const panics are stable there)
