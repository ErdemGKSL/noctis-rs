---
title: 'Story 6.3: Command metadata and context-menu Commands'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: '80622f9587d1e931bf3ae6af820bee782ad41b22'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
warnings: ['oversized']
deferred:
  - summary: >-
      PERMISSIONS mirrors twilight-model 0.17.1, so Discord permission bits twilight lacks (1 << 48, SET_VOICE_CHANNEL_STATUS) cannot be declared in `permissions = ..`.
    evidence: |-
      crates/noctis-rules/src/command/meta.rs jumps from SEND_VOICE_MESSAGES (1 << 46) to SEND_POLLS (1 << 49); twilight-model 0.17.1 defines no flag at 1 << 48. The table is pinned to twilight by design (spec Design Notes), so the gap comes from the dependency, not this story. Settle by documenting the gap in docs/discord-rules.md META-01 or adding a raw-bits form when twilight or a later story adds the flag.
    location: >-
      crates/noctis-rules/src/command/meta.rs:125
    severity: low
  - summary: >-
      CTX-01 checks only length, so a menu name with leading or trailing whitespace (" padded ") compiles; if Discord trims stored names, the byte-exact name match at dispatch never matches.
    evidence: |-
      Matrix 31 makes " padded " compile and call_context_menu compares `input.name != handler.menu_name` byte for byte. Unverified: settle by registering a USER command named " padded " against Discord and checking whether the returned and invoked `data.name` is trimmed; if it is, add a CTX-01 whitespace check (an intent change to Matrix 31).
    location: >-
      crates/noctis-macros/src/menu.rs (check_name); crates/noctis-core/src/menu.rs (call_context_menu)
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** A `#[command]` cannot yet carry `default_member_permissions`, `contexts`, `integration_types`, `nsfw` or a guild scope: the macro rejects these arguments as "planned". `#[user_command]` and `#[message_command]` do not exist yet. Epic 1 already built the rule functions (`CommandMeta`, `validate_meta`, `validate_context_menu_name`, `MenuKind`), but no macro calls them (FR-11, FR-12; AD-26; META-01, META-02, META-03, META-05, CMD-07, CTX-01, CTX-02, CTX-03).

**Approach:**
- A shared metadata-argument parser in `noctis-macros` lowers the metadata arguments into a `const` `noctis_core::CommandMetadata`.
  - Permission names are checked against a new `noctis_rules::command::PERMISSIONS` table (META-01).
  - `validate_meta` runs when `guilds` is given (META-03).
- `#[command]` gains these arguments and exposes the result as `CommandHandler::metadata()`.
- New `#[user_command("Name", ..)]` and `#[message_command("Name", ..)]` macros:
  - they check the name with `validate_context_menu_name` (CTX-01) and accept the same metadata arguments;
  - each turns `async fn(ctx: CommandCtx, target: ResolvedUser | ResolvedMessage) -> HandlerResult<()>` into a `ContextMenuHandler` descriptor for `Module::add`.
- At runtime the descriptor decodes the interaction's `target_id` from `resolved` into the typed target, or returns a `MenuTargetError`.

## Boundaries & Constraints

**Always:**
- Macros call `noctis-rules` with `Limits::default()` and never copy a rule constant (AD-3). Permission names and bits come from `PERMISSIONS`; context and integration names come from the new `InteractionContext::ALL` / `IntegrationType::ALL` and `.name()`.
- Generated code names runtime items only through `::noctis::__private::{core, rules, model}` and `::core` (AD-2). Add `pub use twilight_model as model;` to `noctis::__private`.
- Generated metadata is a `const` item, so its slices are `'static`. The macro emits `.__with_metadata(..)` only when at least one metadata argument is given. Without one, `metadata()` is `CommandMetadata::new()` and the 6.1/6.2 token output is unchanged.
- Unset fields stay `None` (or an empty `guilds`), never a default (FR-11). Lists keep their declared order.
- `#[command]` keeps 6.1's two-tier error model:
  - argument and signature errors fail fast;
  - META-03 violations join the pattern/tree violations in one combined error, after them.
- The context-menu macros report CTX-01 and META-03 together in one combined error.
- No `unwrap`/`expect`/`panic` on user input in macro or library code. Runtime decoding returns `MenuTargetError` and never panics.
- Each public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill), re-blesses `xtask/public-api.txt` and updates the Skill's `API hash`, in the same commit. Fixture and test names cite their rule id.

**Never:**
- No App-build folding, merge or META-01/02/CMD-07 cross-Module conflicts, and no REG-03 duplicate menus (6.4).
- No Publish JSON and no META-04 (6.9). No dispatch from the gateway, Responder or auto-defer (6.5, 6.6).
- No `key`/locale for menus (Epic 11). It stays a "planned" argument error.
- No Module-level scope, no `BaseMeta`, no `Module::base` (6.4 or later).
- No admin-only `NONE` permission form. No `dm_permission` (META-05). No const paths or expressions in `guilds`. No Primary Entry Point menus (CTX-03).
- No options, description or `max_expansions` on context menus (CTX-02). No trybuild `pass` suites. No debug builds.

## I/O & Edge-Case Matrix

"Error" means a compile error whose text contains the quoted parts. `M` is `#[command("ping", description = "d", …)]` on `async fn ping(ctx: CommandCtx) -> HandlerResult<()>`. `U` is `#[user_command("Show profile", …)]` on `async fn show(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<()>`. Error messages from `#[user_command]` say `#[message_command]` when that macro is used.

