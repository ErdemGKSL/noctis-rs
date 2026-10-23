---
title: 'Story 11.3: User and Guild Locale resolution'
type: 'feature'
created: '2026-10-23'
status: 'in-progress'
baseline_revision: '247385f5e22a855330fb07a5080b540084a09acd'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-11-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/traits-and-generics.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Handlers read Discord's raw `locale` / `guild_locale` strings (`Option<&str>`). Nothing resolves them against the locale files the App actually ships: an `en-GB` user with only `en-US.yaml` is not mapped to `en-US`, and a `pt-BR` user does not move on to the Guild Locale. Event handlers have no locale at all. Neither an App-level `LocaleResolver` nor a before-Middleware can override the locale (FR-43, FR-20 override deferred from Story 7.6; AD-16, AD-23 stage 4).

**Approach:** Add one resolution engine in `noctis-core`. It knows which locales have a file in the merged catalogs and the App's default locale. Stage 4 of every run applies it: interactions resolve User → Guild → default, guild events Guild → default, and a code without a file tries its `Locale::sibling()` first. An optional App `LocaleResolver` (sync, stage 4) runs next, then before-Middleware setters can override the result. The resolved pair lives in one per-run cell that the handler context, the Responder, notices and Middleware all read. `ctx.user_locale()`, `ctx.guild_locale()` and `ctx.response_locale()` return `noctis_rules::Locale`. A bounded per-guild locale record, fed by `GUILD_CREATE`/`GUILD_UPDATE`/`GUILD_DELETE`, supplies the Guild Locale to events and to the `Dispatcher`.

## Boundaries & Constraints

**Always:**

- **New module `crates/noctis-core/src/locale.rs`**, re-exported from the `noctis-core` root and from the facade root `crates/noctis/src/lib.rs` (next to `LocaleOwner`). Do not add it to the prelude. Every public item and field has a doc comment and every public item is mentioned in the Usage Skill.
  ```rust
  /// The two locales of one run (FR-43).
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct Locales { pub user: Locale, pub guild: Locale }

  /// What stage 4 hands the App's LocaleResolver.
  #[derive(Clone, Copy, Debug)]
  #[non_exhaustive]
  pub struct LocaleRequest {
      pub invocation: Invocation,
      pub guild_id: Option<Id<GuildMarker>>,
      pub user_id: Option<Id<UserMarker>>,
      pub discord_user: Option<Locale>,  // Discord's `locale`, parsed; None when absent or not one of the 32 codes; always None for events
      pub discord_guild: Option<Locale>, // `guild_locale`, or the guild record (events, Dispatcher)
      pub resolved: Locales,             // the built-in resolution
  }

  pub trait LocaleResolver: Send + Sync + 'static {
      fn resolve(&self, request: &LocaleRequest) -> Locales;
  }
  impl<F: Fn(&LocaleRequest) -> Locales + Send + Sync + 'static> LocaleResolver for F { .. }
  ```
- **The built-in resolution** is a pure `pub(crate) fn` over `(available, default, user: Option<Locale>, guild: Option<Locale>)`. Here `step(c)` is `c` when `c` has a file, else `c.sibling()` when that has a file, else nothing.
  - interactions: `user = step(user) ∥ step(guild) ∥ default` and `guild = step(guild) ∥ step(user) ∥ default`;
  - events: `guild = step(guild) ∥ default`, and `user = guild`.
- **`available` and `default`** are computed once in `build()`, after the 11.2 merge, from the deduplicated `AppStrings::catalogs()`.
  - `available` is the union of every catalog's `locales()`. It is `{en-US}` when no catalog is registered.
  - `default` is the `default_locale()` of the first catalog in merge order (App catalogs first, then Modules in fold pre-order). It is `en-US` without catalogs.
  - Store both, plus the resolver and the guild record, in `AppShared`. Use a 32-bit set indexed by `Locale::ALL` position.
