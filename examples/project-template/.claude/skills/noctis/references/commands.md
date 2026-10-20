# Commands

Covers: `#[command]` paths and patterns, options (`derive(Options)`, `derive(Choices)`),
autocomplete, context menus, command metadata, merging across Modules, Publish.
Rules: CMD-01..15, OPT-01..14, CTX-01..03, META-01..05, REG-01..06, LOC-01..06.
Doctrine: commands are **entry points** that open Views (UD-3).

## 1. Command Paths

```rust
#[command("dashboard", description = "Open the dashboard")]          // 1 word  → command
#[command("setup start", description = "Start the setup")]           // 2 words → subcommand of /setup
#[command("settings logs channel", description = "Set the log channel")] // 3 words → group "logs" + subcommand "channel"
```

Words are separated by exactly one ASCII space. `noctis::rules::command::PathKind::of(path)`
tells the kind: `Command` (1 word), `Subcommand` (2), `GroupSubcommand` (3), `None` otherwise;
`.words()` gives 1/2/3. Subcommand and group nodes come only from paths: nothing sets
`required` on them (OPT-14).

The handler forms that work now: a path takes `(ctx: CommandCtx)`, a pattern with a group
takes `(ctx: CommandCtx, path: <Fn>Path)` (§2); both return `HandlerResult<()>`. Doc
comments stay rustdoc and never become the description.

```rust
use noctis::prelude::*;

/// Rustdoc only.
#[command("setup start", description = "Start the setup")]
pub async fn setup_start(ctx: CommandCtx) -> HandlerResult<()> {
    let _ = ctx.command_path(); // "setup start"
    Ok(())
}
```

[planned] Options structs and extractors as further parameters (Story 6.2), returning a
View (`HandlerResult<T>` where `T: IntoResponse`) and the Responder (Story 6.5).

Register it: `Module::new("setup").add(setup_start())` (the macro turns the fn into a
descriptor constructor `fn setup_start() -> CommandHandler`; call it with `()`).
`CommandHandler` has `name()` (`module_path::fn`), `pattern()` (the literal),
`description()` and `paths()` (every Command Path, expansion order).

Compile-time checks on the literal (each error names its rule):

| Check | Rule |
| --- | --- |
| each word matches `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$` (note: `ʼ` U+02BC, not ASCII `'`) | CMD-01 |
| 1–32 chars per word, no empty words: a leading, trailing or doubled space (`"a  b"`) is CMD-02, any other whitespace (tab, U+00A0) CMD-01 | CMD-02 |
| lowercase where the letter has a lowercase form | CMD-03 |
| description 1–100 chars (UTF-16 counted): `description = "…"` (literal only; doc comments never count); missing = error with `help: add description = "…"`. [planned] With the `i18n` feature, `cmd.<path>.description` from the default-locale file (Story 11.4) | CMD-04 |
| total command size ≤ 8000 (name+desc+options+choices, longest localization per field) | CMD-05 |
| ≤ 3 words | CMD-10 |

### Attribute arguments

The first argument is the path or pattern, a string literal (raw strings work). Landed:
`description` and `max_expansions`. Every other argument below is [planned] and is a
compile error today (`` `#[command]` argument `ephemeral` is planned and not supported yet ``).

| Argument | Meaning |
| --- | --- |
| `description = "…"` | the description (a string literal); required. [planned] optional with the `i18n` feature, then it must match the file |
| `max_expansions = n` | Command Pattern cap (integer literal ≥ 1, default 25) |
| `locales = "<dir>"` | [planned] where this command's default-locale file lives when it isn't the crate's `i18n!` dir (Module catalogs) |
| `ephemeral` | reply privately (default public). Fixed at registration (INT-10); the auto-defer uses it |
| `opens_modal` | the handler answers with a modal and takes `CommandResponder<OpensModal>`; disables auto-defer (INT-05) |
| `permissions = MANAGE_GUILD \| BAN_MEMBERS` | `default_member_permissions` (META-01); unknown names are compile errors |
| `contexts = [Guild, BotDm, PrivateChannel]` | where it can be used |
| `integration_types = [GuildInstall, UserInstall]` | install types (META-04: must be enabled on the app) |
| `nsfw` | age-restricted (CMD-07) |
| `guilds = DEV_GUILDS` | guild-scoped: published only to these guilds (`const DEV_GUILDS: &[u64]`); `contexts`/`integration_types` are then invalid (META-03) |
| `rate_limit(user, 3, per = "10s")` | see `modules-middleware-ratelimits.md` |
| `flags = ["beta"]` | Boot Flags |
| `apps = ["main"]` | publish only for these Apps (multi-App) |

