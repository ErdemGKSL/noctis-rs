# Epic 13 Context: Module operations

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Make Modules a first-class operational unit. A bot author composes Modules into trees with unique path ids, picks feature sets at boot with Boot Flags, enables or disables Modules at runtime (globally or per guild, consistently across a Cluster), rate-limits Commands and Routes before handlers run, and reacts to framework lifecycle events through hooks. One binary can then run different feature sets, switch features off without a deploy, and throttle abuse, with each log line saying which Module it came from.

## Stories

- Story 13.1: Module trees with unique path ids
- Story 13.2: Boot-time feature flags
- Story 13.3: Runtime Module enable and disable
- Story 13.4: Rate limits on Commands and Routes
- Story 13.5: Lifecycle Hooks

## Requirements & Constraints

- **Module tree:** Module ids are the author-written full path (`Module::new("profile").module(Module::new("profile/badges"))`), never joined at nest time. Grammar: `[a-z0-9_-]+` segments joined by single `/`, 1..=64 chars; a root is one segment, a child is `<parent>/<one segment>`; unique per App (all trees, plugins included). Validated at App build in `Registry::fold`: `BuildViolation::InvalidModuleId { id, reason: ModuleIdError }` and `DuplicateModule { id }`, listed first; no Discord rule id. Nested registrations fold into the Registry; Middleware runs parent before child at every level. Registrations carry the Module id in the `module` span field and in error context.
- **Boot Flags:** declared with `Module::flags([..])` and a `flags = [..]` argument on every kind attribute (never `#[autocomplete]`); grammar `[a-z0-9_-]{1,32}`, `all`/`none` reserved (`noctis_rules::boot_flag`, no Discord rule id). An item exists when the selection is `all` or holds any of its flags; nesting is AND. Selection `BootFlags` (default `all`) comes from `AppBuilder::boot_flags`, `NOCTIS_FEATURES` or `--features` (argv wins), fixed before `build()`; `run` fails with `CliError::BootFlagsMismatch` on a differing `--features`. Unknown selected flag is `UnknownBootFlag`. Unselected registrations are not registered, dispatched or published; dry-run JSON/diff reflect the selection; locale catalogs and Templates stay App-wide.
- **Runtime enable/disable:** `ctx.modules().set_enabled(module, guild, enabled)` writes through `ModuleStateStore`; other processes poll `version()` (default 5 s) and propagation delay equals the poll interval (documented). `is_enabled` reads the cached snapshot with no port I/O. A disabled Module's global Command in a guild gets a localized ephemeral `noctis.module.disabled` notice; its components and Event Handlers don't run. Per-guild Publish drops only its guild-scoped Commands (re-counted against REG-02); global Commands stay published.
- **Rate limits:** "N uses per window" per user, member, channel, guild or message, each scope its own key, store App-namespaced. Checked via `RateLimitStore::incr_with_ttl` before auto-defer and handler; limited users get an immediate localized ephemeral notice with time remaining. Handlers can set/extend limits dynamically. Custom limited response overridable per App, Command or Route (most specific wins).
- **Lifecycle Hooks:** App started; Shard ready/resumed/disconnected/fatally closed; all Shards ready; Publish completed or failed (with diff); Module enabled/disabled (with scope); shutdown started. Each fires exactly once per occurrence with typed data, observable in order by a test App. Hook errors and panics go to an Error Sink; the App keeps running.
- Every public API change updates the Usage Skill (and `examples/project-template/`), re-blesses the API snapshot, and passes `skill-check`.

## Technical Decisions

- **Registry is data, built once:** Modules emit plain descriptors (commands, Routes, events, middleware, hooks, locale catalogs). Boot Flags filter descriptors before the fold. The Registry is immutable; Module enablement is a per-Module bitset (global plus per-guild overrides) held in `arc-swap`, refreshed on the store's version poll. That snapshot is the only swapped state.
- **Ports:** dyn-compatible `Send + Sync + 'static`, methods return `BoxFuture<'_, Result<T, PortError>>`, App holds `Arc<dyn Port>`. `RateLimitStore` uses `incr_with_ttl` (atomic, TTL set on create), `get`, `put`; `ModuleStateStore` adds `version() -> u64`. Only core's `Namespaced<P>` prefixes keys `<app>:<port>:`; adapters never prefix; key segments escape `:`. Values are postcard with a leading schema byte. Memory defaults must pass testkit conformance suites.
- **Fixed pipeline:** stage 3 = Boot Flag / GuildAccessList / Module-enabled checks; stage 4 locale; stage 5 rate-limit increment; stage 6 auto-defer; 7 before-Middleware; 8 Guards; 9 handler; 10 after-Middleware and Error Sink. Stages 1-5 run no author code (except LocaleResolver) and no port I/O beyond Ref lookup, cached Module state and the rate-limit increment. Notices from stages 3 and 5 are localized ephemeral replies (follow-up plus delete of deferred original if already deferred). Events run stages 3, 4, 7, 9, 10.
- **Authoring surface:** `rate_limit(..)` is an argument on `#[command(..)]` and on component kind attributes (`#[button("id", rate_limit(..), defaults(..))]`); `#[handler]` takes no arguments. Registration is by type (`module.add::<T>()`). Hooks are registered via `#[hook]` on a Module.
- **Lifecycle ownership:** the gateway only reports `ShardEvent`s on a channel; only core turns them into hooks. Shutdown order: fire `ShutdownStarted`, close shards, drain lanes, await closed, drop ports.
- **Observability:** spans `noctis.interaction`, `noctis.event`, `noctis.publish`, etc. carry `app`, `shard`, `guild_id`, `module`, route/command path. Metrics use the `noctis_` prefix and `app` label; labels never carry ids or free text, so no metric gains a `module` label. Error Sinks receive hook errors.
- Errors are per-crate `thiserror` enums; `noctis::Error` is `#[non_exhaustive]`.

## Cross-Story Dependencies

- 13.1 builds on basic Module nesting from Story 5.6; 13.2 extends the Story 6.9 CLI parser and Publish dry run; 13.5 consumes shard status events from Story 5.8.
- 13.3 requires 13.1's stable Module ids; 13.5's Module-toggle hook depends on 13.3.
- 13.3 reconciles the template dashboard's Module toggles; 13.4 reconciles the template's `/profile` rate limit.
- Cluster behavior (shared stores, designated publisher) comes from Epic 12.