- **Raw codes are parsed exactly** (`str::parse::<Locale>`). An absent, empty, wrong-case or unknown code counts as absent and the chain moves on. This supersedes Story 6.8's "unparseable User Locale means en-US".
- **`AppBuilder::locale_resolver(&mut self, resolver: impl LocaleResolver) -> &mut Self`.** The last call wins.
  - Stage 4 calls it **exactly once per run**, once the Command, Route, menu or event handler is known and before the context is built. That covers slash Commands, context menus, components, modal submits, the Expired Handler chain, autocomplete, events and `Dispatcher::run_command`.
  - Its return value is used verbatim, with no availability re-check.
  - A panic is caught. It logs one `error` on target `noctis::i18n` with the handler name, the run keeps the built-in resolution, and nothing is reported to an Error Sink.
  - Notices sent before stage 4 use the built-in resolution, and the resolver is not called for them. These are a malformed or unknown `custom_id`, a kind mismatch and a missing author.
- **`Invocation` gains `Autocomplete { path: &'static str }`**, documented as "seen only by a `LocaleResolver`; autocomplete runs no Middleware".
- **Context accessors return the cell's current value:**
  - `user_locale(&self) -> Locale` and `guild_locale(&self) -> Locale` on `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx` and `AutocompleteCtx`;
  - `response_locale(&self) -> Locale` (ephemeral → `user`, public → `guild`; the component update rule of Story 8.1 is unchanged) on every context that has it today;
  - a new `EventCtx::guild_locale(&self) -> Locale`;
  - `BeforeCtx` and `AfterCtx` gain `user_locale()` and `guild_locale()` through `run_accessors!`.
  - Rewrite the rustdoc of each accessor.
- **Middleware setters on `BeforeCtx`:**
  - `set_user_locale(&self, Locale)`, `set_guild_locale(&self, Locale)` and `set_locale(&self, Locale)`, which sets both;
  - the last write wins, and a later Middleware reads earlier writes;
  - a call after the before chain ended (from a kept clone) is ignored with one `debug` on `noctis::middleware`, like `insert`.
- **One per-run locale cell** (`pub(crate)`, one `Mutex<Locales>` or an equivalent single atomic, never two separate atomics). Every reader below reads it at use time, so an override reaches each of them:
  - `CtxInner` and `EventCtx`;
  - `AutocompleteParts`;
  - the Responder's `I18n`, for `Text::Key` and `ctx.reply_text(key)`;
  - `Report`, whose notices use `user`: handler error, panic, `Flow::Stop(Some(key))` and Guard denials;
  - `Run`, for the Middleware.
- **Other locale consumers:**
  - Webhook sends (`Shared::webhook`) and the detached `__testing` seams resolve keys in the App default (en-US detached).
  - `CommandInvocation::locale(impl Into<String>)` keeps its signature; its code is parsed at resolution.
  - The Dispatcher takes its Guild Locale from the guild record.
  - Delete `notice::locale` and `CtxInner::response_locale`'s `&str` path, and replace every call site.
- **Guild record** (`pub(crate)`, in `AppShared`): a `std::sync::RwLock<HashMap<Id<GuildMarker>, Locale>>`, no new dependency.
  - The worker drain updates it for every inbound `Event`, before the permit wait and the handler lookup. `GuildCreate::Available` and `GuildUpdate` set the parsed `preferred_locale`, or remove the record when it does not parse. `GuildDelete` with `unavailable != Some(true)` removes it. An outage and `GuildCreate::Unavailable` leave it alone.
  - An interaction with a guild id and a parseable `guild_locale` overwrites an **existing** record only and never inserts one.
  - `Registry::fold` adds `GUILD_CREATE | GUILD_UPDATE | GUILD_DELETE` to the gateway filter whenever at least one event handler is registered.
- **Testkit:** `AppProbe::notice` (`crates/noctis-core/src/lib.rs:385`) resolves through the App's built-in resolution, so `outcome.rs` notice detection matches runtime when no resolver or Middleware overrides.
- The template and Usage Skill reconciliation, the API snapshot re-bless and the `API hash` line ship in the same change (AGENTS.md).

**Never:**