`permissions`, `contexts`, `integration_types` and `nsfw` belong to the **base** command
(META-02). Every leaf under one base must agree, across all Modules, or App build fails
(META-01, META-02, CMD-07, REG-04, FR-9; see §6 "Metadata rules"). Declare them once explicitly with [planned]
`Module::new("x").base("config", BaseMeta::new().permissions(Permissions::MANAGE_GUILD))`.
Base and group descriptions come from `cmd.config.description` and `cmd.config.set.description`; if
missing, a deterministic rule picks one and logs a warning (never registration order).

## 2. Command Patterns

One handler for several paths. `(a|b)` gives one alternative per word; `?` after a group
also yields the path without it. Expansion is left to right in declaration order.

```rust
#[command("config (set|get) (prefix|welcome)", description = "Configure the bot")]
async fn config(ctx: CommandCtx, path: ConfigPath) -> HandlerResult<()> {
    match path {
        ConfigPath::SetPrefix | ConfigPath::SetWelcome => { … }
        ConfigPath::GetPrefix | ConfigPath::GetWelcome => { … }
    }
    Ok(())
}
```

A pattern with at least one group generates the path enum next to the fn; a plain path
generates none.

- Name: the fn name (`r#` stripped) split on `_`, each part's first letter uppercased,
  plus `Path`: `config` → `ConfigPath`, `set_prefix` → `SetPrefixPath`, `r#type` →
  `TypePath`. Same visibility and `#[cfg]` as the fn; every item is documented.
- Variants: the chosen alternatives of each expansion, in order, each word split on `-`
  and `_` and UpperCamelCased, concatenated (`set-prefix` → `SetPrefix`, `a_b` → `AB`,
  `ğüncelle` → `Ğüncelle`, `ßa` → `SSa`). An expansion whose optional groups are all
  absent is `Bare` (`"(a|b)? c"` → `A`, `B`, `Bare`). A leading digit gets `_` (`2fa` →
  `_2fa`); `self` → `Self_`. Two alternatives giving the same name are a compile error
  (`` `#[command]` path variants collide: `x a-b` and `x a_b` both name `AB`; rename one
  alternative ``): rename one, there is no positional fallback.
- `ConfigPath::ALL`: every variant in expansion order; `path.as_str()` → `"config set
  prefix"` (const fn); it implements `noctis::CommandPath` (`PATHS`, `from_path(&str)`
  exact byte match, `as_str`).
- The second parameter must be this enum (else `` the second parameter must be
  `ConfigPath`, the enum `#[command]` generates for this pattern ``).

Invoked with a path the handler does not declare, the descriptor returns
`Err(HandlerError)` wrapping `noctis::UnknownCommandPath` (`path()`, `command()`;
`` `config nope` is not a path of the Command `bot::config` ``) and the handler does not run.

Expands to: `config set prefix`, `config set welcome`, `config get prefix`,
`config get welcome`.

### Patterns that do not compile

```rust
#[command("config (set|get) (prefix|welcome)?")]
```
```text
error: [CMD-11] config set: limit a subcommand or a subcommand group, not both, actual `set`
         = note: `config set` makes `set` a subcommand
         = note: `config set prefix` makes `set` a subcommand group
         = help: remove the `?`: "config (set|get) (prefix|welcome)"
```

The same error follows for `get`: every violation is reported in one compile, spanned on the
pattern (CMD-04 on the `description` literal). To keep a bare form, move it to a separate
path such as `"config show"`. Over the cap, CMD-15 suggests `` raise the cap with
`max_expansions = 26` or split the pattern ``.

Other structural errors:

| Pattern / situation | Rule |
| --- | --- |
| `"config (show)?"` → yields `config` and `config show`: a base with children can't be invoked | CMD-12 |
| more than 25 children under one node | CMD-13 |
| `"a (b\|b)"` or an optional group making two expansions identical | CMD-15 |
| more than `max_expansions` (25) expansions | CMD-15 |
| malformed pattern: `"a (b"`, `"a(b)"`, `"a (b c)"`, `"a ((b))"`, `"a ()"`, `"a ?"`, `"a \| b"` | CMD-16 (names the byte offset) |
| `"a b c d"` | CMD-10 |
| Module A registers `config`, Module B `config set` | CMD-12 at App build |
| two registrations resolve to the same path | REG-04 at App build (names both Modules) |
| a base with its own options **and** subcommands | CMD-12 / FR-9 (never silently dropped) |

Groups and plain subcommands side by side under one base are legal (CMD-14):
`"mod ban"` and `"mod case show"` together are fine.

### Rule functions (`noctis::rules::command`)

`#[command]` and `App::build` call these functions, so a pattern you build at runtime
gets exactly the checks a literal one does. Call `validate_pattern_spacing`, `expand` and
then `validate_tree` before registering a dynamically built pattern; never split patterns
by hand. Every
function takes `&Limits` (pass the same value the App builder gets) and returns
**every** violation at once as `Vec<Violation>` (never empty on `Err`).

**Pattern grammar (CMD-16).** Tokens are separated by Unicode whitespace (collapsed by
`expand`; paths are joined with one ASCII space, and `validate_pattern_spacing` rejects
anything but single ASCII spaces first). A token is a word or a group `(a|b|…)`,
optionally followed directly by `?`. A group is a whole token (`a(b)`, `(b)c`, `(a)(b)`
are errors), its alternatives are non-empty single words (no spaces inside), groups
don't nest, and `|`, `)` or `?` anywhere else is an error. `expand` reports only the
first syntax error, with a `= note: at byte N`.

**Expansion order.** Odometer: the leftmost group varies slowest, alternatives in
declaration order, an optional group's "absent" choice after its alternatives:
`"config (set|get) (prefix|welcome)?"` → `config set prefix`, `config set welcome`,
`config set`, `config get prefix`, `config get welcome`, `config get`. The count is
checked against `Limits::pattern_expansions` (25) before anything is built (CMD-15,
`AtMost(cap)` / `Count(n)`, saturating); then each expansion produced more than once is
one CMD-15 (`= note: produced N times`). `""` expands to `[""]` (CMD-10 later).

