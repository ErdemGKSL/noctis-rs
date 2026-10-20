---
title: 'Story 5.1: App builder, Runtime, Plugin seam and startup safety'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: 'b5527bcd15202b614cd2b20495a2d9ca54c53913'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred:
  - summary: >-
      The configured HTTP timeout is only asserted on the App field, never observed on the twilight client.
    evidence: |-
      twilight-http 0.17.1 Client has no timeout getter; timeout_bounds and build_happy read App::http_timeout(), so dropping `.timeout(config.http_timeout)` in AppBuilder::build would pass every test. The only observable boundary is a real request against a stalled local server, which belongs with the REST/gateway stories (5.4+).
    location: >-
      crates/noctis-core/src/app.rs (AppBuilder::build)
    severity: low
---

<intent-contract>

## Intent

**Problem:** `noctis-core` is an empty skeleton: a bot author cannot build an App from a token, typed state and Adapters, startup mistakes (no tokio runtime, no rustls provider, bad token) would panic inside twilight/rustls, and optional crates (`noctis-image`, `noctis-prefix`) have no seam into core (FR-1, FR-38, NFR-4, NFR-6; AD-9, AD-13, AD-21, AD-24, AD-29; Conventions Config).

**Approach:** Add to `noctis-core` (re-exported from the `noctis` facade) the `App`/`AppBuilder` pair with `from_env`, the redacted `Token`, `State<T>`, port setters with bounded memory defaults, the process `Runtime` with frozen type-erased resources, the `Plugin` trait, the App's single twilight HTTP client, the startup safety checks of `App::run`, and typed errors collected into one `BuildError`. No gateway, lanes, Modules or handlers (Stories 5.2-5.8).

## Boundaries & Constraints

