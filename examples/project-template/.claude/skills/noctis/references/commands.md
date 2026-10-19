# Commands

Covers: `#[command]` paths and patterns, options (`derive(Options)`, `derive(Choices)`),
autocomplete, context menus, command metadata, merging across Modules, Publish.
Rules: CMD-01..15, OPT-01..14, CTX-01..03, META-01..05, REG-01..06, LOC-01..06.
Doctrine: commands are **entry points** that open Views (UD-3).

## 1. Command Paths

```rust
#[command("dashboard")]                       // 1 word  → command
#[command("setup start")]                     // 2 words → subcommand of /setup
#[command("settings logs channel")]           // 3 words → group "logs" + subcommand "channel"
```

The handler is an `async fn` taking `CommandCtx`, optionally an options struct, and
extractors; it returns `HandlerResult<T>` where `T: IntoResponse` (usually a View).

```rust
#[command("setup start", ephemeral, permissions = MANAGE_GUILD, contexts = [Guild])]
async fn setup_start(ctx: CommandCtx) -> HandlerResult<SetupWizard> { … }
```

Register it: `Module::new("setup").add(setup_start())` (the macro turns the fn into a
descriptor constructor; call it with `()`).

Compile-time checks on the literal (each error names its rule):

| Check | Rule |
| --- | --- |
| each word matches `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$` (note: `ʼ` U+02BC, not ASCII `'`) | CMD-01 |
| 1–32 chars per word, no empty words (`"a  b"`) | CMD-02 |
| lowercase where the letter has a lowercase form | CMD-03 |
| description 1–100 chars: `description = "…"`, or with the `i18n` feature `cmd.<path>.description` in the default-locale file (a literal that differs from it is an error); missing = error | CMD-04 |
| total command size ≤ 8000 (name+desc+options+choices, longest localization per field) | CMD-05 |
| ≤ 3 words | CMD-10 |

### Attribute arguments [planned]

| Argument | Meaning |
| --- | --- |
| `description = "…"` | the description; required without the `i18n` feature, optional (must match the file) with it |
| `locales = "<dir>"` | where this command's default-locale file lives when it isn't the crate's `i18n!` dir (Module catalogs) |
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
| `max_expansions = n` | Command Pattern cap (default 25) |

`permissions`, `contexts`, `integration_types` and `nsfw` belong to the **base** command
(META-02). Every leaf under one base must agree, across all Modules, or App build fails
(REG-04, FR-9). Declare them once explicitly with [planned]
`Module::new("x").base("config", BaseMeta::new().permissions(Permissions::MANAGE_GUILD))`.
Base and group descriptions come from `cmd.config.description` and `cmd.config.set.description`; if
missing, a deterministic rule picks one and logs a warning (never registration order).

## 2. Command Patterns

One handler for several paths. `(a|b)` gives one alternative per word; `?` after a group
also yields the path without it. Expansion is left to right in declaration order.

```rust
#[command("config (set|get) (prefix|welcome)", permissions = MANAGE_GUILD)]
async fn config(ctx: CommandCtx, path: ConfigPath, opts: ConfigOptions)
    -> HandlerResult<ConfigView>
{
    match path {
        ConfigPath::SetPrefix | ConfigPath::SetWelcome => { … }
        ConfigPath::GetPrefix | ConfigPath::GetWelcome => { … }
    }
}
```

The macro generates `enum ConfigPath` (fn name in UpperCamelCase + `Path`) with one
variant per expansion (alternative words in UpperCamelCase, concatenated) and
`fn as_str(&self) -> &'static str` (`"config set prefix"`). [planned]

Expands to: `config set prefix`, `config set welcome`, `config get prefix`,
`config get welcome`.

### Patterns that do not compile

```rust
#[command("config (set|get) (prefix|welcome)?")]
```
```text
error: [CMD-11] `set` would be both a subcommand and a subcommand group of `/config`
  = note: `config set` makes `set` a subcommand
  = note: `config set prefix` makes `set` a subcommand group
  = help: remove the `?`: "config (set|get) (prefix|welcome)",
          or move the bare form to a separate path such as "config show"
```

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
gets exactly the checks a literal one does. Call `expand` and then `validate_tree`
before registering a dynamically built pattern; never split patterns by hand. Every
function takes `&Limits` (pass the same value the App builder gets) and returns
**every** violation at once as `Vec<Violation>` (never empty on `Err`).