| Item | What it is |
| --- | --- |
| `validate_pattern_spacing(pattern, &Limits) -> Vec<Violation>` | run before `expand` (which collapses whitespace); path = pattern, help `separate words with exactly one ASCII space`: one CMD-02 for the first empty word (leading, trailing or doubled space; `between 1 and 32`, actual 0, note `empty word at byte N`), then one CMD-01 for the first whitespace other than U+0020 (note `at byte N`); `""` → none |
| `PathKind` | `Command` / `Subcommand` / `GroupSubcommand`; `PathKind::of(path) -> Option<PathKind>` (`None` for empty, an empty word or > 3 words), `.words()` |
| `expand(pattern, &Limits) -> Result<Vec<String>, Vec<Violation>>` | pattern → ordered Command Paths; CMD-16, else CMD-15 cap, else CMD-15 duplicates; never checks names |
| `validate_name(word, &Limits) -> Vec<Violation>` | one name word, path `""`; order CMD-02 (length via `discord_len`), CMD-01 (charset; note `` first invalid character `'` (U+0027) ``, help `` use `ʼ` (U+02BC) instead of `'` `` for ASCII `'`), CMD-03 (help `` use `config` ``); an empty word gives CMD-02 only |
| `validate_tree(&[CommandDecl], &Limits) -> Result<(), Vec<Violation>>` | one tree set (scope ignored): CMD-10 per decl (that decl is then skipped), REG-04 duplicate handler paths (notes `` registered by `m1` ``; the first counts for the budget) and metadata for unregistered paths, then per node in first-appearance pre-order: names (CMD-01..03, path = node path), CMD-04, CMD-11 / CMD-12 (notes name both contributing paths and `` (from `module`) ``), CMD-13 (children, then handler options), CMD-05 per base after its subtree (`= note: longest variant per field`); then REG-04 conflicting descriptions; last, per base in first-appearance order, the metadata checks (§6 "Metadata rules") |
| `validate_merge(&[CommandDecl], &[ContextMenuDecl], &Limits) -> Result<(), Vec<Violation>>` | the whole App: per `Scope` (first appearance, commands then menus) `validate_tree` with a `= note: scope: global` / `scope: guild 42`, then META-03 per command, then per context-menu command (input order; path `` `<path or name>.contexts` `` / `.integration_types`, notes `` declared by `m1` `` then the scope), then CTX-01 per context-menu name (path = the name, same notes), then counts (REG-01 global / REG-02 guild; paths `chat-input commands`, `user commands`, `message commands`; a merged base counts once; meta decls and empty paths don't count), then REG-03 for two context-menu commands with the same type and exact name |
| `CommandDecl<'a>` | `#[non_exhaustive]`, pub fields `source` (Module name, `""` = unnamed), `path` (words split on single ASCII spaces, so `"config  set"` has an empty word → CMD-02), `scope`, `kind`, `description: Option<LocalizedText>`, `name_localized: &[&str]` (localized names of the last word; only counted by CMD-05), `metadata: CommandMeta` (default all unset). `CommandDecl::handler(source, path)` (no options), `CommandDecl::meta(source, path)`; by-value `const` setters `.description(text)`, `.name_localized(&[…])`, `.scope(Scope)`, `.metadata(CommandMeta)`, `.with_options(count, size)` (makes it a handler; pass `with_options(opts.len(), measure_options(&opts, &limits))`) |
| `DeclKind` | `#[non_exhaustive]`: `Handler { options, options_size }` (`options_size` = your computed CMD-05 share of the options: names, descriptions, choice names/values, longest variant each) or `Meta` (description / localized names for a node handlers create) |
| `LocalizedText<'a>` | `{ default, localized: &[&str] }`; `LocalizedText::new("d")`, `.with_localized(&[…])`, `.longest()` (max `discord_len`) |
| `Scope` | `Global` (default) or `Guild(u64)` |
| `ContextMenuDecl<'a>` | `#[non_exhaustive]` `{ source, scope, kind, name, metadata }`; `ContextMenuDecl::new(source, MenuKind::User, "Show Profile")`, `.scope(Scope)`, `.metadata(CommandMeta)`; no description or options exist (CTX-02) |
| `MenuKind` | `User` or `Message` only (CTX-03: no entry-point commands) |

Tree model: every word prefix of a handler path is a node (1 word base, 2 subcommand or
group, 3 subcommand in a group); meta decls attach to an existing node. A node's
description is the first `Some` in input order; a handler node without one is CMD-04
(`= note: missing description`), a base or group without a handler may lack one. CMD-05
per base = Σ nodes (longest of word and `name_localized`) + (longest description
variant) + Σ handler `options_size`.

```rust
use noctis::rules::Limits;
use noctis::rules::command::{
    CommandDecl, LocalizedText, expand, validate_pattern_spacing, validate_tree,
};

let limits = Limits::default();
let spacing = validate_pattern_spacing(&pattern, &limits);
if !spacing.is_empty() {
    return Err(report(spacing));
}
let paths = expand(&pattern, &limits).map_err(report)?;
let decls: Vec<_> = paths
    .iter()
    .map(|p| CommandDecl::handler("settings", p).description(LocalizedText::new("Settings")))
    .collect();
validate_tree(&decls, &limits).map_err(report)?;
```

Each `Violation` prints ``[CMD-11] config set: limit …, actual `set` ``, then one
`= note: …` line per note and a `= help: …` line (`notes`, `help`).

## 3. Options

```rust
#[derive(Options, Debug)]
pub struct WarnOptions {
    /// Doc comments are rustdoc only. Descriptions: `#[option(description = "…")]`, or
    /// with the `i18n` feature cmd.<path>.opt.<name>.description.
    pub member: ResolvedUser,                       // required (not Option)
    #[option(min = 1, max = 30)]
    pub days: Option<i64>,                          // optional
    #[option(min_length = 3, max_length = 200)]
    pub reason: Option<String>,
    #[option(channel_types(GuildText, GuildAnnouncement))]
    pub log_to: Option<ResolvedChannel>,
    pub severity: Option<Severity>,                 // choices from derive(Choices)
    #[option(autocomplete = rule_suggest)]
    pub rule: Option<String>,
    #[option(file_types = ["image", ".pdf"])]
    pub evidence: Option<ResolvedAttachment>,
    #[option(rename = "silent", validate = no_self_warn)]
    pub quiet: Option<bool>,
}