### Metadata arguments (shared by `#[command]`, `#[user_command]`, `#[message_command]`)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 1 | AC example | `M` with `permissions = MANAGE_GUILD \| BAN_MEMBERS, contexts = [Guild], nsfw = true` | compiles. `metadata().default_member_permissions == Some(Permissions::MANAGE_GUILD \| Permissions::BAN_MEMBERS)`, `contexts == Some(&[Guild])`, `nsfw == Some(true)`, `integration_types == None`, `guilds` empty |
| 2 | No metadata | 6.1/6.2 forms | `metadata() == CommandMetadata::new()` (all `None`, `guilds` empty). The macro token output contains no `__with_metadata` |
| 3 | Single permission | `permissions = ADMINISTRATOR` | `Some(Permissions::ADMINISTRATOR)` (bits 8) |
| 4 | Path form | `permissions = Permissions::MANAGE_GUILD \| noctis::Permissions::KICK_MEMBERS` | accepted (last path segment) |
| 5 | Every name | all 51 `PERMISSIONS` names joined by `\|` | compiles; equals `Permissions::all()` (boundary) |
| 6 | Duplicate permission | `MANAGE_GUILD \| MANAGE_GUILD` | accepted; bits 32 |
| 7 | META-01 unknown | `permissions = MANAGE_SERVER` | `[META-01] permissions: limit a permission name of noctis::Permissions, actual `MANAGE_SERVER``, `= help: see noctis::rules::command::PERMISSIONS for every name`, spanned on the ident |
| 8 | META-01 wrong case | `permissions = manage_guild` | as row 7 with actual `` `manage_guild` `` and `= help: use `MANAGE_GUILD`` |
| 9 | META-01 deprecated alias | `MANAGE_EMOJIS_AND_STICKERS` | unknown (row 7). Only the 51 non-deprecated twilight names exist |
| 10 | META-01 syntax | `permissions = 8`; `= "MANAGE_GUILD"`; `= A + B`; `= A \|\| B`; `= \| A`; `= A \|`; `permissions =`; bare `permissions` | ``[META-01] `permissions` takes permission names joined by `|`, such as `permissions = MANAGE_GUILD | BAN_MEMBERS` `` |
| 11 | META-02 per-path form | `permissions(set = MANAGE_GUILD)`, also `contexts(..)`, `integration_types(..)`, `nsfw(..)`, `guilds(..)` | ``[META-02] `permissions` applies to the base command of every path this handler declares; Discord keeps it only on the base command, so it has no per-path form`` |
| 12 | Contexts | `contexts = [Guild, BotDm, PrivateChannel]`; `[InteractionContext::BotDm]`; trailing comma | `Some([Guild, BotDm, PrivateChannel])` in declared order; path form accepted |
| 13 | Contexts bad | `contexts = Guild`; `= []`; `= [Gild]`; `= [Guild, Guild]` | ``` `contexts` takes a list such as `contexts = [Guild, BotDm, PrivateChannel]` ```; ``` `contexts` lists at least one context ```; ``` unknown context `Gild`; expected one of Guild, BotDm, PrivateChannel ```; ``` duplicate context `Guild` ``` |
| 14 | Integration types | `[GuildInstall, UserInstall]` / bad forms as row 13 | same shape: `integration type`, `expected one of GuildInstall, UserInstall`, ``` `integration_types` lists at least one integration type ``` |
| 15 | nsfw | `nsfw = true` / `nsfw = false` | `Some(true)` / `Some(false)` |
| 16 | CMD-07 | `nsfw = 1`; `= "true"`; bare `nsfw`; `= yes` | ``[CMD-07] `nsfw` takes `true` or `false` `` |
| 17 | Guilds | `guilds = [123456789012345678, 42u64]` | `guilds == [Id::new(123456789012345678), Id::new(42)]`, other fields unset |
| 18 | Guilds bad | `guilds = 1`; `= []`; `= [0]`; `= [-1]`; `= [18446744073709551616]`; `= [DEV]`; `= [1, 1]` | ``` `guilds` takes a list of guild id integer literals, such as `guilds = [123456789012345678]` ``` (non-list, negative, overflow, path); ``` `guilds` lists at least one guild id ```; ``` guild ids are never 0 ```; ``` duplicate guild id `1` ``` |
| 19 | Guilds u64::MAX | `guilds = [18446744073709551615]` | compiles |
| 20 | META-03 | `guilds = [42], contexts = [Guild]` | `[META-03] contexts: limit only on global commands, actual `[Guild]``, `= note: scope: guild 42`, spanned on `contexts` |
| 21 | META-03 both | `contexts = [BotDm, Guild], integration_types = [UserInstall], guilds = [7, 8]` (any argument order) | two errors, `contexts` (actual `[Guild, BotDm]`, sorted by `validate_meta`) then `integration_types`; note `scope: guild 7` (first id) |
| 22 | META-03 ok | `guilds = [42], permissions = MANAGE_GUILD, nsfw = true` | compiles |
| 23 | META-03 with pattern errors | `#[command("Ping", guilds = [1], contexts = [Guild])]` (no description) | one combined error: CMD-03, CMD-04, then META-03 |
| 24 | META-05 | `dm_permission = false` (any form) | ``[META-05] `dm_permission` is deprecated by Discord and not supported; use `contexts = [Guild]` to keep a command out of DMs `` |
| 25 | Duplicate argument | `nsfw = true, nsfw = false` | ``duplicate `#[command]` argument `nsfw` `` |
| 26 | Unknown argument | `#[command]` with `foo = 1` / `default_member_permissions = X` | ``unknown `#[command]` argument `foo`: expected `description`, `max_expansions`, `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds` `` |
| 27 | Still planned | `#[command]` with `ephemeral`, `opens_modal`, `rate_limit(..)`, `flags`, `apps`, `locales` | ``argument `ephemeral` is planned and not supported yet`` (6.1 text) |
| 28 | Metadata on pattern | `#[command("config (set\|get)", description = "d", permissions = MANAGE_GUILD)]` | compiles; `metadata()` applies to the handler (every path) |

