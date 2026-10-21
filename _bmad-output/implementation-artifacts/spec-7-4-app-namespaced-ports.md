---
title: 'Story 7.4: App-namespaced ports'
type: 'feature'
created: '2026-10-21'
status: 'in-progress'
baseline_revision: '33615617482c22369d0d71b66288b2514dba9cc9'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/adapters.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Every port key reaches the Adapter un-prefixed, and the App label is the hard-coded `"default"`. Two Apps sharing one Adapter instance therefore read and overwrite each other's Refs, sessions, counters and Module state (FR-71, NFR-6). Bots also have no typed key-value handle yet: the template already calls `ctx.kv()`, which does not exist (FR-38, AD-1).

**Approach:**
- Add a validated `AppId`, set with `AppBuilder::app_id` or `NOCTIS_APP_ID`. By default it is derived from the bot token's application id.
- Core wraps each of the seven byte-store ports in `Namespaced<dyn Port>`, which prefixes every key with `<app>:<port>:`.
- An Adapter passed to the normal setter as `Shared::new(adapter)` gets the prefix `_shared:<port>:` instead.
- Add the typed `Kv` handle as `ctx.kv()` on every context. It wraps the App's namespaced `CacheStore`.
- Docs, the Usage Skill, the template and the API snapshot are updated in the same commit.

## Boundaries & Constraints

**Always:**
- **`AppId`** lives in the new file `crates/noctis-core/src/namespace.rs`. It is re-exported from `noctis-core` and from the facade root `noctis::{AppId, AppIdError}`.
  - Definition: `#[derive(Clone, PartialEq, Eq, Hash)] pub struct AppId(Arc<str>)`.
  - `AppId::new(id: &str) -> Result<AppId, AppIdError>` checks, in this order:
    1. Empty input gives `Empty`.
    2. More than 32 `char`s gives `TooLong { len }`, where `len` is the char count.
    3. The first char outside `[a-z0-9-]` gives `InvalidChar { index, ch }`, where `index` is a char index.
  - Input is never trimmed. A lone `-` is valid, because the pattern is exactly `[a-z0-9-]{1,32}`.
  - `AppId::from_application_id(id: Id<ApplicationMarker>) -> AppId` is the id in lowercase base36 (`0-9a-z`, most significant digit first, at most 13 chars).
  - Accessors: `as_str(&self) -> &str`, `Display` (the id), `Debug` (`AppId("x")`) and `FromStr` (= `new`).
  - The fallback id `default` comes from a crate-private fn (there is no public constant).
- **`AppIdError`** is `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]`. Each message names the rule `[a-z0-9-]{1,32}` and gives a fix, for example ``"App id has `{ch}` at index {index}: use only a-z, 0-9 and -"``.
- **Builder:**
  - `AppBuilder::app_id(&mut self, id: impl Into<String>) -> &mut Self` stores the raw value, and the last call wins.
  - `build()` validates the raw value. A bad value adds `BuildViolation::InvalidAppId { reason: AppIdError }` (message `invalid App id: {reason}`), and `build()` lists it with every other violation.
  - `App::from_env` reads `NOCTIS_APP_ID` (trimmed of ASCII whitespace; empty means unset) and passes it raw to `.app_id`. A later setter call wins.
  - Docs: add the variable to the `from_env` rustdoc list.
  - The new getter `App::app_id(&self) -> &AppId` has a doctest.
  - `App`'s and `AppBuilder`'s `Debug` show `app_id`.
- **Default `AppId`** (no `.app_id` call and no env value):
  - With a token: take the token's first `.`-separated segment and base64-decode it. Accept the standard and URL-safe alphabets, with or without `=` padding.
  - If it decodes to 1-20 ASCII digits that parse to a non-zero `u64`, the default is `from_application_id` of that id.
  - Otherwise the default is `default`, and `build()` logs one `warn!` on target `noctis::app`: `"could not derive the App id from the bot token; call AppBuilder::app_id"`. The warning never contains the token.
  - Without a token: `default`, with no log.
  - The base64 decoder is a small crate-private fn. Do not add a new crate.