#[command("mod warn", permissions = MODERATE_MEMBERS)]
async fn warn(ctx: CommandCtx, opts: WarnOptions) -> HandlerResult<WarnView> { … }
```

Field type → option type (OPT-02); unsupported types don't implement `CommandOption`:

| Rust type | Discord option |
| --- | --- |
| `String` | STRING (3) |
| `i64` (also `i32`, `u32`… with implied ranges) | INTEGER (4), range ±(2^53−1) (OPT-03) |
| `f64` | NUMBER (10) (OPT-04) |
| `bool` | BOOLEAN (5) |
| `ResolvedUser { user, member: Option<PartialMember> }` / `Id<UserMarker>` | USER (6) |
| `ResolvedChannel` / `Id<ChannelMarker>` | CHANNEL (7) |
| `ResolvedRole` / `Id<RoleMarker>` | ROLE (8) |
| `Mentionable` (user or role) | MENTIONABLE (9) |
| `ResolvedAttachment` | ATTACHMENT (11) |
| enum with `derive(Choices)` | STRING/INTEGER with choices |
| `Option<T>` | the same, not required |

Compile-time rules: required fields before optional ones — the derive never reorders
(OPT-01); `min`/`max` only on numbers, `min ≤ max` (OPT-05); `min_length` 0–6000,
`max_length` 1–6000 only on strings (OPT-06); `channel_types` only on channels (OPT-07);
≤ 25 options per level (CMD-13); unique names (CMD-06); `file_types` ≤ 10 entries,
attachment only (OPT-13); choices **or** autocomplete, never both (OPT-10); autocomplete
only on string/integer/number (OPT-11).

### Rule functions (options)

`derive(Options)` and dynamically declared options share these functions in
`noctis::rules::command`. Check a dynamic options array with `validate_options`, then
feed its size into the tree check:

```rust
use noctis::rules::Limits;
use noctis::rules::command::{
    Choice, ChoiceValue, CommandDecl, LocalizedText, NumericValue, OptionDecl, OptionType,
    measure_options, validate_options, validate_tree,
};

let limits = Limits::default();
let severity = [Choice::new("Low", ChoiceValue::Str("low"))];
let opts = [
    OptionDecl::new("days", OptionType::Integer, "How long")
        .required(true)
        .min_value(NumericValue::Int(1))
        .max_value(NumericValue::Int(30)),
    OptionDecl::new("severity", OptionType::String, "How bad").choices(&severity),
];
validate_options(&opts, &limits).map_err(report)?;
let decl = CommandDecl::handler("mod", "mod warn")
    .description(LocalizedText::new("Warn a member"))
    .with_options(opts.len(), measure_options(&opts, &limits));