- No port I/O at stage 4: no `DiscordCache` or `ModuleStateStore` call, and the resolver is sync. Async per-guild lookups belong in before-Middleware.
- No `Option` return for the accessors, and no raw-string accessor on handler contexts.
- No `AppBuilder::default_locale` setter; remove it from the skill. No prelude additions.
- No change to `noctis-i18n`'s or `noctis-rules`' public API, to `i18n!` or to `Strings::of`.
- No insert into the guild record from interactions. No new crate dependency, debug builds, trybuild `pass` suites or weakening of `.cargo/config.toml`.
- No `LocaleSource` (11.5) and no command localization (11.4).

## I/O & Edge-Case Matrix

Codes in quotes are raw Discord strings. `A` is the available set and `D` the default.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Sibling (AC example) | `A={en-US}`, `D=en-US`, user "en-GB", no guild | user = guild = `EnUs` | none |
| 2 | Sibling before default | `A={en-US,tr}`, `D=tr`, user "en-GB" | user `EnUs`, guild `EnUs` | none |
| 3 | Own file wins over sibling | `A={en-GB,en-US}`, user "en-GB" | `EnGb` | none |
| 4 | No sibling, guild has no file | `A={en-US,tr}`, `D=tr`, user "pt-BR", guild "de" | user `Tr` (default), guild `Tr` | none |
| 5 | No sibling, guild step (AC) | same `A`, user "pt-BR", guild "tr" | user `Tr`, guild `Tr` | none |
| 6 | tr user in en-US guild (AC) | `A={en-US,tr}`, user "tr", guild "en-US" | user `Tr`, guild `EnUs`; response ephemeral `Tr`, public `EnUs` | none |
| 7 | es sibling | `A={es-ES}`, user "es-419" | `EsEs` | none |
| 8 | Both of the user's steps miss | `A={tr}`, `D=tr`, user "en-GB", guild "tr" | user `Tr` (guild step), guild `Tr` | none |
| 9 | DM guild falls back to user | `A={en-US,tr}`, `D=en-US`, user "tr", no guild | user `Tr`, guild `Tr` | none |
| 10 | Nothing known | no codes (detached, webhook) | both `D` | none |
| 11 | Unknown code skipped | `A={de,en-US}`, user "xx-YY", guild "de" | user `De` | none, no log |
| 12 | Wrong case / bare / empty | user "en-us", "en", "" or "en_US" | absent; next step | none |
| 13 | Garbage and unicode | user "türkçe", " tr", "tr ", "tr\u{0}", or a 10 KiB string | absent; no panic | none |
| 14 | No catalogs | no `.locales`, user "tr", guild "de" | both `EnUs` | none |
| 15 | All 32 files | every code has a file | each code resolves to itself (loop `Locale::ALL`) | none |
| 16 | Only en-US | `A={en-US}`, each of the 32 codes as user | `EnUs` for all | none |
| 17 | Default without en-US | single catalog default tr, only `tr.yaml`; user "en-US" | `Tr` | none |
| 18 | Merged availability | App catalog (en-US default; en-US, tr) and Module catalog (default de; de) | `A={en-US,tr,de}`, `D=en-US`; user "de" gives `De` | none |
| 19 | Module-only catalog | no App catalog, Module catalog default de | `D=de` | none |
| 20 | Guild event | record guild 9 = tr, `A={en-US,tr}`; `MESSAGE_CREATE` in 9 | `EventCtx::guild_locale() == Tr` | none |
| 21 | Event with no record or DM | guild without a record, or no guild | `D` | none |
| 22 | `GUILD_UPDATE` | 9 updated to "de": with no de file, then with one | `D`, then `De` | none |
| 23 | `GUILD_DELETE` | `unavailable: true`, then `None`/`false` | kept, then removed (`D`) | none |
| 24 | Unavailable or unparseable create | `GuildCreate::Unavailable`, or preferred "en-us" | no record (or removed) | none |
| 25 | Interaction refreshes record | recorded 9 = tr, interaction guild_locale "de" | record `De`; an unrecorded guild stays unrecorded | none |
| 26 | Bounded record | 10 000 `GUILD_CREATE` then 10 000 deletes; then 10 000 interactions from unrecorded guilds | 10 000, then 0, then 0 entries | none |
| 27 | Filter | App with one event handler / none | filter contains / lacks `GUILD_CREATE\|UPDATE\|DELETE` | none |
| 28 | Dispatcher | `A={en-US,tr,de}`, record = tr, `.locale("de")` | user `De`, guild `Tr` | none |
| 29 | Resolver override | resolver sets `guild: PtBr` for guild 7 (no pt-BR file) | `ctx.guild_locale() == PtBr`; keys render with the catalog's fallback | none |
| 30 | Resolver identity | returns `request.resolved` | unchanged | none |
| 31 | Resolver request | user "xx", guild "tr" | `discord_user: None`, `discord_guild: Some(Tr)`, `resolved` = built-in, `invocation` correct per kind | none |
| 32 | Resolver once per run | counting resolver over command, menu, component, modal, expired chain, autocomplete, event, Dispatcher | exactly 1 call each | none |
| 33 | Resolver skipped before stage 4 | malformed id, unknown command | 0 calls; the rejection notice is in the built-in locale | none |
| 34 | Resolver panics | resolver panics | one `error` on `noctis::i18n`; the handler runs with the built-in locales | caught |
| 35 | Resolver set twice | two `.locale_resolver` calls | the second runs | none |
| 36 | Middleware guild override | before sets `set_guild_locale(Tr)` | handler guild `Tr`, user unchanged, public response `Tr`; public `ctx.reply_text(key)` renders tr | none |
| 37 | `set_locale` | `set_locale(De)` | both `De` | none |
| 38 | Parent then child | parent sets Tr; child reads it, then sets De | the child sees `Tr` first; the handler sees `De` | none |
| 39 | Overrides reach notices | `set_user_locale(Tr)`, then `Flow::Stop(Some(k))`; or a handler `HandlerError::user(k)`; or a Guard denial | the notice is in tr | none |
| 40 | Late set | a kept `BeforeCtx` clone calls a setter after the chain | ignored; one `debug`; the handler is unchanged | none |
| 41 | After-Middleware | override in before | `AfterCtx` reads the overridden pair | none |
| 42 | Resolver then Middleware | resolver gives en-US, Middleware sets tr | the handler sees `Tr` | none |
| 43 | Override after auto-defer | a slow handler is auto-deferred, then a Middleware override | the later `edit` resolves keys in the override | none |
| 44 | Event Middleware | an event before-Middleware calls `set_guild_locale(Tr)` | `EventCtx::guild_locale() == Tr` | none |
| 45 | Component response locale | `response = update` on an ephemeral source, a public source, or none | `user`, `guild`, `guild` | none |
| 46 | Autocomplete | user "tr", `MessageKey` label | the label renders in the resolved user locale (after the resolver) | none |
| 47 | Expired chain | stale id, user "en-GB", `A={en-US}` | `ExpiredCtx::user_locale() == EnUs`; the notice is in en-US | none |
| 48 | Debug | `format!("{ctx:?}")` with user tr and guild de | `user_locale: Tr, guild_locale: De` | none |