### Context-menu macros

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 29 | User command | `U` | `fn show() -> ContextMenuHandler`: `kind() == MenuKind::User`, `menu_name() == "Show profile"`, `name()` ends with `::show`, `metadata() == CommandMetadata::new()` |
| 30 | Message command | `#[message_command("Quote")]` on `async fn quote(ctx: CommandCtx, target: ResolvedMessage)` | `kind() == MenuKind::Message` |
| 31 | CTX-01 boundary | 32 × `a`; 16 × `😀` (UTF-16 32); `"x"`; `"Show Profile"`; `"It's"`; `"Report 🚩"`; `" padded "` | compile (length only; case, spaces, apostrophes and emoji are free) |
| 32 | CTX-01 over | 33 × `a` | `[CTX-01] limit between 1 and 32, actual 33`, spanned on the name literal |
| 33 | CTX-01 unicode over | 17 × `😀` | `[CTX-01] … actual 34` |
| 34 | CTX-01 empty | `""` | `[CTX-01] limit between 1 and 32, actual 0` |
| 35 | CTX-01 and META-03 together | 33 × `a`, `guilds = [1], contexts = [Guild]` | one combined error: CTX-01, then META-03 |
| 36 | Metadata on menus | `U` plus `permissions = KICK_MEMBERS, contexts = [Guild], integration_types = [GuildInstall], nsfw = false` | `metadata()` carries every field |
| 37 | Name argument errors | `#[user_command]`; `#[user_command(name = "x")]`; `#[user_command(NAME)]`; `#[user_command(b"x")]` | ``` `#[user_command]` needs the menu name as its first argument: `#[user_command("Show profile")]` ``` for the first two; ``` `#[user_command]` needs its menu name as a string literal ``` for the last two; raw strings are accepted |
| 38 | CTX-02 description | `#[user_command("X", description = "d")]` | ``[CTX-02] context-menu commands have no description `` |
| 39 | CTX-02 options | `async fn f(ctx: CommandCtx, target: ResolvedUser, opts: O)` | ``[CTX-02] context-menu commands take no options: `#[user_command]` handlers take `(ctx: CommandCtx, target: ResolvedUser)` `` |
| 40 | Planned / unknown | `key = "show_profile"`; `ephemeral`; `opens_modal`; `rate_limit(..)`; `flags`; `apps`; `max_expansions = 2`; `foo` | ``` `#[user_command]` argument `key` is planned and not supported yet ``` (first six); ``unknown `#[user_command]` argument `max_expansions`: expected `permissions`, `contexts`, `integration_types`, `nsfw` or `guilds` `` |
| 41 | Param count and type | 0 or 1 params; `self`; `target: ResolvedMessage` on `#[user_command]`; `target: Id<UserMarker>` | ``` `#[user_command]` handlers take `(ctx: CommandCtx, target: ResolvedUser)` ``` (for a message command: `ResolvedMessage`). The type check is syntactic (last path segment); rustc's bound on the constructor enforces the real type |
| 42 | Signature shape | not `async`, `const`, `unsafe`, `extern`, generic, variadic | 6.1's messages with `#[user_command]` / `#[message_command]` in place of `#[command]` |
| 43 | Wrong return type | `-> HandlerResult<u8>` | rustc type error at the generated constructor call (return-type span); `tests/types/` fixture |
| 44 | Docs and visibility | `pub(crate)` with `/// doc` | descriptor keeps the visibility, the docs and the `#[cfg]` attributes; no `missing_docs` warning |

### Runtime (`__testing::call_context_menu(handler, json)`)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 45 | User target with member | type 2, `data {type: 2, name: "Show profile", target_id: "U", resolved {users {U}, members {U}}}` | the handler receives `ResolvedUser { user, member: Some(..) }`; `ctx.command_path() == "Show profile"` |
| 46 | User target without member (DM) | as row 45, with no `members` | `member: None` |
| 47 | Message target | `data {type: 3, name: "Quote", target_id: "M", resolved {messages {M}}}` | the handler receives `ResolvedMessage { message }` with `message.id == M` |
| 48 | Target missing | `target_id` not in `users`/`messages`, or `resolved` absent | `MenuTargetError::Unresolved { id }`, ``target 99 is not in the resolved data``; handler not run |
| 49 | Wrong kind | user handler + `data.type 3` (and the reverse) | `WrongKind { expected: User, found: Message }`, `expected a USER command, got a MESSAGE command` |
| 50 | Not a context menu | `data.type` 1 or absent; `data.type` 4; interaction `type` 3 | `NotContextMenu { interaction_type, command_type }`, `not a context-menu command: interaction type 2, command type 1` (absent counts as 1) |
| 51 | Name mismatch | `"Show Profile"` sent to the `"Show profile"` handler | `UnknownCommandPath`, ``"`Show Profile` is not a path of the Command `<module>::show`"`` (exact byte match) |
| 52 | Unicode name | handler and JSON both `"Profili göster 🚀"` | runs |
| 53 | Malformed | invalid JSON; a non-object; missing `type`; `data` not an object; missing `data`; non-integer `data.type`; missing `data.name`; missing `data.target_id`; `target_id` `"0"`/`"abc"`/`true`; bad `resolved` | `Malformed { reason }`, Display `malformed context-menu interaction: <reason>`. Reasons: `invalid JSON: …`, `the interaction is not an object`, ``missing interaction `type` ``, `` `data` is not an object ``, ``missing `data` ``, `` `data.type` is not an integer ``, ``missing `data.name` ``, ``missing `data.target_id` ``, ``bad `data.target_id`: …``, ``bad `resolved` data: …`` |
| 54 | target_id as number | `"target_id": 123` | accepted (twilight `Id` deserializes both) |
| 55 | Handler error | the handler returns `Err` | passed through unchanged |