validate_tree(&[decl], &limits).map_err(report)?;
```

| Item | What it is |
| --- | --- |
| `validate_options(&[OptionDecl], &Limits) -> Result<(), Vec<Violation>>` | one options array in isolation; every violation at once (order below) |
| `measure_options(&[OptionDecl], &Limits) -> usize` | the array's CMD-05 share (saturating): per option the longest of name and `name_localized` + longest description variant + per choice (longest name variant + value length: `discord_len` of a string, the `Display` length of a number, `2.0` → `2` = 1) |
| `OptionDecl<'a>` | `#[non_exhaustive]`, pub fields `name`, `name_localized: &[&str]` (only measured), `description: LocalizedText`, `kind: OptionType`, `required`, `min_value` / `max_value: Option<NumericValue>`, `min_length` / `max_length: Option<usize>`, `channel_types: &[u8]` (raw Discord channel type values), `choices: &[Choice]`, `autocomplete`, `file_types: &[&str]`; empty slices mean unset. `OptionDecl::new(name, OptionType::String, "description")` (optional, everything unset), by-value `const` setters `.name_localized(&[…])`, `.description_localized(&[…])`, `.required(bool)`, `.min_value(v)`, `.max_value(v)`, `.min_length(n)`, `.max_length(n)`, `.channel_types(&[0, 5])`, `.choices(&[…])`, `.autocomplete(bool)`, `.file_types(&["image", ".pdf"])` |
| `OptionType` | `String`, `Integer`, `Boolean`, `User`, `Channel`, `Role`, `Mentionable`, `Number`, `Attachment` (OPT-02; no subcommand types, OPT-14); `OptionType::ALL`; `.value()` → 3..=11; `.name()` → `"STRING"`, … |
| `NumericValue` | `Int(i64)` or `Float(f64)`; `Display` (`1.0` → `1`, NaN → `NaN`, ∞ → `inf`) |
| `ChoiceValue<'a>` | `Str(&str)`, `Int(i64)` or `Float(f64)` |
| `Choice<'a>` | `#[non_exhaustive]` `{ name: LocalizedText, value: ChoiceValue }`; `Choice::new("Low", ChoiceValue::Str("low"))`, `.name_localized(&[…])` |