</intent-contract>

## Code Map

- `crates/noctis-core/src/locale.rs` (new): the public types, the resolution fn, the guild record and the per-run cell. Unit tests go in `locale/tests.rs` (rows 1-19).
- `crates/noctis-core/src/handler.rs`:
  - `:202` `AppShared`: add the locale table. `strings` is at `:229`.
  - `:261-345` `EventCtx` and its inner struct: add the cell and `guild_locale()`.
- `crates/noctis-core/src/app.rs`:
  - `:1301` `catalogs` and `:2145-2165`, where `AppStrings::new` and the 11.2 merge run. Compute `available`/`default` from `AppStrings::catalogs()`.
  - `:1531-1560` `locales` docs. Add `locale_resolver` next to `expired_handler` (`:1691`), and add the builder field, its Debug and its `AppShared` init.
- `crates/noctis-core/src/notice.rs:12` `locale`: delete it and move its tests to `locale/tests.rs`.
- `crates/noctis-core/src/command.rs:95-180,255-275,350-365` `CtxInner` (`user_locale`/`guild_locale: Option<String>` → cell), `response_locale`, `DEFAULT_LOCALE`, the accessors and Debug. The test at `:922` changes.
- `crates/noctis-core/src/component.rs:190-210`: macro accessors for the component, modal and expired contexts.
- `crates/noctis-core/src/autocomplete.rs:190-295`: `AutocompleteParts`, `labels()` and the accessors.
- `crates/noctis-core/src/responder.rs:1320-1370,1474-1501`: `I18n` (strings and cell), `text()`, `set_i18n`, `webhook`.
- `crates/noctis-core/src/responder/ergonomics.rs:159-185`: `resolve` and `text_in` take a `Locale` pair.
- `crates/noctis-core/src/pipeline.rs`:
  - `:344` `Report.locale` (read from the cell) and `:752` the notice render;
  - `:821-930` `run_interaction` stage 4 (slash Commands and context menus, via `Resolved`, `:206`) and `:995-1085` `run_autocomplete`;
  - `:1440-1560` `CommandInvocation::locale` and `Dispatcher::run_command` (`message_shared`, `notice_locale`).