### Rules additions (unit)

| # | Scenario | Expected |
|---|----------|----------|
| 56 | `PERMISSIONS` table | 51 entries, names unique, bits unique and single-bit. Every entry equals twilight's `Permissions::from_name(name).bits()` (core test `meta_01_permission_table_matches_twilight`). The OR of all bits is `Permissions::all().bits()` |
| 57 | `permission(name)` | `"MANAGE_GUILD"` → `Some(32)`; `"manage_guild"`, `""`, `"MANAGE_EMOJIS_AND_STICKERS"` → `None` |
| 58 | `ALL` and `MenuKind` | `InteractionContext::ALL == [Guild, BotDm, PrivateChannel]`, `IntegrationType::ALL == [GuildInstall, UserInstall]`; `MenuKind::User.value() == 2`, `Message.value() == 3`, `.name()` `"USER"`/`"MESSAGE"` |
| 59 | `CommandMetadata::to_rules` | maps permission bits to `u64`, keeps the lists and `nsfw` as they are; `scopes()` is `[Global]` without guilds, else one `Scope::Guild(id)` per guild in order |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/command/meta.rs` -- `InteractionContext`/`IntegrationType` (`value`, `name`). `CommandMeta` (`default_member_permissions: Option<u64>`). `validate_meta(meta, Scope, limits)` emits the META-03 Display used by rows 20-21. Add `ALL`, `PERMISSIONS`, `permission()` and the tests here.
- `crates/noctis-rules/src/command/mod.rs` -- `MenuKind` (L349; add `value`/`name`), `ContextMenuDecl`, `Scope`, re-exports (L134-145), module docs (`# Metadata`, `# Context menus`).
- `crates/noctis-rules/src/command/name.rs` L87 -- `validate_context_menu_name`: CTX-01 with path `""`, measured with `discord_len` (UTF-16).
- `crates/noctis-rules/src/rule.rs` -- `Violation::new/with_note/with_help`, Display (L269): `[ID] path: limit …, actual …`, then the `= note:`/`= help:` lines. `RuleId::{META_01, META_02, META_05, CMD_07, CTX_02}` exist.
- `crates/noctis-macros/src/command.rs`:
  - `PLANNED` (L26): remove `permissions`, `contexts`, `integration_types`, `nsfw` and `guilds`.
  - `Args`/`parse_args` (L42-170).
  - `segments`/`span_of`/`arg_name`/`value_of`: share them with the new modules, either through `common.rs` or `pub(crate)`.
  - `check_shape` (L173): parametrize the macro name.
  - `check_pattern` (L240) and `report` (L281): append the META-03 violations here.
  - `expand` (L480): add `.__with_metadata(..)`.
  - Unit tests (L578+): style for exact-string asserts.
- `crates/noctis-macros/src/lib.rs` L166-200 -- the attribute entry points; add `user_command` and `message_command`. Update the crate doc ("Implemented so far").
- `crates/noctis-macros/src/common.rs` -- `error`, `combine`.
- `crates/noctis-core/src/command.rs`:
  - `CommandCtx::new(path)` (pub(crate), `&'static str`): use it for menus;
  - `CommandHandler` (fields L133, Debug L326);
  - `unknown()`/`failed()` helpers;
  - the `call_command*` seams (L342-370) to mirror.
- `crates/noctis-core/src/options.rs` L298-372 -- `CommandInput::from_json`: the JSON-walk style and the Malformed reasons to mirror. `empty_resolved()` (L372).
- `crates/noctis-core/src/module.rs` -- the `Sealed`/`Descriptor` impls, `commands` plus `commands()` with `expect(dead_code)`, and `Debug`. Add `menus` the same way.
- `crates/noctis-core/src/lib.rs` -- re-exports (L125-143), `__testing` (L147).
- `crates/noctis-ui/src/modal/resolved.rs` -- `ResolvedUser` (L36, `from_resolved` L116) is the user target. Add `ResolvedMessage` next to it, plus the re-export in `noctis-ui/src/lib.rs` L271.
- twilight-model 0.17.1:
  - `guild::Permissions` (bitflags 2.13; `from_bits_retain`/`union` are const, `from_name`, `all()`; `MANAGE_EMOJIS_AND_STICKERS` is the deprecated alias of `MANAGE_GUILD_EXPRESSIONS`, bit 30);
  - `Id::new` is `const fn` (panics on 0);
  - `InteractionDataResolved.messages: HashMap<Id<MessageMarker>, Message>`; see `src/application/interaction/resolved.rs` L184 for a message JSON fixture;
  - `CommandData.target_id: Option<Id<GenericMarker>>`.
- `crates/noctis/src/lib.rs`:
  - root `pub use noctis_core::{…}` (L297);
  - the `command` macro docs (L440-485);
  - prelude (L847-888);
  - `__private` (L892);
  - `ui` re-exports (L1391).
- `crates/noctis/tests/{commands.rs, options.rs, ui.rs, boundary.rs, boundary/cmd.rs}` -- style references. The UI suite globs `tests/ui/**/*.rs` and `tests/types/**/*.rs`.
- `xtask/rules-pending.toml` -- the five `story = "6.3"` entries: CMD-07, CTX-01, META-01, META-02, META-03 (all `diagnosed`).
- `xtask/src/rules_catalog/mod.rs` -- fixture layout: `<area>` = the id's area lowercased (`ui/meta/`, `ui/ctx/`, `ui/cmd/`), and `.stderr` must contain `[ID]`.
- `docs/discord-rules.md` -- rows CMD-07 (L84), CTX-01..03 (L132-134), META-01..05 (L135-139).
- `examples/project-template/`:
  - `src/modules/profile.rs` L48-64 (`TargetUser`, `key`, `contexts` marked [planned]);
  - `dashboard.rs` L76-80 and `setup.rs` L101-105 (`permissions`/`contexts` marked [planned]);
  - `.claude/skills/noctis/references/commands.md` §1 attribute table (L62-85) and §5 (L500-520);
  - `SKILL.md` L21 (API hash);
  - `AGENTS.md`.