Paths: `P` is the option name, or `options[i]` (0-based) when the name is empty; violations
use `P`, `P.min_value`, `P.max_value`, `P.min_length`, `P.max_length`, `P.channel_types`,
`P.choices`, `P.choices[j]`, `P.autocomplete`, `P.file_types`, `P.file_types[k]`; CMD-13
uses `options`. Order: CMD-13 (`AtMost(25)`); then per option in input order: name rules
(as `validate_name`), CMD-04 (default, then localized, notes `in the default description`
/ `in localized description #i`), `min_value` then `max_value` (OPT-05 "only on INTEGER or
NUMBER", else OPT-05 "an integer bound on an INTEGER option" for a `Float` on INTEGER, else
OPT-03 ±(2^53−1) for INTEGER / OPT-04 finite ±2^53 for NUMBER), OPT-05 `min_value ≤
max_value` (actual `` `6 > 5` ``), `min_length` then `max_length` (OPT-06 "only on STRING
options", else `Between{0,6000}` / `Between{1,6000}`), OPT-06 `min_length ≤ max_length`,
OPT-07 (`channel_types` on a non-CHANNEL), OPT-08 (choices on a non-STRING/INTEGER/NUMBER,
then more than 25), per choice: OPT-09 name 1–100 (notes `in the choice name` / `in
localized choice name #k`), then for STRING/INTEGER/NUMBER only: OPT-09 value type (actual
`` `integer` ``/`` `string` ``/`` `number` ``, note `option type STRING`), else OPT-09 string
value ≤ 100 (empty is fine, note `in the choice value`) / OPT-03 / OPT-04, then OPT-05 "a
choice value within min_value and max_value"; then OPT-10 (choices and autocomplete),
OPT-11 (autocomplete on another type), OPT-13 (`file_types` on a non-ATTACHMENT, then more
than 10, then each entry that isn't `image`, `video`, `audio` or `.` + ASCII letters/digits:
`.pdf`, `.PNG`, `.7z` pass, `pdf`, `.tar.gz`, `Image` fail); then OPT-01 for each required
option after an optional one (``= note: after optional `b` ``); then CMD-06 per duplicate
name, first-occurrence order (`= note: declared at positions 0, 2`). Orderings run only when
both bounds passed their own checks. Localized option names are not validated here
(LOC-02, planned).

Runtime: a missing required option or wrong type is a typed error, never a panic. A
failing `validate = fn` (`fn(&T, &CommandCtx) -> Result<(), MessageKey>`) sends a
localized ephemeral message and the handler doesn't run. File-type filters are
extension-only; check `content_type` yourself (FR-19).

### Choices

```rust
#[derive(Choices, Debug, Clone, Copy)]
pub enum Severity {
    #[choice("low")]  Low,
    #[choice("high")] High,
}
```

One variant = one choice; ≤ 25 (OPT-08); value ≤ 100 chars (OPT-09). Choice names come
from `cmd.<path>.opt.<opt>.choice.<value>`; enums used in modals/selects use
`#[choices(key = "prefix")]` → `<prefix>.<value>` and expose `fn label(&self, Locale) ->
String`. Integer choices: `#[choice(1)]` with `#[repr(i64)]`-like values. [planned]

## 4. Autocomplete

```rust
#[autocomplete]
async fn rule_suggest(ctx: AutocompleteCtx, focused: Focused<String>, partial: Partial<WarnOptions>)
    -> HandlerResult<Responded>
{
    let rules = load_rules(ctx.guild_id()).await?;
    let choices = rules
        .into_iter()
        .filter(|r| r.name.contains(focused.as_str()))
        .map(|r| Choice::new(r.name.clone(), r.id.to_string()));
    Ok(ctx.respond(choices).await?)
}
```

`AutocompleteCtx` has exactly one response method, `respond` (INT-04, INT-06).

- `Partial<T>` has every field as `Option`, because Discord may omit required options
  (OPT-12). Re-validate in the real handler: autocomplete values are not constrained.
- More than 25 suggestions are truncated to 25 with a warning (INT-07, always).
- No defer exists for autocomplete (INT-06). On error or timeout Noctis answers an
  empty list and routes the error to the Autocomplete Error Sink.
- Register the autocomplete fn in the Module too: `.add(rule_suggest())`.

Rule function: `noctis::rules::command::validate_autocomplete(&choices, OptionType::String,
&limits) -> Vec<Violation>` checks a response the way `respond` does. More than
`autocomplete_choices` (25) choices is one INT-07 violation at path `choices`
(`help: Noctis sends the first 25 choices and logs this as a warning`): the caller
truncates and warns, in every mode. Each kept choice (`choices[j]`) follows OPT-09 (name
1–100 and localized names, value type, string value ≤ 100) and OPT-03 / OPT-04 for
numbers; dropped choices are not checked. On an option type other than STRING, INTEGER or
NUMBER the only violation is OPT-11 (`autocomplete only on STRING, INTEGER or NUMBER
options`).

```rust
use noctis::rules::RuleId;
use noctis::rules::command::{Choice, ChoiceValue, OptionType, validate_autocomplete};

let mut choices: Vec<Choice<'_>> = rules.iter().map(|r| Choice::new(&r.name, ChoiceValue::Str(&r.id))).collect();
for v in validate_autocomplete(&choices, OptionType::String, &limits) {
    if v.rule == RuleId::INT_07 {
        tracing::warn!("{v}"); // too many choices: truncate below
    } else {
        tracing::error!("{v}"); // Discord rejects the response: fix or drop that choice
    }
}
choices.truncate(limits.autocomplete_choices);
```

Only INT-07 is a truncate-and-warn case; any other violation (OPT-09, OPT-03, OPT-04,
OPT-11) means Discord rejects the response, so fix or drop the offending choices first.

## 5. Context-menu commands

```rust
#[user_command("Show profile", key = "show_profile", contexts = [Guild])]
async fn show_profile(ctx: CommandCtx, target: TargetUser) -> HandlerResult<ProfileView> { … }

#[message_command("Report message", key = "report")]
async fn report(ctx: CommandCtx, target: TargetMessage) -> HandlerResult<ReportView> { … }
```

- Name 1–32 chars, mixed case and spaces allowed (CTX-01); no description, no options
  (CTX-02). Localized as a whole: `ctxcmd.<key>.name`.
- Check a dynamic name with `noctis::rules::command::validate_context_menu_name(name,
  &limits) -> Vec<Violation>`: length only (`discord_len`, 1 to
  `Limits::context_menu_name_len`), path `""`, CTX-01 `Between{1,32}` / `Count(n)`;
  `"Show Profile"`, `"It's"`, `"Report 🚩"` pass. `validate_merge` runs it on every menu.
- `TargetUser { user, member: Option<PartialMember> }`, `TargetMessage { message }`.
- Same name + same type twice is a `BuildError` (REG-03). Limits: 15 user and 15
  message commands globally (REG-01).

## 6. Merging across Modules

Commands sharing a base word merge into one Discord command (tescord semantics). App
build fails, naming both Modules, when: metadata conflicts (META-01/02), a path is
duplicated (REG-04), a base mixes options and subcommands (CMD-12), the merged tree
breaks CMD-11/13/05, or the App exceeds 100 global chat-input commands (REG-01).
`App::build` runs `noctis::rules::command::validate_merge` on everything registered
(see §2 "Rule functions"), so every conflict is reported at once.

### Metadata rules

| Item | What it is |
| --- | --- |
| `CommandMeta<'a>` | `#[non_exhaustive]`, `Default`; pub fields `default_member_permissions: Option<u64>` (raw permission bits), `contexts: Option<&[InteractionContext]>`, `integration_types: Option<&[IntegrationType]>`, `nsfw: Option<bool>`; `None` = unset. `CommandMeta::new()` plus by-value `const` setters of the same names; no `dm_permission` (META-05: use `contexts`) |
| `InteractionContext` | `Guild` (0), `BotDm` (1), `PrivateChannel` (2); `.value()`, `.name()` (variant name) |
| `IntegrationType` | `GuildInstall` (0), `UserInstall` (1); `.value()`, `.name()` |
| `validate_meta(&CommandMeta, Scope, &Limits) -> Vec<Violation>` | META-03: a `Scope::Guild` declaration may not set `contexts` or `integration_types`; paths `contexts`, `integration_types` (that order), `Describe("only on global commands")`, actual the set sorted by value and deduplicated (`` `[Guild, PrivateChannel]` ``, `` `[]` ``), note `scope: guild 42` |

Attach metadata with `CommandDecl::handler(..).metadata(CommandMeta::new().nsfw(true))`
or, once for the whole base, `CommandDecl::meta("m0", "config").metadata(…)`.
`validate_tree` checks per base (first-appearance order), after REG-04 description
conflicts:

1. META-02 for every meta declaration **below** the base that sets metadata (input order;
   path = its path, ``= note: declared by `m1` ``, ``= help: declare it on `config` ``);
   it takes no further part.
2. Per field — META-01 `default_member_permissions`, META-02 `contexts`, META-02
   `integration_types`, CMD-07 `nsfw` (path and actual = the base): without an explicit
   base declaration (a meta decl at the 1-word path that sets the field) **unset is a
   value**: every handler under the base must agree, so `8` vs unset conflicts
   (``= note: `config get` leaves it unset (from `m2`)``, ``= help: set it on every
   command under `config`, or once on `config` with a metadata declaration``). With one,
   unset handlers inherit it and every declaration that sets it must match. Notes list
   each participant in input order: `` `config set` sets `8` (from `m1`) ``. Lists compare
   as sets (`[Guild, BotDm]` = `[BotDm, Guild, Guild]`).

CMD-10-dropped declarations, orphan meta declarations and empty paths never take part.
Different scopes and different bases never conflict.

## 7. Publish

See `app-and-sharding.md` §3. `--publish-guild <id>` in development (instant),
`--publish` in production. `--dry-run` prints JSON plus diff and runs every check. Boot
Flags and `apps = [...]` filter what is published. Disabled Modules: only their
guild-scoped commands are left out of a guild publish; global commands stay published
and answer "disabled here".

## 8. Handler context (`CommandCtx`)

Available now: `ctx.command_path() -> &'static str`, the invoked Command Path (`"config set
prefix"`). `CommandCtx` is `Clone` and `Debug`.

[planned] Shared accessors (trait `Ctx`, also on every other context): `user_id()`, `user()`,
`guild_id()`, `channel_id()`, `user_locale()`, `guild_locale()`, `response_locale()`,
`kv()` (your key-value data), `cache()` (Discord resources), `modules()`,
`rate_limits()`, `render(..)`, `http()`,
`gateway()`, `app_permissions()`, `member()`. Command-only: `attachment_size_limit()`.
