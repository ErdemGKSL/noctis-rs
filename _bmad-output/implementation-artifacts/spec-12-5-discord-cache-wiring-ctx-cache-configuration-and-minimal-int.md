---
title: 'Story 12.5: Discord cache wiring, `ctx.cache()`, configuration and minimal intents'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '1c9b9902fba93e808858f8109398cf495f5afc5a'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The `DiscordCache` port and `MemoryDiscordCache` exist since Stories 3.1/3.2, but nothing feeds them: the App's default backend caches nothing (`ports.rs:111`), no context has `ctx.cache()`, there is no `.discord_cache(..)` configuration, cache needs never join the gateway filter or the intents, and the template's `ctx.cache()` reads do not compile. FR-40, FR-38, FR-6; AD-1, AD-14, AD-21, AD-25; GW-05.

**Approach:** A new core module `discord_cache` owns `DiscordCacheConfig` (which resources, message cap, or `none()`), derives from it the event types to parse, the intents to add and the privileged intents to require (GW-05), and feeds the backend from the drain loop (events, before dispatch) and the interaction tasks (before the pipeline). Every context gets `ctx.cache()`, a cheap `DiscordCacheReader` whose async getters return owned twilight-model values or a typed `CacheError::Disabled(CacheDisabled)`. Docs, template, Usage Skill and testkit seeding follow.

## Boundaries & Constraints

**Always:**
- **Public types** in new `crates/noctis-core/src/discord_cache.rs` (tests in `discord_cache/tests.rs`), re-exported from the `noctis_core` root and the `noctis` facade root (not the prelude), together with `noctis_ports::memory::DiscordCacheResources`:
  - `#[derive(Clone, Copy, Debug)] pub struct DiscordCacheConfig { resources: DiscordCacheResources, message_cap: usize }` with `pub const DEFAULT_RESOURCES = GUILD | CHANNEL | ROLE`; `pub const fn new(resources)` (cap `MemoryDiscordCache::DEFAULT_MESSAGE_CAP`); `pub const fn none()` (= `new(empty())`); `Default` = `new(DEFAULT_RESOURCES)`; `#[must_use] pub const fn with_resources(self, r)` (union), `without_resources(self, r)`, `with_message_cap(self, cap: usize)`; getters `resources()` (stored set minus `MESSAGE` when the cap is 0), `message_cap()`, `is_enabled()` (= `!resources().is_empty()`). `PartialEq`/`Eq` compare `(resources(), message_cap())` (manual impl).
  - `#[derive(Clone)] pub struct DiscordCacheReader` (backend `Option<Arc<dyn DiscordCache>>` + effective resources; `Debug` prints resources only). Async getters, each one backend call except the lists: `guild(Id<GuildMarker>) -> Result<Option<Guild>, CacheError>` [GUILD], `channel` [CHANNEL], `role` [ROLE], `member(guild, user)` [MEMBER], `user` [USER], `message` [MESSAGE], `channel_ids(guild)`/`role_ids`/`member_ids` -> `Result<Option<Vec<Id<_>>>, CacheError>` [CHANNEL/ROLE/MEMBER], and `channels(guild)`/`roles(guild)`/`members(guild)` -> `Result<Option<Vec<T>>, CacheError>` (ids then one get per id, in id order, ids whose get is `None` skipped, first `Err` returned). A getter whose resource is not in the effective set returns `Err(CacheError::Disabled(CacheDisabled { resource }))` **without calling the backend**. `pub fn resources(&self)`, `pub fn is_enabled(&self)`. Every returned future is `Send` and owns or clones what it needs (no borrow of a guard; the values are owned clones).
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)] #[non_exhaustive] pub struct CacheDisabled { pub resource: DiscordCacheResources }`, message `Discord cache resource {resource} is not enabled: add it with AppBuilder::discord_cache(DiscordCacheConfig::default().with_resources(..))`.
  - `#[derive(Debug, thiserror::Error)] #[non_exhaustive] pub enum CacheError { #[error(transparent)] Disabled(#[from] CacheDisabled), #[error("Discord cache backend failed")] Port(#[source] #[from] PortError) }` (`?` into `HandlerError` works through the blanket `From<E: Error>`).