**Always:**
- **Builder style (memlog decision):** tokio-style mutable builder. Every `AppBuilder` setter is `pub fn x(&mut self, ..) -> &mut Self`; `pub fn build(&mut self) -> Result<App, BuildError>` moves the configuration out and leaves the builder empty (as fresh, still attached to its Runtime). `App::builder() -> AppBuilder` and `App::from_env() -> Result<AppBuilder, Error>` attach to `Runtime::global()`. `configure(&mut App::from_env()?).build()?` must compile.
- **AppBuilder methods (exact, 5.1 set):** `token(Token)`, `state<T: Send + Sync + 'static>(T)`, `intents(Intents)` (unions into the requested set; default and floor `Intents::GUILDS`), `http_timeout(Duration)`, `runtime(&Runtime)` (attach; clones the handle), `current_runtime(&self) -> &Runtime` (getter for plugins), `plugin<P: Plugin>(P)`, `plugin_mut<P: Plugin>(&mut self) -> Option<&mut P>`, `report(BuildViolation)`, `renderer(Arc<dyn Renderer>)`, and the port setters `ref_store`, `session_store`, `identify_queue`, `module_state_store`, `rate_limit_store`, `cache_store`, `render_cache`, `asset_store`, `locale_source`, `guild_access`, `discord_cache_backend`, each taking `impl <Port>` (stored as `Arc<dyn Port>`, last call wins). Unset ports default at `build()` to `MemoryRefStore::new()`, `MemorySessionStore::new()`, `MemoryIdentifyQueue::default()`, `MemoryModuleStateStore::new()`, `MemoryRateLimitStore::new()`, `MemoryCacheStore::new()`, `MemoryRenderCache::new()`, `MemoryAssetStore::new()`, `NoLocaleOverlay`, `AllowAllGuilds`, `MemoryDiscordCache::new(DiscordCacheResources::empty())`; `renderer` defaults to none. `Intents` is `twilight_model::gateway::Intents`.
- **App (exact getters):** `runtime(&self) -> &Runtime`, `http(&self) -> &Arc<twilight_http::Client>` (the same `Arc` on every call), `http_timeout(&self) -> Duration`, `intents(&self) -> Intents`, `state<T: Send + Sync + 'static>(&self) -> Option<State<T>>`, `has_renderer(&self) -> bool`, `async fn run(self) -> Result<(), Error>`. Ports live in a `pub(crate)` `Ports` struct (one `Arc<dyn P>` per port plus a record of which slots took the memory default) reachable only inside core.
- **HTTP client (AD-24):** `build()` constructs exactly one `Arc<twilight_http::Client>` per App via `twilight_http::Client::builder().timeout(t)` plus `.token(token.expose())` when a token is set, only after every check passed. Because twilight-http 0.17.1's `ClientBuilder::default()` calls `tokio::spawn` (rate-limiter actor) and rustls needs a process provider, `build()` first ensures the crypto provider and checks `tokio::runtime::Handle::try_current()`; no runtime ⇒ `BuildViolation::NoTokioRuntime` (any flavor is enough for build). `http_timeout` allowed range 1 ms..=300 s inclusive (default 10 s).
- **Startup safety (AD-21):** `App::run` checks, in order: `Handle::try_current()` Err ⇒ `StartError::NoRuntime`; `runtime_flavor() != RuntimeFlavor::MultiThread` ⇒ `StartError::NotMultiThread`; crypto provider ensure fails ⇒ `StartError::CryptoProvider`; no token ⇒ `StartError::MissingToken`. Then it logs `tracing::warn!(target: "noctis::app", "gateway not available yet; App::run returns")` and returns `Ok(())` (Story 5.4 replaces this). Ensure = if `rustls::crypto::CryptoProvider::get_default()` is `None`, call `rustls::crypto::ring::default_provider().install_default()`; an `Err` from install is success when `get_default()` is now `Some` (lost race), else the typed error. An already-installed provider is never replaced.
- **Token (NFR-6, AD-29):** `Token::new(impl Into<String>) -> Result<Token, TokenError>` and `FromStr`; strips one exact leading `"Bot "`, then requires ≥ 1 char, every char printable ASCII `0x21..=0x7E`; no maximum. `TokenError` (`thiserror`, `Debug, Clone, PartialEq, Eq`, `#[non_exhaustive]`): `Empty`, `InvalidChar { index: usize }` (char index after stripping). `Token: Clone`; `Debug` prints `Token(<redacted>)`, `Display` prints `<redacted>`; the secret is reachable only through `pub(crate) fn expose(&self) -> &str`. No `PartialEq`, no serde. `Debug` of `AppBuilder`, `App`, every error never contains the token text.
- **from_env (Conventions Config):** reads only `NOCTIS_TOKEN` in 5.1 (other `NOCTIS_*` variables arrive with their setters' stories). Value is trimmed of ASCII whitespace; empty after trim ⇒ treated as unset (no token, no error); non-UTF-8 ⇒ `EnvError::NotUnicode { var: "NOCTIS_TOKEN" }`; invalid ⇒ `EnvError::InvalidToken { var: "NOCTIS_TOKEN", source: TokenError }`. Implemented over an internal `pub(crate) fn from_lookup(impl Fn(&str) -> Option<OsString>)` so tests never call `std::env::set_var` (unsafe in edition 2024; `unsafe_code` is denied).
- **State<T> (AD-9):** `pub struct State<T>(Arc<T>)` with `Deref<Target = T>`, `Clone`, `Debug` (when `T: Debug`); no mutable access (interior mutability lives inside `T`). One value per type: a second `.state::<T>(..)` keeps the first and records `BuildViolation::DuplicateState { type_name }`.
- **Runtime (AD-24):** `#[derive(Clone)] pub struct Runtime(Arc<Inner>)`; `Runtime::global() -> &'static Runtime` (one `OnceLock`), `Runtime::for_test() -> Runtime` (fresh, `is_test() == true`, never equal to global), `is_test(&self) -> bool`, `PartialEq/Eq` by `Arc::ptr_eq`, `Debug` (is_test, resource count). `pub trait Resource: Send + Sync + 'static { type Config: PartialEq + Debug + Send + Sync + 'static; fn init(config: &Self::Config) -> Self; }`. `resource_or_init<T: Resource>(&self, config: T::Config) -> Result<Arc<T>, ResourceConflict>`: map `Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>` holding `(T::Config, Arc<T>)`; lock, look up, unlock; on a miss run `init` outside the lock, re-lock and insert only if still absent (a racer's entry wins and is compared). Same config ⇒ the same `Arc` (`ptr_eq`); different ⇒ `ResourceConflict { resource: &'static str /* type_name */, frozen: String, requested: String /* Debug of configs */ }`. Lock poisoning is recovered with `PoisonError::into_inner`. Docs: configs must not hold secrets (Debug is printed).
- **Plugin (AD-24):** `pub trait Plugin: Send + Sync + 'static { fn install(&self, app: &mut AppBuilder); }`. Stored as `Box<dyn ErasedPlugin>` (private trait: `Plugin` + `as_any_mut`). Second plugin of a type ⇒ first kept, `DuplicatePlugin { type_name }`. `build()` order: (1) take the plugin list, call each `install` once in registration order; plugins registered during an install are installed after, in the same build, until none is new (`plugin_mut` cannot see plugins whose install batch is running — documented); (2) checks: `http_timeout` range, crypto provider, tokio runtime; (3) if any violation was recorded (setters, plugins' `report`, checks), return `BuildError` with all of them in that recorded order; else construct ports, client and `App`.
- **Errors (AD-13):** all `thiserror`, `#[non_exhaustive]` on enums. `BuildError { violations: Vec<BuildViolation> }` (never empty; `violations(&self) -> &[BuildViolation]`, `into_violations(self)`; `Display`: `"{n} App-build violation(s)"` then one `"\n  - {violation}"` per entry). `BuildViolation`: `NoTokioRuntime`, `CryptoProvider`, `DuplicateState { type_name: &'static str }`, `DuplicatePlugin { type_name: &'static str }`, `ResourceConflict(ResourceConflict)` (+ `From`), `InvalidHttpTimeout { actual: Duration }` (message states 1 ms..=300 s), `Plugin { plugin: &'static str, message: String }` with constructor `BuildViolation::plugin::<P: Plugin>(message: impl Into<String>)`. `StartError`: `NoRuntime`, `NotMultiThread`, `CryptoProvider`, `MissingToken`. `EnvError`: `NotUnicode { var: &'static str }`, `InvalidToken { var: &'static str, #[source] source: TokenError }`. `Error`: `Env(EnvError)`, `Build(BuildError)`, `Start(StartError)` with `#[from]`. Each message names the cause and a fix (e.g. NotMultiThread: "use #[tokio::main] (multi-thread) or Builder::new_multi_thread()").
- **Facade:** `noctis` gains normal deps `noctis-core`, `twilight-http`, `twilight-model`; root re-exports `App, AppBuilder, BuildError, BuildViolation, EnvError, Error, Plugin, Resource, ResourceConflict, Runtime, StartError, State, Token, TokenError`; `noctis::prelude` adds `App, AppBuilder, State`; new `pub mod twilight { pub use twilight_http as http; pub use twilight_model as model; }` (AD-2). If `api-snapshot` expands either crate item by item, narrow to `pub mod http { pub use twilight_http::Client; }` and `pub mod model { pub mod gateway { pub use twilight_model::gateway::Intents; } }` and log the decision.
- **Dependencies:** `crates/noctis-core/Cargo.toml` `[dependencies]`: `noctis-ports.workspace = true`, `rustls.workspace = true`, `thiserror.workspace = true`, `tokio = { workspace = true, features = ["rt"] }`, `tracing.workspace = true`, `twilight-http.workspace = true`, `twilight-model.workspace = true`; `[dev-dependencies]` `tokio = { workspace = true, features = ["macros", "rt", "rt-multi-thread"] }`. Root `[workspace.dependencies]` pins are already correct (rustls 0.23.46 `ring,std,tls12,logging`; twilight-gateway/http 0.17.1 with AD-21 features) and `deny.toml` already bans `aws-lc-rs`/`aws-lc-sys`: keep them unchanged. If `cargo deny` reports a license from the new transitive deps, add a crate-scoped exception in `deny.toml` and log a `(decision)` memlog line.
- Every public item has rustdoc and a doctest (`no_run` where a runtime/network is involved). No `unwrap`/`expect` outside tests (except a documented invariant downcast under `#[expect(clippy::expect_used, reason = "…")]`). Release-only cargo commands.