## Tasks & Acceptance

**Execution:**

**`crates/noctis-rules/src/command/meta.rs`, `mod.rs`**
- Add `pub const ALL: [Self; 3]` to `InteractionContext` and `pub const ALL: [Self; 2]` to `IntegrationType`.
- Add `pub const PERMISSIONS: [(&str, u64); 51]`: the twilight-model 0.17.1 `Permissions` names in declaration order, without `MANAGE_EMOJIS_AND_STICKERS`.
- Add `pub fn permission(name: &str) -> Option<u64>` (exact, case-sensitive).
- `MenuKind` gains `pub const fn value(self) -> u8` (2/3) and `pub const fn name(self) -> &'static str` (`"USER"`/`"MESSAGE"`).
- Re-export, document (module docs `# Metadata`) and unit-test Matrix 56 (the rules half: uniqueness, single bit), 57 and 58 (`meta_01_*`, `ctx_03_*` names).

**`crates/noctis-ui/src/modal/resolved.rs` (+ `lib.rs` re-export)**
- Add `#[derive(Debug, Clone, PartialEq)] #[non_exhaustive] pub struct ResolvedMessage { pub message: Message }`.
- Add `pub fn from_resolved(&InteractionDataResolved, Id<MessageMarker>) -> Option<Self>`, with a doctest.
- Update the crate-doc list.

**`crates/noctis-core/src/metadata.rs` (new)**
- `#[non_exhaustive] #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct CommandMetadata` with these pub fields:
  - `default_member_permissions: Option<Permissions>`;
  - `contexts: Option<&'static [InteractionContext]>`;
  - `integration_types: Option<&'static [IntegrationType]>`;
  - `nsfw: Option<bool>`;
  - `guilds: &'static [Id<GuildMarker>]`.
- Its functions:
  - `pub const fn new()`, plus by-value `const` setters with the field names;
  - `pub fn is_guild_scoped()`;
  - `pub(crate) fn to_rules(&self) -> noctis_rules::command::CommandMeta<'static>` and `pub(crate) fn scopes(&self) -> Vec<Scope>`, each `cfg_attr(not(test), expect(dead_code, reason = "Story 6.4 …"))`.
- Docs say: unset fields are omitted from Publish (6.9), and META-02 means the values belong to the base command.
- Unit tests for Matrix 59, plus `meta_01_permission_table_matches_twilight` (Matrix 56).

**`crates/noctis-core/src/menu.rs` (new)**
- `ContextMenuHandler` (Clone, opaque) with:
  - `name()`, `menu_name()`, `kind() -> MenuKind`, `metadata() -> &CommandMetadata`;
  - a `Debug` that prints name, menu_name, kind and metadata;
  - `#[doc(hidden)] pub fn __new_user<F: Fn(CommandCtx, ResolvedUser) -> Fut + Send + Sync + 'static, Fut>(name, menu_name, metadata, f)` and `__new_message` (with `ResolvedMessage`).
- `#[derive(Clone, Debug, PartialEq, thiserror::Error)] #[non_exhaustive] pub enum MenuTargetError` with the Matrix 48-53 variants and Displays: `Malformed { reason: String }`, `NotContextMenu { interaction_type: u64, command_type: u64 }`, `WrongKind { expected: MenuKind, found: MenuKind }`, `Unresolved { id: u64 }`.
- `pub(crate) fn call_context_menu(handler, json)` does, in order:
  1. decode the JSON (Malformed / NotContextMenu);
  2. check the kind (WrongKind);
  3. check the name, exact (`UnknownCommandPath`);
  4. resolve the target (Unresolved);
  5. run the handler with `CommandCtx::new(menu_name)`.
- Every error is wrapped in `HandlerError::new`.
- Unit tests for Matrix 45-55, named `ctx_03_*` where they concern the type.

**`crates/noctis-core/src/command.rs`, `module.rs`, `lib.rs`**
- `CommandHandler` gains `metadata: CommandMetadata` (default `new()`), `pub fn metadata(&self) -> &CommandMetadata` and `#[doc(hidden)] pub fn __with_metadata(self, CommandMetadata) -> Self`. Its `Debug` includes the metadata.
- `CommandCtx::command_path` docs: for a context menu it returns the menu name.
- `Module` gains `menus: Vec<ContextMenuHandler>`, `Sealed` + `Descriptor` for `ContextMenuHandler`, `pub(crate) fn menus()` (dead_code expect like `commands()`), and `menus` (menu names) in its `Debug`.
- `lib.rs`:
  - `pub use metadata::CommandMetadata; pub use menu::{ContextMenuHandler, MenuTargetError}; pub use twilight_model::guild::Permissions; pub use noctis_rules::command::{InteractionContext, IntegrationType, MenuKind};`;
  - `__testing::call_context_menu(&ContextMenuHandler, &[u8]) -> BoxFuture<'static, HandlerResult<()>>`;
  - a crate-doc paragraph.

**`crates/noctis-macros/src/meta.rs` (new)**
- `MetaArgs` and `parse_meta_arg(name, segment) -> Option<syn::Result<…>>`, covering Matrix 3-19 and 24.
- META-02 detection: the segment is `name` followed directly by a parenthesized group.
- `fn meta03(&MetaArgs) -> Vec<syn::Error>`: `validate_meta` with `Scope::Guild(first id)`, spanned on the argument name.
- `fn tokens(&MetaArgs) -> TokenStream`, the `const` block:
  ```rust
  { const M: ::noctis::__private::core::CommandMetadata = ::noctis::__private::core::CommandMetadata::new()
      .default_member_permissions(::noctis::__private::core::Permissions::from_bits_retain(40u64))
      .contexts(&[::noctis::__private::rules::command::InteractionContext::Guild])
      .guilds(&[::noctis::__private::model::id::Id::new(42u64)]); M }
  ```
