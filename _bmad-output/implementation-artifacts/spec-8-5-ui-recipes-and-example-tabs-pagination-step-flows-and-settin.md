---
title: 'Story 8.5: UI recipes and example: tabs, pagination, step flows and settings screens'
type: 'feature'
created: '2026-10-22'
status: 'done'
baseline_revision: '78e7e2f28489ea5b1a6a7a7eaa464e971d81c84e'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/tooling-and-project-layout.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-49/FR-50 and AD-27 promise bot authors' agents compiling, hand-written examples of the UI Doctrine recipes (tabs, pagination, step flow, settings screen) plus a rulebook. Today the recipes in `views-and-recipes.md` are prose code against planned API (`State<Db>` handler parameters, `Strings`, `.default_values`, `ctx.values()`), nothing compiles or tests them, and `ctx.values()` (select values) does not exist, so no select handler can read what the user picked.

**Approach:** Add `ComponentCtx::values() -> &SelectedValues` to core; add a workspace example `examples/ui-recipes/` that implements the four recipes with shipped API only and tests them with `noctis::testkit`; make `views-and-recipes.md` show the example's code verbatim, enforced by a new `skill-check` `recipes` check; extend the doctrine lint to every `examples/*` crate.

## Boundaries & Constraints

**Always:**
- **`SelectedValues`** (new, `noctis-core/src/component.rs`, re-exported as `noctis_core::SelectedValues` and `noctis::SelectedValues`; not in the prelude): `#[derive(Debug, Clone, Default, PartialEq, Eq)]`, private fields; `strings(&self) -> &[String]` (every raw value as sent, any select kind), `users(&self) -> &[Id<UserMarker>]`, `roles(&self) -> &[Id<RoleMarker>]`, `channels(&self) -> &[Id<ChannelMarker>]`, all in sent order, duplicates kept. Population by component kind: string select → `strings` only; user / role / channel select → the value parsed as a non-zero `u64` id into that list; mentionable select → an id found in `resolved.roles` goes to `roles`, else one found in `resolved.users` or `resolved.members` goes to `users`, else typed lists skip it; a value that does not parse is kept in `strings` only; buttons and unknown kinds → empty typed lists. Built once in `read_input` from twilight's `MessageComponentInteractionData { values, resolved }`, stored in `CtxInner` (empty in `detached_parts`). `ComponentCtx::values(&self) -> &SelectedValues` only; `ModalSubmitCtx`/`ExpiredCtx` get nothing. Rustdoc with a doctest on `SelectedValues::default()`.
- **Example crate** `examples/ui-recipes/`: package `ui-recipes`, `version`/`edition`/`rust-version`/`license`/`repository` `.workspace = true`, `publish = false`, `[lints] workspace = true`; `[dependencies] noctis.workspace = true`, `tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }`; `[dev-dependencies] noctis = { workspace = true, features = ["testkit"] }`. Root `Cargo.toml` `members` gains `"examples/ui-recipes"`. No other dependency.
- Example layout (each `src/*.rs` file has a `//!` header, then a `// recipe: <name>` line, the whole module body, and a closing `// recipe-end: <name>` line):
  - `src/lib.rs` (recipe `app`): `pub mod pagination; pub mod settings; pub mod steps; pub mod store; pub mod tabs;` and `pub fn configure(builder: &mut AppBuilder, data: Data) -> &mut AppBuilder` registering `data.catalog`, `data.setup`, `data.tickets` with `.state(..)` and mounting `Module::new("tabs")`, `"pagination"`, `"steps"`, `"settings"`, each `.add::<X>()` of its structs.
  - `src/main.rs` (no region, `//!` header): `#[tokio::main] async fn main() -> Result<(), noctis::Error>`: `let mut builder = App::from_env()?; ui_recipes::configure(&mut builder, Data::sample()); builder.build()?.run().await`.
  - `src/store.rs` (recipe `store`): `Data { pub catalog: Catalog, pub setup: SetupStore, pub tickets: TicketStore }` + `Data::sample()` (catalog `Catalog::sample(300)`, empty stores); `Catalog(pub Vec<String>)` with `Catalog::sample(n)` (deterministic names `Item NNN …`, each ≤ `ITEM_NAME_MAX = 100` chars, lengths varying); `pub const ITEM_NAME_MAX: usize = 100`; `SetupStore` (a `Clone` handle over `Arc<Mutex<BTreeMap<Id<GuildMarker>, SetupConfig { log_channel, mod_role }>>>`, `save`, `get`); `TicketStore` (a `Clone` handle over `Arc<Mutex<..>>` of per-guild `Vec<TicketSystem { id: u32, name: String, enabled: bool, roles: Vec<Id<RoleMarker>> }>` plus a next-id counter; `systems(guild)`, `rename`, `set_enabled`, `set_roles`, `delete` → `bool` (false = not this guild's / gone), `add(guild) -> Option<u32>` (`None` at `MAX_SYSTEMS = 25`; new name `Ticket system <id>`, enabled, no roles), `TicketStore::with(guild, systems)` for tests); `pub fn missing<T>() -> HandlerError` (a `HandlerError::new` of a small `MissingState(&'static str)` error naming `type_name::<T>()`). Locks use `.lock().unwrap_or_else(PoisonError::into_inner)`; a comment says the stores stand in for the bot's database.
  - `src/tabs.rs` (recipe `tabs`): `#[derive(PayloadEnum)] DashTab { Overview, Activity, About }` with `ALL`; `#[derive(Button)] #[button("tabs.open", defaults(style = Secondary, label = "Overview"))] OpenTab { tab: DashTab }`; `#[derive(SlashCommand)] #[command("dashboard", description = "Open the dashboard")] struct Dashboard;` (public) calling `ctx.respond(render_dashboard(DashTab::Overview))`; `pub fn render_dashboard(tab: DashTab) -> V2Message`: one Container, `## Dashboard` title, a `match tab` body (distinct text per tab), a tab row where each tab's label is its name, the active tab `Primary` and disabled, the others `Secondary`; `OpenTab` handler `ctx.update(render_dashboard(self.tab))`.
  - `src/pagination.rs` (recipe `pagination`): `#[command("items", description = "Browse the item list")] struct Items;` (public); `#[button("items.page", defaults(style = Secondary, label = "Next"))] TurnPage { page: u16 }`; `pub const ROW_MAX_CHARS: usize = 2 * ITEM_NAME_MAX + 8`; `pub fn render_items(items: &[String], page: u16, limits: &Limits) -> V2Message`: empty list → title + `-# No items yet.` and no pager; else `per_page = Budget::of(&fixed, limits).fits(&TextDisplay::new("x".repeat(ROW_MAX_CHARS))).max(1)` where `fixed` = frame + worst-case page line (`page_line(u16::MAX - 1, u16::MAX)`) + pager; page clamped to `pages - 1`; one Text Display per row `format!("{n}. {}", escape_markdown(name))` (1-based global index); `-# Page {p} of {pages}`; pager Previous (`page.saturating_sub(1)`, disabled on page 0) and Next (`page + 1`, disabled on the last page); no `as` casts (`u16::try_from(..)`); handlers read `ctx.state::<Catalog>()` and `ctx.limits()`.
  - `src/steps.rs` (recipe `step-flow`): `#[command("setup", description = "Set up the bot", response = ephemeral, contexts = [Guild])] struct Setup;` `PayloadEnum SetupStep { Channel, Role, Confirm }`; `#[button("setup.nav", owner = owner, defaults(style = Primary, label = "Next"))] SetupState { owner: Id<UserMarker>, step: SetupStep, log_channel: Option<Id<ChannelMarker>>, mod_role: Option<Id<RoleMarker>> }`; `#[channel_select("setup.channel", owner = owner, defaults(placeholder = "Pick the log channel", channel_types = [GuildText]))] PickChannel { owner, mod_role }`; `#[role_select("setup.role", owner = owner, defaults(placeholder = "Pick the moderator role"))] PickRole { owner, log_channel }`; `CancelSetup { owner }` (Secondary "Cancel"); `FinishSetup { owner, log_channel: Id<ChannelMarker>, mod_role: Id<RoleMarker> }` (Success "Finish", non-`Option` fields). `pub fn check_step(st: SetupState) -> Result<SetupState, (SetupState, &'static str)>` (pure: Role needs a channel, Confirm needs a channel then a role; failure = the first incomplete step, values kept, with `"Pick a log channel first."` / `"Pick a moderator role first."`). `pub fn render_setup(st: SetupState, notice: Option<&str>) -> V2Message`: `## Step {n} of 3: {Title}` (titles `Log channel`, `Moderator role`, `Confirm`), `-# ⚠️ {notice}` when set, the step's select pre-filled with `default_channels`/`default_roles` of the current value, or on Confirm a summary (`<#id>`, `<@&id>`) plus Finish when both are set; nav row Channel: [Cancel, Next]; Role: [Back, Cancel, Next]; Confirm: [Back, Cancel, Finish]; Next always enabled. Handlers: `SetupState` → `check_step` then `ctx.update(render_setup(ok, None))` or `(back, notice)` → `ctx.update(render_setup(back, Some(notice)))`; selects read `ctx.values().channels()/.roles().first().copied()` and stay on their step; Cancel → `ctx.update` of a message with one Text Display `Setup cancelled.`; Finish → `SetupStore::save` for `ctx.guild_id()` then one Text Display `Setup complete: logs in <#c>, moderators <@&r>.`.
  - `src/settings.rs` (recipe `settings`): `#[command("tickets", description = "Configure ticket systems", response = ephemeral, contexts = [Guild])] struct Tickets;` Components (all carry `system: u32` unless noted): `PickSystem` (unit string select `"tickets.pick"`, options per render, value = id), `RenameSystem` (button, `auto_defer = false`, `ComponentResponder<Manual>`, `show_modal(RenameForm::blank(self.system))` with a comment that pre-fill is Story 9.4), `#[derive(Modal)] #[modal("tickets.rename", defaults(title = "Rename ticket system"))] RenameForm { system: u32, #[text_input("Name", max_length = 40)] name: String }`, `SetRoles` (role select, `min_values = 0`, `max_values = 5`, pre-filled with `default_roles`), `ToggleSystem { system, enable: bool }`, `AddSystem` (unit), `AskDelete`, `ConfirmDelete` (`by_ref, one_shot, ttl = "15m"`, Danger), `BackToSettings` (Secondary "Cancel"). `pub fn render_tickets(systems: &[TicketSystem], selected: Option<u32>) -> V2Message` (current = selected if present, else the first): `# Ticket systems`; switcher only when `systems.len() > 1` (option label = name, `default` on current); Section name + `Rename` accessory; caption + role select; Section `**Enabled** · on|off` + toggle accessory (`Disable` Danger / `Enable` Success, Payload `enable = !enabled`); Section + `Delete` (Danger) accessory; Separators between blocks; empty state `-# No ticket systems yet.`; Section `Add a ticket system` + `Add` accessory (Primary only in the empty state, Secondary otherwise) only while `len < MAX_SYSTEMS`; names through `escape_markdown`. `pub fn render_confirm_delete(system: &TicketSystem) -> V2Message`: `Delete **{name}**? This can't be undone.` + [BackToSettings, ConfirmDelete]. Every handler: guild from `ctx.guild_id()` (none → `reply_text("Use this in a server.")`), store op, a `false`/`None` result → `ctx.reply_text("That ticket system no longer exists.")` or `"You can have at most 25 ticket systems."`, else `ctx.update(render_tickets(&store.systems(guild), Some(id)))`. `RenameForm` trims the name; empty → `reply_text("The name can't be empty.")`. `PickSystem` parses `ctx.values().strings().first()` as `u32` (fail → not-found notice).
- Recipe sync: the ```` ```rust recipe=<name> ```` blocks in `views-and-recipes.md` §3 are exactly the regions above (tabs, pagination, step-flow, settings, store, app). Each recipe compiles only with `use` lines inside its region.
- Doctrine: no `tokio::spawn`, collectors, await loops, embeds, `content` or `LegacyMessage` in the example; every handler answers with `ctx.respond`/`ctx.update`/`ctx.reply_text` or `r.show_modal`.
- All example code: workspace lints clean under `-D warnings` (docs on every `pub` item, pedantic clippy, no `unwrap`/`expect` outside tests).

**Never:**
- No widget, no framework UI state, no per-message listener (UD-7), no i18n, `.default_values`, pre-filled modals or other planned API in the example.
- No change to dispatch behaviour, Limits, rule ids, `docs/discord-rules.md` or `noctis-testkit`'s public API.
- No `noctis-*` package name for the example (deps-check `unknown-crate`), no dependency of it on a `noctis-*` crate.
- No change to `.github/workflows-disabled/ci.yml` (the example runs inside `cargo test --release --workspace`; skill-check already runs).

## I/O & Edge-Case Matrix

Fixture for example tests (`examples/ui-recipes/tests/recipes.rs`, `#[tokio::test(flavor = "multi_thread")]`): guild `G=10` with text channel `C=20`, role `R=30`; users `A=40`, `B=41`; `TestApp::build(configure(&mut App::builder(), data))`. (U) = unit test in the module (`#[cfg(test)]` inside `src/*.rs` but outside the recipe region) or `crates/noctis-core` unit test; (X) = xtask unit test.

**SelectedValues** (U, core)

| # | Input | Expected |
|---|---|---|
| V1 | button click | all four slices empty |
| V2 | string select `["a","b"]`; `[]` (min 0) | `strings == ["a","b"]`, typed empty; all empty |
| V3 | user select `["41"]`; role `["30"]`; channel `["20"]` | that typed list `[id]`, `strings` the raw value |
| V4 | user select `["abc"]`, `["0"]`, `["18446744073709551616"]` | `strings` kept, `users` empty |
| V5 | mentionable `["41","30"]`, resolved users{41} roles{30} | `users [41]`, `roles [30]` |
| V6 | mentionable `["42"]` not in resolved / resolved absent | typed lists empty, `strings ["42"]` |
| V7 | 25 values, duplicates `["41","41"]` | all kept in order |
| V8 | end to end (testkit `testapp.rs` X7 upgraded) | the user/role/channel/mentionable/string select handlers note `ctx.values()` and the test asserts them |

**Tabs**

| # | Input | Expected |
|---|---|---|
| T1 | `/dashboard` | `Reply`, public; Overview button disabled + Primary, Activity/About enabled Secondary |
| T2 | click Activity, then About | `Update`; only the clicked tab disabled; body text differs per tab |
| T3 | `render_dashboard` for each tab (U) | `noctis::ui::validate` ok; 3 distinct tab `custom_id`s (no CID-02) |

**Pagination** (P = per_page from the budget)

| # | Input | Expected |
|---|---|---|
| P1 | 300 items, every page 0..pages via `render_items` (U) | `validate` ok against `Limits::default()`; pages = ceil(300 / P) |
| P2 | 300 names of 100 `*` each (escape doubles them) (U) | every page still validates |
| P3 | `/items`, then Next until disabled (e2e) | page 1: Previous disabled; last: Next disabled; items 1..=300 each seen once; every `cost()` ≤ 40 components / 4000 chars |
| P4 | page `u16::MAX` (U) | renders the last page |
| P5 | 0 items (U) | `No items yet.`, no buttons |
| P6 | 1 item; exactly `2 * P` items (U) | one page, both buttons disabled; 2 pages, Next disabled on page 2 (no empty page) |

**Step flow**

| # | Input | Expected |
|---|---|---|
| S1 | `/setup` by A in G | ephemeral `Reply`; `Step 1 of 3: Log channel`; [Cancel, Next] |
| S2 | Next with no channel | `Update`; still step 1, text contains `Pick a log channel first.` |
| S3 | select channel C, Next | step stays 1 with C as default; then `Step 2 of 3: Moderator role` |
| S4 | Back on step 2 | step 1, channel kept |
| S5 | Next on step 2 without a role | step 2 with `Pick a moderator role first.` |
| S6 | select R, Next, Finish | `Step 3 of 3: Confirm` with `<#20>`/`<@&30>` and Finish; then one Text Display, no buttons/selects; `SetupStore` holds (C, R) for G |
| S7 | Cancel on any step | one Text Display `Setup cancelled.`, `components()` has exactly one entry (type 10) |
| S8 | B clicks A's ephemeral step | `Err(TestError::NotVisible)` |
| S9 | `check_step` table (U): each target × each value combination | as Always; values never cleared |

**Settings screen**

| # | Input | Expected |
|---|---|---|
| E1 | `/tickets`, no systems | empty-state text; only the Add section (Primary) |
| E2 | Add | `Update`; system 1 shown (name Section + Rename, role select, toggle `Disable`, Delete); no switcher |
| E3 | Add again | switcher with 2 options, `default` on 2 |
| E4 | switcher → `"1"` | system 1 shown, option 1 `default` |
| E5 | toggle | text `· off`, button `Enable` Success; toggle again → `· on` |
| E6 | Rename → modal → `"Destek 🎫"` | `Modal` then `Update` of the same message showing the name; `"*bold*"` shows escaped |
| E7 | rename to `"   "` | ephemeral `Reply` `The name can't be empty.`; store unchanged |
| E8 | 41-char name | testkit `Err(InvalidInput)` (max_length 40) |
| E9 | role select `[R]`; then `[]` | store roles `[R]` and the re-rendered select's `message_json()` carries `default_values` `[{"id":"30","type":"role"}]`; then roles empty, no `default_values` |
| E10 | Delete → confirm screen → Cancel | confirm text + Danger Confirm; Cancel → settings again, system kept |
| E11 | Delete → Confirm; click the same Confirm again | system gone, re-render; second click `is_expired_notice()` |
| E12 | toggle on a stale message after its system was deleted | `That ticket system no longer exists.` |
| E13 | 24 systems (`TicketStore::with`), Add | 25 systems, no Add section; the stale Add → `You can have at most 25 ticket systems.` |

**skill-check recipes** (X, `xtask/src/skill_check/recipes.rs`)

| # | Input | Expected |
|---|---|---|
| R1 | regions and blocks equal (trailing spaces / edge blank lines differ) | ok, count reported |
| R2 | one line differs | ``<md>: recipe `x` differs from <rs> (recipe line N)`` |
| R3 | block without region / region without block | ``recipe `x` has no region in examples/ui-recipes/src`` / ``recipe region `x` is not in <md>`` |
| R4 | a required name missing from both | ``recipe `x` is missing (required: tabs, pagination, step-flow, settings)`` |
| R5 | region not closed / end without start / nested / duplicate name | one problem each, naming the file and name |
| R6 | `examples/ui-recipes/src` unreadable | ``cannot read: not a directory`` |
| R7 | doctrine lint over `examples/` | every `examples/<crate>/{src,tests}` `.rs` file linted (template + ui-recipes); a `tokio::spawn` in a `#[handler]` of ui-recipes is reported |

</intent-contract>

## Code Map

- `crates/noctis-core/src/component.rs`: `ctx_accessors!` L66; `ComponentCtx` L193 (add `values()` beside `limits()` L202); add `SelectedValues` + `pub(crate) fn from_data(kind: Option<ComponentKind>, values: Vec<String>, resolved: Option<&InteractionDataResolved>) -> Self`.
- `crates/noctis-core/src/command.rs` L100 `CtxInner` (add `values: SelectedValues`), L154 `detached_parts` (empty).
- `crates/noctis-core/src/pipeline/component.rs`: `Input::Component` L187-195, `read_input` L474 (twilight `MessageComponentInteractionData` has `values` and `resolved`), `CtxInner` built L676 (set `values` from the input).
- `crates/noctis-core/src/lib.rs` L221 export list; `crates/noctis/src/lib.rs` L564 `pub use noctis_core::{..}` (add `SelectedValues`).
- `crates/noctis-testkit/tests/testapp.rs`: `PickStr` ~L819 and `select_route!` ~L835 handlers note `ctx.values()`; X7 assertions upgraded.
- Patterns to copy for the example: `crates/noctis-testkit/tests/testapp.rs` (derive syntax, `configure`, `TestApp`, `select(..).channels(..)`, `show_modal(X::blank(..))`, `by_ref, one_shot, ttl`), `crates/noctis-testkit/tests/guards.rs` L232 (one-shot), `crates/noctis/tests/budget.rs` (Budget).
- `xtask/src/skill_check/mod.rs`: `TEMPLATE_DIRS` L43 → discover `examples/<crate>/{src,tests}` (skip missing dirs; never `target/`), summary `… <F> example files linted, <R> recipes in sync`; add `mod recipes;` (constants `RECIPES_DIR = "examples/ui-recipes/src"`, `REQUIRED = ["tabs","pagination","step-flow","settings"]`, md = `{SKILL_DIR}/references/views-and-recipes.md`). `doctrine.rs` unchanged apart from fixtures.
- Skill `examples/project-template/.claude/skills/noctis/`:
  - `references/views-and-recipes.md`: §1/§2 (remove the duplicated "Each returns `HandlerResult<Responded>`" paragraph L78-83; state helpers are `ctx.state::<T>()`, no `State<T>` parameters), §3 rewritten as the six `recipe=` blocks with short prose and the per-recipe bullets kept (≤ 5 tabs, `per_page + 1` queries, modal chains, spill), a note that strings are literals until `i18n!` (Epic 11) and modals are blank until pre-fill (Story 9.4); §6 rulebook, §7 best practices + pitfalls table and §8 decision table kept and aligned with the code; §9 points at `examples/ui-recipes/` in noctis-rs (compiled and tested on every change; the blocks above are verbatim copies) and the template modules.
  - `references/components-v2.md` L394-395 and `references/routes-payloads-refs.md` L786-787: `ctx.values()` → `SelectedValues` (`strings()`, `users()`, `roles()`, `channels()`; mentionable values split into users/roles), no `.mentionables()`, `ctx.resolved()` `[planned]`.
  - `SKILL.md` L23 API hash.
- Template `examples/project-template/AGENTS.md` Policy UI bullet: one sentence — select handlers read `ctx.values()` (`SelectedValues`), and the recipes in `views-and-recipes.md` §3 are verbatim copies of noctis-rs's tested `examples/ui-recipes/`. Template code already calls `ctx.values().channels()/.roles()` and stays as is.
- Spine `ARCHITECTURE-SPINE.md` AD-27 L1231-1232: skill-check lints every crate under `examples/` and checks that the `recipe=` blocks of `views-and-recipes.md` equal `examples/ui-recipes/` regions.
- Root `Cargo.toml` `members`; `Cargo.lock` gains `ui-recipes` (no new external crate).
- Memlog: the three Story 8.5 planning `(decision)` lines are already written.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/src/{component.rs,command.rs,pipeline/component.rs,lib.rs}` -- `SelectedValues`, population, `ComponentCtx::values()`, unit tests V1-V7 -- the switcher and step selects need it.
2. `crates/noctis/src/lib.rs` -- re-export `SelectedValues`.
3. `crates/noctis-testkit/tests/testapp.rs` -- V8.
4. `Cargo.toml` + `examples/ui-recipes/{Cargo.toml,src/*.rs}` -- the crate and recipes as in Always.
5. `examples/ui-recipes/tests/recipes.rs` + module unit tests -- T*, P*, S*, E*.
6. `xtask/src/skill_check/{mod.rs,recipes.rs}` -- examples-wide doctrine lint and the recipes check with R1-R7 tests.
7. Skill references, template `AGENTS.md`, spine AD-27.
8. `cargo run --release -p xtask -- api-snapshot --bless`; paste the printed hash into `SKILL.md`.
9. Run every Verification gate.

**Acceptance Criteria:**
- Given `examples/ui-recipes/` in the workspace `members` with `publish = false`, when `cargo test --release --workspace` runs, then its testkit tests prove the option-less entry Commands answer with `ctx.respond(render_x(..))`, the active tab is disabled, Previous is disabled on the first page and Next on the last, no page of the 300-item list exceeds a Limit, the 3-step flow validates per step with an inline notice, cancel leaves no interactive component, and the settings screen's switcher, toggle, add, delete-with-one-shot-confirm and button → modal → re-render chain work on the same Payload.
- Given a select handler, when the user picks values, then `ctx.values()` returns them typed by the select's kind.
- Given the Usage Skill, when `xtask skill-check` runs, then the `recipe=` blocks of `views-and-recipes.md` equal the example's regions, the file holds the rulebook, decision table, schwi-style best practices and pitfalls, and no example uses a per-message listener; the doctrine lint covers `examples/` and passes.
- Given the new public item, when the story is done, then the API snapshot is re-blessed, `SKILL.md` carries its hash and the skill mentions `SelectedValues` and its methods.

## Spec Change Log

## Review Triage Log

### 2026-10-21 — Review pass
- verdicts: 31 findings — high 0, medium 4, low 18, false 9, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) `/tickets` and `/setup` have no permission gate — confirmed: both `#[command]`s lack `permissions`, the shipped `#[command]` supports `permissions = MANAGE_GUILD` (template setup/dashboard use it), the old recipe had `permissions = ADMINISTRATOR`; any member can change guild config. Patch: add `permissions = MANAGE_GUILD` to both and sync the md blocks.
  - `[low]` `[reject]` (blind) recipes store select/Payload ids without the guild-membership check §2 asks for — entity select values come from Discord's own picker and Payloads only from the bot's own components; the harm needs a forged interaction never shown reachable, and the fix adds guards.
  - `[low]` `[reject]` (blind) `lint_examples` skips a missing `src`/`tests` silently — true, but the spec Code Map prescribes "skip missing dirs"; deleting the template's `src/` also breaks the workspace build, and a required-dir set adds branches.
  - `[false]` `[reject]` (blind) a system with more than 5 roles breaks the role select — roles are only written by `SetRoles` (`max_values = 5`) and by `TicketStore::with` in tests; no reachable path stores more than 5.
  - `[low]` `[patch]` (blind) pagination loses items past 65535 pages and the `ROW_MAX_CHARS` comment is wrong — the comment names `65535. ` but the prefix is the 1-based global item number: patch rewords the comment; the u16 clamp needs >1M items (catalog 300), not worth extra branches.
  - `[low]` `[patch]` (blind) `ITEM_NAME_MAX` doc gives a wrong reason (select option label) — names render only in Text Displays. Patch rewords the doc.
  - `[low]` `[reject]` (blind) settings handlers repeat the same preamble — the spec prescribes this per-handler flow; a helper refactor is more than a direct correction and the named harm is only copy-paste length.
  - `[low]` `[reject]` (blind) `run_component_over` clones `SelectedValues` per click — true but negligible (a few small vecs); `decoded` still borrows `input` at that point, so moving out is not a direct change.
  - `[low]` `[reject]` (blind) no public constructor for `SelectedValues` — the spec deliberately keeps fields private with `Default` only; select logic is tested through `TestApp::select(..)`; a constructor adds public surface.
  - `[false]` `[reject]` (blind) the fence parser ends a block at the first bare fence line — no recipe contains a nested fence; on such input the check fails loudly with a "differs" problem, which is correct behaviour for an unreached case.
  - `[low]` `[reject]` (blind) `real_repo_passes` pins "6 recipes in sync" — the spec Verification names 6 recipes; adding a recipe updating that test is intended.
  - `[low]` `[reject]` (blind) test code hides failures / claims more than it checks — the CID-02 claim holds (`validate` runs CID-02 on messages, noctis-ui lib.rs L205); `u16::try_from(page).unwrap_or(0)` cannot fail for ≤ 16 pages; 40/4000 is the spec's P3 oracle.
  - `[low]` `[reject]` (blind) two temp-dir tests leak files on panic — test-only, r6 writes nothing, r7 cleans before asserting; a drop guard adds code for a rare case.
  - `[low]` `[reject]` (blind) owner guard never exercised by a recipe test — the guard is framework code covered by noctis-testkit guard tests; the ephemeral reply makes it unreachable for B in this flow; a new test is not a direct correction.
  - `[medium]` `[patch]` (edge) `/tickets` lets non-admins edit systems — same root cause as the first row; shares its patch.
  - `[medium]` `[patch]` (edge) `/setup` lets non-admins overwrite saved setup — same root cause as the first row; shares its patch.
  - `[low]` `[reject]` (edge) forged/stale Finish payload saves foreign ids — custom ids come only from the bot's own message; a stale deleted id is a sample-store concern; fix adds guards.
  - `[false]` `[reject]` (edge) `render_items` with names over `ITEM_NAME_MAX` exceeds the budget — the only catalog source (`Catalog::sample`) truncates to `ITEM_NAME_MAX`, and an oversize page fails loudly in Responder validation; no reachable path shown.
  - `[low]` `[reject]` (edge) items past 65535 pages unreachable — needs > 1M items; the recipe's catalog holds 300; cap/report adds branches.
  - `[low]` `[reject]` (edge) deleted/unreadable template `src` passes skill-check silently — same as the blind row: spec-prescribed skip, build breaks anyway.
  - `[low]` `[reject]` (edge) a symlinked example crate is skipped — no example is a symlink; hypothetical.
  - `[low]` `[patch]` (edge) §3 intro claims every block compiles as `src/<name>.rs`, wrong for `app` (it is `src/lib.rs`) — confirmed at views-and-recipes.md L84-85. Patch: name `src/lib.rs` for `app`.
  - `[low]` `[reject]` (vgap other) `lint_examples` skips a missing `src`/`tests` silently — same as the blind row: spec-prescribed, no test removed.
  - `[false]` `[reject]` (intent 1) public API grows inside a recipes story — the spec deliberately adds `SelectedValues`/`ctx.values()` (Approach, Design Notes) as a read-only primitive, allowed by maintainer decisions §1.
  - `[medium]` `[patch]` (intent 2) template step flow (Next disabled until set) contradicts §3.3 (Next always enabled) while §9 says the template applies the same recipes — confirmed in template `setup/view.rs` L18 and md L309/L1459; agents learn from the skill alone. Patch: one sentence naming the template variant and the default.
  - `[false]` `[reject]` (intent 3) no recipe combines tabs and pagination — FR-49's tabbed paginated View is the template dashboard (pre-existing, `dash.tab`/`dash.page`); §3.1 points at adding `page` to the tab Payload.
  - `[false]` `[reject]` (intent 4) cancel leaves one Text Display, not zero components — the spec's Design Notes settle this (V2 needs one component, UD-1 forbids `content`).
  - `[false]` `[reject]` (intent 5) the 300-item test uses hard-coded limits and never asserts Next disabled on the last page — 40/4000 is the spec's P3 oracle; the loop exits only on a disabled Next and then asserts items 1..=300 seen once, which proves Next is disabled exactly on the last page.
  - `[low]` `[patch]` (intent 6) views-and-recipes.md lacks the AC's patterns/anti-patterns — confirmed: decision table and pitfalls exist, no patterns/anti-patterns list. Patch: add a short do/don't list.
  - `[false]` `[reject]` (intent 7) rulebook and decision table are mostly pre-existing — the AC only requires the file to contain them, and it does (§6, §8).
  - `[false]` `[reject]` (intent 8) literal strings and blank modals — the spec's Never excludes i18n and pre-filled modals, and §3 documents both as temporary (Epic 11, Story 9.4).

## Design Notes

- **Why add `ctx.values()` here.** The skill and template already call it without a `[planned]` marker, and the AC's entity switcher and the step flow's selects cannot be demonstrated without reading the pick. It is a read-only view over data the pipeline already parses; no behaviour changes.
- **Why Next stays enabled.** The AC asks for per-step validation in the handler with an inline notice. The testkit refuses clicks on disabled buttons, so a disabled Next would make that path untestable; enabling Next and validating in `check_step` keeps one path, covered end to end.
- **"No components" after cancel** is read as no interactive components: a V2 message needs at least one component, so cancel leaves one Text Display (UD-1 forbids a legacy `content` reply).
- **Why a sync check, not includes.** Markdown cannot include Rust files and agents read the skill only; a byte-level comparison in skill-check keeps the copies honest, and marker comments keep the example readable.
- Golden shape of a region:

```rust
//! Tabs (UD-5): a `PayloadEnum` field, a `match`, the active tab disabled.

// recipe: tabs
use noctis::prelude::*;
use noctis::ui::{ActionRow, Button, ButtonStyle, Components, Container, TextDisplay, V2Message};
/* … the whole module … */
// recipe-end: tabs
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean (example included)
- `cargo test --release --workspace` -- all pass, incl. `ui-recipes` tests, core `SelectedValues` tests, upgraded X7, xtask recipes tests
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok (`ui-recipes` depends only on `noctis`)
- `cargo run --release -p xtask -- rules-catalog` -- ok (unchanged)
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed snapshot (install nightly-2026-10-15 and cargo-public-api 0.52.0 first if missing, per AGENTS.md)
- `cargo run --release -p xtask -- skill-check` -- ok, reports 6 recipes in sync and the example files linted
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `bash .github/scripts/check-build-config.sh` -- ok
- `test ! -d target/debug` -- no debug profile created

## Auto Run Result

- **Summary:** Adds the `examples/ui-recipes` crate (tabs, pagination, step flow, settings screen with entity switcher, sample store) with end-to-end testkit tests; rewrites the usage skill's `views-and-recipes.md` with recipe blocks kept byte-identical to the example by a new `skill-check` recipes sync check; adds the read-only `SelectedValues` / `ctx.values()` primitive to noctis-core (API snapshot re-blessed).
- **Files changed:** `crates/noctis-core/src/{component.rs,command.rs,lib.rs,pipeline.rs,pipeline/component.rs}` (SelectedValues, ctx.values()); `crates/noctis/src/lib.rs` (re-export); `crates/noctis-testkit/tests/testapp.rs` (select value tests); `examples/ui-recipes/**` (new example + tests); `examples/project-template/.claude/skills/noctis/**`, `examples/project-template/AGENTS.md` (skill recipes, API hash); `xtask/src/skill_check/{mod.rs,recipes.rs}`, `xtask/tests/skill_check.rs` (recipe sync + example lint); `xtask/public-api.txt`; `Cargo.toml` (workspace member); `ARCHITECTURE-SPINE.md` (note).
- **Review findings:** 31 total (high 0, medium 4, low 18, false 9). Patched: 4 medium (missing `permissions = MANAGE_GUILD` on `/tickets` and `/setup`, three rows one root cause; template step-flow vs. §3.3 Next-enabled contradiction documented) and 4 low (ROW_MAX_CHARS comment, ITEM_NAME_MAX doc, §3 intro `src/lib.rs` for `app`, patterns/anti-patterns list). Deferred: none. Rejected: 23, each with its reason in the spec triage log (spec-prescribed missing-dir skip, unreachable >1M-item / >5-role / forged-payload cases, deliberate private `SelectedValues` fields, spec P3 oracle, cancel leaving one Text Display per Design Notes, etc.).
- **Follow-up review recommended: true** — first pass patched 4 medium (0 high). Named risk: the permission-gate patch changes `/tickets` and `/setup` command metadata in both the example and the skill md blocks; the sync check and tests pass, but no test asserts that a non-MANAGE_GUILD member is actually refused.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, test, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check with 6 recipes in sync, cargo deny, MSRV 1.91, build-config, no target/debug). CI paused by maintainer.
- **Residual risks:** pagination u16 page clamp loses items past ~1M items (documented, unreachable for 300-item catalog); skill-check silently skips a missing example `src`/`tests` dir (spec-prescribed); settings handlers duplicate their preamble.