**Never:**
- No gateway connection, `GET /gateway/bot`, shard config, lanes, Modules, `module(..)`, handlers, `Ext<T>`, Error Sinks, metrics recorder, shutdown, CLI parsing (`run_with_args`), `app_id`, `http_proxy`, `gateway_url`, `payload_key`, port namespacing — later stories; do not stub them as public API.
- Never log, `Debug`-print or put in an error message the token; never pass it to a port.
- No `static mut`, no globals except `Runtime::global()`'s `OnceLock`; no `std::env::set_var` in tests; no `catch_unwind` around `install` (author code; documented that a panicking install panics `build`).
- No twilight HTTP client constructed anywhere but `noctis-core`'s build path; no second client per App.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Token happy | `Token::new("abc.DEF_ghi-1")` | Ok; `expose() == "abc.DEF_ghi-1"` |
| 2 | Bot prefix | `"Bot abc"` / `"Bot Bot abc"` / `"bot abc"` | Ok `"abc"` / `InvalidChar{index:3}` / `InvalidChar{index:3}` |
| 3 | Empty | `""`, `"Bot "` | `TokenError::Empty` |
| 4 | Whitespace / control | `" abc"`, `"ab c"`, `"abc\n"`, `"a\tb"`, `"a\u{7f}"` | `InvalidChar{index}` = 0, 2, 3, 1, 1 |
| 5 | Unicode | `"tökén"`, `"🙂"` | `InvalidChar{index:1}`, `InvalidChar{index:0}` |
| 6 | Long / bounds | 1 char `"a"`; 10 000 × `'a'`; `"!"` and `"~"` | all Ok |
| 7 | Redaction | `format!("{t:?}")`, `"{t}"`, `Debug` of builder/App/`BuildError`/`EnvError` holding token `"SECRET123"` | `Token(<redacted>)`, `<redacted>`; no output contains `SECRET123` |
| 8 | from_env | lookup: absent / `""` / `"  \n"` / `"  tok \n"` / `"a b"` / non-UTF-8 bytes | no token / no token / no token / token `tok` / `EnvError::InvalidToken{source: InvalidChar{index:1}}` / `EnvError::NotUnicode`; error `Display` never shows the value |
| 9 | Build happy | `#[tokio::test]` (current_thread), token + state + custom `cache_store` | Ok; `app.http()` same `Arc` twice (`ptr_eq`); `http_timeout() == 10 s`; `intents() == GUILDS`; `state::<T>()` derefs to the value; `state::<U>()` (unregistered) = None |
| 10 | Build w/o token | `App::builder().build()` inside a runtime | Ok (TestApp case); client built without token |
| 11 | Two Apps | two builds | different client `Arc`s (`!ptr_eq`) |
| 12 | Defaults | no port setters | `Ports` records all 11 store slots as memory defaults; `has_renderer() == false`; default `CacheStore` round-trips `put`/`get` |
| 13 | Custom port | counting `CacheStore` set twice (A then B) | B used (its counter increments on `ports.cache_store.get`), slot not default; A untouched |
| 14 | Timeout bounds | `0`, `999 µs`, `1 ms`, `300 s`, `300 s + 1 ns`, `Duration::MAX` | violation, violation, Ok, Ok, violation, violation (`InvalidHttpTimeout{actual}`) |
| 15 | Intents | `.intents(GUILD_MESSAGES)` then `.intents(MESSAGE_CONTENT)`; `.intents(Intents::empty())` | `GUILDS\|GUILD_MESSAGES\|MESSAGE_CONTENT`; `GUILDS` |
| 16 | Duplicate state | `.state(1u32).state(2u32)` | BuildError with `DuplicateState{type_name:"u32"}`; first value would be kept |
| 17 | No runtime build | sync `#[test]`: `App::builder().build()` | `Err`, contains `NoTokioRuntime`; no panic, no client |
| 18 | All at once | sync test: duplicate state + timeout 0 + duplicate plugin + plugin `report` | one `BuildError` listing all 5 (incl. NoTokioRuntime) in recorded order; `Display` has 5 bullet lines |
| 19 | Build resets builder | `build()` twice on one builder | second App has no token/state/custom ports, same Runtime |
| 20 | Runtime attach | `App::builder()` / `.runtime(&Runtime::for_test())` | `app.runtime() == Runtime::global()` / `is_test()` and `!= global()` |
| 21 | Runtime identity | `Runtime::for_test()` twice; `global()` twice | different / equal; `global().is_test() == false` |
| 22 | Resource freeze | test Runtime: `resource_or_init::<Dummy>(1)` twice; then `(2)` | same `Arc` (`ptr_eq`, `init` ran once); `Err(ResourceConflict{frozen:"1", requested:"2"})` |
| 23 | Resource isolation | two `for_test()` Runtimes, configs 1 and 2 | both Ok (no interference) |
| 24 | Resource race | 8 threads `resource_or_init::<Dummy>(1)` on one Runtime | all get `ptr_eq` Arcs |
| 25 | Conflict via plugin | test plugin calling `resource_or_init` + `report` on conflict; App A config 1, App B config 2 on one test Runtime | A Ok; B `BuildError` with `ResourceConflict` |
| 26 | Plugin installs renderer | test plugin `install` sets a fake `Renderer` | `has_renderer()`; `install` called exactly once (counter) |
| 27 | Plugin order / nesting | plugins P1, P2 (P1's install registers P3) | install order P1, P2, P3, each once |
| 28 | Duplicate plugin / plugin_mut | `.plugin(P(1)).plugin(P(2))`; `plugin_mut::<P>()` before build; `plugin_mut::<Q>()` | `DuplicatePlugin`; returns `P(1)` mutable (edit visible in install); `None` |
| 29 | run outside runtime | App built in a runtime, `run()` polled once with `Waker::noop()` outside any runtime | `Ready(Err(Error::Start(NoRuntime)))` |
| 30 | run current_thread | `#[tokio::test]` | `Err(Start(NotMultiThread))` |
| 31 | run multi_thread | `flavor = "multi_thread"`, token set / no token | `Ok(())` / `Err(Start(MissingToken))` |
| 32 | Crypto provider | fresh process: ensure; ensure again / provider pre-installed by test | installed, Ok, idempotent / Ok, existing provider kept (`Arc::ptr_eq` with `get_default()` before) |
| 33 | deps-check http-client | synthetic workspace: `noctis-ui` depends on `twilight-http`; gateway source contains `Client::builder(` | `[http-client]` violations; core/facade deps alone pass |

</intent-contract>

## Code Map

- `crates/noctis-core/src/lib.rs` -- empty skeleton (crate docs only); becomes module list + `pub use`. Lints: workspace (`missing_docs` warn, clippy pedantic, `unwrap_used`/`expect_used` warn).
- `crates/noctis-core/Cargo.toml` -- no deps yet; add per Boundaries. Layer rules allow core → ports (`xtask/src/deps_check.rs` GRAPH L47-67).
- `crates/noctis-ports/src/lib.rs`, `store.rs` L86-150 (port traits, all `Send + Sync + 'static`, object safe), `identify.rs` L17, `discord_cache.rs` L19, `access.rs` L7, `locale.rs` L6, `render.rs` L349 (`Renderer`); `memory/stores.rs` (`MemoryXStore::new()` + `Default`), `memory/identify.rs` L46-66 (`MemoryIdentifyQueue::default()` = concurrency 1; Story 5.4 re-creates it from `max_concurrency` when unset), `memory/discord.rs` L27-60, L196 (`DiscordCacheResources::empty()`, `MemoryDiscordCache::new`), `memory/trivial.rs` (`NoLocaleOverlay`, `AllowAllGuilds`). Read-only.
- twilight-http 0.17.1 `src/client/builder.rs`: `Default` spawns `RateLimiter::default()` (tokio::spawn), `timeout(Duration)`, `token(String)` adds `"Bot "`; `Client` has no timeout getter (keep it on `App`). `src/client/connector.rs` calls `rustls::ClientConfig::builder()` (needs provider).
- `crates/noctis/src/lib.rs` -- facade: crate docs L1-86, `#![deny(missing_docs)]` L87, `prelude` L290-314, `__private` L317. Add root re-exports, `twilight` module, prelude items, and a crate-doc section "App" with a `no_run` doctest. `crates/noctis/Cargo.toml` -- add normal deps.
- `xtask/src/deps_check.rs` -- `Workspace::from_metadata` L204-226 keeps only internal deps: also keep deps named `twilight-http`; add `Rule::HttpClient` (`"http-client"`) L255-279; check in `check()` L325-370; a source scan helper reading `crates/noctis-gateway/src/**/*.rs` and `crates/noctis/src/**/*.rs` for `Client::new(`, `Client::builder(`, `ClientBuilder`; tests module L587+. Update module docs L1-12.
- `deny.toml` L46-51 -- bans already present; licenses allow-list L20-37.
- `xtask/public-api.txt`, `examples/project-template/.claude/skills/noctis/SKILL.md` L21 -- API hash line; L39, L246-260 builder examples.
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- §1 build/run, methods table (all `[planned]`), §2 env vars, §10 Runtime, §12 Plugins. `references/testing.md` L18, L30, L96 and `references/adapters.md` L412 -- `configure(App::builder())` call sites. Template `src/main.rs` L19, `src/lib.rs` L27-45 `configure`, `tests/templates.rs` L16, `tests/flows.rs` L21, `AGENTS.md` L20, `README.md` L46-60. Template `src/` is not compiled except `adapters/bounded_cache.rs` (via `crates/noctis/tests/template_adapter.rs`).
- `xtask/src/skill_check/mention.rs` -- every public item's last path segment must appear as a token in the skill's Markdown.
- Continuity (Epic 4 last spec 4.6): same gate list; facade re-export pattern (`pub use` lists, doc-hidden `__private`); errors are `thiserror` `#[non_exhaustive]`; decisions logged in the architecture memlog.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-core/Cargo.toml` -- add deps per Boundaries.
2. `crates/noctis-core/src/token.rs` -- `Token`, `TokenError`, `FromStr`; unit tests Matrix 1-7.
3. `crates/noctis-core/src/runtime.rs` -- `Runtime`, `Resource`, `ResourceConflict`; unit tests Matrix 20-24 (dummy resource with an init counter).
4. `crates/noctis-core/src/plugin.rs` -- `Plugin`, private `ErasedPlugin`.
5. `crates/noctis-core/src/state.rs` -- `State<T>` and the `pub(crate)` type map.
6. `crates/noctis-core/src/ports.rs` -- `pub(crate) Ports` with default resolution and default-slot record.
7. `crates/noctis-core/src/error.rs` -- `Error`, `BuildError`, `BuildViolation`, `StartError`, `EnvError`.
8. `crates/noctis-core/src/startup.rs` -- `pub(crate) fn check_runtime(multi_thread_required: bool)`, `pub(crate) fn ensure_crypto_provider()`.
9. `crates/noctis-core/src/app.rs` -- `App`, `AppBuilder` (all methods, `from_env`/`from_lookup`, `build`, `run`), manual redacting `Debug`; unit tests Matrix 8-19, 25-31 (`#[tokio::test]`, sync `#[test]`, `std::task::Waker::noop()` poll for 29).
10. `crates/noctis-core/tests/crypto_provider.rs` and `tests/crypto_preinstalled.rs` -- one `#[test]` each (own process) for Matrix 32.
11. `crates/noctis-core/src/lib.rs` -- modules, `pub use`, crate docs with a `no_run` example.
12. `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` -- facade per Boundaries; doc section and doctest.
13. `xtask/src/deps_check.rs` -- `http-client` rule + unit tests (Matrix 33); `cargo run --release -p xtask -- deps-check` passes on the real workspace.
14. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; copy the printed hash to `SKILL.md` L21.
15. `examples/project-template/` -- `app-and-sharding.md`: real `&mut` builder in §1 and §10/§12 (Runtime, `Resource`, writing a `Plugin`, `report`), methods table split into implemented rows and `[planned, Story N]` rows (http_proxy 5.2, gateway_url 5.3, shard_total/shards 5.4, module 5.6, error_sink 5.7, shutdown_grace 5.8, app_id 7.4, payload_key 7.2, publisher 6.9, …), §2 `NOCTIS_TOKEN` implemented (trim/empty rules) with other variables `[planned, Story N]`, new "Errors" section (`BuildError`, `BuildViolation`, `StartError`, `EnvError`, `TokenError`, `Error`); `run()` documented as returning after the checks until Story 5.4. `SKILL.md` L246-260, `testing.md` L18/L30/L96, `adapters.md` L412, `src/main.rs`, `src/lib.rs` (`pub fn configure(builder: &mut AppBuilder) -> &mut AppBuilder`), `tests/templates.rs`, `tests/flows.rs`, `AGENTS.md` L20 -- reconcile to the `&mut` builder. Run `skill-check`.

**Acceptance Criteria:**
- Given `App::builder()`/`App::from_env()`, a token, `State<T>` values and port implementations, when `.build()` runs inside a tokio runtime, then it returns an `App` whose unset ports are the bounded memory defaults, or one `BuildError` listing every violation at once.
- Given the token, when any public `Debug`/`Display` of the token, builder, App or error is formatted, then the token text never appears.
- Given `App::run` outside a multi-thread tokio runtime or without a token, when it starts, then it returns a typed `noctis::Error::Start(..)` instead of panicking; given no process crypto provider, then Noctis installs rustls's `ring` provider.
- Given a built App, when `http()` is read, then it is the App's one `Arc<twilight_http::Client>` with the configured timeout (default 10 s), and `deps-check` fails if any crate other than core/gateway/facade depends on `twilight-http` or gateway/facade source constructs a client.
- Given two Apps on one Runtime whose plugins request one resource with conflicting configs, when the second builds, then its `BuildError` contains `ResourceConflict`; on two `Runtime::for_test()` they both build.
- Given plugins, when `build()` runs, then each `install` runs exactly once in registration order before the checks, and a test plugin's fake Renderer is installed.
- Given the new public API, when `api-snapshot`, `skill-check` and `deps-check` run, then all pass and the Usage Skill and template use the real builder.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 34 findings — high 0, medium 1, low 17, false 16, maybe-false 0
- findings:
  - `[low]` `[reject]` Plugins cannot declare a shared sub-plugin (diamond ⇒ unavoidable DuplicatePlugin) — AD-24's `.image()`/`.theme()` register before `build`, where `plugin_mut` sees registered plugins; only a plugin adding a plugin inside `install` hits it, no planned plugin does, and the fix adds public surface.
  - `[low]` `[reject]` A plugin's setter silently overrides a port/renderer the user set — real by the spec's last-call-wins + install-in-build order, but only when the user adds a plugin that sets the same slot; a warn/"set if unset" path adds new API and branches.
  - `[false]` `[reject]` `App::run` returns `Ok(())` after a warn — the spec deliberately stages this until Story 5.4 and the skill documents it.
  - `[false]` `[reject]` `noctis::twilight::http` re-export defeats the deps-check one-client rule — the rule governs noctis crates (one client per App); a bot author's own Client is not an App client, and AD-2 mandates the re-export.
  - `[low]` `[reject]` Client-construction scan is textual (misses aliases/whitespace/tests, flags comments) — the spec defines exactly this scan; evasion needs a deliberate alias and a false positive fails loudly; a robust scan is more than a direct correction.
  - `[false]` `[reject]` twilight types in public API couple semver — AD-2/AD-24 choose this coupling; workspace pins hold one twilight version.
  - `[false]` `[reject]` ring provider may land on a different rustls copy than twilight's — Cargo.lock resolves a single `rustls` 0.23.46.
  - `[low]` `[reject]` UI-test stderr now prints `noctis::ui::ActionRow` etc. — real cosmetic diagnostics change caused by the spec-mandated twilight-model dependency; no direct fix.
  - `[false]` `[reject]` Template `configure` chains methods the real builder lacks — the template is excluded from the workspace, carries "written against the planned Noctis API" and the skill marks those methods `[planned, Story N]`; the signature was reconciled as the spec asks.
  - `[low]` `[reject]` `build()` discards the configuration on failure — spec-settled unconditional move-out; retrying a failed build on the same builder is rare (builder is usually a temporary) and a conditional reset would also need violation/plugin re-state.
  - `[false]` `[reject]` Redaction not checked on `app.http()` Debug — twilight-http 0.17.1 `Client` derives Debug over its private `Token`, whose Debug writes `<redacted>`; the token text never appears.
  - `[low]` `[patch]` `report` rustdoc example reports framework-owned `CryptoProvider` — patch: example uses `BuildViolation::plugin::<P>(..)`.
  - `[false]` `[reject]` `#[allow]` instead of `#[expect]` on temporary suppressions — `app.ports()` is read by `app/tests.rs` (L123, L140), so `dead_code` does not fire under `cfg(test)` and an `#[expect]` would be unfulfilled there; `allow` is the correct tool.
  - `[low]` `[patch]` Minor API/doc inconsistencies — `Greeting` undefined in skill §12 example is real (patch: define it); `INITS` sub-claim is false (`freeze_and_conflict` asserts `COUNTED_INITS == 1`); `Error` without Clone/PartialEq and the NoRuntime wording are negligible.
  - `[low]` `[reject]` `NOCTIS_TOKEN="Bot "` trims to a literal token `Bot` — real but requires an env value that is only the prefix; failure surfaces as a Discord 401; fix adds a guard.
  - `[low]` `[reject]` Pre-installed provider without usable cipher suites panics in twilight's connector — needs a deliberately broken custom provider; a TLS probe in build is extra machinery.
  - `[low]` `[reject]` `Resource::init` recursing into `resource_or_init::<T>` overflows the stack — a programmer bug that fails loudly; an in-progress set adds thread-local state.
  - `[low]` `[reject]` A plugin switching `runtime(..)` mid-install splits resources across Runtimes — contrived; a guard adds state tracking.
  - `[low]` `[patch]` DuplicatePlugin message advises `plugin_mut`, which cannot reach an installed plugin — patch: reword the message so the advice holds.
  - `[false]` `[reject]` Failed build still installs ring's provider — the spec orders the checks unconditionally and never replaces an installed provider; authors install their own before building.
  - `[low]` `[reject]` deps-check walk follows symlink loops — no symlinks in the scanned `src/` trees; a loop guard is extra complexity.
  - `[low]` `[reject]` deps-check flags `Client::new(` in comments/strings — same as the textual-scan entry: loud false positive, spec-defined scan.
  - `[low]` `[reject]` Alias or whitespace evades the client-construction scan — same as the textual-scan entry.
  - `[low]` `[reject]` Unusable custom provider still panics in `build()` — same as the cipher-suite entry.
  - `[medium]` `[patch]` Token never verified to reach the App's HTTP client — patch: assert `app.http().token()` with/without token and for a `"Bot "`-prefixed input.
  - `[low]` `[defer]` Configured HTTP timeout never observed on the client — deferred to the REST/gateway stories (needs a stalled local server).
  - `[false]` `[reject]` `from_env` reads only `NOCTIS_TOKEN` — the spec deliberately defers the other variables to their setters' stories; the skill marks them planned.
  - `[false]` `[reject]` Plugin getter is `current_runtime()`, not `runtime()` — Rust cannot overload the attach setter; the spec's Design Notes settle the name.
  - `[false]` `[reject]` ResourceConflict becomes a BuildError only via plugin `report` — per AD-24 resources are requested by plugins; the spec's AC tests exactly that path.
  - `[false]` `[reject]` Crypto provider ensured in both build and run — twilight's client build needs the provider, so the spec puts the check in both.
  - `[false]` `[reject]` `build()` requires a tokio runtime — twilight-http's ClientBuilder spawns its rate limiter; the spec settles `NoTokioRuntime`.
  - `[false]` `[reject]` Template body not reconciled — same as the template entry above.
  - `[false]` `[reject]` "Never logged" not tested at the log surface — the token has no public getter (`expose` is `pub(crate)`) and no tracing call takes it.
  - `[false]` `[reject]` Public surface beyond the intent — every listed item is specified in the spec's Boundaries.

## Design Notes

- Mutable builder: AD-24 fixes `install(&self, app: &mut AppBuilder)` and requires install to use the same public port setters; consuming setters cannot be called through `&mut`, and duplicating every setter is API noise. tokio's `runtime::Builder` is the precedent. Doc example:
  ```rust
  let app = configure(&mut App::from_env()?).build()?; // temporaries live to the end of the statement
  ```
- `runtime(&Runtime)` keeps the AC's attach name, so the plugin getter is `current_runtime()`.
- Missing token is a start error, not a build error, because `TestApp` (Stories 5.2, 8.6) builds token-less Apps; the fakes accept any token.
- `App::run` returning `Ok(())` after the checks is a staged behavior; the skill says so, and Story 5.4 replaces it with shard startup.
- Resource `init` runs outside the lock so an `init` that itself calls `resource_or_init` cannot deadlock; a lost race wastes one init.

## Verification

**Commands** (repo root; CI is paused, local gates only; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0.
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0 (no `aws-lc-rs`/`aws-lc-sys` in the graph).
- `cargo run --release -p xtask -- deps-check`, then `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- each exit 0 (`api-snapshot` needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0, see AGENTS.md pitfalls).
- `cargo tree -p noctis -e normal -i aws-lc-rs` -- fails with "did not match any packages" (aws-lc-rs absent). `test ! -d target/debug` -- true.

## Auto Run Result

- **Summary:** Added the noctis-core App layer: mutable `AppBuilder` (`App::builder`/`App::from_env`), `Runtime` with typed resources (`resource_or_init`, conflict detection), the `Plugin` seam (AD-24 `install(&self, &mut AppBuilder)`, `plugin_mut`, `report`), ports slots, redacted `Token` (`NOCTIS_TOKEN`, `Bot ` prefix stripped), startup safety (ring CryptoProvider ensured, `NoTokioRuntime`, missing-token start error), typed `BuildError`/`BuildViolation`, and an xtask deps-check rule enforcing one twilight-http Client per App; `noctis` facade re-exports and the project template/skill were updated.
- **Files changed:**
  - `crates/noctis-core/src/{app.rs,app/tests.rs}` — AppBuilder, App, build/run checks and tests.
  - `crates/noctis-core/src/{error.rs,plugin.rs,ports.rs,runtime.rs,startup.rs,state.rs,token.rs,lib.rs}` — errors, Plugin trait, port slots, Runtime/resources, crypto/startup checks, shared state, redacted token, exports.
  - `crates/noctis-core/tests/crypto_{provider,preinstalled}.rs` — provider install integration tests.
  - `crates/noctis/{Cargo.toml,src/lib.rs}` — facade features and re-exports (incl. `noctis::twilight`).
  - `crates/noctis/tests/ui/**.stderr` — re-blessed diagnostics (paths now print `noctis::ui::*`).
  - `examples/project-template/**` — skill (app-and-sharding reference rewritten, API hash), AGENTS.md, code reconciled to the builder signature.
  - `xtask/public-api.txt`, `xtask/src/deps_check.rs`, `xtask/tests/deps_check.rs` — API snapshot; client-construction scan.
- **Review findings (34: high 0, medium 1, low 17, false 16):**
  - Patched (1 medium, 3 low): token reaching `app.http()` now asserted (plain, absent, `Bot `-prefixed); `report` rustdoc example uses `BuildViolation::plugin::<P>`; DuplicatePlugin message reworded; skill section 12 example defines `Greeting`.
  - Deferred (1 low): configured HTTP timeout not observed on the client — to REST/gateway stories (needs stalled local server).
  - Rejected (13 low, 16 false), reasons recorded in the triage log: shared sub-plugin diamond (no planned plugin hits it), plugin setter overrides user slot (last-call-wins by spec), staged `run` Ok(()) until 5.4, twilight re-export vs one-client rule (rule governs noctis crates), textual client scan limits (spec-defined, loud false positives), twilight semver coupling (AD-2/AD-24), single rustls copy, UI stderr paths, template planned methods, build consumes config on failure, Debug redaction (twilight Token Debug is `<redacted>`), `allow` vs `expect`, `NOCTIS_TOKEN="Bot "`, broken custom provider panics, recursive init overflow, runtime switch mid-install, failed build installs ring, symlink loops, from_env only token, `current_runtime` naming, ResourceConflict via report, provider check in build and run, build needs tokio, log-surface token test, public surface per Boundaries.
- **Follow-up review recommended:** false — first pass patched 0 high and 1 medium (below the two-medium threshold).
- **Verification:** all local gates via gates.sh green — fmt, clippy `-D warnings`, `cargo test --release --workspace`, `cargo hack --each-feature`, MSRV 1.91 check, cargo deny, xtask deps-check/rules-catalog/api-snapshot/skill-check; aws-lc-rs absent from the graph; no `target/debug`. CI paused by the maintainer.
- **Residual risks:** HTTP timeout unverified at runtime (deferred); `App::run` is a staged no-op after checks until Story 5.4; client-construction scan is textual; plugin setters silently override user-set slots.