- Unit tests with exact strings for every row in 3-27.

**`crates/noctis-macros/src/command.rs`**
- Wire `MetaArgs` into `parse_args`. Use the new unknown-argument text (row 26).
- Run META-03 after the pattern violations, in the same combined error.
- `expand` appends `.__with_metadata(#tokens)` to the constructor call when any metadata argument is given.
- Update the existing tests whose texts change.

**`crates/noctis-macros/src/menu.rs` (new) + `lib.rs` `#[proc_macro_attribute] user_command` / `message_command`**
- Implement Matrix 29-44.
- The output is `#(#attrs)* #vis fn name() -> ::noctis::__private::core::ContextMenuHandler { #inner #block <__new_user|__new_message>(concat!(module_path!(), "::", fn_name), "<menu name>", <metadata or CommandMetadata::new()>, __noctis_menu) }`, with the constructor `quote_spanned!` on the output span (as `command.rs` does).
- Unit tests.

**`crates/noctis/src/lib.rs`**
- `__private`: add `model`.
- Root: re-export `CommandMetadata, ContextMenuHandler, MenuTargetError, Permissions, InteractionContext, IntegrationType, MenuKind`.
- Add a documented `pub use noctis_macros::{user_command, message_command};`: arguments, handler forms, a doctest building both and adding them to a Module.
- `ui`: re-export `ResolvedMessage`.
- Prelude: add `user_command` and `message_command`.
- Extend the `command` macro docs with the metadata-argument table (syntax, rule ids, META-02 base semantics, unset = omitted).

**Facade tests**
- `crates/noctis/tests/menus.rs` (new) -- Matrix 29, 30, 36, 45-55 end to end through `noctis::__private::core::__testing::call_context_menu`; per-test statics (6.2 review lesson: no shared `WARNED`).
- `crates/noctis/tests/commands.rs` -- Matrix 1, 2, 3, 12, 15, 17, 28 via `metadata()`.
- `crates/noctis/tests/boundary/ctx.rs` + `meta.rs` (new), declared through `#[path]` in `boundary.rs`:
  - `ctx_01_32_chars`, `ctx_01_16_emoji`, `ctx_01_one_char`;
  - `meta_01_every_permission` (Matrix 5);
  - `meta_03_guild_without_contexts` (Matrix 22);
  - `meta_03_u64_max_guild` (Matrix 19).
- Fixtures (each a minimal `use noctis::prelude::*;` file with `fn main() {}`, plus its blessed `.stderr`):
  - `tests/ui/cmd/CMD-07_nsfw_not_bool.rs`;
  - `tests/ui/ctx/CTX-01_user_command_name_33.rs`, `CTX-01_message_command_empty_name.rs`, `CTX-02_user_command_description.rs`, `CTX-02_message_command_options_param.rs`;
  - `tests/ui/meta/META-01_unknown_permission.rs`, `META-02_per_path_permissions.rs`, `META-03_contexts_on_guild_command.rs`, `META-03_integration_types_on_guild_user_command.rs`, `META-05_dm_permission.rs`;
  - `tests/types/context_menu_returns_value.rs` (Matrix 43).
- Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`; existing `.stderr` files stay unchanged.

**Repo docs and tracking**
- `xtask/rules-pending.toml` -- delete the five `story = "6.3"` entries.
- `docs/discord-rules.md` -- update the rows below; the enforcement tiers stay the same:
  - CMD-07: `nsfw = true|false`, a non-bool is `[CMD-07]`.
  - CTX-01/02/03: `#[user_command]`/`#[message_command]`, no `description` argument or options parameter, typed target.
  - META-01: names from `noctis::rules::command::PERMISSIONS` (twilight names), `[META-01]` on an unknown name or bad syntax.
  - META-02: the per-path form is `[META-02]`.
  - META-03: `guilds = [..]` with `contexts`/`integration_types`.
  - META-05: the `dm_permission` argument is `[META-05]`.
- `examples/project-template/`:
  - `profile.rs`: `target: ResolvedUser` replaces `TargetUser`; `key = "show_profile"` is marked `// [planned]`; drop the `[planned]` marks on `contexts`.
  - `dashboard.rs`/`setup.rs`: drop the `[planned]` marks on `permissions`/`contexts`.
  - `commands.md`:
    - §1 table: landed syntax for `permissions`, `contexts`, `integration_types`, `nsfw = true`, `guilds = [<id literal>]`; `CommandMetadata`/`metadata()`; drop `DEV_GUILDS`; mark `BaseMeta` [planned].
    - §5: `ResolvedUser`/`ResolvedMessage`, `ContextMenuHandler`, `MenuTargetError`, `key` [planned].
  - `AGENTS.md`: one bullet on metadata and context menus.
  - `SKILL.md`: the `API hash`.
- `xtask/public-api.txt` -- re-bless.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the planning decisions are already logged; add implementation decisions there.

