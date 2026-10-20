# Commands

Covers: `#[command]` paths and patterns, options (`derive(Options)`, `derive(Choices)`),
autocomplete, context menus, command metadata, merging across Modules, Publish.
Rules: CMD-01..15, OPT-01..14, CTX-01..03, META-01..05, REG-01..06, LOC-01..06.
Doctrine: commands are **entry points** that open Views (UD-3).

> **Converged form [planned: Stories 6.10 and 6.11].** A command is a struct: `#[derive(SlashCommand)]`
> (fields = options, unit struct when none) with `#[command("path", ..)]`, and its handler
> is `impl X { #[handler] async fn handle(self, ctx: CommandCtx[, path: XPath][, r]) }`;
> context menus are `#[derive(UserCommand)]` / `#[derive(MessageCommand)]` unit structs;
> the Module registers them with `.add::<X>()`. `response = public | ephemeral` and
> `auto_defer = true | false` are command arguments. The examples below use that form.
> **This release** still takes free functions: `#[command("path", ..)] async fn x(ctx:
> CommandCtx[, path][, r][, opts: XOptions])` with `#[derive(Options)] struct XOptions`,
> the bare `ephemeral` flag, `opens_modal`, and `Module::add(x())`. The parameter-order
> and argument errors quoted below are this release's; Story 6.10 replaces `ephemeral`
> and `opens_modal`, and Story 6.11 rewrites the rest for the struct form (SKILL.md §8
> maps every name).

## 1. Command Paths

```rust
#[derive(SlashCommand)] #[command("dashboard", description = "Open the dashboard")]          // 1 word  → command
pub struct Dashboard;
#[derive(SlashCommand)] #[command("setup start", description = "Start the setup")]           // 2 words → subcommand of /setup
pub struct SetupStart;
#[derive(SlashCommand)] #[command("settings logs channel", description = "Set the log channel")] // 3 words → group "logs" + subcommand "channel"
pub struct LogsChannel { #[option(description = "Where")] pub channel: Id<ChannelMarker> }
```

Words are separated by exactly one ASCII space. `noctis::rules::command::PathKind::of(path)`
tells the kind: `Command` (1 word), `Subcommand` (2), `GroupSubcommand` (3), `None` otherwise;
`.words()` gives 1/2/3. Subcommand and group nodes come only from paths: nothing sets
`required` on them (OPT-14).

The handler forms: a path takes `(ctx: CommandCtx[, r: CommandResponder][, options:
O])`, a pattern with a group takes `(ctx: CommandCtx, path: <Fn>Path[, r:
CommandResponder][, options: O])` (§2), with `O` a `#[derive(Options)]` struct (§3) and
the responder as in `responder.md` §4, in exactly this order. Any other parameter list is
a compile error (`` `#[command("ping")]` handlers take `(ctx: CommandCtx[, r:
CommandResponder][, options: O])` with `O: Options`, in this order ``); a non-Options type
as the last parameter is E0277 `` `String` is not an options struct ``. The handler
returns `HandlerResult<R>` with `R` a `V2Message`, `Components`, a `LegacyMessage` or the
`Responded` from `acked.done()` (`responder.md` §1): a returned message is the reply
(or, after `defer`/`reply`, an edit of the original). `HandlerResult<()>` is E0277 `` `()`
is not a Command response ``: a command must answer. Doc comments stay rustdoc and never
become the description.

```rust
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

/// Rustdoc only.
#[derive(SlashCommand)]
#[command("setup start", description = "Start the setup", response = ephemeral)]
pub struct SetupStart;

impl SetupStart {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Components> {
        let _ = ctx.command_path(); // "setup start"
        Ok(Components::new().push(TextDisplay::new("Let's set things up.")))
    }
}
```

`response = ephemeral` makes every response private, the auto-defer included (INT-10,
`responder.md` §2b and §3); `CommandHandler::ephemerality()` reads it. This release
spells it as the bare flag `ephemeral`.

[planned] Extractors as further parameters, and `ctx.respond(render_x(..))` as the
default answer (Story 8.1; `views-and-recipes.md`).