- `crates/noctis-core/src/pipeline/component.rs`:
  - `:586-640` the `Rejecter` locale (built-in) and `shared_for`;
  - `:688-760` "Stage 3: build the context". Call the resolver here, before the Expired chain branch at `:762`.
- `crates/noctis-core/src/middleware.rs`:
  - `:132` `Invocation` (add `Autocomplete`), `:636` `RunInfo` (add the cell);
  - `:656-720` `Run` (setters and their freeze check), `:723` `run_accessors!`, `:806-828` `BeforeCtx` setters.
- `crates/noctis-core/src/worker.rs:263-276`: drain `InboundKind::Event` (update the record before the permit loop) and `:439-459` `run_handler` (resolver plus cell into `RunInfo`/`EventCtx`).
- `crates/noctis-core/src/registry.rs:202-250`: add the `GUILD_*` flags when any event handler exists.
- `crates/noctis-core/src/lib.rs:210-291` re-exports and `:382-396` `AppProbe::notice`.
- Tests to update or add:
  - `pipeline/tests.rs:354,373,728,896` and `pipeline/tests/components.rs:1103-1230` (wire tests with `"locale"`/`"guild_locale"` JSON);
  - `responder/tests/ergonomics.rs:45,1066` and `autocomplete/tests.rs:49,339`;
  - `middleware/tests.rs:35-60` (`info()`/`ctx()` helpers);
  - `worker/tests.rs:134` (`MESSAGE_CREATE` helper, for the event rows);
  - `app/tests.rs` (`Catalog::__new` statics from 11.2 for rows 14, 17-19).
- `crates/noctis-testkit/src/app/outcome.rs:185-190`: unchanged API; benefits from `AppProbe::notice`.
- `crates/noctis/src/lib.rs:578-590` (root re-exports) and `:406` (docs; `Locale: Display` still compiles).
- `crates/noctis/tests/modal_flow.rs:45-52`: the `Strings::of(_: &str)` shim becomes `Locale`.
- `crates/noctis/tests/i18n_app.rs` and `i18n_modules.rs`: the patterns for the new `i18n_locales.rs`. Fixtures live under `tests/fixtures/i18n/`, and each needs a leading `#` comment naming the story.
- `.github/workflows-disabled/ci.yml:65`: the `i18n,testkit` test line.
- `xtask/public-api.txt`: the locale accessor lines (`:211`, `:439-448`, …) change.
- Usage Skill under `examples/project-template/.claude/skills/noctis/`:
  - `references/i18n.md` §3 ("Interim, until Story 11.3" block) and §7;
  - `references/commands.md:619,849-850,872`;
  - `references/responder.md:150,323-325`;
  - `references/routes-payloads-refs.md:732,749-763,819`;
  - `references/modules-middleware-ratelimits.md:202,398`;
  - `references/app-and-sharding.md:136-137`;
  - `references/app-and-sharding.md` and `references/modules-middleware-ratelimits.md`, wherever they list the `EventCtx` accessors;
  - `SKILL.md:23` (API hash).
- `examples/project-template/AGENTS.md`: add the locale rule line. Template code already calls `Strings::of(ctx.response_locale())`/`ctx.user_locale()` typed as `Locale`; keep it as is.

## Tasks & Acceptance

**Execution:**