- **The App label is the `AppId`.**
  - `AppShared.app` becomes `Arc::from(app_id.as_str())`, and so do the `Edge::start` label and every `app` span, metric and report field.
  - Remove the two "`"default"` until `AppId`, Story 7.4" comments: `app.rs` around L1974 and `handler.rs` L205.
- **Namespaced ports.** Exactly these seven slots are wrapped. Each wrapper's port segment is the constant in parentheses:
  - `ref_store` (`ref`)
  - `session_store` (`session`)
  - `module_state_store` (`module_state`)
  - `rate_limit_store` (`rate_limit`)
  - `cache_store` (`cache`)
  - `render_cache` (`render_cache`)
  - `asset_store` (`asset`)

  These slots are never wrapped: `identify_queue`, `locale_source`, `guild_access` and `discord_cache_backend` (AD-14: `DiscordCache` is per-App, and `IdentifyQueue` is coordination keyed by bucket).
  - Memory defaults are wrapped too, so every byte-store slot is uniformly a `Namespaced`.
- **`Namespaced<P: ?Sized + NamespacedPort>`** is public. It is re-exported as `noctis::ports::Namespaced`, together with the sealed `NamespacedPort` trait, which is implemented for the seven `dyn Port` types and carries the segment.
  - Constructors:
    - `Namespaced::new(app: &AppId, inner: Arc<P>)` sets the prefix to `<esc(app)>:<esc(P::SEGMENT)>:`.
    - `Namespaced::shared(inner: Arc<P>)` sets the prefix to `_shared:<esc(P::SEGMENT)>:`.
  - `prefix(&self) -> &str`.
  - `Debug` prints `Namespaced { prefix: "..", .. }`.
  - The prefix is built once, as a `Box<str>`.
  - Each wrapper implements its port trait, e.g. `impl CacheStore for Namespaced<dyn CacheStore>`. Every op forwards exactly one call to `inner` with the key `prefix + key`. The logical key is appended verbatim as the last segment.
  - Values, TTLs and return values pass through unchanged. `ModuleStateStore::version` is forwarded as is.
  - A `PortError::InvalidValue` from the inner store keeps the key the Adapter received, which is the full prefixed key.
- **Escaping.** The crate-private `escape_segment(&str) -> Cow<str>` maps `%` to `%25`, then `:` to `%3A`, and leaves every other char alone. It is applied to the namespace segments. A valid `AppId` and the fixed segments never contain either character, so the function returns `Cow::Borrowed`.
- **Sharing.** `pub struct Shared<P>(P)` has `Shared::new(p)`, `into_inner(self) -> P` and `#[derive(Debug, Clone)]`.
  - It implements no port trait. It is re-exported as `noctis::ports::Shared`.
  - The seven byte-store setters change from `port: impl Port` to `port: impl IntoPort<dyn Port>`.
  - `IntoPort<P: ?Sized>` is a public sealed trait in `namespace.rs`, re-exported as `noctis::ports::IntoPort`. Its single method is `#[doc(hidden)]` and returns the `Arc<P>` plus whether the port is shared.
  - It has two blanket impls per port: `impl<T: CacheStore> IntoPort<dyn CacheStore> for T` and `impl<T: CacheStore> IntoPort<dyn CacheStore> for Shared<T>`. Coherence holds because `Shared` is local and never implements a port; a scratch crate proved this.
  - A shared port resolves to `Namespaced::shared(..)`. This realises AD-14's "kept as `Shared<P>`".
  - The other four setters keep `impl Port`, so `Shared::new(..)` passed to them is a compile error (E0277).