**Acceptance Criteria:**
- Given `#[command("ping", description = "d", permissions = MANAGE_GUILD | BAN_MEMBERS, contexts = [Guild], integration_types = [GuildInstall], nsfw = true)]`, when the crate compiles and `ping().metadata()` is read, then each field equals the declared typed value. Given `guilds = [42]` plus `contexts`, compilation fails with `[META-03]`. An unknown permission fails with `[META-01]` and a non-bool `nsfw` with `[CMD-07]`. No `dm_permission` argument exists (`[META-05]`).
- Given `#[user_command("Show profile")]` and `#[message_command("Quote")]`, when they compile, then a 33-character name fails with `[CTX-01]`, a `description` argument or an options parameter fails with `[CTX-02]`, and `kind()` is `User`/`Message` from the macro alone (CTX-03).
- Given a USER (or MESSAGE) command interaction JSON passed to `__testing::call_context_menu`, when the target is in `resolved`, then the handler receives `ResolvedUser` with `member` when present, or `ResolvedMessage`. A missing target, the wrong kind, a non-context-menu interaction or malformed JSON each return the matching `MenuTargetError` (downcastable from the `HandlerError`), never panic, and do not run the handler.
- Given `cargo run --release -p xtask -- rules-catalog`, when it runs, then it passes with the five 6.3 entries removed. Given the new public API, `api-snapshot` and `skill-check` pass, with commands.md documenting metadata and context menus and the template's "Show profile" user command using the real API.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 30 findings — high 0, medium 0, low 12, false 16, maybe-false 2
- findings:
  - `[low]` `[reject]` blind: `from_json` reads `data.type` before checking interaction `type`, so a non-command interaction with a non-integer `data.type` is Malformed, not NotContextMenu — real, but Discord never sends such a payload and the fix needs a separate branch for non-command interactions.
  - `[false]` `[reject]` blind: NotContextMenu reports a made-up command type 1 — Matrix 50 deliberately defines "absent counts as 1" and the `command_type: u64` field shape.
  - `[false]` `[reject]` blind: name mismatch reuses UnknownCommandPath — Matrix 51 and Tasks step 3 deliberately select UnknownCommandPath.
  - `[low]` `[reject]` blind: `path_last` ignores the qualifier (`contexts = [IntegrationType::Guild]`) — the value is resolved by its last segment as Matrix 4/12 specify, so the result matches the name; misqualified paths are rare and rejecting them adds per-argument prefix checks.
  - `[low]` `[defer]` blind: PERMISSIONS lacks Discord bit 1 << 48 (SET_VOICE_CHANNEL_STATUS) — confirmed twilight-model 0.17.1 has no flag there; the table is pinned to twilight by design, so the gap is the dependency's; deferred.
  - `[low]` `[patch]` blind: rustdoc says handlers sharing a base "must agree (checked at App build)" as current behaviour — confirmed at crates/noctis-core/src/metadata.rs:15 and crates/noctis/src/lib.rs:501 while `to_rules`/`scopes`/`menus()` are dead until 6.4; patch: mark the check as planned (Story 6.4).
  - `[maybe-false]` `[defer]` blind: CTX-01 accepts surrounding whitespace, so a padded name may never match at dispatch — depends on whether Discord trims context-menu names; Matrix 31 deliberately allows it; deferred (medium, unverified) with the Discord check that settles it.
  - `[low]` `[reject]` blind: `check_params` does not check that the first parameter is `CommandCtx` — the wrong type still fails to compile at the constructor bound (Matrix 41 leaves the real type check to rustc, as `#[command]` does); only the diagnostic is less direct, and the fix adds a check plus fixture.
  - `[false]` `[reject]` blind: `__with_metadata` vs constructor argument inconsistency — Boundaries mandate `.__with_metadata(..)` only when metadata is given so 6.1/6.2 token output stays unchanged; no harm named.
  - `[low]` `[reject]` blind: unreachable `WrongKind` arms and the `Target` enum in the erased closures — confirmed unreachable after `call_context_menu`'s kind check, but harmless defensive code; removing it is a restructure, not a direct correction.
  - `[low]` `[reject]` blind: `MenuInput::target(&self)` clones the resolved Message — real one clone per invocation, invisible to users; avoiding it needs a new take-by-value resolution path.
  - `[false]` `[reject]` blind: META-03 note names only the first guild — Matrix 21 deliberately specifies `scope: guild 7` (first id).
  - `[false]` `[reject]` blind: inconsistent receivers and non-const `is_guild_scoped` — spec Tasks define `pub fn is_guild_scoped()`; the type is Copy so receivers behave identically, and no caller needs a const context.
  - `[false]` `[reject]` blind: `Module` Debug lists menus by menu name — the Tasks deliberately specify "`menus` (menu names) in its `Debug`".
  - `[false]` `[reject]` blind: `metadata_argument_errors` omits `rate_limit` — `rate_limit(user, 3)` is already asserted planned at crates/noctis-macros/src/command.rs:820.
  - `[low]` `[reject]` blind: no trybuild fixtures for list-argument errors — the messages are asserted by exact-string unit tests through the same parser; spans through the real expansion are the only gap, and the fixture list is the spec's.
  - `[false]` `[reject]` blind: twilight-model types in the root public API — the spec mandates the `Permissions` re-export and the facade already exposes twilight-model types (`ResolvedUser.user`), so no new coupling class is added.
  - `[maybe-false]` `[defer]` edge: CTX-01 does not reject leading/trailing whitespace — same root cause and deferred entry as the blind whitespace finding.
  - `[low]` `[reject]` edge: any path prefix accepted for permissions/contexts — same evidence as the blind `path_last` finding.
  - `[low]` `[reject]` edge: a present but non-integer interaction `type` reports "missing interaction `type`" — real misleading reason, but Discord always sends an integer and the fix splits the match into a new branch.
  - `[low]` `[patch]` edge: docs say `member` is None only outside a guild, but it is also None when the target user is not a member of the guild — confirmed at crates/noctis/src/lib.rs:544 and the Usage Skill commands.md:538; patch: reword both (and the template comment).
  - `[false]` `[reject]` edge: `to_rules(self)` vs spec `&self` — `CommandMetadata` is Copy and `to_rules` is pub(crate), so behaviour is identical and no caller can diverge.
  - `[false]` `[reject]` intent D1: runtime target delivery only through the test seam — the spec's Never list excludes dispatch (6.5, 6.6) and the AC names `__testing::call_context_menu` as the surface.
  - `[false]` `[reject]` intent D2: registered menus and metadata are never read — Registry merge and Publish are excluded (6.4, 6.9) by the spec's Never list.
  - `[false]` `[reject]` intent D3: template `show_profile` keeps `key` and returns `HandlerResult<ProfileView>` — the template is a planned-API showcase (file header note), `key` is marked `[planned]` as the Tasks require, and the slash `profile` handler already used the same View return before this story.
  - `[false]` `[reject]` intent D4: `skill-check` passing is not shown in the diff — gates are run by the Verification step, not carried by a diff.
  - `[false]` `[reject]` intent D5: `guilds` narrowed to literals without a const-rejection test — deliberate spec decision (Never: no const paths) and `guilds = [DEV]` is asserted in crates/noctis-macros/src/meta.rs:606.
  - `[false]` `[reject]` intent D6: `nsfw` stored as `Option<bool>` — the spec deliberately keeps unset as `None` (FR-11).
  - `[false]` `[reject]` intent D7: META-03 only for attribute-local `guilds` — Module-level scope is excluded (6.4) by the spec's Never list.
  - `[low]` `[reject]` intent D8: no compile-fail fixtures for `permissions`/`nsfw` errors on menu macros — the menu macros call the same `parse_meta_arg` covered by unit tests; adding fixtures is not a direct correction and divergence is unlikely.