- [ ] `crates/noctis-core/src/locale.rs` (+ `locale/tests.rs`): the types, `LocaleResolver` with its closure impl, the pure resolution fn, the available set, the guild record and the per-run cell. Unit tests cover rows 1-17 and 26. This is the engine.
- [ ] `crates/noctis-core/src/app.rs`, `handler.rs` (`AppShared`): `locale_resolver`, plus computing `available`/`default` after the catalog merge. Unit tests in `app/tests.rs` cover rows 14 and 17-19, and row 35. This is the App config.
- [ ] `crates/noctis-core/src/middleware.rs`: `Invocation::Autocomplete`, the cell in `RunInfo`/`Run`, the `BeforeCtx` setters and the `Before`/`AfterCtx` getters. Unit tests cover rows 37, 38, 40 and 41. This is the Middleware override.
- [ ] `command.rs`, `component.rs`, `autocomplete.rs`, `responder.rs`, `responder/ergonomics.rs`, `notice.rs`: the accessors return `Locale` from the cell, `I18n`, `text_in`, `Report` and Labels read the cell, and `notice::locale` is removed. Update the existing tests. This is the context surface.
- [ ] `pipeline.rs`, `pipeline/component.rs`: stage-4 resolution plus one resolver call per run for every kind, the Dispatcher's guild record, and the built-in locale for pre-stage-4 rejections. Wire tests in `pipeline/tests.rs` and `pipeline/tests/components.rs` cover rows 6, 11, 28-34, 36, 39, 42, 43 and 45-48. These are the interaction pipelines.
- [ ] `worker.rs`, `registry.rs`, `handler.rs` (`EventCtx`): record updates in the drain, the `GUILD_*` filter, and event resolution with the resolver and the Middleware cell. Tests in `worker/tests.rs` (and `registry/tests.rs` for row 27) cover rows 20-27 and 44, feeding parsed `Event`s through `Workers`. This covers guild events.
- [ ] `crates/noctis-core/src/lib.rs`: the re-exports and `AppProbe::notice` on the App's resolution. `crates/noctis/src/lib.rs`: root re-exports of `Locales`, `LocaleRequest` and `LocaleResolver`, plus the i18n feature docs. This is the facade.
- [ ] `crates/noctis/tests/fixtures/i18n/resolve/{en-US,tr}.yaml` and `crates/noctis/tests/i18n_locales.rs` (`#![cfg(all(feature = "i18n", feature = "testkit"))]`, `mod strings { noctis::i18n!(default = "tr", dir = "tests/fixtures/i18n/resolve"); }`). This is the outermost-surface proof for AC 1 and AC 3:
  - a `/where` command replies `format!("{} {} {}", ctx.user_locale(), ctx.guild_locale(), ctx.response_locale())`;
  - TestApp runs cover rows 2, 4, 5 and 6 (en-GB → `en-US`, pt-BR → `tr`, tr user in an en-US guild);
  - a second App adds `.locale_resolver(..)` for row 29;
  - a Module adds a before-Middleware `set_guild_locale(Locale::Tr)` plus a public `reply_text(keys::X)` for row 36.
- [ ] `crates/noctis/tests/modal_flow.rs`: change the shim to `Locale`.
- [ ] `.github/workflows-disabled/ci.yml`: add `--test i18n_locales` to the `i18n,testkit` line. This keeps the gate in sync.
- [ ] Usage Skill (all files in the Code Map). This is the AC for the skill and API gates:
  - `i18n.md` §7: the exact resolution rules, `available`/`default`, `step`, the DM rule, unknown codes, events and the guild record (with the `GUILDS` intent and the filter), `LocaleResolver` (once per run, verbatim, panic handling, sync only), the Middleware setters, and the rule **"User Locale for ephemeral single-user UI, Guild Locale for shared content"**;
  - remove every `Option<&str>`/`&str` locale accessor, the `.unwrap_or("en-US")` calls, the "[planned, Story 11.3]" markers and the `.default_locale` row;
  - mention every new public item;
  - then run `cargo run --release -p xtask -- api-snapshot --bless` and set `SKILL.md`'s `API hash` to the printed hash.