- **`Arc` passthrough.** `noctis-ports` gets `impl<P: RefStore + ?Sized> RefStore for Arc<P>`, and the same for the other six byte stores, each forwarding every op. With it, one instance (`Arc::clone`) can be handed to two Apps, and `Arc<dyn CacheStore>` is accepted by the setters.
- **`Kv`** (new `crates/noctis-core/src/kv.rs`) is `#[derive(Clone)] pub struct Kv`. It is re-exported from `noctis-core` and the facade root (`noctis::Kv`).
  - It holds `Option<Arc<dyn CacheStore>>` (the App's namespaced store) and `schema: u8` (default 0).
  - `Debug` prints `Kv { schema, attached }`.
  - Methods (each makes exactly one `CacheStore` call):
    - `schema(self, schema: u8) -> Kv` (`#[must_use]`): the schema byte for typed ops.
    - `get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, PortError>`: `get`, then `decode_value(schema, ..)`.
      - A missing key and a schema mismatch are `Ok(None)`.
      - Empty, malformed or trailing bytes are `Err(PortError::Value(..))`.
    - `put<T: Serialize + ?Sized>(&self, key, value: &T, ttl: Option<Duration>) -> Result<(), PortError>`: `encode_value(schema, value)`, then `put`.
    - `put_if_absent<T: Serialize + ?Sized>(..) -> Result<bool, PortError>`: `true` only for the winner.
    - `incr(&self, key, by: u64, ttl: Option<Duration>) -> Result<u64, PortError>`: `incr_with_ttl`.
    - `counter(&self, key) -> Result<u64, PortError>`: `get`.
      - A missing key gives 0.
      - Exactly 8 bytes are read as a big-endian `u64`.
      - Any other length gives `PortError::invalid_value(key, "not an 8-byte counter")`, with the logical key.
    - `delete(&self, key) -> Result<(), PortError>`: `delete`.
  - Encoding happens before the store call. A serialize error is `Err(PortError::Value(ValueError::Serialize))` and makes no store call.
  - Every returned future is `Send` for any `T: Serialize + ?Sized` (no `Sync` bound): encode eagerly, so `&T` is not held across an await.
  - A detached `Kv` (the `__testing` seams, no App) answers every op with `PortError::unavailable("this context has no App")`.
- **`ctx.kv() -> Kv`** (a cheap clone) is added to these contexts:
  - `ComponentCtx`, `ModalSubmitCtx` and `ExpiredCtx` (through `ctx_accessors!`);
  - `CommandCtx`, which context menus share;
  - `EventCtx`;
  - `AutocompleteCtx`.

  `AppShared` gains `kv: Arc<dyn CacheStore>`, the resolved namespaced `cache_store`.

**Never:**
- Middleware `ctx.kv()` (Story 7.6).
- Ref minting or lookup (Story 7.5).
- Rate-limit or Module-state use of the stores (Epic 13).
- Conformance suites (FR-39).
- `from_env_prefixed` or several Apps per process.
- Escaping or rewriting the logical key.
- Adapters adding a prefix.
- A second `.share(..)` method.
- Wrapping `DiscordCache`/`IdentifyQueue`/`LocaleSource`/`GuildAccessList`.
- New external crates, or a debug build.
- Logging the token or putting it in an `AppIdError`.
- Changing `PortError`/`ValueError` or the op contracts.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | AppId min | `AppId::new("a")`, `"0"`, `"-"` | Ok | — |
| 2 | AppId max | 32 × `a`; 33 × `a` | Ok; `TooLong { len: 33 }` | typed |
| 3 | AppId empty | `""` | `Empty` | typed |
| 4 | AppId invalid char | `"My-bot"`, `"a_b"`, `"a:b"`, `"a b"`, `" a"`, `"a."` | `InvalidChar { index: 0, ch: 'M' }`, `{1,'_'}`, `{1,':'}`, `{1,' '}`, `{0,' '}`, `{1,'.'}` | typed |
| 5 | AppId unicode | `"bötü"`; 33 × `ö` | `InvalidChar { index: 1, ch: 'ö' }`; `TooLong { len: 33 }` (length checked first) | typed |
| 6 | Base36 | ids 1, 35, 36, 1234567890123456789, `u64::MAX` | `1`, `z`, `10`, `9do1sj396nf9`, `3w5e11264sgsf` | — |
| 7 | Token default | token `MTIzNDU2Nzg5MDEyMzQ1Njc4OQ.x.y`, the same with `==` padding, URL-safe alphabet, `Bot ` prefix | `app_id() == "9do1sj396nf9"` | — |
| 8 | Token undecodable | `fake-token`; `abc.def`; segment decoding to `abc`; to `0`; to 21 digits; to `18446744073709551616` | `default` + one warn without the token | log |
| 9 | No token | `App::builder().build()` | `default`, no warn | — |
| 10 | Explicit wins | token as in row 7 + `.app_id("my-bot")`; `.app_id("a")` then `.app_id("b")` | `my-bot`; `b` | — |
| 11 | Invalid explicit | `.app_id("Bad")`, `.app_id("")` together with `.ack_deadline(3 s)` | `BuildError` lists `InvalidAppId` (`InvalidChar`/`Empty`) and `InvalidAckDeadline` | build |
| 12 | Env | `NOCTIS_APP_ID=" prod-1 "`; `""`; `"PROD"`; then `.app_id("x")` | `prod-1`; unset → default; `InvalidAppId` at build; `x` | build |
| 13 | Prefix | `Namespaced::new(&AppId::new("bot-a")?, cache)` with key `profile:1:2` | inner sees `bot-a:cache:profile:1:2` (key verbatim, colons kept) | — |
| 14 | Key edge | key `""`; key `"ü:%"`; 10 000-char key | `bot-a:cache:`; `bot-a:cache:ü:%`; the prefix + the full key | — |
| 15 | Segments | each of the 7 slots | prefixes `a:ref:`, `a:session:`, `a:module_state:`, `a:rate_limit:`, `a:cache:`, `a:render_cache:`, `a:asset:` | — |
| 16 | Shared prefix | `.cache_store(Shared::new(s))` | inner sees `_shared:cache:k`; `_shared` can never equal an `AppId` (`_` is outside the alphabet) | — |
| 17 | escape_segment | `"a:b"`, `"%"`, `"%3A"`, `""`, `"abc"` | `a%3Ab`, `%25`, `%253A`, `""` (borrowed), `abc` (borrowed) | — |
| 18 | Isolation | two `Namespaced` (`a`, `b`) over one `Arc` store, for each of RefStore, SessionStore, RateLimitStore and ModuleStateStore (and CacheStore): A `put`s `k`=1, B `put`s `k`=2 | A `get` → 1, B `get` → 2; A `take`/`delete` leaves B's entry; B `incr_with_ttl` is independent of A's; `touch` on A does not create B's | — |
| 19 | Sharing both | Apps `a` and `b`, both `Shared::new(handle)` over one store | A `put` `k` → B `get` `k` sees it | — |
| 20 | Mixed | A namespaced, B shared, one store | neither sees the other's `k` | — |
| 21 | Ops forward | a counting inner store, one call per op on each wrapper | exactly one inner call per op; return values (`bool`, `u64`, `Option<Bytes>`, `version`) unchanged | — |
| 22 | Errors pass | inner returns `Backend`/`InvalidValue{key:"a:cache:k"}`/`Unavailable` | the same variant reaches the caller | propagate |
| 23 | Kv get/put | `put("k", &V{n:1}, None)` then `get::<V>("k")` | `Some(V{n:1})`; stored bytes `[0] ++ postcard` under `<app>:cache:k`; 1 store call each | — |
| 24 | Kv schema | `put` with `.schema(1)`, `get` with `.schema(2)`; `.schema(255)` round trip | `None` (miss); Some | — |
| 25 | Kv missing / bad | `get` on a missing key; stored `[]`; stored `[0, 0xff]` for `u64`; trailing bytes | `None`; `Err(Value(Empty))`; `Err(Value(Malformed))`; `Err(Value(Trailing{..}))` | propagate |
| 26 | Kv put_if_absent | twice on `k`; 32 concurrent callers | `true` then `false`; exactly one `true` | — |
| 27 | Kv incr/counter | `counter` on missing; `incr(k,1,None)` ×3; `incr(k,0,ttl)` on missing; `counter` after `put` of a typed value (≠ 8 bytes) | 0; 1,2,3, then `counter` 3; 0; `Err(InvalidValue{key:"k"})` | typed |
| 28 | Kv incr saturate | stored `u64::MAX - 1`, `incr(k,5,None)` | `u64::MAX` | — |
| 29 | Kv TTL | `put(k,&v,Some(1 ms))`, paused clock +2 ms | `get` → None | — |
| 30 | Kv delete | `delete` on existing / missing | gone / `Ok(())` | — |
| 31 | Kv serialize error | a value whose `Serialize` errors | `Err(Value(Serialize))`, 0 store calls | typed |
| 32 | Kv detached | `ComponentCtx::detached().kv().get::<u8>("k")` | `Err(Unavailable)` | typed |
| 33 | Kv Send | `fn assert_send` on `put::<NotSync>`'s future | compiles | — |
| 34 | ctx.kv per ctx | a handler on each of Command, user menu, Component, Modal, Expired hook, Event, Autocomplete writes `k` | the store sees `<app>:cache:k` | — |
| 35 | Setter compile errors | `.identify_queue(Shared::new(MemoryIdentifyQueue::default()))` | E0277 (trybuild `types/shared_identify_queue.rs`) | compile |
| 36 | Arc passthrough | `.cache_store(Arc::clone(&s))` on two builders; `.cache_store(arc_dyn)` | compiles; one instance, prefixes `a:`/`b:` | — |
| 37 | Labels | an App with `.app_id("bot-a")` | span/metric/report `app="bot-a"`; existing tests without a token stay `default` | — |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/store.rs`: the seven byte-store traits. Add the `Arc<P>` forwarding impls here (a `macro_rules!` per op list). Update `lib.rs` L47-48 ("core's wrappers add") to name `Namespaced`/`Shared`.
- `crates/noctis-ports/src/value.rs`: `encode_value`/`decode_value`, reused by `Kv`.
- `crates/noctis-ports/src/error.rs`: `PortError::{unavailable, invalid_value}`.
- `crates/noctis-core/src/ports.rs`:
  - The `ports!` macro has `PortOverrides`, `Ports::resolve`.
  - `PortOverrides` now holds, for the seven byte stores, the `IntoPort` output (`Arc` plus shared flag).
  - `Ports::resolve(overrides, &AppId)` wraps them, defaults included.
  - Keep `default_slots`/`is_default`. The simplest split is two macro sections: namespaced slots and plain slots.
- `crates/noctis-core/src/app.rs`:
  - `port_setters!` (L1242) is split the same way; the namespaced setters take `impl IntoPort<dyn Port>`, and their doctests also show `Shared::new`.
  - The builder fields (L1188) gain `app_id: Option<String>`.
  - `from_lookup` (env, ~L195-260 with the `*_VAR` consts L55-72) gains `NOCTIS_APP_ID`.
  - `build` (L1940-2000): validate the id or derive the default before `Edge::start`, then `Ports::resolve(config.ports, &app_id)`, and put `app` and `kv` into `AppShared`.
  - `Debug` impls L1143 / L2055.
- `crates/noctis-core/src/token.rs`: add the crate-private `application_id(&self) -> Option<Id<ApplicationMarker>>`, using the hand-rolled base64 and unit tests (rows 7-8).
- `crates/noctis-core/src/error.rs` L139: `BuildViolation::InvalidAppId { reason: AppIdError }`.
- `crates/noctis-core/src/handler.rs` L203 `AppShared` (+ `kv`), L247 `EventCtx` (+ `kv()`).
- `crates/noctis-core/src/component.rs` L61 `ctx_accessors!` (+ `kv()` from `self.0.app`); `command.rs` L81/L219; `autocomplete.rs` L202/L306.
- New `crates/noctis-core/src/{namespace.rs, kv.rs}` with `#[cfg(test)] mod tests`, registered and re-exported in `crates/noctis-core/src/lib.rs` (L203-259).
- `crates/noctis-core/src/app/tests.rs` L130-190: `CountingCache` tests keep passing through the wrapper. Add row 10-12, 19-20 and 36 tests.
- `crates/noctis/src/lib.rs`:
  - Root re-exports `AppId`, `AppIdError`, `Kv`.
  - The `pub mod ports` (L1677) gets `Namespaced`, `NamespacedPort`, `Shared` and `IntoPort`, with module-doc bullets.
  - The prelude (L1836) is unchanged.
- `crates/noctis-testkit/tests/components.rs` pattern (`FakeGateway` + `FakeDiscordHttp` + log capture): new `crates/noctis-testkit/tests/namespacing.rs`.
- `crates/noctis/tests/types/` (trybuild compile_fail, bless with `TRYBUILD=overwrite`).
- Docs:
  - `crates/noctis-ports/src/lib.rs` doc.
  - Spine AD-14 (L551-590) and the Conventions row "App ids" (L1215).
  - Skill: `references/adapters.md` §2.6 L286, §3 L329, §6 L437; `references/app-and-sharding.md` L133, L276, L476, L680-681, L744; `SKILL.md` L23 (API hash), L73, L323, L445.
  - Template: `examples/project-template/src/lib.rs` L37, `src/modules/profile/events.rs` L29, `AGENTS.md` L61.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-ports/src/{store.rs, lib.rs}`: add the `Arc<P>` forwarding impls and docs, plus a unit test that an `Arc<MemoryCacheStore>` works through `dyn CacheStore`.
2. `crates/noctis-core/src/namespace.rs` (+ `lib.rs`): `AppId`, `AppIdError`, base36, `escape_segment`, `NamespacedPort` (sealed), `Namespaced` and its seven port impls, `Shared` and `IntoPort`. Unit tests for rows 1-6, 13-18 and 20-22, with a counting/recording inner store.
3. `crates/noctis-core/src/token.rs`: application id extraction; tests for rows 7-8.
4. `crates/noctis-core/src/{error.rs, ports.rs, app.rs, handler.rs}`: `InvalidAppId`; port resolution with wrapping; setters; `app_id`; env; default derivation and its warn; `App::app_id`; `AppShared.{app,kv}`; `Debug`. Tests for rows 9-12, 19 and 36-37 in `app/tests.rs`.
5. `crates/noctis-core/src/kv.rs` + `ctx.kv()` on every context (`component.rs`, `command.rs`, `handler.rs`, `autocomplete.rs`). Tests for rows 23-33 against a counting `CacheStore` (one call per op) and `MemoryCacheStore`, with `tokio::time::pause` for row 29.
6. `crates/noctis/src/lib.rs`: re-exports and docs, with a doctest on `AppBuilder::app_id` and one on `Shared::new`.
7. `crates/noctis/tests/types/shared_identify_queue.rs` (+ `.stderr`, blessed and read): row 35.
8. `crates/noctis-testkit/tests/namespacing.rs` (all through the real wire):
   - `two_apps_never_see_each_others_kv`: two Apps (`bot-a`, `bot-b`) share one `Arc<MemoryCacheStore>`. A derived `#[derive(Button)]` handler does `ctx.kv().incr("clicks", 1, None)` and replies with the count. Clicking on each App gives 1 and 1, and the store holds `bot-a:cache:clicks` and `bot-b:cache:clicks`.
   - `shared_cache_store_is_seen_by_both_apps`: the same setup with `Shared::new` gives 1 then 2, under `_shared:cache:clicks`.
   - `event_and_command_ctx_kv`: covers row 34 for Command and Event.
9. Docs, spine amendment, skill and template (see Code Map):
   - Drop `[planned, Story 7.4]` and `[planned]` on §3/§6 of `adapters.md` and on `.app_id`/`NOCTIS_APP_ID`.
   - Document the default (base36 from the token, else `default` + warn), sharing, `Kv` (all six methods + `schema`) and the port segments.
   - Leave Middleware `kv()` marked [planned, Story 7.6].
   - Template: keep `.app_id("noctis-template")` and drop the `// [planned, Epic 7]` on `ctx.kv()`. The template `AGENTS.md` gets one line: "Port keys are namespaced by `app_id`; share one only with `Shared::new(..)`; store bot data through `ctx.kv()`".
   - Then run `cargo run --release -p xtask -- api-snapshot --bless` and update the skill `API hash`.
10. Run every gate in Verification.

**Acceptance Criteria:**
- Given two Apps built with `.app_id("bot-a")` / `.app_id("bot-b")` and one shared store instance, when each App's handler writes and reads the same logical `ctx.kv()` key through `FakeGateway` clicks, then each reads only its own value and the store holds `bot-a:cache:<key>` and `bot-b:cache:<key>`.
- Given both Apps pass `Shared::new(store)` to `.cache_store`, when one writes and the other reads, then the other sees the value under `_shared:cache:<key>`.
- Given one store instance under two `Namespaced` wrappers, when RefStore, SessionStore, RateLimitStore and ModuleStateStore each run their ops for the same key, then neither App sees, takes, deletes, touches or increments the other's entry.
- Given an App built with a real-format token and no `app_id`, when it is built, then `app_id()` is the base36 application id and the metric and span `app` label equals it.
- Given `.app_id("Bad")`, when `build()` runs, then the `BuildError` contains `InvalidAppId` naming index 0 and `B`.
- Given a counting `CacheStore`, when each `Kv` method runs once, then exactly one store call is made, with the namespaced key.
- Given the Usage Skill, when `skill-check` runs, then it passes; `app_id`, namespacing, `Shared::new` and `ctx.kv()` are documented without `[planned, Story 7.4]`; and the API hash matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Default id is base36, not base62 (decision).** AD-14 says "base62", but base62 has upper-case digits, which the `[a-z0-9-]` alphabet of the same rule forbids. Lowercase base36 keeps the id compact (≤ 13 chars) and valid. Amend AD-14 and the Conventions row with an `Amended (2026-10-21)` line.
- **Default id comes from the token (decision).**
  - Ports must be namespaced before the first `SessionStore` read, which happens before the gateway connects. So the id is needed at `build()`, without a network call.
  - A bot token's first segment is base64 of the bot user id, which equals the application id. It is also identical on every process of a Cluster.
  - Without a decodable token the id is `default`: the current label, so existing tests keep `app="default"`. When a token was given, a warn is logged.
- **Escaping (decision).** The logical key is the opaque last segment and stays verbatim, so Redis keys stay readable. The prefix is still injective: neither namespace segment can contain `:`, so the prefix always ends at the second `:`, and `_shared` is outside the `AppId` alphabet. `escape_segment` guards the namespace segments, as AD-14's "Key segments escape `:`" requires.
- **`Shared` is detected by type (decision).** `Shared<P>` implementing the port traits would make the setter unable to tell it apart (no specialization). Instead the setters take the sealed `IntoPort`, and a shared port is kept as `Namespaced::shared` with the `_shared:<port>:` prefix. Any of the seven byte stores may be shared.
- **`Kv` schema (decision).** The default schema byte is 0. `ctx.kv().schema(n)` is the bump knob when a stored type changes incompatibly; old entries then read as misses.
- Golden sketch:
  ```rust
  impl CacheStore for Namespaced<dyn CacheStore> {
      fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
          let key = self.key(key);                 // prefix + key, one String
          Box::pin(async move { self.inner.get(&key).await })
      }
      // put / put_if_absent / delete / incr_with_ttl: same shape
  }
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok (API hash matches)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rn "planned, Story 7.4\|until \`AppId\`" crates examples/project-template docs --include=*.rs --include=*.md` -- prints nothing
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