## Design Notes

**Separate descriptor (decision).** Context menus get their own `ContextMenuHandler`, not a `CommandHandler` kind:
- they have no paths, options or pattern;
- 6.4 validates them through `ContextMenuDecl` and 6.9 publishes them with their own type.
They reuse `CommandCtx` (`command_path()` = the menu name), so 6.5's Responder and pipeline serve both.

**Targets (decision).** The planned `TargetUser`/`TargetMessage` would duplicate `ResolvedUser` (same `user` + `member` shape). The handler takes `noctis_ui::ResolvedUser`, and the new `ResolvedMessage` joins the same family. The template and skill are reconciled to these names.

**Metadata syntax (decision; see memlog).**
- Permission names follow twilight's `Permissions` (so `USE_SLASH_COMMANDS`, not Discord's `USE_APPLICATION_COMMANDS`). They are validated against the rules table, and the core test pins the table to twilight.
- `guilds` takes literals only: the META-03 note needs a real id, and dev-guild publishing comes from the 6.9 CLI.
- Lists are non-empty and duplicate-free, because FR-11 publishes them as declared.
- There is no `NONE` form: write `ADMINISTRATOR` for admin-only.

**META-02 at compile time.** One `#[command]` sets one value for every path it expands to, so metadata cannot conflict within a handler. The only compile-time META-02 is the per-path form (`permissions(set = …)`), which the macro rejects with `[META-02]`. Conflicts across handlers stay App-build (6.4).

**Generated metadata.** Inside a `const` item, `&[Id::new(42)]` is promoted to `'static`; in a fn body it would be a temporary. The argument order in the attribute does not change the output: setters are emitted in field order.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis --test ui` (new `cmd/`, `ctx/`, `meta/` fixtures; existing `.stderr` unchanged), `boundary`, `menus`, `commands`
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok (no new crate edges)
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok, the five 6.3 entries gone
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot` -- expected: ok; copy the printed hash into SKILL.md (needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0; see AGENTS.md pitfalls)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok (MSRV)

## Auto Run Result

- **Summary:** Story 6.3 adds command metadata (`permissions`, `contexts`, `integration_types`, `nsfw`, `guilds`) to `#[command]` with compile-time rule checks (META-01/02/03/05, CMD-07), plus `#[user_command]`/`#[message_command]` context-menu handlers (own `ContextMenuHandler` descriptor, CTX-01/02 checks) that take `ResolvedUser` / new `ResolvedMessage` targets, a `__testing::call_context_menu` seam, and a `Permissions` re-export.
- **Files changed:** `crates/noctis-macros` (metadata parser, menu macros); `crates/noctis-core/src/metadata.rs` (CommandMetadata, permission table pinned to twilight); `crates/noctis/src/lib.rs` (menu descriptor, Module menus, test seam); `crates/noctis-ui/src/modal/resolved.rs` (ResolvedMessage); `crates/noctis/tests/*` (menus, commands, boundary, ui fixtures cmd/ctx/meta); `docs/discord-rules.md` and `xtask/rules-pending.toml` (rules moved to enforced); `examples/project-template/*` (code, AGENTS.md, Usage Skill + API hash); `xtask/public-api.txt` (re-blessed).
- **Review findings (30):** high 0, medium 0, low 12, false 16, maybe-false 2.
  - Patched (2, both low): rustdoc now marks cross-handler metadata agreement as planned (Story 6.4); `member` None semantics reworded (also None when target user is not a guild member) in lib.rs, Usage Skill, template.
  - Deferred (2 roots): PERMISSIONS lacks bit 1<<48 (absent in twilight-model 0.17.1); CTX-01 whitespace in names (needs Discord trimming check).
  - Rejected (26): see Review Triage Log for each reason (spec-mandated behaviour, Never-list scope for 6.4/6.5/6.9, harmless defensive code, unit-test coverage in place of fixtures).
- **Follow-up review recommended:** false (patched: high 0, medium 0, low 2).
- **Verification:** all local gates via gates.sh green (fmt, clippy -D warnings, workspace tests incl. trybuild ui, cargo hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** menus and metadata are not yet merged, validated across handlers, dispatched or published (6.4/6.5/6.9); whitespace-padded menu names may not match at dispatch; one missing permission bit pending twilight.
