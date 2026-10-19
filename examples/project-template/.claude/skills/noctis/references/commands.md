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
| description 1–100 chars (from `cmd.<path>.desc` in the default locale; missing = error) | CMD-04 |
| total command size ≤ 8000 (name+desc+options+choices, longest localization per field) | CMD-05 |
| ≤ 3 words | CMD-10 |

### Attribute arguments [planned]

| Argument | Meaning |
| --- | --- |
| `ephemeral` | reply privately (default public). Fixed at registration (INT-10); the auto-defer uses it |
| `opens_modal` | the handler answers with a modal; disables auto-defer (INT-05) |
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
Base and group descriptions come from `cmd.config.desc` and `cmd.config set.desc`; if
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
| `"a b c d"` | CMD-10 |
| Module A registers `config`, Module B `config set` | CMD-12 at App build |
| two registrations resolve to the same path | FR-56 at App build (names both Modules) |
| a base with its own options **and** subcommands | CMD-12 / FR-9 (never silently dropped) |

Groups and plain subcommands side by side under one base are legal (CMD-14):
`"mod ban"` and `"mod case show"` together are fine.

## 3. Options

```rust
#[derive(Options, Debug)]
pub struct WarnOptions {
    /// Doc comments are rustdoc only; descriptions come from cmd.<path>.opt.<name>.desc.
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
    -> HandlerResult<Suggestions<String>>
{
    let rules = load_rules(ctx.guild_id()).await?;
    Ok(rules
        .into_iter()
        .filter(|r| r.name.contains(focused.as_str()))
        .map(|r| Suggestion::new(r.name.clone(), r.id.to_string()))
        .collect())
}
```

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
duplicated (FR-56), a base mixes options and subcommands (CMD-12), the merged tree
breaks CMD-11/13/05, or the App exceeds 100 global chat-input commands (REG-01).

## 7. Publish

See `app-and-sharding.md` §3. `--publish-guild <id>` in development (instant),
`--publish` in production. `--dry-run` prints JSON plus diff and runs every check. Boot
Flags and `apps = [...]` filter what is published. Disabled Modules: only their
guild-scoped commands are left out of a guild publish; global commands stay published
and answer "disabled here".

## 8. Handler context (`CommandCtx`) [planned]

Shared accessors (trait `Ctx`, also on every other context): `user_id()`, `user()`,
`guild_id()`, `channel_id()`, `user_locale()`, `guild_locale()`, `response_locale()`,
`cache()`, `modules()`, `rate_limits()`, `discord()`, `render(..)`, `http()`,
`gateway()`, `app_permissions()`, `member()`. Command-only: `command_path()`,
`attachment_size_limit()`.