- **noctis-ports** (`memory/discord.rs`, `discord_cache.rs`): `DiscordCacheResources::without` becomes `pub const fn`; `impl Display` (bit order `GUILD | CHANNEL | ROLE | MEMBER | USER | MESSAGE` joined by ` | `, empty = `none`); `impl<P: DiscordCache + ?Sized> DiscordCache for Arc<P>` forwarding every method (like `identify.rs:37`).
- **Resource table** (crate-private `const fn`s in `discord_cache.rs`, one row per resource, union over the set):

  | Resource | Event types fed (EventTypeFlags) | Intents added | Privileged (required) |
  |---|---|---|---|
  | GUILD | GUILD_CREATE, GUILD_UPDATE, GUILD_DELETE, UNAVAILABLE_GUILD | GUILDS | — |
  | CHANNEL | GUILD_CREATE, GUILD_DELETE, UNAVAILABLE_GUILD, CHANNEL_CREATE/UPDATE/DELETE, CHANNEL_PINS_UPDATE, THREAD_CREATE/UPDATE/DELETE, THREAD_LIST_SYNC | GUILDS | — |
  | ROLE | GUILD_CREATE, GUILD_DELETE, UNAVAILABLE_GUILD, ROLE_CREATE/UPDATE/DELETE | GUILDS | — |
  | MEMBER | GUILD_CREATE, GUILD_DELETE, UNAVAILABLE_GUILD, MEMBER_ADD/UPDATE/REMOVE, MEMBER_CHUNK | — | GUILD_MEMBERS |
  | USER | GUILD_CREATE | GUILDS | — |
  | MESSAGE | MESSAGE_CREATE/UPDATE/DELETE, MESSAGE_DELETE_BULK | GUILD_MESSAGES, DIRECT_MESSAGES | — |

  Interactions are fed iff the set intersects `MEMBER | USER | ROLE` (what twilight 0.17.1 `InteractionCreate::update` reads).