Register it: `Module::new("setup").add(setup_start())` (the macro turns the fn into a
descriptor constructor `fn setup_start() -> CommandHandler`; call it with `()`).
`CommandHandler` has `name()` (`module_path::fn`), `pattern()` (the literal),
`description()`, `paths()` (every Command Path, expansion order) and `options()` (the
options struct's `OPTIONS`, empty without one).

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
`description`, `max_expansions`, `ephemeral`, `opens_modal` and the metadata arguments
`permissions`, `contexts`, `integration_types`, `nsfw` and `guilds`. Every argument marked
[planned] is a compile error today (`` `#[command]` argument `rate_limit` is planned and
not supported yet ``); an unknown one lists the landed names (`` unknown `#[command]`
argument `foo`: expected `description`, `max_expansions`, `ephemeral`, `opens_modal`,
`permissions`, `contexts`, `integration_types`, `nsfw` or `guilds` ``), and each argument
may appear once (`` duplicate `#[command]` argument `nsfw` ``). Story 6.10 replaces `ephemeral` with `response = …` and `opens_modal` with
`auto_defer = …` in these lists, with no aliases.

| Argument | Meaning |
| --- | --- |
| `description = "…"` | the description (a string literal); required. [planned] optional with the `i18n` feature, then it must match the file |
| `max_expansions = n` | Command Pattern cap (integer literal ≥ 1, default 25) |
| `locales = "<dir>"` | [planned] where this command's default-locale file lives when it isn't the crate's `i18n!` dir (Module catalogs) |
| `response = public \| ephemeral` | [planned: Story 6.10; today the bare flag `ephemeral`, `` `ephemeral` takes no value `` otherwise] how the command answers (default public). Fixed at registration (INT-10); `reply`, `defer_reply()`, a returned message and the auto-defer use it; `response = update` is a compile error (INT-04) |
| `auto_defer = true \| false` | [planned: Story 6.10; today the bare flag `opens_modal`] default `true`. `false` disables the auto-defer and is required to open a modal: the handler must take `r: CommandResponder<Manual>` (today `CommandResponder<OpensModal>`) and may call `r.show_modal(m)` (today `r.modal(m)`); a mismatch is a compile error citing `[INT-05]` (`responder.md` §2b): the flag with a plain `CommandResponder` (`` [INT-05] `opens_modal` is declared, so the handler must take `CommandResponder<OpensModal>` ``), `CommandResponder<OpensModal>` without the flag (`` [INT-05] this handler takes `CommandResponder<OpensModal>`, so its `#[command]` must declare `opens_modal` ``), or the flag without a responder parameter (`` [INT-05] `opens_modal` handlers take `r: CommandResponder<OpensModal>` to send the modal ``). `CommandHandler::opens_modal()` reads it. See `responder.md` §4 |
| `permissions = MANAGE_GUILD \| BAN_MEMBERS` | `default_member_permissions` (META-01): `noctis::Permissions` names joined by `\|`, a path's last segment counts (`Permissions::MANAGE_GUILD`). The names are twilight's (`noctis::rules::command::PERMISSIONS`, 51 of them: `USE_SLASH_COMMANDS`, not `USE_APPLICATION_COMMANDS`; `MANAGE_GUILD_EXPRESSIONS`, not the deprecated `MANAGE_EMOJIS_AND_STICKERS`). Admin-only: `ADMINISTRATOR` (there is no `NONE`). Errors: ``[META-01] permissions: limit a permission name of noctis::Permissions, actual `MANAGE_SERVER` `` (help ``use `MANAGE_GUILD` `` for a wrong case), and ``[META-01] `permissions` takes permission names joined by `\|` `` for any other syntax (`8`, `"X"`, `A + B`, `A \|\| B`) |
| `contexts = [Guild, BotDm, PrivateChannel]` | where it can be used (`noctis::InteractionContext` names; `InteractionContext::Guild` works): at least one, no duplicates, declared order kept (`` unknown context `Gild`; expected one of Guild, BotDm, PrivateChannel ``, `` duplicate context `Guild` ``) |
| `integration_types = [GuildInstall, UserInstall]` | install types (`noctis::IntegrationType`), same shape; META-04: must be enabled on the app |
| `nsfw = true` | age-restricted: `true` or `false`, anything else is `` [CMD-07] `nsfw` takes `true` or `false` `` |
| `guilds = [123456789012345678]` | guild-scoped: published only to these guilds. Integer literals only (no consts or paths), non-zero, distinct, `u64` (`42u64` works). `contexts`/`integration_types` are then a compile error: ``[META-03] contexts: limit only on global commands, actual `[Guild]` `` with `= note: scope: guild 42` (the first id). Dev-guild publishing comes from `--publish-guild` |
| `rate_limit(user, 3, per = "10s")` | [planned] see `modules-middleware-ratelimits.md` |
| `flags = ["beta"]` | [planned] Boot Flags |
| `apps = ["main"]` | [planned] publish only for these Apps (multi-App) |

There is no `dm_permission` (`` [META-05] `dm_permission` is deprecated by Discord and not
supported; use `contexts = [Guild]` to keep a command out of DMs ``). An argument left out
stays unset and is omitted when publishing, so Discord applies its own default (FR-11).

Read the result with `handler.metadata() -> &noctis::CommandMetadata` (`#[non_exhaustive]`,
`Copy`, pub fields `default_member_permissions: Option<Permissions>`, `contexts:
Option<&'static [InteractionContext]>`, `integration_types: Option<&'static
[IntegrationType]>`, `nsfw: Option<bool>`, `guilds: &'static [Id<GuildMarker>]` (empty =
global); `CommandMetadata::new()` = all unset, by-value `const` setters of the field names,
`is_guild_scoped()`):

```rust
#[derive(SlashCommand)]
#[command("ban", description = "Ban a member", permissions = BAN_MEMBERS, contexts = [Guild])]
pub struct Ban;
impl Ban {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<LegacyMessage> { Ok(LegacyMessage::new("Banned.")) }
}

let handler = Ban::descriptor(); // [planned: Story 6.11]; this release: `ban()` from a free fn
assert_eq!(handler.metadata().default_member_permissions, Some(noctis::Permissions::BAN_MEMBERS));
assert_eq!(handler.metadata().nsfw, None); // unset, omitted on Publish
```

`permissions`, `contexts`, `integration_types` and `nsfw` belong to the **base** command
(META-02): on a pattern they apply to every path the handler declares, and there is no
per-path form (`` [META-02] `permissions` applies to the base command of every path this
handler declares; Discord keeps it only on the base command, so it has no per-path form ``
for `permissions(set = …)`). Every leaf under one base must agree, across all Modules, or
`App::build` fails with a `BuildViolation::Command` (META-01, META-02, CMD-07, REG-04,
FR-9; see §6). Without an explicit base declaration **unset is a value**: a leaf with
`permissions = MANAGE_GUILD` and another without conflict. Declare a base's description and
metadata once with `Module::base`, a subcommand group's description with `Module::group`:

```rust
use noctis::{BaseMeta, Permissions};

pub fn module() -> Module {
    Module::new("config")
        .add(config_set())                 // "config set prefix", "config set welcome"
        .base(
            "config",
            BaseMeta::new()
                .description("Configure the bot")            // = cmd.config.description
                .default_member_permissions(Permissions::MANAGE_GUILD),
        )
        .group("config set", "Change a setting")          // = cmd.config.set.description
}
```

- `BaseMeta` (`noctis::BaseMeta`, `#[non_exhaustive]`, `Copy`, `Default`): pub fields
  `description: Option<&'static str>`, `default_member_permissions: Option<Permissions>`,
  `contexts: Option<&'static [InteractionContext]>`, `integration_types:
  Option<&'static [IntegrationType]>`, `nsfw: Option<bool>`; `BaseMeta::new()` and by-value
  `const` setters of the same names. No `guilds`: scope comes from the handlers.
- `Module::base(name, BaseMeta) -> Module` and `Module::group(path, description) -> Module`
  (both `#[must_use]`, kept in call order, any Module may declare them). They are checked at
  `App::build` after merging: a set metadata field is the explicit base value (unset leaves
  inherit it, set leaves must match); two Modules with different descriptions for one node
  are REG-04 (`` `a` declares `A` ``, `` `b` declares `B` ``); a path no handler registers is
  REG-04 `metadata only for a registered command path`; a description outside 1-100
  characters is CMD-04 (note `` declared by `b` ``); a `BaseMeta` metadata field on a path
  below a base is META-02 (``help: declare it on `config` ``); a path of more than 3 words
  is CMD-10; `contexts`/`integration_types` on a base whose handlers are guild-scoped is
  META-03. The values apply in every scope the node exists in.
- Description fallback (FR-57): a base or group with no handler at its exact path and no
  declared description takes the description of a leaf below it: the leaves of the
  lexicographically smallest Module id (byte order), then the smallest path, then the first
  registered. The fallback counts toward CMD-05. When the build passes it logs one warning
  per scope and node (target `noctis::registry`): ``no description declared for `config`
  (scope: global): using the description of `config set` from Module `a`; declare one with
  `Module::base` or `Module::group` `` (guild scopes print `scope: guild 42`). Declare every
  base and group description to keep the log clean (see `src/modules/setup.rs`). Metadata
  never falls back: only descriptions do.

## 2. Command Patterns

One handler for several paths. `(a|b)` gives one alternative per word; `?` after a group
also yields the path without it. Expansion is left to right in declaration order.

```rust
#[derive(SlashCommand)]
#[command("config (set|get) (prefix|welcome)", description = "Configure the bot")]
pub struct Config;

impl Config {
    #[handler]
    async fn handle(self, ctx: CommandCtx, path: ConfigPath) -> HandlerResult<Components> {
        let msg = match path {
            ConfigPath::SetPrefix | ConfigPath::SetWelcome => { … }
            ConfigPath::GetPrefix | ConfigPath::GetWelcome => { … }
        };
        Ok(msg)
    }
}
```

A pattern with at least one group generates the path enum `<Struct>Path` next to the
struct (this release: `<Fn>Path` next to the fn); a plain path generates none.

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
#[command("config (set|get) (prefix|welcome)?", description = "Configure the bot")]
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
| `validate_merge(&[CommandDecl], &[ContextMenuDecl], &Limits) -> Result<(), Vec<Violation>>` | the whole App (what `App::build` runs): per `Scope` (first appearance, commands then menus) `validate_tree` with a `= note: scope: global` / `scope: guild 42` (always the last note), then META-03 per command, then per context-menu command (input order; path `` `<path or name>.contexts` `` / `.integration_types`, notes `` declared by `m1` `` then the scope), then CTX-01 per context-menu name (path = the name, same notes), then counts (REG-01 global / REG-02 guild; paths `chat-input commands`, `user commands`, `message commands`; a merged base counts once; meta decls and empty paths don't count; note `` registered by `a`, `b` `` naming the counted sources), then REG-03 for two context-menu commands with the same type and exact name |
| Module notes | when any involved `source` is non-empty, each violation names its sources before the scope note: CMD-13 children and CMD-05 `` contributed by `a`, `b` `` (distinct subtree sources, input order; CMD-05's after `longest variant per field`), CMD-13 options, CMD-10 and CMD-04 `` declared by `a` `` (CMD-04: the declaration that gave the description, or the first handler when missing), REG-01/REG-02 `` registered by `a`, `b` ``; `""` prints `(unnamed)`, and all-`""` sources add no note |
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

A command's options are the fields of its `#[derive(SlashCommand)]` struct, read through
`self` (this release: a separate `#[derive(Options)]` struct taken as the handler's last
parameter). Each field is one option, in declaration order (never reordered).

```rust
use noctis::prelude::*; // SlashCommand, Options (trait), Choices, CommandOption, MessageKey
use noctis::twilight::model::id::{Id, marker::ChannelMarker};
use noctis::ui::{ResolvedAttachment, ResolvedUser};

#[derive(SlashCommand, Debug)]
#[command("mod warn", description = "Warn a member")]
pub struct Warn {
    /// Rustdoc only: never the option's description.
    #[option(description = "Who to warn")]
    pub member: ResolvedUser,                                  // required (not Option)
    #[option(description = "How bad it is")]
    pub severity: Severity,                                    // STRING with choices
    #[option(description = "How long, in days", min_value = 1, max_value = 30, validate = positive)]
    pub days: Option<i64>,                                     // optional
    #[option(description = "Why", min_length = 3, max_length = 200)]
    pub reason: Option<String>,
    #[option(description = "Where to log", channel_types = [GuildText, GuildAnnouncement])]
    pub log_to: Option<Id<ChannelMarker>>,
    #[option(description = "Which rule", autocomplete)]          // see §4
    pub rule: Option<String>,
    #[option(description = "Evidence", file_types = ["image", ".pdf"])]
    pub evidence: Option<ResolvedAttachment>,
    #[option(description = "Do not notify", rename = "silent")]
    pub quiet: Option<bool>,
}

fn positive(days: &i64, _ctx: &CommandCtx) -> Result<(), MessageKey> {
    if *days > 0 { Ok(()) } else { Err(MessageKey::new("mod.warn.days.positive")) }
}

impl Warn {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Components> { /* self.member, self.days, … */ }
}
```

### `#[option(..)]` arguments

Each at most once (`` duplicate `#[option]` argument `description` ``), one `#[option]` per
field; unknown names list the valid ones (`min`/`max` say `` use `min_value` ``).

| Argument | Takes | Option types | Rules |
| --- | --- | --- | --- |
| `description = "…"` | string literal, 1–100 chars (UTF-16 counted); **required**: a field without one is CMD-04 with `` help: add `#[option(description = "…")]` `` | all | CMD-04 |
| `rename = "…"` | string literal; the option name (default: field name, `r#` stripped) | all | CMD-01..03, CMD-06 |
| `min_value = n`, `max_value = n` | unsuffixed integer or float literal, optionally negated (`-5`, `1.5`); an integer outside `i64` fails at expansion | INTEGER (`i64`, integers only, ±(2^53−1)), NUMBER (`f64`, ±2^53) | OPT-03..05 |
| `min_length = n`, `max_length = n` | unsuffixed integer; `min_length` 0–6000, `max_length` 1–6000, min ≤ max | STRING (`String`) | OPT-06 |
| `channel_types = [GuildText, …]` | twilight `ChannelType` variant names (`GuildText`, `Private`, `GuildVoice`, `Group`, `GuildCategory`, `GuildAnnouncement`, `AnnouncementThread`, `PublicThread`, `PrivateThread`, `GuildStageVoice`, `GuildDirectory`, `GuildForum`, `GuildMedia`; a path's last segment counts, `ChannelType::GuildText` works); at least one | CHANNEL | OPT-07 |
| `file_types = ["image", ".pdf"]` | string literals: `image`, `video`, `audio` or `.` + ASCII letters/digits; ≤ 10 | ATTACHMENT | OPT-13 |
| `autocomplete` | [planned: Story 6.11] a bare flag, paired with the struct's `#[autocomplete(field)]` method returning `Suggestions<T>` (§4); one without the other is an OPT-11 compile error. This release (Story 6.7): `autocomplete = path` names an `#[autocomplete]` fn for this options struct (§4b); it sets the flag and links the handler (no Module registration), and anything else is E0277 `` `autocomplete = …` is not an `#[autocomplete]` handler for `O` `` | STRING, INTEGER, NUMBER; not with a Choices type | OPT-10, OPT-11 |
| `validate = path` | `fn(&T, &CommandCtx) -> Result<(), MessageKey>` (`T` = the field type without `Option`; E0308 on another signature); sync, called only when the value is present, in field order, after the whole struct decoded | all | — |

### Field types (`CommandOption`, OPT-02)

| Rust type | Discord option |
| --- | --- |
| `String` | STRING (3) |
| `i64` | INTEGER (4) (no other integer types) |
| `f64` | NUMBER (10) |
| `bool` | BOOLEAN (5) |
| `noctis::ui::ResolvedUser { user, member: Option<InteractionMember> }` / `Id<UserMarker>` | USER (6) |
| `noctis::ui::ResolvedChannel` / `Id<ChannelMarker>` | CHANNEL (7) |
| `noctis::ui::ResolvedRole` / `Id<RoleMarker>` | ROLE (8) |
| `noctis::ui::Mentionable` (`User(ResolvedUser)` or `Role(ResolvedRole)`; user wins) / `Id<GenericMarker>` | MENTIONABLE (9) |
| `noctis::ui::ResolvedAttachment` / `Id<AttachmentMarker>` | ATTACHMENT (11) |
| enum with `derive(Choices)` | STRING with choices |
| `Option<T>` (last path segment `Option`, one argument) | the same, not required |

Any other type (`u8`, `Vec<String>`, `Option<Option<i64>>`, `&str`) is E0277
`` [OPT-02] `u8` is not a command option type `` with a note listing these. `Id` types decode
from the id alone; `Resolved*` / `Mentionable` look the id up in the interaction's resolved
data (`ResolvedUser::from_resolved(&resolved, id)` etc. do the same by hand).

### Compile errors

All expansion errors are reported at once. Rules that do not depend on the field type are
normal errors spanned on the literal or field: CMD-04 (description), CMD-01..03 (name; an
empty `rename = ""` reports path `options[i]`), CMD-13 (> 25 fields, on the struct),
OPT-01 (a required field after an optional one, on the field, ``note: after optional `a` ``),
CMD-06 (duplicate names, on the later field, `note: declared at positions 0, 1`), OPT-13
entries and count. Rules that depend on it are `const _` items over
`<T as CommandOption>::KIND` and fail as:

```text
error[E0080]: evaluation panicked: [OPT-05] days.min_value: limit min_value / max_value only on INTEGER or NUMBER options, actual `STRING`
```

These are OPT-03/04 (bound ranges), OPT-05 (misuse, a float bound on `i64`, `min_value ≤
max_value`), OPT-06, OPT-07, OPT-11, OPT-13 misuse, OPT-08 (a Choices type with more than 25
variants: `` [OPT-08] p.choices: limit at most 25, actual `more than 25` ``, note `` `Big` has
more variants than a command option takes ``) and OPT-10 (Choices plus `autocomplete`).
Several problems on one field each get their own error.

### Runtime decoding

`noctis::CommandInput::from_json(&[u8])` walks the APPLICATION_COMMAND interaction:
`path()` (`"mod warn"`, subcommands and groups followed), `get(name) -> Option<&OptionValue>`
(the leaf options), `resolved()`. `OptionValue` is `#[non_exhaustive]`: `String`, `Integer(i64)`,
`Number(f64)`, `Boolean`, `User(Id)`, `Channel(Id)`, `Role(Id)`, `Mentionable(Id<GenericMarker>)`,
`Attachment(Id)`; `.kind() -> OptionType`. Unknown options and `focused` are ignored.

The struct is decoded first (field order; first error wins), then the validators run, then
the handler. Decoding never panics; every failure is a `HandlerError` wrapping a typed
error (downcast with `err.as_error().downcast_ref::<OptionError>()`), and the handler does not run:

| `noctis::OptionError` (`#[non_exhaustive]`) | `Display` |
| --- | --- |
| `Malformed { reason }` | `malformed command interaction: <reason>` (bad JSON, numeric snowflakes, …) |
| `NotSlashCommand { interaction_type, command_type }` | `not a slash command: interaction type 3, command type 1` |
| `Duplicate { option }` | ``option `days` is sent twice`` |
| `Missing { option }` | ``required option `member` is missing`` |
| `WrongType { option, expected, found }` | ``[OPT-02] option `days`: expected INTEGER, found STRING`` |
| `OutOfRange { option, value }` | ``[OPT-03] option `days`: 9223372036854775808 is not an i64`` |
| `Unresolved { option, id }` | ``option `member`: id 42 is not in the resolved data`` |
| `UnknownChoice { option, value }` | ``option `severity`: `zzz` is not one of its choices`` |

A failing validator gives `noctis::InvalidOption` (`option() -> &'static str`, `key() ->
MessageKey`; ``option `days` failed validation: mod.warn.days.positive``). The handler does
not run; the user gets its key as a localized ephemeral notice, and it is a user condition
(a `debug` log, no Error Sink; `err.user_message() == Some(invalid.key())`). A decode
failure (`OptionError`) shows `noctis.validation.failed` and does reach the Command Error
Sink. See `responder.md` §5.
Discord enforces declared bounds itself; Noctis does not re-check `min_value`, `max_length`,
`channel_types` at runtime (use a validator for custom checks).

Traits: `Options` (`const OPTIONS: &[OptionDecl]`, `from_input(&CommandInput)`,
`validate(&self, &CommandCtx)`), `CommandOption` (`const KIND: OptionType`, `const CHOICES:
&[Choice]`, `decode`). Write them with the derives; `OPTIONS[i].description.default` is the
literal.

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
| `CHANNEL_TYPES` / `channel_type(name) -> Option<u8>` | the 13 `(name, value)` pairs `channel_types = [..]` accepts (`("GuildText", 0)` … `("GuildMedia", 16)`) and an exact-name lookup (`channel_type("GuildForum") == Some(15)`) |
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

### Choices

```rust
#[derive(Choices, Debug, Clone, Copy)]
pub enum Severity {
    #[choice("Low")]  Low,                        // value "Low" (the variant name)
    #[choice("High", value = "hi")] High,
}
```

One derive serves modals and commands: it implements `Choices` and `CommandOption` (KIND
STRING, `CHOICES` = `[("Low", "Low"), ("High", "hi")]`, decoding by value; an unknown value is
`OptionError::UnknownChoice`). Labels and values are 1–100 chars, reported as `` [OPT-09] High:
limit at most 100, actual 101 `` with notes `in the choice value` and `the same limit applies
to modal and select options (MOD-07, CV2-09)`; an empty `value = ""` and descriptions stay
MOD-07. The enum may have more than 25 variants (modal selects), but as an option field it
fails OPT-08. Command choices are STRING only: integer/number choices are [planned].
[planned] Localized choice names from `cmd.<path>.opt.<opt>.choice.<value>` (Story 11.4).

## 4. Autocomplete

[planned, Story 6.11] Mark the option with the bare `autocomplete` flag and add
the paired `#[autocomplete(field)]` method to the command struct's `impl`. The method
returns the suggestions; Noctis sends them. You never call `respond` in autocomplete.

```rust
#[derive(SlashCommand)]
#[command("warn", description = "Warn a member", response = ephemeral)]
struct Warn {
    #[option(description = "Who to warn")]
    user: ResolvedUser,
    #[option(description = "Which rule", autocomplete)]
    rule: i64,
}

impl Warn {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { /* self.rule: re-check it exists */ }

    #[autocomplete(rule)]
    async fn rule(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<i64>> {
        // Filter and cap in the query (LIMIT 25), not after loading every row.
        let rules = ctx.state::<Db>().search_rules(ctx.guild_id()?, typed, 25).await?;
        Ok(rules.into_iter().map(|r| (r.id, r.name)).collect())
    }
}
```

The flag and the method pair up, checked at `cargo check`:

| You write | Result |
| --- | --- |
| `autocomplete` on a field, no `#[autocomplete(field)]` method | `[OPT-11]` compile error |
| `#[autocomplete(f)]` where `f` has no `autocomplete` flag, or no field `f` exists | `[OPT-11]` compile error |
| `autocomplete` on a `Choices` field | `[OPT-10]` compile error (choices or autocomplete) |
| `autocomplete` on a type other than `String`, `i64`, `f64` (or their `Option`) | `[OPT-11]` compile error |
| `Suggestions<T>` with `T` other than the field's type without `Option` | `[OPT-11]` compile error |
| `auto_defer` or `response` on `#[autocomplete]`, or a `self` parameter | `[INT-06]` compile error (autocomplete has no defer) |

The method's parameters, always these three, in this order:

- `ctx: AutocompleteCtx`: `guild_id()` returns `Result<Id<GuildMarker>, HandlerError>` (an
  error outside a guild, so `?` answers an empty list), `state::<T>()` returns `State<T>`
  (registered with `.state(..)`), plus `user_locale()`, `cache()`, `modules()` and
  `limits()`. No
  `respond`, no `defer`, no Responder (INT-04, INT-06).
- `typed: &str`: the raw focused input. Discord sends it as a string even for INTEGER and
  NUMBER options, and it may be partial (`""`, `"-"`, `"1"`). Parse it yourself
  (`typed.parse::<i64>().ok()`), never unwrap.
- `filled: Partial<Self>`: every other option as `Option<_>`, untrusted and partially
  filled; Discord may omit even required options (OPT-12). Use it to narrow the search
  (`filled.user`), and re-validate the final values in `handle`: autocomplete values
  are not constrained to your suggestions.

The return value, `HandlerResult<Suggestions<T>>`:

- `Suggestions<T>` collects from `(T, label)` pairs; the label is a `String`, a `&str` or
  a `MessageKey` (resolved in the user's locale): `.map(|r| (r.id, r.name)).collect()`.
- More than 25 suggestions: the first 25 are sent and a warning is logged (INT-07, in
  every mode). Cap the query at 25 instead.
- A label over 100 characters is cut to 100 with `…` and a warning. A value Discord would
  reject (a `String` over 100 characters, an INTEGER outside ±2^53, a non-finite NUMBER)
  is dropped with a warning.
- An error, a panic or more than 3 s sends an empty list and routes the error to the
  Autocomplete Error Sink. Keep the work to one bounded query; there is no defer.
- Registering the command (`.add::<Warn>()`) registers its autocomplete methods too.

### 4b. This release (Story 6.7): the shipped autocomplete shape

Until Story 6.11 converts it to the form above, autocomplete is a free function linked
from the options struct. Sending already follows the rules above (25 cap with a warning,
empty list and the Autocomplete Error Sink on failure), except that an over-long choice
name is dropped rather than truncated and the budget is 2.5 s from receipt.

```rust
use noctis::prelude::*; // autocomplete, AutocompleteCtx, AutocompleteChoice, Partial

#[derive(Options)]
pub struct PrefixOptions {
    #[option(description = "The new prefix", autocomplete = prefix_suggest)]
    pub value: String,
    #[option(description = "How many suggestions")]
    pub limit: Option<i64>,
}

#[autocomplete]
async fn prefix_suggest(ctx: AutocompleteCtx, partial: Partial<PrefixOptions>) -> HandlerResult<Responded> {
    let typed = ctx.focused().value().to_owned();          // raw text typed so far
    let limit = usize::try_from(partial.limit.unwrap_or(5)).unwrap_or(0); // every field is Option
    Ok(ctx.respond(                                         // sync: no `.await`
        ["!", "?", "."]
            .into_iter()
            .filter(|p| p.starts_with(&typed))
            .take(limit)
            .map(|p| AutocompleteChoice::new(p, p)),        // (name, value)
    ))
}

#[command("config (set|get) prefix", description = "Prefix settings")]
async fn config(ctx: CommandCtx, path: ConfigPath, opts: PrefixOptions) -> HandlerResult<Components> { … }

let module = Module::new("config").add(config()); // no `.add(prefix_suggest())`
```

- `#[autocomplete]` turns `async fn name(ctx: AutocompleteCtx[, partial: Partial<O>]) ->
  HandlerResult<Responded>` into `fn name() -> AutocompleteHandler<O>`; link it with
  `#[option(autocomplete = name)]` on a field of `O`. It is **not** added to a Module:
  the Command carries it. Without `partial` the handler fits any options struct
  (`name::<O>()`). Other signatures fail: `` `#[autocomplete]` needs an `async fn` ``,
  `` … cannot be generic ``, `` `#[autocomplete]` handlers take `(ctx: AutocompleteCtx[,
  partial: Partial<Options>])` ``, `` `#[autocomplete]` takes no arguments ``; any return
  type other than `HandlerResult<Responded>` is E0271 at the return type.
- `AutocompleteCtx` has exactly one response, `respond(self, choices) -> Responded`:
  synchronous, infallible, consuming the ctx (INT-04, INT-06). There is no `defer`, no
  `reply` and no Responder (`ctx.defer()` is E0599). Accessors: `command_path()`,
  `module()`, `shard()`, `guild_id()`, `channel_id()`, `user_id()`, `user_locale()`,
  `guild_locale()`, `focused()`, `state::<T>()`, `http()`.
- `ctx.focused()` is the `Focused` option being typed in: `name()`, `kind()` (`OptionType`),
  `value()` (the raw text, exactly as sent; unvalidated, so an INTEGER option may hold
  `"-"`), `as_integer() -> Option<i64>`, `as_number() -> Option<f64>` (finite only).
- `Partial<O>` is the `<Name>Partial` struct `#[derive(Options)]` generates (here
  `PrefixOptionsPartial`, which implements `PartialOptions` and is `Options::Partial`):
  every field is `Option<T>`, because Discord may omit any
  option, required ones included (OPT-12; `let n: i64 = partial.limit` is E0308).
  Decoding never fails: a missing, mistyped, unresolved or unknown-choice value is
  `None`, and `validate = path` validators do not run. Re-validate in the Command handler:
  the final value need not be one of your suggestions.
- `AutocompleteChoice::new(name, value)`: `name` 1–100 characters, `value` an
  `AutocompleteValue` from `&str`/`String` (STRING), `i64` (INTEGER or NUMBER) or `f64`
  (NUMBER), matching the option's type.
- Noctis sends the first 25 choices; more is truncated with one warning on target
  `noctis::autocomplete` (`handler`, `returned`, `sent`), in every limit mode (INT-07).
  Each kept choice Discord would reject (OPT-09 name or value length, wrong value type,
  OPT-03 / OPT-04 number range) is dropped, with one `noctis::autocomplete` warning
  naming the count and the rule ids; the rest are still sent.
- The handler has **2.5 s** from when the interaction arrived (fixed). On an error, a
  panic, the deadline (`AutocompleteError::DeadlineMissed`) or shutdown, Noctis answers
  an empty list and routes the failure to the Autocomplete Error Sink
  (`ErrorOrigin::Autocomplete`); the user sees no error. An unknown Command or an option
  without a linked handler also gets an empty list, with a `noctis::dispatch` warning.
- Exactly one type-8 callback goes out per autocomplete interaction; Noctis sends it,
  never your code.

Rule function: `noctis::rules::command::validate_autocomplete(&choices, OptionType::String,
&limits) -> Vec<Violation>` checks a choice list the way Noctis checks returned
suggestions; use it for lists you send outside a `#[autocomplete]` method. More than
`autocomplete_choices` (25) choices is one INT-07 violation at path `choices`
(`help: Noctis sends the first 25 choices and logs this as a warning`): the caller
truncates and warns, in every mode. Each kept choice (`choices[j]`) follows OPT-09 (name
1–100 and localized names, value type, string value ≤ 100) and OPT-03 / OPT-04 for
numbers; dropped choices are not checked. On an option type other than STRING, INTEGER or
NUMBER the only violation is OPT-11 (`autocomplete only on STRING, INTEGER or NUMBER
options`). You need it only for choices built outside `respond` (for example to test a
suggestion list).

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

## 5. Context-menu commands

```rust
use noctis::prelude::*; // user_command, message_command
use noctis::ui::{Components, ResolvedMessage, ResolvedUser, TextDisplay};

#[derive(UserCommand)]
#[user_command("Show profile", contexts = [Guild], response = ephemeral)]
pub struct ShowProfile;

impl ShowProfile {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Components> {
        let _ = ctx.command_path(); // "Show profile"
        let _ = target.member; // None when the target is not a member of the guild, including outside a guild
        Ok(Components::new().push(TextDisplay::new(format!("<@{}>", target.user.id))))
    }
}

#[derive(MessageCommand)]
#[message_command("Report message")]
pub struct ReportMessage;

impl ReportMessage {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedMessage, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.defer_reply_as(Ephemerality::Ephemeral).await?;          // this release: r.defer(..)
        acked.edit_reply(Components::new().push(TextDisplay::new("Reported."))).await?;  // this release: edit_original
        let _ = target.message.content;
        Ok(acked.done())
    }
}

let module = Module::new("mod").add::<ShowProfile>().add::<ReportMessage>();
```

- The derive builds a `noctis::ContextMenuHandler` descriptor for `Module::add::<X>()`
  (this release: the macro turns a free fn into `fn show_profile() ->
  noctis::ContextMenuHandler` for `Module::add`). `ContextMenuHandler` has `name()` (`module_path::fn`), `menu_name()`,
  `kind() -> noctis::MenuKind` (`User` / `Message`, from the macro: CTX-03; `.value()` 2 / 3,
  `.name()` `"USER"` / `"MESSAGE"`), `metadata() -> &CommandMetadata` and
  `ephemerality() -> Ephemerality`.
- Arguments: the name literal first (raw strings work; `` `#[user_command]` needs the menu
  name as its first argument: `#[user_command("Show profile")]` `` / `` … needs its menu
  name as a string literal ``), then the metadata arguments of §1 (`permissions`,
  `contexts`, `integration_types`, `nsfw`, `guilds`, same rules and errors, META-03 included)
  and `response = public | ephemeral` (this release: the bare flag `ephemeral`).
  `auto_defer` (this release: `opens_modal`, landed for `#[command]` only), `key = "…"`
  (localization key), `rate_limit(..)`, `flags` and `apps` are [planned] here; `max_expansions` and other names are
  unknown arguments.
- Name 1–32 chars, UTF-16 counted, mixed case, spaces, apostrophes and emoji allowed
  (CTX-01: `` [CTX-01] limit between 1 and 32, actual 33 `` on the literal; reported together
  with a META-03 error). No description and no options (CTX-02): `description = "…"` is
  `` [CTX-02] context-menu commands have no description ``, an extra parameter is
  `` [CTX-02] context-menu commands take no options: … ``. The handler is exactly
  `(ctx: CommandCtx, target: ResolvedUser[, r: CommandResponder])` (or `ResolvedMessage`
  for `#[message_command]`) returning `HandlerResult<R>` like a slash Command (`R` a
  `V2Message`, `Components`, `LegacyMessage` or `Responded`; `()` is E0277). [planned]
  Localized as a whole: `ctxcmd.<key>.name` (with `key`, Epic 11); `ctx.respond(..)`
  (Story 8.1).
- Check a dynamic name with `noctis::rules::command::validate_context_menu_name(name,
  &limits) -> Vec<Violation>`: length only (`discord_len`, 1 to
  `Limits::context_menu_name_len`), path `""`, CTX-01 `Between{1,32}` / `Count(n)`;
  `"Show Profile"`, `"It's"`, `"Report 🚩"` pass. `validate_merge` runs it on every menu.
- Targets: `noctis::ui::ResolvedUser { user, member: Option<InteractionMember> }` (the
  same type as a USER option) and `noctis::ui::ResolvedMessage { message }` (twilight
  `Message`; `ResolvedMessage::from_resolved(&resolved, id)`). The target is looked up by the
  interaction's `target_id` in its resolved data. When that fails the handler does not run
  and the call returns a `HandlerError` wrapping `noctis::MenuTargetError` (`#[non_exhaustive]`):
  `Malformed { reason }` (`malformed context-menu interaction: …`), `NotContextMenu {
  interaction_type, command_type }`, `WrongKind { expected, found }` (`expected a USER
  command, got a MESSAGE command`), `Unresolved { id }` (`target 99 is not in the resolved
  data`). Another menu name is `UnknownCommandPath` (exact byte match).
- Same name + same type twice is a `BuildError` (REG-03). Limits: 15 user and 15
  message commands globally (REG-01).

## 6. Merging across Modules

Commands sharing a base word merge into one Discord command (tescord semantics): Module
`a` with `"config set"` and Module `b` with `"config get prefix"` give one `/config` with
`set` and `get prefix`. Merging is per scope: global commands and each guild of `guilds =
[..]` are separate (a guild `config set` and a global `config get` never merge or
conflict). `App::build` lowers every mounted Module's `#[command]` paths (one declaration
per path and scope, `source` = the Module id), context menus and `Module::base` /
`Module::group` declarations, and runs `noctis::rules::command::validate_merge` once with
the default `Limits` (see §2 "Rule functions"). Every broken rule becomes one
`BuildViolation::Command(Violation)` in the single `BuildError`, all at once, never "first
wins": after the setter/plugin violations and `[GW-05]` `MissingIntent`, before the
crypto/runtime checks. Its `Display` is the violation (`[META-01] config: limit …` with
`= note:` lines); the notes name the Modules (`(from …)`, `registered by`, `declared by`,
`contributed by`) and end with `scope: global` / `scope: guild <id>`.

It fails when: metadata conflicts (META-01/02, CMD-07), a path is registered twice (REG-04,
also the same descriptor added twice to one Module), two Modules declare different
base/group descriptions (REG-04), a base has a handler and subcommands (CMD-12), a
subcommand is also a group (CMD-11), the merged tree breaks CMD-13 (25 children per level)
or CMD-05 (8000 characters, including fallback descriptions), a scope exceeds 100
chat-input, 15 user or 15 message commands (REG-01 global, REG-02 per guild; a merged base
counts once), or two context menus of the same type share a name in one scope (REG-03,
byte equality; user and message menus and chat-input commands never clash). Different
scopes never conflict.

### Metadata rules

| Item | What it is |
| --- | --- |
| `CommandMeta<'a>` | `#[non_exhaustive]`, `Default`; pub fields `default_member_permissions: Option<u64>` (raw permission bits), `contexts: Option<&[InteractionContext]>`, `integration_types: Option<&[IntegrationType]>`, `nsfw: Option<bool>`; `None` = unset. `CommandMeta::new()` plus by-value `const` setters of the same names; no `dm_permission` (META-05: use `contexts`) |
| `InteractionContext` | `Guild` (0), `BotDm` (1), `PrivateChannel` (2); `InteractionContext::ALL`, `.value()`, `.name()` (variant name); also `noctis::InteractionContext` |
| `IntegrationType` | `GuildInstall` (0), `UserInstall` (1); `IntegrationType::ALL`, `.value()`, `.name()`; also `noctis::IntegrationType` |
| `PERMISSIONS` / `permission(name) -> Option<u64>` | the 51 `(name, bit)` pairs `permissions = …` accepts (twilight's `Permissions` names in declaration order, no deprecated aliases) and an exact, case-sensitive lookup (`permission("MANAGE_GUILD") == Some(32)`) |
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

See `app-and-sharding.md` §3. `cargo run --release -- --publish-guild <id>` in
development (instant; every Command mirrored into that guild, without
`contexts`/`integration_types`), `--publish` in production (global Commands globally,
`guilds = [..]` Commands only to their guilds). `--dry-run` prints the JSON, the diff
and the create count and writes nothing. Publish writes only when the canonical remote
commands differ (REG-05), checks META-04 against the application's enabled install
types before any write, and never runs on shard READY. Programmatic:
`app.publish(PublishScope::Global).dry_run(true).await?` (`noctis::publish`).

The JSON comes only from the Registry: one object per base and per context menu;
`"options": []` on every option-less command, subcommand and group; set metadata exactly
as declared (`default_member_permissions` as the bit string, `contexts` and
`integration_types` as integers in declared order, `nsfw` only when set); unset fields
are omitted; option fields only when set (`required: true`, `choices`, `min_value` /
`max_value` as integers for integer bounds, `min_length` / `max_length`,
`channel_types`, `autocomplete: true`, `file_types`). Menus carry `type` and `name` only
(plus metadata). Localizations arrive with Epic 11.

[planned] Boot Flags (Story 13.2), per-guild Module enablement (Story 13.3) and
`apps = [...]` filter what is published. Disabled Modules: only their guild-scoped
commands are left out of a guild publish; global commands stay published and answer
"disabled here".

## 8. Handler context (`CommandCtx`)

`CommandCtx` is `Clone` (one `Arc`) and `Debug` (no HTTP client). Available now:

| Accessor | Returns |
| --- | --- |
| `command_path()` | `&'static str`: the invoked Command Path (`"config set prefix"`), or the menu name (`"Show profile"`) |
| `module()` | `&'static str`: the id of the Module the handler was added to |
| `shard()` | `u32`: the shard that received the interaction |
| `guild_id()` | `Option<Id<GuildMarker>>` |
| `channel_id()` | `Option<Id<ChannelMarker>>` |
| `user_id()` | `Id<UserMarker>`: the invoking user |
| `user_locale()` / `guild_locale()` | `Option<&str>`: Discord's `locale` / `guild_locale` |
| `response_locale()` | `&str`: ephemeral → User, then Guild Locale; public → Guild, then User Locale; else `"en-US"` (`responder.md` §3) |
| `ephemerality()` | `Ephemerality`: the declared one |
| `state::<T>()` | `Option<State<T>>`: state registered with `AppBuilder::state` |
| `http()` | `&Arc<twilight_http::Client>`: the App's one client |

Interactions resolve by default (unlocalized) names only (LOC-06): `data.name` and the
subcommand names, in the scope Discord sends (`data.guild_id` → that guild, else global;
no fallback between them). An unknown name logs `unknown command` on target
`noctis::dispatch` and sends nothing. The span `noctis.interaction` records `module` and
`path`. Calling a handler directly from bot tests is [planned]; never use
`noctis::__private`.

[planned] Shared accessors (trait `Ctx`, also on every other context): `user()`,
`kv()` (your key-value data), `cache()` (Discord resources), `modules()`,
`rate_limits()`, `render(..)`, `gateway()`, `app_permissions()`, `member()`.
Command-only: `attachment_size_limit()`.