**Pattern grammar (CMD-16).** Tokens are separated by Unicode whitespace (collapsed;
paths are joined with one ASCII space). A token is a word or a group `(a|b|…)`,
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
| `expand(pattern, &Limits) -> Result<Vec<String>, Vec<Violation>>` | pattern → ordered Command Paths; CMD-16, else CMD-15 cap, else CMD-15 duplicates; never checks names |
| `validate_name(word, &Limits) -> Vec<Violation>` | one name word, path `""`; order CMD-02 (length via `discord_len`), CMD-01 (charset; note `` first invalid character `'` (U+0027) ``, help `` use `ʼ` (U+02BC) instead of `'` `` for ASCII `'`), CMD-03 (help `` use `config` ``); an empty word gives CMD-02 only |
| `validate_tree(&[CommandDecl], &Limits) -> Result<(), Vec<Violation>>` | one tree set (scope ignored): CMD-10 per decl (that decl is then skipped), REG-04 duplicate handler paths (notes `` registered by `m1` ``; the first counts for the budget) and metadata for unregistered paths, then per node in first-appearance pre-order: names (CMD-01..03, path = node path), CMD-04, CMD-11 / CMD-12 (notes name both contributing paths and `` (from `module`) ``), CMD-13 (children, then handler options), CMD-05 per base after its subtree (`= note: longest variant per field`); last, REG-04 conflicting descriptions |
| `validate_merge(&[CommandDecl], &[ContextMenuDecl], &Limits) -> Result<(), Vec<Violation>>` | the whole App: per `Scope` (first appearance, commands then menus) `validate_tree` with a `= note: scope: global` / `scope: guild 42`, then counts (REG-01 global / REG-02 guild; paths `chat-input commands`, `user commands`, `message commands`; a merged base counts once; meta decls and empty paths don't count), then REG-03 for two context-menu commands with the same type and exact name |
| `CommandDecl<'a>` | `#[non_exhaustive]`, pub fields `source` (Module name, `""` = unnamed), `path` (words split on single ASCII spaces, so `"config  set"` has an empty word → CMD-02), `scope`, `kind`, `description: Option<LocalizedText>`, `name_localized: &[&str]` (localized names of the last word; only counted by CMD-05). `CommandDecl::handler(source, path)` (no options), `CommandDecl::meta(source, path)`; by-value `const` setters `.description(text)`, `.name_localized(&[…])`, `.scope(Scope)`, `.with_options(count, size)` (makes it a handler) |
| `DeclKind` | `#[non_exhaustive]`: `Handler { options, options_size }` (`options_size` = your computed CMD-05 share of the options: names, descriptions, choice names/values, longest variant each) or `Meta` (description / localized names for a node handlers create) |
| `LocalizedText<'a>` | `{ default, localized: &[&str] }`; `LocalizedText::new("d")`, `.with_localized(&[…])`, `.longest()` (max `discord_len`) |
| `Scope` | `Global` (default) or `Guild(u64)` |
| `ContextMenuDecl<'a>` | `#[non_exhaustive]` `{ source, scope, kind, name }`; `ContextMenuDecl::new(source, MenuKind::User, "Show Profile")`, `.scope(Scope)` |
| `MenuKind` | `User` or `Message` |

Tree model: every word prefix of a handler path is a node (1 word base, 2 subcommand or
group, 3 subcommand in a group); meta decls attach to an existing node. A node's
description is the first `Some` in input order; a handler node without one is CMD-04
(`= note: missing description`), a base or group without a handler may lack one. CMD-05
per base = Σ nodes (longest of word and `name_localized`) + (longest description
variant) + Σ handler `options_size`.

```rust
use noctis::rules::Limits;
use noctis::rules::command::{CommandDecl, LocalizedText, expand, validate_tree};

let limits = Limits::default();
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

## 5. Context-menu commands

```rust
#[user_command("Show profile", key = "show_profile", contexts = [Guild])]
async fn show_profile(ctx: CommandCtx, target: TargetUser) -> HandlerResult<ProfileView> { … }

#[message_command("Report message", key = "report")]
async fn report(ctx: CommandCtx, target: TargetMessage) -> HandlerResult<ReportView> { … }
```

- Name 1–32 chars, mixed case and spaces allowed (CTX-01); no description, no options
  (CTX-02). Localized as a whole: `ctxcmd.<key>.name`.
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

## 7. Publish

See `app-and-sharding.md` §3. `--publish-guild <id>` in development (instant),
`--publish` in production. `--dry-run` prints JSON plus diff and runs every check. Boot
Flags and `apps = [...]` filter what is published. Disabled Modules: only their
guild-scoped commands are left out of a guild publish; global commands stay published
and answer "disabled here".

## 8. Handler context (`CommandCtx`) [planned]

Shared accessors (trait `Ctx`, also on every other context): `user_id()`, `user()`,
`guild_id()`, `channel_id()`, `user_locale()`, `guild_locale()`, `response_locale()`,
`kv()` (your key-value data), `cache()` (Discord resources), `modules()`,
`rate_limits()`, `render(..)`, `http()`,
`gateway()`, `app_permissions()`, `member()`. Command-only: `command_path()`,
`attachment_size_limit()`.