- [ ] `examples/project-template/AGENTS.md`: one line with the rule and that `ctx.*_locale()` return a resolved `Locale`. This reconciles the template.
- [ ] `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: append any implementation `(decision)` lines. The planning ones are already logged.

**Acceptance Criteria:**

- Given an App built with `i18n!(default = "tr")` over `en-US.yaml` and `tr.yaml`, when TestApp runs `/where` as an `en-GB` user in a DM, a `pt-BR` user in a DM and a `tr` user in an `en-US` guild, then the replies read `en-US en-US en-US`, `tr tr tr` and `tr en-US en-US`.
- Given a guild whose `GUILD_CREATE` carried `preferred_locale = "tr"`, when a `MESSAGE_CREATE` handler in that guild runs, then `ctx.guild_locale()` is `Locale::Tr`; for a guild with no record or a DM it is the App default.
- Given an App `LocaleResolver` that maps guild 7 to `pt-BR`, or a before-Middleware calling `ctx.set_guild_locale(Locale::Tr)`, when the handler runs, then `ctx.guild_locale()` returns the overridden locale and a public `ctx.reply_text(key)` is rendered in it.
- Given the finished story, when the gates run, then `xtask/public-api.txt` matches the re-blessed snapshot, `SKILL.md`'s `API hash` equals it, `skill-check` passes, and `references/i18n.md` states the resolution order and the User/Guild Locale rule.

## Spec Change Log

## Review Triage Log

## Design Notes

These decisions are logged in the architecture memlog.

- **Resolved, non-`Option` accessors.** Authors pass a locale straight to `Strings::of`, and the template is already written that way. A non-i18n bot therefore sees `en-US`. This is honest, because that is the only language it ships. The raw codes are visible to a `LocaleResolver`.
- **`guild_locale()` in a DM falls back to the user.** A DM has one reader. This matches the shipped "public: Guild, then User" response order (AD amendment, Story 9.3). The FR's "User → Guild → default" is the order of `user_locale()`, the interaction's own locale.
- **"Has a file" means any merged catalog has one.** `Strings::of` and `AppStrings` already fall back per catalog, so resolution only picks the best code. The default is the first catalog's, so there is no separate setter to drift.
- **Overrides are verbatim.** "It sees the overridden locale" is the AC. A missing file still renders through each catalog's own fallback chain.
- **The resolver runs at stage 4 only** (AD-23: sync, no port I/O). Rejections before the handler is known keep the built-in resolution, because no invocation exists yet to describe.
- **Guild record.** The spine names a fixed per-guild framework record (Module bitset, locale; NFR-1). Events carry no guild locale, and `DiscordCache` is async port I/O, which stage 4 forbids.
  - The record is bounded by gateway guild membership: interactions never insert, so user-installed apps in foreign guilds cannot grow it.
  - It costs `GUILD_*` parsing only for Apps with event handlers.
- **Golden flow (Command):**

  ```rust
  let raw = (parse(locale), parse(guild_locale));
  let built_in = app.locales.resolve(raw.0, raw.1);
  let resolved = app.locales.resolver(LocaleRequest { invocation, guild_id, user_id: Some(user_id),
      discord_user: raw.0, discord_guild: raw.1, resolved: built_in }); // once, catch_unwind
  let cell = LocaleCell::new(resolved); // shared by CtxInner, I18n, Run, Report
  ```

## Verification

**Commands:**

- `cargo fmt --all`: expected no diff.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: expected clean.
- `cargo test --release --workspace --all-features`: expected all pass, including the `noctis-core` locale, pipeline, worker and middleware tests and `tests/i18n_locales.rs`.
- `cargo test --release -p noctis --features i18n,testkit --test i18n_app --test i18n_modules --test i18n_locales`: expected pass.
- `cargo hack test --release --workspace --each-feature`: expected all pass.
- `cargo run --release -p xtask -- deps-check`: expected ok.
- `cargo run --release -p xtask -- rules-catalog`: expected ok (no rule rows change).
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot`: expected a match. If they are missing, first install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 per AGENTS.md.
- `cargo run --release -p xtask -- skill-check`: expected ok.
- `cargo deny check`: expected ok.
- `cargo +1.91 check --release --workspace`: expected ok.
- `test ! -d target/debug`: expected no debug profile.