- **Builder/App** (`app.rs`): `AppBuilder::discord_cache(&mut self, config: DiscordCacheConfig) -> &mut Self` (last call wins, default `DiscordCacheConfig::default()`); `App::discord_cache(&self) -> DiscordCacheConfig`. In `build()`, after the fold: one `BuildViolation::MissingCacheIntent { resource: DiscordCacheResources, intent: Intents }` per privileged intent a configured resource needs and `.intents(..)` did not request, appended after the fold's violations (update `build`'s `# Errors` order text); message `[GW-05] Discord cache resource {resource} needs the privileged intent {name}: add `.intents(Intents::{name})` and enable it in the Discord Developer Portal, or remove {resource} from `.discord_cache(..)`` (reuse `IntentName`). `App.intents = requested | registry.intents() | cache intents`. Gateway filter = `registry.event_types() | cache event types` (`app.rs:996`); droppable = `registry.droppable().difference(cache event types)` (`app.rs:657`), so a cache-fed type is never dropped. When the `DiscordCache` slot took its default, replace it after `Ports::resolve` with `MemoryDiscordCache::with_message_cap(config.resources(), config.message_cap())` (keep the macro default; update the setter's doc example text to say the default follows `.discord_cache(..)`). With `none()` and a custom backend, log once at build `warn!(target: "noctis::app", "a DiscordCache backend is set but the Discord cache is disabled (DiscordCacheConfig::none()); it is never read or fed")`.
- **AppShared** (`handler.rs:203`) gains `discord_cache: Arc<crate::discord_cache::CacheFeed>` (backend, config, `healthy: AtomicBool` starting `true`).
- **Feeding** (crate-private `CacheFeed::event(&self, app: &str, event: &Event)` and `::interaction(&self, app, &Interaction)`):
  - Events: in `worker.rs::drain`, right after `locales.observe(&event)` and before the `handlers(..).is_none()` check, `await` the feed when `EventTypeFlags::from(event.kind())` is in the cache set; nothing else changes order. Each update is bounded by `CACHE_UPDATE_TIMEOUT = 1 s` (`tokio::time::timeout`).
  - Interactions: in `Workers::interaction`, inside each spawned task (command, autocomplete, component/modal) and before `run_*` starts, feed `Event::InteractionCreate(Box::new(InteractionCreate(interaction.clone())))` when the set feeds interactions; pings are not fed. The ack deadline still counts from `Inbound::received_at` (unchanged).
  - A failed or timed-out update never stops the event or interaction: increment `noctis_discord_cache_update_errors_total{app}`; when `healthy` flips `true -> false` log one `warn!(target: "noctis::cache", error = %chain, "Discord cache updates are failing; events still reach their handlers")`, when a later update succeeds and it flips back log one `info!(target: "noctis::cache", "Discord cache updates recovered")`. No other log per failure; never log event contents.
  - Disabled config: never call the backend.
- **Metric**: add `noctis_discord_cache_update_errors_total` (counter, labels `app`) to `observe::METRICS` (doc table + row), `describe_metrics`, and `observe/tests.rs`'s label list.
- **`ctx.cache()`** next to every `kv()` (`command.rs:297`, `autocomplete.rs:327`, `component.rs:237` macro = `ComponentCtx`/`ModalSubmitCtx`, `handler.rs:355` `EventCtx`, `middleware.rs:810` macro = `BeforeCtx`/`AfterCtx`): `pub fn cache(&self) -> crate::DiscordCacheReader` via `DiscordCacheReader::of(Option<&Arc<AppShared>>)`; a context without an App gets a reader with no backend and an empty set (every getter `Disabled`).
- **Testkit**: hidden `__testing::AppProbe::feed_discord_cache(&self, event: &Event) -> BoxFuture<'_, ()>` (the same `CacheFeed::event` path and event-type filter, so with `none()` a seed is a no-op). `TestApp::guild(..)` keeps its signature; every `*Run::run` (`app/runs.rs`), under the run lock and before sending its interaction, feeds each not-yet-fed guild seed once as `GUILD_CREATE` (an available `Guild` deserialized from JSON: id, name, `preferred_locale` = seed locale, channels with `guild_id`/name/type/position 0, roles with name/position = seed index/permissions `0`/flags 0, `owner_id` 1, every other required field at Discord's zero value); a re-seeded id is fed again. Remove the `dead_code` expectation on `TestGuild::name`. Interactions already carry the user, so no user seeding is added.
- **Docs**: `docs/discord-rules.md` GW-05 Notes add "a configured Discord cache resource's privileged intent (`MEMBER` → `GUILD_MEMBERS`) is `BuildViolation::MissingCacheIntent`; non-privileged ones are added". Spine: append to AD-1's `DiscordCache` bullet (`:115-119`) an `Amended (2026-10-24, Story 12.5)` line: default resources `GUILD | CHANNEL | ROLE`, feed points (drain before dispatch, interaction task before pipeline), never droppable, failures counted and never block. Memlog: `(decision)` lines for the default set, interaction feeding, failure policy, `DiscordCacheReader` naming/`CacheError`.
- **Template** (`examples/project-template/`): `src/lib.rs::configure` adds `.discord_cache(DiscordCacheConfig::default().with_resources(DiscordCacheResources::USER))` with a comment (profile reads users; GUILD|CHANNEL|ROLE come with the default); `src/modules/dashboard/handlers.rs:121-123` → `guild(..).await?.map_or_else(|| guild.to_string(), |g| g.name)` and `roles(guild).await?.unwrap_or_default()`; `src/modules/profile/handlers.rs:200` → `Option<User>` with a fallback name `user_id.to_string()` when not cached; `tests/flows.rs:166-174` drop `#[ignore]` and the `[planned, Story 12.5]` comment (seed `TestGuild` roles there if missing); `AGENTS.md`: one bullet on `ctx.cache()` (owned values, `Option` = not cached, `CacheError::Disabled` = resource not configured, default resources, `.discord_cache(..)`, `none()`, `.discord_cache_backend(..)`, MEMBER needs `GUILD_MEMBERS`).
- **Usage Skill** (`.claude/skills/noctis/`): `adapters.md` §1 table row and the `ctx.cache()` section (`:134-140`, drop `[planned]`, full getter list, errors, config API, backend swap with `Arc` for a Cluster, feeding and failure policy), `app-and-sharding.md` builder rows (`:128`, `:148`), intents/filter section (`:495`, resource table), §11 metric row; `modules-middleware-ratelimits.md:239` (`cache()` shipped on EventCtx/BeforeCtx/AfterCtx); `testing.md:81-82` (guild seeds feed `ctx.cache()`); `limits-cheatsheet.md` GW-05 note; `troubleshooting.md` one entry for `MissingCacheIntent` and `CacheError::Disabled`; `SKILL.md` API hash.
- `rust-best-practices` applies (`references/api-design.md`, `async-and-concurrency.md`, `error-handling.md`, `testing.md`).

**Never:** no env var for cache config; no cache feed or getter in `noctis-gateway`; no reference, guard or lock exposed by the reader; no `Namespaced` wrapping of `DiscordCache`; no per-event warn spam; no blocking of handlers or interactions on a failed/slow update beyond the 1 s bound; no change to `Registry::fold`'s signature or its event types (cache types join at the App level); no new resource kinds (presences, voice, emojis) and no change to `MemoryDiscordCache`'s lossy-field rules; no network in tests.

## I/O & Edge-Case Matrix

Pure (`discord_cache/tests.rs`, `noctis-ports` unit tests):

| # | Input | Expected |
|---|---|---|
| P1 | `DiscordCacheConfig::default()` | resources `GUILD\|CHANNEL\|ROLE`, cap 100, enabled; intents `GUILDS`; privileged empty; feeds interactions (ROLE) |
| P2 | `none()`, `new(empty())`, `new(MESSAGE).with_message_cap(0)` | all `!is_enabled()`, `resources()` empty, event types empty, intents empty; `none() == new(empty())` |
| P3 | `new(all())` | event types = union of every row; intents `GUILDS\|GUILD_MESSAGES\|DIRECT_MESSAGES`; privileged `GUILD_MEMBERS`; feeds interactions |
| P4 | each single resource | exactly its table row (event types, intents, privileged); interactions fed only for MEMBER, USER, ROLE |
| P5 | `with_message_cap(0)` then `with_resources(MESSAGE)`; `with_resources(MESSAGE).with_message_cap(0)` | `resources()` has no MESSAGE both ways |
| P6 | `with_message_cap(1)`, `usize::MAX` | accepted, `message_cap()` returns it, MESSAGE kept |
| P7 | `without_resources(ROLE)` on default; `without_resources(all())` | `GUILD\|CHANNEL`; empty (disabled) |
| P8 | `DiscordCacheResources` Display: empty / `GUILD` / `ROLE\|GUILD` / `all()` | `none` / `GUILD` / `GUILD \| ROLE` / all six in bit order |
| P9 | `CacheDisabled{MEMBER}` Display; `CacheError::Port` source | contains `MEMBER` and `AppBuilder::discord_cache`; `source()` is the `PortError` |

Reader (`discord_cache/tests.rs`, `MemoryDiscordCache` fed with twilight events, plus a stub backend that counts calls and can fail):

| # | State | Call | Expected |
|---|---|---|---|
| G1 | default, GUILD_CREATE fed (2 channels, 3 roles) | `guild`, `channel`, `role`, `role_ids`, `roles`, `channel_ids`, `channels` | owned values; ids sorted; `roles` 3 in id order |
| G2 | default, nothing fed | `guild(1)`, `roles(1)` | `Ok(None)`, `Ok(None)` |
| G3 | default | `member`, `members`, `member_ids`, `user`, `message` | `Err(Disabled{MEMBER/MEMBER/MEMBER/USER/MESSAGE})`, stub backend call count 0 |
| G4 | `none()` | every getter | `Disabled{<its resource>}`, 0 backend calls |
| G5 | detached reader (`of(None)`) | `guild(1)` | `Disabled{GUILD}` |
| G6 | stub `role_ids` = [1,2,3], `role(2)` = None | `roles(g)` | `Some([r1, r3])` |
| G7 | stub `role(2)` = Err | `roles(g)` | `Err(Port(..))` |
| G8 | stub every getter Err | `guild(1)` | `Err(CacheError::Port(e))`, message preserved |
| G9 | type-level | — | `DiscordCacheReader: Clone + Send + Sync + 'static`; each getter future `Send`; outputs `'static` (static assert fns) |

Feed (core unit tests on `CacheFeed` with a stub backend and a capturing subscriber + `DebuggingRecorder`, or testkit where marked):

| # | Scenario | Expected |
|---|---|---|
| F1 | default, ROLE_CREATE event | `update` called once |
| F2 | default, MESSAGE_CREATE | not called |
| F3 | `none()` + custom backend | never called; one build warn naming `DiscordCacheConfig::none()` |
| F4 | update Err, Err, Ok, Err | counter +3; exactly one warn after the first Err, one info after the Ok, one warn after the last Err |
| F5 | update sleeps 5 s (paused clock) | returns after 1 s, counter +1, warn once |
| F6 | interaction feed: default vs `new(GUILD\|CHANNEL)` | fed (as `InteractionCreate`) vs not fed |
| F7 | droppable: App with an all-droppable `RoleCreate` handler, default cache | App droppable flags exclude ROLE_CREATE; with `none()` include it |

Build (`app/tests.rs`; names contain `gw_05` where marked):

| # | Builder | Expected |
|---|---|---|
| B1 | `App::builder().build()` | `intents() == GUILDS`; `discord_cache() == default()`; gateway filter ⊇ GUILD_CREATE, ROLE_CREATE, CHANNEL_CREATE, THREAD_LIST_SYNC, INTERACTION_CREATE; no MESSAGE_CREATE |
| B2 | `.discord_cache(none())` | filter == `registry.event_types()` (INTERACTION_CREATE only for interaction-only) |
| B3 | `.discord_cache(new(MEMBER))` without `GUILD_MEMBERS` (gw_05) | `BuildError` with exactly one `MissingCacheIntent{MEMBER, GUILD_MEMBERS}`; text has `[GW-05]`, `MEMBER`, `GUILD_MEMBERS`, `.discord_cache(` |
| B4 | same + `.intents(GUILD_MEMBERS)` | builds; intents ⊇ `GUILD_MEMBERS`; filter ⊇ MEMBER_ADD, MEMBER_CHUNK |
| B5 | `.discord_cache(new(MESSAGE))` | builds; intents == `GUILDS\|GUILD_MESSAGES\|DIRECT_MESSAGES` |
| B6 | MEMBER missing + a `GuildMemberAdd` handler missing `GUILD_MEMBERS` | handler `MissingIntent` first, then `MissingCacheIntent` |
| B7 | two `.discord_cache(..)` calls | the last wins |
| B8 | default slot | backend is a `MemoryDiscordCache` of the config's resources/cap (observable via B1-style read after feeding) |

End to end (new `crates/noctis-testkit/tests/discord_cache.rs`, `FakeGateway` + `FakeDiscordHttp`/`TestApp`):

| # | Scenario | Expected |
|---|---|---|
| E1 | default App; `GUILD_CREATE` (guild 10 "G", role 30 "Mods", channel 20) dispatched; then a slash command reading `ctx.cache().guild(10)`, `.roles(10)`, `.channel(20)` | reply names "G", "Mods", channel 20 |
| E2 | ordering: `RoleCreate` event handler reads `ctx.cache().role(id)` | sees `Some` (fed before dispatch) |
| E3 | each context: `ComponentCtx`, `ModalSubmitCtx`, `AutocompleteCtx`, `EventCtx`, `BeforeCtx` | `cache().guild(10)` returns the seeded guild |
| E4 | interaction feed: `.discord_cache(default().with_resources(USER))`, command by user 40 with a resolved user 41 option | handler reads `cache().user(40)` and `user(41)` as `Some` |
| E5 | `none()` | handler gets `CacheError::Disabled{GUILD}`; IDENTIFY intents == `GUILDS` |
| E6 | Cluster stand-in: one `Arc<SerdeCache>` (test backend wrapping a `MemoryDiscordCache(all())` that round-trips every update and every returned value through `serde_json` bytes, as an out-of-process store would) passed to App A and App B via `.discord_cache_backend(Arc::clone(..))`; GUILD_CREATE only to A's gateway | B's command handler reads the guild name from `ctx.cache()` |
| E7 | `TestApp::guild(TestGuild …role…)` then a command reading `roles(10)` | seeded roles returned (seed feeding) |
| E8 | backend whose `update` always errs | events/commands still answered; one `noctis::cache` warn |

Compile-fail (`crates/noctis/tests/types/discord_cache_no_guard.rs` + `.stderr`): in a `CommandCtx` handler, `let g = ctx.cache().guild(id).await?.unwrap(); let _ = g.value();` fails with E0599 `no method named `value` found for struct `Guild`` — twilight's cache returns dashmap guards with `.value()`; the reader returns the owned `Guild`, so no guard exists to hold across `.await`.

</intent-contract>

## Code Map

- `crates/noctis-ports/src/discord_cache.rs` -- trait; add `Arc<P>` forwarding impl. `memory/discord.rs:28-102` -- `DiscordCacheResources` (`without` pub, Display); `MemoryDiscordCache::with_message_cap` (`:206`) is the default backend constructor.
- `crates/noctis-core/src/ports.rs:107-112` -- `discord_cache_backend` plain slot (default replaced after resolve when `is_default(PortSlot::DiscordCache)`).
- `crates/noctis-core/src/app.rs` -- builder fields/`new` (`:1680-1720`), `port_setters!` doc (`:2496`), `build()` (`:2545-2760`: fold at `:2672`, `Ports::resolve` at `:2711`, `AppShared` at `:2720`, `intents` at `:2743`), `App::intents` doc (`:557-570`), droppable (`:657`), gateway options `event_types` (`:996`), Debug impls (`:1521`, `:2835`).
- `crates/noctis-core/src/registry.rs:299-312` -- `event_types`/`droppable`/`intents` (read-only; combine at App level).
- `crates/noctis-core/src/error.rs:447-465` -- `MissingIntent` pattern; add `MissingCacheIntent` after it; `IntentName` (`:922`).
- `crates/noctis-core/src/worker.rs:254-280` -- drain: feed after `locales.observe`; `:135-180` `Workers::interaction`: feed inside each spawned task.
- `crates/noctis-core/src/handler.rs:203-249` -- `AppShared` field; `:355` `EventCtx::kv` neighbour.
- `crates/noctis-core/src/{command.rs:297,autocomplete.rs:327,component.rs:237,middleware.rs:810}` -- `cache()` next to `kv()`; detached test seams construct `AppShared`-less contexts (reader `of(None)`).
- `crates/noctis-core/src/kv.rs` -- model for `DiscordCacheReader` (`of`, detached, docs with example).
- `crates/noctis-core/src/observe.rs:75-215`, `observe/tests.rs:100` -- metric row.
- `crates/noctis-core/src/lib.rs:228-310` -- re-exports; `:351-470` `__testing::AppProbe` (add `feed_discord_cache`).
- `crates/noctis-core/src/test_support.rs:76-90` -- `guild_create` helpers to reuse in unit tests.
- `crates/noctis/src/lib.rs:595-611` -- facade root re-exports (`DiscordCacheConfig`, `DiscordCacheReader`, `DiscordCacheResources`, `CacheError`, `CacheDisabled`); `:1931-2060` `noctis::ports` docs mention `ctx.cache()`/config.
- `crates/noctis-testkit/src/{app.rs:241,seed.rs:25-35,app/runs.rs}` -- seed feeding; `tests/events.rs` (capture + `Running` helpers), `tests/testapp.rs`, `tests/cluster.rs` patterns for E1-E8.
- `crates/noctis/tests/ui.rs:53` -- `types/**` compile-fail suite (trybuild never `--release`; regenerate `.stderr` with `TRYBUILD=overwrite`).
- Docs/template/skill paths as listed under Always; `xtask/public-api.txt` re-bless.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ports/src/{discord_cache.rs,memory/discord.rs}` -- `Arc<P>` impl, `without` pub, Display; P8 tests.
- `crates/noctis-core/src/discord_cache.rs` (+`discord_cache/tests.rs`), `lib.rs` -- config, resource table, reader, errors, `CacheFeed`; P1-P7, P9, G1-G9, F1-F6.
- `crates/noctis-core/src/{error.rs,app.rs,ports.rs,handler.rs,worker.rs,observe.rs}` + `app/tests.rs`, `observe/tests.rs` -- violation, builder/App wiring, filter/droppable/intents, feed points, metric; B1-B8, F7.
- `crates/noctis-core/src/{command.rs,autocomplete.rs,component.rs,middleware.rs}` -- `cache()` on every context.
- `crates/noctis/src/lib.rs`, `crates/noctis/tests/types/discord_cache_no_guard.{rs,stderr}` -- facade re-exports, docs, compile-fail.
- `crates/noctis-testkit/src/{app.rs,app/runs.rs,seed.rs}`, `crates/noctis-testkit/tests/discord_cache.rs` -- seeding, E1-E8.
- `docs/discord-rules.md`, spine AD-1 amendment, memlog -- records.
- `examples/project-template/{src/lib.rs,src/modules/dashboard/handlers.rs,src/modules/profile/handlers.rs,tests/flows.rs,AGENTS.md,.claude/skills/noctis/**}` -- reconcile; then `cargo run --release -p xtask -- api-snapshot --bless` and put the printed hash into `SKILL.md`'s `API hash` line.

**Acceptance Criteria:**
- Given the default configuration, when an interaction-only App is built, then it caches GUILD, CHANNEL and ROLE, identifies with exactly `Intents::GUILDS`, and parses only interactions plus the table's event types for those resources (B1, E5 for `none()`).
- Given a running App, when a gateway event of a cached resource arrives, then the backend is updated before any handler of that event runs, and the event still reaches its handlers when the update fails or times out (E2, E8, F4, F5).
- Given any of `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx`, `AutocompleteCtx`, `EventCtx`, when a handler calls `ctx.cache()` getters, then it receives owned values, `Ok(None)` for a miss, and `CacheError::Disabled(CacheDisabled { resource })` for a resource not configured or with `none()` (E1, E3, G3, G4).
- Given a configured resource needing a privileged intent that was not requested, when `App::build` runs, then it fails with a `[GW-05]` `MissingCacheIntent` naming the resource and the intent (B3).
- Given one out-of-process-style backend shared by two Apps through `.discord_cache_backend(Arc::clone(..))`, when only App A receives a guild, then App B's handler reads the same guild (E6).
- Given the story is done, when every local gate runs (`Verification`), then all pass, the compile-fail fixture holds, the template's cache reads match the shipped API, and the Skill's API hash equals the hash `api-snapshot --bless` printed.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 35 findings — high 0, medium 2, low 24, false 9, maybe-false 0
- findings:
  - `[low]` `[reject]` (blind) Drain throughput drops to ~1 event/s while a backend hangs — real only for a hanging (not failing) backend; the spec's Design Notes deliberately accept that a slow backend throttles intake to keep gateway order before dispatch; a circuit breaker adds state and would drop ordered updates.
  - `[low]` `[reject]` (blind) Interaction feed spends up to 1 s of the ack window — only with a slow backend; auto-defer still fires at 2000 ms from `received_at` as the spec requires; moving the feed changes the spec-settled order and adds branches.
  - `[low]` `[patch]` (blind) A timed-out `update` is cancelled midway and can leave an out-of-process backend half-written — real for multi-key custom backends; patch documents that `DiscordCache::update` must be cancel-safe (dropped after 1 s).
  - `[low]` `[patch]` (blind) A custom backend caching fewer resources than the config is not detected; getters return `Ok(None)` forever — real; patch adds a doc sentence on `discord_cache_backend` that a custom backend must cache every configured resource.
  - `[false]` `[reject]` (blind) Cache memory not reported (NFR-1) — Story 12.6 (memory bench) owns reporting Discord cache bytes separately; the spine sentence is satisfied there, not by a gauge here.
  - `[low]` `[reject]` (blind) `USER` row fed only by `GUILD_CREATE`, users go stale — spec-settled table; users are refreshed by every interaction they make, member events need privileged `GUILD_MEMBERS`; widening the row is not a direct correction.
  - `[low]` `[reject]` (blind) `MESSAGE` row has no purge events and no `MESSAGE_CONTENT` warning — memory stays bounded by cap × channels seen, `MESSAGE` without `CHANNEL` is unusual, and the Usage Skill (bot authors' only source) already notes empty content; fix changes the spec table.
  - `[low]` `[reject]` (blind) `READY` never fed, guilds left while disconnected stay cached — rare (non-resumed reconnect after leaving a guild), entries are unreachable by interactions; reconciliation logic is not a direct fix.
  - `[low]` `[reject]` (blind) List getters make 1 + n sequential calls; rustdoc contradicts itself — 1 + n is the spec's design (port has no batch read); the rustdoc already says list getters "read the ids and then each value", so no contradiction.
  - `[low]` `[reject]` (blind) Interaction feed deep-clones every interaction — negligible cost; a pre-check adds branches.
  - `[low]` `[reject]` (blind) `DiscordCacheConfig` equality/Debug disagree with semantics — `PartialEq` on `(resources(), message_cap())` and P5 masking are spec-settled; only the derived `Debug` showing the raw field is real and cosmetic, and a manual impl is more than a direct correction.
  - `[low]` `[reject]` (blind) `feeds_interactions` uses double `without` instead of `intersects` — readability only (commented); the suggested fix adds public surface.
  - `[low]` `[reject]` (blind) TestApp seeds only guilds, not members/users — spec says no user seeding (interactions carry the user); a new seeding hook adds public surface.
  - `[low]` `[patch]` (blind) Duplicate Display test and stale `discord_cache_backend` doc example — the doc half shares the backend-mismatch root cause and is fixed by that doc sentence; the duplicated test is negligible developer cost, not patched.
  - `[low]` `[reject]` (edge) worker.rs drain 1 s per event while backend hangs — same as the blind drain-throughput row; deliberate spec trade-off.
  - `[low]` `[reject]` (edge) Interaction feed concurrent with drain can overwrite a newer MEMBER/ROLE update with older interaction data — real but narrow race, self-heals on the next update; serialising through the drain is a design change.
  - `[low]` `[patch]` (edge) Timeout drops `update` mid-write — grouped with the blind cancel-safety row; same doc patch.
  - `[low]` `[reject]` (edge) Concurrent applies finishing out of order flap the healthy flag — affects logs only; a generation counter adds state.
  - `[low]` `[reject]` (edge) Shutdown waits up to 1 s per in-flight interaction feed — bounded by the 1 s timeout and covered by the shutdown grace; a select adds branches.
  - `[low]` `[patch]` (edge) Custom backend caching fewer resources silently returns `Ok(None)` — grouped with the blind backend-mismatch row; same doc patch.
  - `[false]` `[reject]` (edge) `with_message_cap(0)` then `with_message_cap(n)` restores `MESSAGE` — the spec defines `resources()` as the stored set masked by cap 0, and the method doc speaks of `resources()`; restoring with a positive cap is the specified behavior.
  - `[low]` `[reject]` (edge) `USER` excludes `USER_UPDATE`/member events — grouped with the blind `USER` row; `USER_UPDATE` concerns only the bot's own user in twilight.
  - `[low]` `[reject]` (edge) `MESSAGE` without `MESSAGE_CONTENT` yields empty content — documented in the Usage Skill; a build warning adds a branch.
  - `[false]` `[reject]` (edge) testkit marks seeds fed before `create_event` succeeds — `create_event` fails only on a testkit bug, and that run already fails loudly with `TestError::Unsupported`.
  - `[low]` `[reject]` (edge) Re-seeded guild leaves old channels/roles readable by id — twilight resets the per-guild id sets, so lists are correct; only direct by-id reads of removed seed entries remain, rare in tests; feeding `GUILD_DELETE` first adds code.
  - `[low]` `[reject]` (edge) Seeds not fed before a raw `gateway_fake().dispatch` — testing.md states seeds are fed "before the next run"; a second feed path adds code.
  - `[medium]` `[patch]` (vgap) Gateway filter wiring of cache-fed types untested at the shard boundary — pre-verified; patch adds an E-style test with no `GuildCreate`/`ChannelCreate` handler that dispatches `GUILD_CREATE` and `CHANNEL_CREATE` and reads them back via `ctx.cache()`.
  - `[medium]` `[patch]` (vgap) Interaction feed in autocomplete and component branches untested — pre-verified; patch extends E4 so a click and an autocomplete by a never-seen user read `cache().user(<invoker>)` as `Some`.
  - `[low]` `[reject]` (vgap) TestApp re-seed refeed untested — pre-verified gap but a regression would be unlikely and visible quickly; a new test is more than a direct correction.
  - `[false]` `[reject]` (intent) Default resources fixed, not derived from features — Story 12.6's "stated default cache configuration (no members, no messages)" measured against `none()` selects the fixed-set reading the spec settled.
  - `[false]` `[reject]` (intent) GW-05 cache check covers only privileged intents — the sibling AC (default intents are the minimum required by handlers and cache resources) implies non-privileged ones are added, as for handlers in Story 5.6.
  - `[false]` `[reject]` (intent) Out-of-process backend is an in-process serde stand-in — tests may not use the network (spec Never); the serde round-trip models the boundary, and the `Arc<P>` impl is what a Cluster uses.
  - `[false]` `[reject]` (intent) `CacheDisabled` wrapped in `CacheError` — the typed `CacheDisabled` is returned (as `CacheError::Disabled`); backend failures need a variant too.
  - `[false]` `[reject]` (intent) Template reconciled toward the API — "reconciled" admits either direction; the `Option` shape keeps the port's not-cached meaning.
  - `[false]` `[reject]` (intent) No-guard check uses one method name; `shared_identify_queue.stderr` regenerated — the intent allows a compile-fail fixture and G9 asserts owned/`Send` outputs for every getter; the stderr change follows from the new facade re-exports changing rustc's printed paths.

## Design Notes

- **Default resources = GUILD | CHANNEL | ROLE** (decision): exactly what the always-requested `GUILDS` intent delivers, nothing that needs another intent or grows per user or message — the "stated default cache configuration (no members, no messages)" that NFR-1 and Story 12.6 measure against `none()`. An interaction-only bot keeps `Intents::GUILDS` but now parses the guild/channel/role lifecycle events (FR-6: "or that the cache needs"); `none()` restores the bare interaction filter. USER is opt-in because users cached from interactions are never evicted.
- **Feed in the drain, inline** (decision): ordering of cache updates must follow gateway order and precede dispatch, so the drain awaits the update (bounded 1 s). A slow backend therefore throttles event intake; the Skill says to use a fast backend. Interactions are fed inside their own task so the interaction lane never waits on the cache.
- **Failure policy**: state-flip logging (one warn per outage, one info on recovery) plus a counter, never per-event logs.
- **Disabled vs miss**: `Option` keeps the port's meaning (not cached); `CacheError::Disabled` tells the author a configuration mistake, checked before any backend call so it is cheap and deterministic.
- Reader getter shape:

```rust
pub async fn role(&self, id: Id<RoleMarker>) -> Result<Option<Role>, CacheError> {
    let backend = self.backend_for(DiscordCacheResources::ROLE)?; // Disabled if not configured
    Ok(backend.role(id).await?)
}
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` (wrap in `tools/story-loop/bg.sh`) -- clean.
- `cargo test --release -p noctis-ports`, `-p noctis-core`, `-p noctis-testkit --test discord_cache`, `-p noctis --test ui` -- pass.
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass.
- `cargo run --release -p xtask -- deps-check`, `-- tw-grammar --check`, `-- rules-catalog`, `-- api-snapshot` (after `--bless`), `-- skill-check` -- pass.
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass.
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass.
- `cargo metadata --format-version 1 --manifest-path examples/project-template/Cargo.toml > /dev/null` -- resolves (template build stays tracking-only).

## Auto Run Result

- **Summary:** Wires the Discord cache port into the runtime: `ctx.cache()` on every context (owned, `Send` outputs, no guards), `DiscordCacheConfig` (resource set, message cap; default fixed set with no members/messages), gateway feed through the worker drain with a 1 s per-call timeout plus interaction feeding (commands, components, autocomplete), minimal default intents derived from handlers and cache resources (GW-05), custom/out-of-process backends via `discord_cache_backend`, and TestApp guild seeding fed into the cache.
- **Files changed:** `crates/noctis-core/src/discord_cache.rs` (+tests) facade, config, feed; `worker.rs`/`app.rs` feed wiring and backend selection; context modules (`command`, `component`, `autocomplete`, `handler`, `middleware`) expose `cache()`; `error.rs` `CacheError`; `noctis-ports` port docs and memory impl; `noctis-testkit` seeding and `tests/discord_cache.rs`; `noctis` facade re-exports and compile-fail no-guard fixture; `docs/discord-rules.md`; project template code, AGENTS.md and Usage Skill references; `xtask/public-api.txt` re-blessed.
- **Review findings (35: high 0, medium 2, low 24, false 9):** patched 2 medium (gateway-filter E-style test for cache-fed `GUILD_CREATE`/`CHANNEL_CREATE` without handlers; E4 extended so click and autocomplete by an unseen user populate `cache().user`) and 4 low grouped into 2 doc patches (`DiscordCache::update` must be cancel-safe; custom backend must cache every configured resource). Deferred: none. Rejected 29 with recorded reasons in the triage log (spec-settled design trade-offs such as drain throttling under a hanging backend, `USER`/`MESSAGE` row scope, 1+n list reads; 9 false positives).
- **Follow-up review recommended:** true (patched: high 0, medium 2, low 4). Named risk: the new shard-boundary gateway-filter test and the extended interaction-feed test have not been independently reviewed for whether they truly exercise the intent filter and component/autocomplete feed paths rather than passing via seeding.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, deps-check, tw-grammar, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV); CI paused by maintainer.
- **Residual risks:** a hanging backend throttles intake to ~1 event/s (accepted); narrow race where an interaction feed overwrites a newer member/role update until the next event; Discord cache memory reporting deferred to Story 12.6.
