---
name: 'Noctis'
type: architecture-spine
purpose: build-substrate
altitude: initiative
paradigm: 'hexagonal (ports and adapters) core over an event-driven, lane-based runtime'
scope: 'The noctis-rs monorepo: every noctis-* crate, the noctis facade, examples/project-template, xtask and CI'
status: final
created: '2026-10-19'
updated: '2026-10-19'
binds: ['FR-1..FR-79', 'NFR-1..NFR-10', 'UD-1..UD-8']
sources:
  - ../../prds/prd-noctis-rs-2026-10-19/prd.md
  - ../../prds/prd-noctis-rs-2026-10-19/addendum.md
  - ../../briefs/brief-noctis-rs-2026-10-19/brief.md
  - ../../research/
  - ../../../../docs/discord-rules.md
companions:
  - ../../../../docs/discord-rules.md
---

# Architecture Spine: Noctis

## Design Paradigm

The core is **hexagonal**. Domain logic (rules, codec, UI model) sits in pure crates
with no I/O. Every storage-like concern is a **port** in `noctis-ports` with a bounded
in-memory default. Discord I/O is concentrated in two adapters: the gateway (a driving
adapter that `noctis-core` starts and owns) and the App's single twilight-http client
(a driven adapter that `noctis-core` owns). Rendering is a driven port (`Renderer`)
implemented by `noctis-image`.

The runtime around the core is **event-driven and lane-based**. Shard tasks feed
lanes; a worker set in `noctis-core` drains them into handlers.

| Layer | Crates | I/O |
| --- | --- | --- |
| 0 Rules | `noctis-rules` | none |
| 1 Wire and ports | `noctis-codec`, `noctis-ports` | none (memory defaults only) |
| 2 UI model | `noctis-ui` | none |
| 3 Edge and services | `noctis-gateway`, `noctis-i18n`, `noctis-image` | gateway socket; image asset fetch and render pool |
| 4 Application | `noctis-core` | Discord HTTP through the App's one client; storage through ports |
| 5 Extensions | `noctis-prefix`, `noctis-testkit` | through core |
| 6 Facade | `noctis` | none (re-exports) |
| Compile time | `noctis-macros` (proc-macro) | reads the invoking crate's locale and theme files |
| Tooling | `xtask` | CI and release only |

## Invariants & Rules

```mermaid
graph TD
  noctis[noctis facade] --> core[noctis-core]
  noctis --> macros[noctis-macros]
  noctis --> image[noctis-image]
  noctis --> i18n[noctis-i18n]
  noctis --> prefix[noctis-prefix]
  noctis -.->|testkit feature, dev-dependencies only| testkit[noctis-testkit]
  prefix --> core
  testkit --> core
  testkit --> gateway
  testkit --> ports
  core --> gateway[noctis-gateway]
  core --> i18n
  core --> ui[noctis-ui]
  core --> ports[noctis-ports]
  core --> codec[noctis-codec]
  core --> rules[noctis-rules]
  image --> ui
  image --> ports
  i18n --> ports
  i18n --> rules
  gateway --> ports
  ui --> codec
  ui --> rules
  codec --> rules
  macros --> rules
  macros -.->|tw module only, render feature off| image
```

Arrows point from dependent to dependency. The drawn edges are the only allowed
workspace edges, except that the facade may depend on any crate. Every edge points to
a lower layer. No crate depends on `noctis`, except internal crates' own tests through
`[dev-dependencies]`. Only the facade's `testkit` feature, which bot crates enable in
`[dev-dependencies]` only, reaches `noctis-testkit`. `xtask deps-check` enforces this
list in CI.

### AD-1 — Hexagonal core; storage only through ports [ADOPTED]
- **Binds:** all crates; FR-38 to FR-40, FR-71 and every Adapter FR.
- **Prevents:** a feature persisting state its own way (a static `HashMap`, a direct
  Redis call) that a bot author can't swap, or that breaks under a Cluster.
- **Rule:**
  - Every storage-like concern is a trait in `noctis-ports`, with a bounded
    in-memory default in `noctis_ports::memory`. This covers anything that stores,
    caches, queues or resumes data that must outlive one call.
  - The v1 ports are `RefStore`, `SessionStore`, `IdentifyQueue`,
    `ModuleStateStore`, `RateLimitStore`, `CacheStore`, `DiscordCache`,
    `RenderCache`, `AssetStore`, `LocaleSource` and `GuildAccessList`. `Renderer`
    is a driven port in the same crate, not a store (AD-22).
  - `CacheStore` is the general key-value store with TTL that bots and Modules use,
    reached through the typed, namespaced `ctx.kv()` handle (`get`, `put`,
    `put_if_absent`, `incr`, `counter`, `delete`). It never backs the Discord cache.
  - `DiscordCache` is a separate port for Discord resources (guilds, channels,
    roles, members, users, messages). Its default in `noctis_ports::memory` wraps
    `twilight-cache-inmemory`, limited to the resource types the App needs (AD-21)
    and to twilight's per-channel message cap; its memory is reported apart from
    framework memory (NFR-1). Handlers read it through `ctx.cache()`.
  - Framework code may hold such data only through a port. A new concern adds a new
    port; it doesn't extend an unrelated one.
  - Exempt: twilight-http's transport rate-limiter state, per-connection gateway
    state (heartbeat, send limiter) and in-flight coordination that dies with its
    request (render single-flight). `RateLimitStore` holds only author-declared
    FR-46 limits.

### AD-2 — `noctis` is the only public API [ADOPTED]
- **Binds:** all crates; FR-48, FR-50, NFR-7.
- **Prevents:** bot code depending on internal crates, version skew between
  sub-crates, macro output that only compiles in one crate, and API drift the Usage
  Skill doesn't describe.
- **Rule:**
  - Bot code depends only on `noctis`. Every capability is a cargo feature of
    `noctis`, and every public item is re-exported from it.
  - All workspace crates share one version (`version.workspace = true`) and are
    released together (AD-28). Sub-crate docs say "internal; use `noctis`".
  - twilight is re-exported as `noctis::twilight`. Renderer types are reachable only
    through `noctis::image::unstable`, which semver doesn't cover.
  - Macro output names runtime items only through `::noctis::__private::<crate>`
    paths. Internal crates that use derives in their own tests either dev-depend on
    `noctis` or pass `#[noctis(crate = path)]`.

### AD-3 — One rules crate, used at compile time and at run time
- **Binds:** `noctis-rules`, `noctis-macros`, `noctis-ui`, `noctis-core`; FR-24,
  FR-75 to FR-79, NFR-10.
- **Prevents:** a macro and a runtime validator disagreeing about a Discord limit,
  rules that never reach the catalog, and packers and validators using different
  Limits.
- **Rule:**
  - Every Discord limit and rule lives once, in `noctis-rules`. The crate is sync,
    does no I/O and has no async. It may depend on small pure crates (`regex` with
    Unicode tables for CMD-01).
  - Each rule has the stable id from `docs/discord-rules.md` (`<AREA>-<nn>`, for
    example `CID-01`), used in the catalog, in error messages and in its tests.
  - Limit values sit in one `Limits` struct with Discord's defaults and a
    `mode: Strict | Lenient` (default Strict). Lenient truncates text with `…` and
    warns; it never applies to structural rules (CV2-03, CV2-05, CV2-06, CID-01,
    CID-02). Autocomplete choices are always truncated to 25 with a warning.
  - Every function that measures, packs or validates takes `&Limits`.
    `Limits::default()` is called only by App builder defaults, macros and tests.
  - Macros check against the defaults. A runtime override that lowers a limit makes
    the runtime validator check every tree, static ones included; an override that
    raises a limit never relaxes a compile-time check.
  - `noctis_rules::len::discord_len` (the larger of scalar values and UTF-16 units)
    is the only length function for user text.
  - Proc macros call `noctis-rules` functions directly. They never copy a constant.

### AD-4 — Enforcement ladder
- **Binds:** every rule; FR-75 to FR-79, NFR-10.
- **Prevents:** a static mistake surfacing only in production, or a dynamic check
  being skipped because "the macro handles it".
- **Rule:** each rule is checked at the earliest stage its input is known:
  - **Compile time:** types, typestate, `const` asserts or proc macros, for literals
    and fixed-shape data.
  - **App-build:** `App::build`, before any connection, for anything that depends
    on combining Modules (merged commands, duplicate Routes, Routes with no handler,
    Locale key clashes, missing Extensions, intents). It returns every violation at
    once.
  - **Runtime:** only for data built at run time.

  A compile-time check must fail under `cargo check`, because trybuild runs
  compile-fail suites with `cargo check`. The allowed mechanisms are proc-macro
  errors, trait bounds and free `const _` items over concrete types:
  - Placement and arity rules are traits implemented only for legal shapes (for
    example the array-length trait behind `ActionRow::buttons([b; N])` exists only
    for `N` in 1..=5). Each such trait carries
    `#[diagnostic::on_unimplemented(message = "[<RULE-ID>] …")]`, so the rule id
    reaches the error text.
  - Literal text goes through a proc macro (`label!("…")`, attribute literals) that
    measures it with `noctis-rules` at expansion.
  - A `const` assertion inside generic code is evaluated only at monomorphization
    (`cargo build`), so it is never the enforcement point of a rule.

  Compile-time checks have `trybuild` compile-fail tests. App-build and runtime
  checks have unit tests. Every test names its rule id, and `xtask rules-catalog`
  fails CI when a Compile-time or App-build row has no test citing its id.

### AD-5 — `custom_id` wire format
- **Binds:** `noctis-codec`, `noctis-core`, `noctis-ui`, `noctis-macros`; FR-15 to
  FR-19, FR-25, FR-60, FR-76; CID-01 to CID-07.
- **Prevents:**
  - length miscounts from non-ASCII characters;
  - two encoders producing incompatible ids;
  - stale ids decoding as wrong data after a deploy;
  - guessable, forged or cross-Route Refs.
- **Rule:**
  - A `custom_id` is ASCII only. This layout is normative; `docs/discord-rules.md`
    quotes it:

    ```text
    custom_id = F route ":" [S] [E] [N] body [T]

    F      1 char    base62 digit = 16 * generation + flags
                     flags: 1 = S present, 2 = E present, 4 = N present, 8 = T present
                     v1 writes generation 0 (F < 16); F >= 16 is a CodecError
    route  1..24     [a-z0-9._-], unique per App; ids starting with "_" are reserved
    S      1 char    Route schema version 1..=61 (#[route(schema = n)]); absent = 0
    E      5 chars   View envelope (AD-20): action 1, tab 1, page 2, state version 1
    N      2 chars   per-render select nonce (FR-25); select Routes only
    body   inline    field ("," field)*
           by ref    "~" + 11 base62 Ref
    T      6 chars   integrity tag: base62 of truncated keyed BLAKE3 over all
                     preceding chars; inline bodies of #[route(signed)] only
    ```

  - Decoding is total: malformed input gives `CodecError`, never a panic. A flag set
    that doesn't match the Route's declared shape, or an `S` that differs from the
    Route's schema, decodes as `Stale` (FR-60) and goes to the Route's stale handler,
    else the Expired Handler. An unknown route or a bad tag is forged and takes the
    unknown-Route path (FR-19).
  - Appending `Option<T>` fields is compatible: missing trailing fields decode as
    `None`, with no schema bump. Every other Payload change, and changing a Route's
    `view`, `select`, `signed` or `by_ref` shape, bumps `schema`. Renaming a Route id
    breaks live components.
  - Fields are base62 integers (signed ones zig-zag), snowflakes, 1-char enums and
    booleans, `Option` with a 1-char presence tag, and `BoundedStr<N>` (ASCII only,
    escape character `!`). Inline bodies and Ref records use the same field encoding.
  - A Ref is 11 base62 characters from the OS CSPRNG (`getrandom`, ≥ 64 bits),
    never sequential and never derived from interaction data. It resolves to a
    `RefRecord { route, schema, payload }` encoded by the codec. Core rejects a
    record whose route differs from the id's route (forged) and sends a schema
    mismatch to `Stale`.
  - The integrity key is `NOCTIS_PAYLOAD_KEY` (32 bytes), identical across a
    Cluster. One previous key may be configured for verification during rotation.
    It is never logged and never passed to an adapter.
  - The codec is the only code that builds or parses a `custom_id`. Its layout
    constants (`FMT_LEN`, `SEP_LEN`, `SCHEMA_LEN`, `VIEW_ENV_LEN`, `NONCE_LEN`,
    `REF_MARKER_LEN`, `REF_LEN`, `TAG_LEN`) are `pub const`, and
    `xtask rules-catalog` asserts the catalog quotes the same values.

### AD-6 — Payload length is a compile-time property
- **Binds:** `noctis-codec`, `noctis-macros`; FR-16, FR-17, FR-76; CID-01.
- **Prevents:** a Route that overflows 100 characters only for some values, and a
  macro bound that disagrees with the real encoding.
- **Rule:**
  - Every field type implements `noctis_codec::Field { const MAX_LEN: Option<usize> }`,
    worst-case escaping included: `bool` 1, `u8`/`i8` 2, `u16`/`i16` 3,
    `u32`/`i32` 6, `u64`/`i64`/`Id<T>` 11, `#[derive(PayloadEnum)]` (≤ 62 variants)
    1, `Option<T>` `T + 1`, `BoundedStr<N>` `2N`, `String`/`Vec<T>` `None`. A bot
    type joins by implementing `Field`.
  - `#[derive(Route)]` on a struct implements `Route` and `Payload` for it. It never
    inspects type names: it emits a `const` sum of `<F as Field>::MAX_LEN` plus
    `fields − 1` separators through codec `const fn`s. Generic Routes are rejected.
  - The bound is
    `MAX_LEN = FMT_LEN + route.len() + SEP_LEN + S + E + N + max(inline, by_ref)`,
    where `inline = Payload::MAX_ENCODED_LEN + T` and `by_ref = REF_MARKER_LEN + REF_LEN = 12`.
    `S`, `E`, `N` and `T` count only when the Route declares them (1, 5, 2, 6).
  - One attribute syntax. `#[route("id", …)]` takes two kinds of arguments, and
    the derive rejects anything else:
    - **Codec arguments** change the `custom_id` shape and length, and Story 2.5
      derives them: `by_ref` or `overflow = spill`, `schema = n` (1..=61), `view`,
      `select` (adds the `N` nonce; select constructors require it) and `signed`.
    - **Runtime arguments** don't change the encoding. The derive records them in an
      associated `RouteRuntime` const that core reads: `ttl = "…"` and `one_shot`
      (by_ref/spill Routes only; Story 7.5), `owner = field`, `permissions = …` and
      `guard = path` (Route Guards; Story 7.7), and `opens_modal` (Story 7.8). Each
      runtime argument enters the derive in the story named.
  - `#[handler(…)]` takes only handler arguments: `response = update | ephemeral |
    public` (Story 7.2), `on_expired = fn` and `on_stale = fn` (Story 7.3), and
    `rate_limit(..)` (Story 13.4). Rate limits are declared on `#[command]` and
    `#[handler]`, never on `#[route]`.
  - An inline Route whose bound is `None` or over 100 doesn't compile; `by_ref`
    with `signed` doesn't compile. A `spill` Route encodes inline when the actual
    value fits, else by Ref. FR-17's "error" overflow policy is this compile-time
    rejection of a plain inline Route; no runtime `overflow = error` exists.
  - With a 24-character route, the by-ref worst case is 46, so `by_ref` and `spill`
    Routes always fit.
  - The derive emits a free `const _: () = ::noctis::__private::codec::assert_fits::<T>();`
    and a `#[test]` that prints the width breakdown.

### AD-7 — Registry is data, built once
- **Binds:** `noctis-core`, `noctis-macros`; FR-7 to FR-9, FR-11, FR-13, FR-15,
  FR-44, FR-45, FR-47, FR-56 to FR-58, FR-73.
- **Prevents:** order-dependent registration side effects, O(n) dispatch, merge
  rules that differ between publish and dispatch, and Registry swaps racing
  dispatch.
- **Rule:**
  - Modules produce **descriptors** (commands, Routes, event handlers, middleware,
    hooks, locale catalogs), which are plain data.
  - Boot Flags (FR-73) and per-App restrictions (FR-47) filter descriptors before
    the fold, so unselected registrations are never registered, dispatched or
    published.
  - `App::build` folds the remaining descriptors into one immutable `Registry`,
    performing every App-build check. Base and group metadata declared by two
    Modules with different values is an error; undeclared metadata takes the value
    from the lexicographically first Module id and logs a warning (FR-57).
  - Dispatch uses only the Registry's O(1) maps. Publish JSON is generated only from
    the Registry.
  - The Registry is immutable for the App's life. Module enablement is a per-Module
    bitset (global plus per-guild overrides) cached from `ModuleStateStore` and
    refreshed on its version poll. `arc-swap` holds only that snapshot.

### AD-8 — Command Patterns expand in the macro
- **Binds:** `noctis-macros`, `noctis-rules`; FR-7 to FR-9, FR-56, FR-77.
- **Prevents:** a pattern that is legal in code but illegal on Discord, for example
  `config (set|get) (prefix|welcome)?`, and handlers receiving combinations in an
  order that differs between macro and runtime.
- **Rule:**
  - `#[command("…")]` expands the pattern at compile time with
    `noctis_rules::command::expand`, left to right in declaration order (FR-8). The
    cap is 25 expansions by default; `#[command(max_expansions = n)]` changes it.
    Duplicate expansions are rejected (CMD-15).
  - `noctis_rules::command::validate_tree` then checks name rules (CMD-01 to 03),
    at most 3 words (CMD-10), a name that is both leaf and group at one level
    (CMD-11), a base that is both invokable and has children (CMD-12), at most 25
    children per node (CMD-13) and the 8000 budget (CMD-05). Sibling groups and
    subcommands under one base are legal (CMD-14).
  - Violations are `compile_error!`s that name the rule id. Conflicts between
    Modules are checked by the same `validate_tree` at App-build.

### AD-9 — Handlers are stateless; state is ports or typed app state
- **Binds:** `noctis-core`, all handler-facing APIs; NFR-2, UD-7.
- **Prevents:** a per-message listener, closure or timer, and hidden global mutable
  state.
- **Rule:**
  - A handler is an `async fn` taking the context, the decoded Payload or options,
    and injected `State<T>`, `Ext<T>` and port handles.
  - The framework never stores a closure, task or timer per message or component.
    Interactive state travels in Payloads and Refs, or the bot keeps it behind a port.
  - Mutable shared bot state goes in `State<T>` registered at build. There are no
    `static mut` items and no global singletons outside the process-scoped resources
    of AD-24.

### AD-10 — Runtime lanes and back-pressure
- **Binds:** `noctis-gateway`, `noctis-core`, `noctis-testkit`; FR-2 to FR-6,
  FR-21, NFR-3, NFR-4.
- **Prevents:** a slow handler stalling heartbeats, interactions missing the 3 s
  deadline under event load, unbounded memory growth, and gateway and core
  disagreeing on lane item shape or ordering.
- **Rule:**
  - Each Shard runs in its own tokio task. That task only reads, filters,
    parses and enqueues with `try_send`. It never awaits lane capacity or a handler.
  - `noctis-gateway` defines the lanes and the item
    `Inbound { shard, received_at: Instant, kind: Interaction(Box<Interaction>, RawJson) | Event(Box<Event>) }`.
    `RawJson` is the original `d` payload as `Bytes`, kept only for interactions.
    `noctis-testkit` injects `Inbound` values into the same lanes, and its Discord
    fakes (AD-24) drive the real gateway and HTTP paths.
  - Interactions go to the **interaction lane**. It has priority, is never dropped,
    and is exempt from the event bound.
  - Other events go to the bounded **event lane**. When it is full, the event's
    type policy applies: droppable types (presence, typing and others marked
    droppable) drop the oldest queued event of a droppable type; other types go to a
    metered overflow buffer with a hard cap, beyond which they are dropped with an
    error log. Every drop is counted per event type.
  - `noctis-core` owns the semaphore-bounded worker set, which drains the
    interaction lane first. Event Handlers marked ordered run before the concurrent
    ones for that event, in per-guild order, through guild-keyed sub-queues (guild id
    hashed to a fixed sub-queue); all others run concurrently.
  - CPU-heavy work goes to the render pool or `spawn_blocking`. Nothing blocks the
    async runtime.

### AD-11 — The Responder owns every reply
- **Binds:** `noctis-core`, `noctis-ui`; FR-21 to FR-23, FR-29, FR-61, FR-65;
  INT-01 to INT-12, WH-01 to WH-03.
- **Prevents:** double acknowledgements, a modal sent after a defer, a V2 message
  edited without the flag, ephemerality changing mid-flow, and webhook sends that
  silently drop components.
- **Rule:**
  - Handler kinds have their own context types (`CommandCtx`, `ComponentCtx`,
    `ModalSubmitCtx`, `AutocompleteCtx`), so the kind fixes the legal callbacks at
    compile time (INT-04). `AutocompleteCtx` offers only `respond`.
  - The full type is `Responder<K, S = Pending>`. `K` is the handler kind:
    `Command<M>`, `Component<M>` or `ModalSubmit`, where `M` is `NoModal` (default)
    or `OpensModal`. A handler that responds explicitly takes it as a parameter
    through the aliases `CommandResponder<M = NoModal>`,
    `ComponentResponder<M = NoModal>` and `ModalResponder`; a handler that returns a
    View takes none. `Responder<Pending>` and `Responder<Acked>` in the rules below
    mean `Responder<K, Pending>` and `Responder<K, Acked>` for any `K`.
  - `Responder<Pending>` methods (`reply`, `defer`, `update`, `modal`) consume
    `self` and return `Responder<Acked>`, which offers only `edit_original`,
    `followup(msg, Visibility) -> FollowupHandle` and `delete_original` (INT-03);
    `done()` turns it into the `Responded` value the handler returns. `update`
    exists only for `Component` and `ModalSubmit`. `modal` exists only for
    `Command<OpensModal>` and `Component<OpensModal>` (INT-05).
  - `opens_modal` is declared on the `#[command]` or on the `#[route]` the handler
    serves. The handler macro requires the `OpensModal` responder alias exactly when
    it is declared (any mismatch is an INT-05 compile error), and the declaration
    disables auto-defer for that handler.
  - A runtime state machine (an atomic shared by the handler and the auto-defer
    task) covers what types can't: when the auto-defer wins, a `Pending` call becomes
    `edit_original`; follow-ups are counted (5 for user-installed apps, INT-11); after
    15 minutes calls return `TokenExpired` without an HTTP request (INT-02).
  - Ephemerality is declared at registration (default: public for Commands,
    update-in-place for components); the auto-defer uses it. Commands defer with
    type 5 (only `EPHEMERAL`), components and modal submits with type 6; the V2 flag
    goes on the edit (INT-08).
  - A handler that returns a View gets the right callback without author code
    (FR-61). The Responder adds `IS_COMPONENTS_V2` to every V2 send and edit,
    finalizes Refs (AD-18) and attachments (AD-22), and runs the runtime validator.
  - Webhook sends and edits go through the same serializer: always
    `with_components=true`, per-endpoint flag sets, and `ForeignWebhook::send` takes
    only a non-interactive `StaticView` (WH-02).

### AD-12 — Noctis owns the Components V2 model
- **Binds:** `noctis-ui`, `noctis-core`; FR-23 to FR-31, FR-63, FR-64; CV2-*,
  MOD-*, MSG-03, MSG-07.
- **Prevents:** being blocked by twilight-model gaps (no Radio Group, unknown types
  dropped, media `content_type: null` serialized), and two component models in one
  codebase.
- **Rule:**
  - `noctis-ui` defines the V2 message and modal component types. Placement rules
    are encoded in types: `ContainerChild`, `SectionAccessory`, an `ActionRow`
    enum, separate message and modal select types, const-generic arity and per-node
    `const COST`.
  - These types serialize directly to Discord JSON (unset fields omitted), sent as a
    raw twilight-http body. Flag sets (`CreateFlags`, `EditFlags`, `CallbackFlags`)
    and mention sets (`Users::All | Users::Only(ids)`) are typed per endpoint.
  - twilight-model is used only for ids, interactions, events and commands.
  - Modal submissions decode from `RawJson` (AD-10), so Radio Group, Checkbox Group
    and Checkbox values reach the typed submission struct even when twilight drops
    them (FR-64). Incoming component data that Noctis doesn't model is kept as
    `serde_json::Value`, never dropped.
  - Modal pre-fill values (FR-63) go through the same validator before sending.

### AD-13 — Error model
- **Binds:** all crates; FR-19, FR-22, NFR-4.
- **Prevents:** stringly errors, panics escaping into the runtime, and incompatible
  error shapes between crates.
- **Rule:**
  - Each crate has its own `thiserror` enum. The facade exposes `noctis::Error`
    (`#[non_exhaustive]`), which wraps them.
  - Handlers return `Result<_, HandlerError>`. `HandlerError` is the one public
    type-erased error: `HandlerError::new(impl Error + Send + Sync + 'static)`, with
    an optional user message as `Option<MessageKey>` (AD-16).
  - Every handler, hook and event future runs as
    `AssertUnwindSafe(fut).catch_unwind()`. Every Render Job runs under
    `catch_unwind` on the render pool. A panic becomes an error-sink event and, for
    an interaction, an error response; the Shard keeps running.
  - Library code never uses `unwrap`/`expect` on external input.

### AD-14 — Port shape
- **Binds:** `noctis-ports`, `noctis-core`, `noctis-testkit`, all adapter
  implementers; FR-38, FR-39, FR-45, FR-46, FR-60, FR-71, NFR-6.
- **Prevents:** generic type explosion on `App`, adapters with incompatible
  atomicity, double or missing key prefixes, and serialization mismatches between
  processes.
- **Rule:**
  - Port traits are `Send + Sync + 'static` and dyn-compatible. Async methods return
    `futures_util::future::BoxFuture<'_, Result<T, PortError>>`. The App holds
    `Arc<dyn Port>`.
  - Store ports take opaque keys and `Bytes` values with TTL and draw from one op
    vocabulary: `get`, `put(ttl)`, `put_if_absent(ttl) -> bool`, `take` (atomic get
    and delete), `delete`, `touch`, `incr_with_ttl(key, by, ttl) -> u64` (atomic;
    TTL set on create). Each port exposes the subset it needs: `RefStore` uses
    `put_if_absent`, `get`, `take`, `delete`, `touch`; `RateLimitStore` uses
    `incr_with_ttl`, `get`, `put`; `ModuleStateStore` adds `version() -> u64`, which
    core polls.
  - `CacheStore` uses `get`, `put`, `put_if_absent`, `delete` and `incr_with_ttl`.
  - `IdentifyQueue` is a coordination port: `acquire(bucket) -> Lease`.
  - `DiscordCache` is typed, not a byte store: `update(&Event)` (core feeds it the
    gateway events its configured resources need) and owned getters (`guild`,
    `channel`, `role`, `member`, `user`, `message`, plus per-guild lists) returning
    `BoxFuture<'_, Result<Option<T>, PortError>>` of twilight-model values. No
    reference or guard crosses the port. An out-of-process implementation serializes
    those values itself (they are serde types); in a Cluster each process writes only
    its own shards' guilds. It is not wrapped in `Namespaced<P>`: each App owns its
    instance, and a shared backend keys by the `AppId` it is given.
  - Conformance suites in `noctis-testkit` test TTL, overwrite, delete, missing key
    and atomicity under concurrent callers (exactly one `take` or `put_if_absent`
    winner, no lost increments). The memory defaults and every third-party adapter
    must pass them.
  - Only core's typed wrapper `Namespaced<P>` prefixes keys with `<app>:<port>:`.
    Adapters store keys verbatim and never prefix. `<app>` is the `AppId`
    (`[a-z0-9-]{1,32}`, default: application id in base62), identical across a
    Cluster. An Adapter opted into sharing (FR-71) is wrapped in `Shared<P>` with
    the prefix `_shared:<port>:`. Key segments escape `:`.
  - Typed values are postcard with a leading schema byte; a schema mismatch reads as
    a miss. Ref records use the codec encoding (AD-5).
  - Adapters receive only keys and serialized bytes, never secrets.

### AD-15 — Rendering pipeline
- **Binds:** `noctis-image`, `noctis-macros`, `xtask`; FR-32 to FR-37, FR-66 to
  FR-70, NFR-3, NFR-8.
- **Prevents:**
  - renders blocking the runtime or starving stills;
  - unbounded render memory;
  - Tailwind classes that pass validation but render as nothing;
  - stale cached images after a data, asset, theme or version change;
  - any Node or Tailwind-CLI tooling.
- **Rule:**
  - `noctis-image` wraps takumi `=2.17.0`. Renders run on the process-scoped rayon
    pool (AD-24) behind a bounded queue; a full queue is a typed error. Admission
    weight is the frame count. Per-job caps (default ≤ 4 MP per job, ≤ 1 MP per
    frame, ≤ 90 frames, ≤ 6 s; fps cap 50 for GIF, 90 for WebP and APNG) are checked
    before enqueue. A job whose requester is gone is skipped; per-frame animations
    stop between frames.
  - Noctis owns a strict Tailwind v4 class grammar in `noctis_image::tw`, which is
    takumi-free and builds with the `render` feature off. `xtask tw-grammar`
    generates it from takumi's source, with theme-token checks and a v3 → v4 rename
    table for suggestions. `tw!`/`view!` validate literal classes against it at
    compile time, together with the tokens in the crate's `noctis-theme.css`
    (parsed with `cssparser`; only custom properties and `@keyframes` are read).
  - Every file a proc macro reads (theme, locale files) is also emitted as
    `const _: &[u8] = include_bytes!("<absolute path>");`, so editing it rebuilds
    the crate. A newly added file is picked up on the next rebuild of the crate,
    which the Usage Skill documents.
    Runtime class strings use the same grammar: an unknown class warns once per
    distinct string and fails tests under the testkit. Dynamic values use typed
    style setters.
  - CI job `tw-crosscheck` renders every grammar entry with takumi and fails on
    drift.
  - Results are cached through `RenderCache`. The key is `blake3(KEY_SCHEMA,
    NOCTIS_VERSION, TAKUMI_VERSION, template_id, template_version, theme_digest,
    postcard(data), sorted asset (id, version) pairs, size, format,
    encode_settings)`. Template data must serialize deterministically (the derive
    rejects `HashMap` fields). Single-flight deduplication is process-local.
  - Outputs are PNG, JPEG and WebP stills, animated WebP (default) and GIF or APNG.
    The size budget is the smaller of the Upload Budget and a target (default about
    2 MB); over budget, settings degrade in the order fps, size, quality, then still
    frame. The settings used are reported.
  - No font is bundled unless `default-font` is on; no registered font is a startup
    error. The emoji font is opt-in (`image-emoji`), with a startup warning when
    absent. Theme keyframes come from `noctis-theme.css`; redefining a built-in
    animation name is a registration error.
  - Remote images (declared by the Template, including custom emoji) are fetched
    only by `noctis-image`'s `AssetFetcher`: host allow-list (Discord CDN by
    default), byte cap, timeout and bounded concurrency, cached through `AssetStore`.
    takumi never fetches.
  - The `image` facade feature is off by default.

### AD-16 — Localization is compiled
- **Binds:** `noctis-i18n`, `noctis-macros`, `noctis-rules`, `noctis-ui`,
  `noctis-core`; FR-41 to FR-43, FR-72; LOC-01 to LOC-06.
- **Prevents:**
  - runtime "missing key" surprises and placeholder mismatches;
  - command names and content using different sources;
  - widgets hard-coding English or each crate defining its own `Locale`;
  - an overlay changing a published command name.
- **Rule:**
  - `i18n!(default = "en", dir = "locales")` compiles the crate's locale files
    (YAML, named by Discord locale code, parsed with `yaml-rust2`) into typed
    accessors and a `MessageKey` const per key. Both arguments are optional and
    default to the values shown, relative to the crate root. A key missing from the
    default locale is a compile error; other locales fall back with a build warning.
  - The `i18n` facade feature switches on `i18n!`, the public `noctis::i18n` items
    and locale-file reading in `#[command]`. Without it, core still uses
    `noctis-i18n` for the built-in `noctis.*` strings, so the `core → i18n` edge is
    unconditional.
  - `Locale` (the closed 32-code enum), `MessageKey` and `trait Translate` live in
    `noctis-rules`. Widgets in `noctis-ui` take `&dyn Translate` and use only keys
    under the reserved `noctis.` namespace. `noctis-i18n` implements `Translate` and
    ships default `noctis.*` strings.
  - Each Module's catalog is a descriptor. `App::build` merges catalogs per
    language; a key defined by two Modules fails with an error naming both (FR-72).
  - Command localization keys are fixed: `cmd.<path>.name`, `cmd.<path>.desc`,
    `cmd.<path>.opt.<opt>.name`, `cmd.<path>.opt.<opt>.desc`,
    `cmd.<path>.opt.<opt>.choice.<value>`, per-word `cmdword.<word>` (which a
    full-path key overrides), and `ctxcmd.<key>.name` for context-menu Commands,
    whose `key = "…"` attribute names them.
  - **Descriptions.** Without the `i18n` feature, a Command's description is the
    literal `description = "…"` on `#[command]` (option descriptions likewise on
    `#[option]`); a missing one is a CMD-04 compile error. With the `i18n` feature
    and a default-locale file present, `cmd.<path>.desc` in that file is the
    source: a literal may be omitted, a literal that differs from the file is a
    compile error, and a missing key with no literal is CMD-04. `#[command]` reads
    the same `dir`/`default` as `i18n!`; a Module whose `i18n!` uses another `dir`
    passes it as `#[command(.., locales = "<dir>")]`. Locale files are first read at
    compile time in Epic 11 (Stories 11.1 and 11.4); Epic 6 uses literals only.
  - `#[command]` validates default-locale names in its own crate; merged and
    non-default validation (CMD-01 to 05, LOC-02, LOC-03) runs at `App::build` on
    the merged catalog, which is the single source for Publish JSON.
  - `LocaleSource` overlays apply to content keys only. Overlaying a `cmd.*` or
    `cmdword.*` key is rejected, and overlay values are checked against the compiled
    placeholder set when loaded.
  - Resolution (FR-43): User → Guild → default for interactions, Guild → default
    for guild events; a regional code falls back to its base language first.

### AD-17 — Project Template tracks the public API
- **Binds:** `examples/project-template`, `xtask`, CI; FR-49, FR-50.
- **Prevents:** the Noctis Usage Skill drifting from the real API, so agents read
  Noctis source or write code that doesn't compile.
- **Rule:**
  - `xtask api-snapshot` records the facade's public API with `cargo public-api`,
    run on the dated nightly pinned in the Stack (the only nightly use in CI).
  - `xtask skill-check` fails CI if a public item isn't mentioned in
    `examples/project-template/.claude/skills/noctis/`, or if the snapshot changed
    and the skill's recorded API hash didn't.
  - CI builds the template against the workspace.

### AD-18 — Refs are minted in two phases
- **Binds:** `noctis-codec`, `noctis-ui`, `noctis-core`, `noctis-ports`; FR-17,
  FR-27, FR-58.
- **Prevents:** a pure View minting a dangling Ref, a crate below core touching
  `RefStore`, and widgets that can't delete their Refs.
- **Rule:**
  - `noctis-codec` defines `EncodedId = Inline(ArrayString<100>) | RefSlot(PendingRef)`
    with `PendingRef { route, schema, payload: Bytes, ttl, one_shot }`. `noctis-ui`
    and widgets produce only `EncodedId`; they never generate Ref characters.
  - Before validation and send, core's Responder stores each `RefSlot` with
    `RefStore::put_if_absent` (retrying on collision) and calls
    `codec::finish(slot, ref)` for the final `custom_id`. Identical slots in one
    message share one Ref.
  - Ref deletions requested by pure code (Wizard cancel) are a `RefOps` list on the
    returned response; only core executes them. One-shot Refs are consumed with
    `take`.
  - No crate below `noctis-core` holds a `RefStore` handle.

### AD-19 — Route identity lives in `noctis-codec`
- **Binds:** `noctis-codec`, `noctis-ui`, `noctis-core`, `noctis-macros`; FR-15,
  FR-58, FR-59.
- **Prevents:** two Route abstractions, components that can't name their Route, and
  components pointing at Routes with no handler.
- **Rule:**
  - `noctis-codec` defines `trait Route { const ID: RouteId; const SHAPE: RouteShape; const SCHEMA: u8; type Payload: Payload; }`,
    the `Payload` and `Field` traits and the codec. `#[derive(Route)]` implements
    exactly these.
  - `noctis-ui` builds every component from `R: Route` plus an `R::Payload` value
    (`BuyButton { .. }.button()`); there is no string `custom_id` constructor. Route
    component defaults (FR-59) are an associated const on `noctis-ui`'s
    `ComponentRoute`; per-render overrides apply, then the Limits check.
  - Component constructors require `R: Handled`, which only the handler macro
    implements (compile-time half of FR-58). `noctis-core` binds handlers by `R::ID`
    in their own descriptor; `App::build` fails when a Route referenced by a mounted
    Module's descriptors has no mounted handler (dynamic half).

### AD-20 — Widget state rides the View's own Route
- **Binds:** `noctis-ui`, `noctis-codec`, `noctis-core`; FR-25 to FR-28, FR-62,
  UD-4 to UD-6.
- **Prevents:** widgets registering framework Routes that collide with bot Routes,
  runtime-appended fields breaking the proven length, and modal-opening widget
  controls on Routes the author didn't declare.
- **Rule:**
  - Widgets never register Routes. Every control (tab, page, jump, wizard
    next/back/cancel, page indicator) encodes to the owning View's Route, declared
    `#[route("id", view)]`, through the `E` envelope (AD-5): action (`_` author,
    `t` tab, `p` page, `j` jump, `n`/`b`/`c` wizard, `i` inert indicator), tab, page
    and state version.
  - Core handles widget actions by re-rendering the View with the new tab, page or
    step; action `_` goes to the author's handler.
  - Jump-to-page is a String Select, never a modal.
  - A View with a state version (FR-25) re-renders from current state with a notice
    when the incoming version is older.
  - Controls differ by envelope, so ids in one message are unique (CID-02); the
    runtime validator still checks.
  - Wizard step state travels in the Payload when it fits; otherwise the View Route
    is `overflow = spill`.

### AD-21 — Gateway lifecycle: startup safety, sessions and shutdown order
- **Binds:** `noctis-gateway`, `noctis-core`; FR-1 to FR-4, FR-53 to FR-55;
  GW-01 to GW-07.
- **Prevents:** twilight panics at startup, identify storms, double or out-of-order
  session writes, hooks fired by two crates, and lost interactions during shutdown.
- **Rule:**
  - `App::run` checks for a multi-thread tokio runtime and installs a rustls crypto
    provider if none is installed; failures are typed errors, never panics.
  - Startup reads `GET /gateway/bot`: shard count is the recommended one unless
    fixed (a too-small fixed count is a `BuildError`); identify goes through
    `IdentifyQueue` buckets built from `max_concurrency`, never
    `InMemoryQueue::default()`; boot refuses when `session_start_limit.remaining`
    can't cover the shards to identify; a Shard Range not aligned to
    `max_concurrency` logs a warning (FR-3).
  - Intents and event-type filters are derived from registered handlers and cache
    needs; a handler needing an intent the App didn't request is a `BuildError`.
    The interaction-only default is `Intents::GUILDS`.
  - Shard tasks parse raw gateway messages themselves (`twilight_gateway::parse`),
    so interactions keep their raw JSON (AD-10, AD-12).
  - Presence and gateway commands (FR-53) go to the owning Shard's sender; a guild
    outside this process's range and a gateway rate-limit are typed errors.
  - `noctis-gateway` is the only reader and writer of `SessionStore`. It persists
    `(session_id, seq, resume_url)` after observing `GatewayClose` following
    `CloseFrame::RESUME`, and clears entries in `--no-resume` mode. The stored
    sequence is the last received; events dropped under back-pressure are not
    replayed (at-most-once).
  - The gateway reports `ShardEvent`s on a channel; only core turns them into
    Lifecycle Hooks (FR-54).
  - Core owns shutdown, in this order: fire `ShutdownStarted`; call
    `gateway.close_all(Resumable | Fresh)` (shards stop enqueuing); drain both lanes
    until empty or the grace period ends, with the Responder and HTTP still live;
    await `gateway.closed()`; drop ports. Handlers still running at the deadline are
    cancelled and reported to the Error Sink.

### AD-22 — Attachments are content-addressed; renders are awaited in handlers
- **Binds:** `noctis-ui`, `noctis-image`, `noctis-core`, `noctis-ports`; FR-30,
  FR-34, FR-36, FR-37; ATT-01 to ATT-07.
- **Prevents:** two attachment naming schemes, re-uploading unchanged images on
  edit, two Upload Budget checks, and lazy render nodes nobody executes.
- **Rule:**
  - `noctis-ui` defines `Attachment { name, bytes: Bytes, content_type, alt }`.
    `Attachment::new(bytes, ext)` names it `<first 16 hex of blake3(bytes)>.<ext>`;
    `Attachment::with_digest` takes a render cache-key digest instead. No other
    naming exists. `Attachment::rendered(RenderOutput)`, which `ctx.render` uses,
    names it by digest and marks its origin `Rendered`; this marker is how the
    validator tells a rendered image from any other (FR-30). A rendered View is `(MessageTree, AttachmentSet)` keyed by name,
    so one image used twice uploads once.
  - `noctis-ui` validates only that every `attachment://` reference has a member.
    Core's Responder alone enforces the Upload Budget (the interaction's
    `attachment_size_limit`, else 20 MiB) and computes the edit diff: names already
    on the message are kept by id, new ones are uploaded, absent ones are dropped.
  - Views hold only ready attachments. Rendering is awaited inside the handler
    (`ctx.render(template).await`), so the auto-defer covers it.
  - `noctis-ports` owns the render contract, so core never depends on
    `noctis-image`:
    - `trait Template: Serialize + Send + Sync + 'static { const ID: &'static str;
      const VERSION: u32; const SIZE: Size; const FORMAT: Format; fn view() -> ErasedView; }`.
      The value is the Template's data. `ErasedView` is an opaque `Arc<dyn Any + Send + Sync>`
      that only the Renderer implementation interprets.
    - `RenderJob { template_id, template_version, data: Bytes (postcard), size, format }`;
      `Format` carries the encoder settings (fps, duration, quality).
    - `Renderer::render(&self, RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>>`,
      with `RenderOutput { bytes, digest, format, settings_used }`, and
      `Renderer::register(&self, &[TemplateDescriptor]) -> Result<(), RenderError>`.
  - `Module::template::<T>()` records a `TemplateDescriptor { id, version, view }`;
    `App::build` rejects duplicate ids and hands the descriptors to the Renderer,
    which fails the build for a Template it can't render.
  - `ctx.render(t)` postcard-encodes `t`, builds the `RenderJob` from `T`'s consts
    (`ctx.render_with` overrides size and format), calls the App's
    `Arc<dyn Renderer>` and returns `Attachment::rendered(output)`.
  - `noctis-image` implements `Renderer`; `#[derive(Template)]` (image feature)
    implements `Template` and supplies the `ErasedView`. Without a Renderer,
    `ctx.render` returns a typed error.

### AD-23 — Fixed interaction pipeline and Extensions
- **Binds:** `noctis-core`, `noctis-prefix`; FR-14, FR-18 to FR-22, FR-43, FR-45,
  FR-46, FR-47.
- **Prevents:** guards, rate limits, module checks and middleware running in
  different orders per implementer, rate-limit notices arriving after a defer, and
  Extension conflicts resolved differently at build and at run time.
- **Rule:**
  - Core runs every interaction through exactly these stages:
    1. decode the `custom_id` or resolve the command (malformed, unknown or bad tag:
       FR-19 path with a logged warning);
    2. resolve a Ref (`take` for one-shot, else `get`): missing goes to the Expired
       Handler, route mismatch is forged, schema mismatch is stale;
    3. Boot Flag, `GuildAccessList` and Module-enabled checks ("disabled here");
    4. locale resolution, including the App's optional `LocaleResolver`;
    5. rate-limit `incr_with_ttl` (FR-46);
    6. auto-defer armed at `received_at + ack_deadline` (default 2500 ms) unless
       `opens_modal`;
    7. before-Middleware, parent Module first, then registration order (may override
       locale, insert Extensions or stop);
    8. Route Guards;
    9. handler;
    10. after-Middleware in reverse order, then Error Sink routing.
  - Stages 1 to 5 call no author code except `LocaleResolver`, and do no port I/O
    except the Ref lookup, the cached Module state and the rate-limit increment.
  - Notices from stages 3, 5 and 8 are localized ephemeral replies; after an
    auto-defer they are sent as an ephemeral follow-up and the deferred original is
    deleted.
  - Events run stages 3, 4, 7, 9 and 10. `noctis-prefix` reuses stages 3 to 10.
  - Extensions are a per-interaction type map of `Send + Sync + 'static` values.
    Inserting an existing type replaces it (the child Module wins) with a debug
    trace. Middleware descriptors declare the types they provide; `App::build`
    fails when a handler extracts `Ext<T>` that no before-Middleware on every
    ancestor path provides, and `Option<Ext<T>>` opts out. Extensions reach
    after-Middleware and Error Sinks and are never written to a port.

### AD-24 — Resource scoping and the one Discord HTTP client
- **Binds:** `noctis-core`, `noctis-gateway`, `noctis-image`, `noctis-testkit`,
  facade; FR-34, FR-47, FR-51, FR-52.
- **Prevents:** a request bypassing the configured proxy, N render pools for N Apps,
  global caches reconfigured after first use, and merged metrics across Apps.
- **Rule:**
  - Core builds each App's single `Arc<twilight_http::Client>`, with the proxy when
    configured (`proxy(url, true)` and `ratelimiter(None)`) and a request timeout
    (default 10 s), and passes it to `noctis-gateway`. No other crate constructs a
    Discord HTTP client. A Cluster without a proxy logs a startup warning naming the
    risk (FR-52).
  - **Discord fakes for tests** live in `noctis-testkit` (`noctis_testkit::fake`):
    `FakeDiscordHttp`, a local `hyper` server with per-test routes and a request
    log, and `FakeGateway`, a local `tokio-websockets` server speaking HELLO,
    IDENTIFY/READY, RESUME, heartbeats, dispatches and close codes. The App reaches
    them only through twilight's own override points: the HTTP proxy setting above
    and the gateway `proxy_url` (exposed as `.gateway_url(..)`, also used for gateway
    proxies). Core has no test-only branch. End-to-end tests for Epics 5 to 7 live in
    `crates/noctis-testkit/tests/`, and `TestApp` (Story 8.6) runs on the same fakes.
  - Non-Discord HTTP exists only in `noctis-image`'s `AssetFetcher` (AD-15).
  - **Process-scoped** (owned by `noctis::Runtime`, which Apps attach to): render
    pool and queue, takumi global caches, font registry, emoji font, metrics
    recorder. The first App's configuration freezes them; a later conflicting one is
    a `BuildError`.
  - **App-scoped:** HTTP client, gateway and Shards, Registry, Responder, namespaced
    ports, Theme, `State<T>`, `Limits`, `AppId`.

### AD-25 — Allocation and memory discipline
- **Binds:** `noctis-codec`, `noctis-core`, `noctis-gateway`, `noctis-ports`,
  `noctis-testkit`; UD-8, NFR-1 to NFR-3, NFR-9.
- **Prevents:** per-interaction heap churn and unbounded framework memory at millions
  of users.
- **Rule:**
  - Route lookup and fixed-width inline Payload decode allocate nothing: the codec
    decodes from `&str` slices, Registry maps are keyed by `&'static str` and looked
    up with borrowed keys, and string fields decode to `Cow<'a, str>`. Encoding
    writes into `ArrayString<100>`.
  - The dispatch path makes at most one port call for the Payload (a Ref resolve)
    and boxes one handler future per interaction.
  - Every framework cache, queue and map has an entry or byte budget with a
    documented default (moka weighers by bytes). No unbounded map is keyed by guild,
    user, channel, message or interaction.
  - Per-guild framework state is a fixed-size record (Module bitset, locale), within
    the NFR-1 budget (initial goal ≤ 1 KiB per idle guild, excluding the Discord
    cache).
  - Benchmarks live in `noctis-core/benches` with budgets in
    `benches/budgets.toml`: a dhat counting-allocator test that fails on any
    allocation in Route lookup or inline decode; a memory bench over 10k idle
    guilds; the NFR-3 latency bench (10k events/s on 4 cores: 60% `MESSAGE_CREATE`
    with no handler, 25% handled guild events, 10% component interactions with
    inline Payloads, 5% command interactions; no-op handlers). The reference card
    render bench lives in `noctis-image/benches` (it needs no core) and reads the
    same `budgets.toml`.
  - The testkit ships `assert_memory_flat`, which runs N interactions and fails if
    framework memory grows with N (NFR-2).

### AD-26 — Declarative, tescord-like authoring surface
- **Binds:** `noctis-macros`, `noctis-core`, facade, Usage Skill; FR-7, FR-8,
  FR-13, FR-44, FR-61, FR-74, UJ-4.
- **Prevents:** an API that is idiomatic but foreign to the target author, so that
  migrating from tescord becomes a rewrite.
- **Rule:**
  - A Command is an `async fn` with its Command Path or Pattern:
    `#[command("settings logs channel")]`. Word count decides command, subcommand
    or group. Static commands have no builder trees. Context-menu commands use
    `#[user_command]` and `#[message_command]`.
  - A Module is the unit a tescord Pack maps to: `Module::new("a/b")` collects
    commands, Routes, event handlers, Middleware and hooks; Modules merge by base
    word (FR-9).
  - Components are built from Route values (`BuyButton { .. }.button()`), the typed
    counterpart of tescord's `buildComponent`.
  - Publish needs no registration code, but it is diff-based and runs only in the
    publisher process or the CLI mode (FR-13), never on Shard ready.
  - Names follow tescord where the concept exists (Module ≈ Pack, Route, Ref,
    Locale). The Usage Skill ships the tescord → Noctis concept map.
  - Where tescord's behavior and Discord's rules disagree, Noctis follows Discord
    and fails at compile time.

### AD-27 — The UI Doctrine is built into `noctis-ui`
- **Binds:** `noctis-ui`, `noctis-core`, `noctis-image`, Project Template, Usage
  Skill; UD-1 to UD-6, FR-25 to FR-28, FR-61, FR-62.
- **Prevents:** built-in widgets or examples drifting from the doctrine, and lists
  that fail on Limits instead of paginating.
- **Rule:**
  - Built-in widgets (`View`, `Navbar`, `Wizard`, `Paginator`) produce only V2
    messages. Legacy `content` and embeds exist only behind `LegacyMessage`
    (UD-1).
  - Rendered images enter Views only as Thumbnail or Media Gallery attachments and
    never carry actionable information (UD-2).
  - A Command with no options and a View-returning handler needs no response code
    (UD-3, FR-61).
  - `Wizard` shows "Step n of m: Title" progress, never page numbers (UD-4).
    `Navbar` is a top Action Row of View-Route buttons with the active tab disabled;
    more tabs than one row holds is a typed error (UD-5).
  - The default Overflow Policy paginates (UD-6, FR-62): the Paginator reserves the
    Chrome's cost, packs items against the App's `Limits`, and paginates inside the
    active Navbar tab. Only an item that can't fit beside the Chrome is a typed
    error. `Error` and `Truncate` are opt-in.
  - Views default to `allowed_mentions.parse = []` (MSG-05).
  - `xtask skill-check` also lints the Project Template for doctrine violations
    (embeds or `content` in interactive surfaces, manual response-type selection).

### AD-28 — Release, versioning and supply chain
- **Binds:** workspace, `xtask`, CI, release; FR-48, FR-74, NFR-7, NFR-8, PRD §6.
- **Prevents:** unannounced breaking changes, sub-crates published out of order,
  silent MSRV bumps, vulnerable or unlicensed dependencies, and renderer upgrades
  that change output unseen.
- **Rule:**
  - Noctis follows semver. Before 1.0, breaking changes bump the minor version and
    get changelog migration notes. `noctis::image::unstable` is outside semver.
    Public types wrapping upstream enums are `#[non_exhaustive]`.
  - release-plz opens the release PR from `main` and, on merge, publishes all
    crates at the shared version in layer order. The crates.io token exists only in
    the release job's protected environment.
  - **Toolchain and builds.** `rust-toolchain.toml` pins stable 1.97.0 for
    development, clippy, tests and UI tests; `.stderr` files are blessed on it, and a
    toolchain bump re-blesses them in the same PR. The MSRV stays 1.91 and is checked
    by its own CI job. The workspace uses `resolver = "3"`. Every cargo command
    uses `--release`. trybuild never passes `--release` but runs cargo under
    `target/tests/trybuild/`, so the repo-root `.cargo/config.toml` sets
    `[profile.dev] opt-level = 3, debug = false, debug-assertions = false,
    overflow-checks = false, incremental = false`; the dev profile is therefore
    release-equivalent for every cargo invocation in the repo, and this file must not
    be weakened. `[profile.release]` stays the real release profile. trybuild suites
    are `compile_fail` only (`cargo check`, no codegen); boundary passes are ordinary
    integration tests. `cargo public-api` and `cargo semver-checks` run in CI only.
  - Required PR gates: fmt; clippy `-D warnings` on all targets; tests and doctests;
    `missing_docs` denied for `noctis`; each facade feature alone and all features
    together; `cargo +1.91 check --release` (MSRV); trybuild UI tests on the pinned
    toolchain; `cargo-semver-checks` on `noctis`; `cargo-deny` (advisories, licenses,
    bans, sources); `xtask deps-check`, `rules-catalog`, `api-snapshot` and
    `skill-check`; `tw-crosscheck`; the allocation test; `insta` View snapshots;
    Project Template pixel snapshots; the template build.
  - On `main`: latency and memory benchmarks report against `budgets.toml` and gate
    once PRD Q3 confirms the numbers; a job builds against the latest takumi and
    reports.
  - The MSRV is the highest `rust-version` among dependencies (1.91 now). Raising it
    is a minor release noted in the changelog.
  - A takumi bump is its own PR that regenerates the Tailwind grammar and passes
    `tw-crosscheck` and the pixel snapshots.
  - Every FR-74 guide exists at release and is linked from the README.

### AD-29 — Observability contract
- **Binds:** all runtime crates; FR-22, FR-44, NFR-5, NFR-6.
- **Prevents:** metric names and labels diverging between crates, unbounded label
  cardinality, merged metrics across Apps, and secrets in logs.
- **Rule:**
  - Tracing spans: `noctis.shard`, `noctis.interaction`, `noctis.event`,
    `noctis.render`, `noctis.publish`. Fields: `app`, `shard`, `guild_id`, `module`,
    and the route or command path. The interaction span covers every pipeline stage.
  - Metrics go through the `metrics` facade with the `noctis_` prefix and an `app`
    label: `noctis_lane_depth` (lane), `noctis_events_dropped_total` (event type),
    `noctis_event_overflow_depth`, `noctis_ack_latency_seconds`,
    `noctis_autodefer_total`, `noctis_handler_duration_seconds` (kind),
    `noctis_handler_errors_total` (kind), `noctis_render_duration_seconds`
    (template), `noctis_render_queue_depth`, `noctis_render_rejected_total`
    (reason), `noctis_ref_lookups_total` (result: hit, miss, stale, forged),
    `noctis_shard_status` (shard), `noctis_http_requests_total` (status class).
  - Labels never carry user, guild, channel or message ids or free text.
  - The token and the integrity key are newtypes with redacted `Debug` and
    `Display`; neither is ever logged.
  - Error Sinks receive every handler error, panic, hook error and overflow drop.
    The default sink logs at `error`.

## Consistency Conventions

| Concern | Convention |
| --- | --- |
| Crate naming | `noctis-<area>`, lib name `noctis_<area>`; facade feature names equal the area (`image`, `prefix`, `i18n`, `testkit`), plus `image-gif`, `image-emoji` and `default-font` |
| Route ids | `[a-z0-9._-]{1,24}` ASCII, set by `#[route("id")]`, stable across releases; ids starting with `_` are reserved; renaming one breaks live components |
| Module ids | path-like `a/b/c`, unique per App |
| App ids | `[a-z0-9-]{1,32}`, identical across a Cluster |
| Rule ids | `<AREA>-<nn>` as assigned in `docs/discord-rules.md`, shared by code, error messages, docs and tests; new rules take the next number; ids are never reused |
| Text length | `noctis_rules::len::discord_len` for user text; framework-encoded strings are ASCII |
| Compile errors | `[<RULE-ID>] <what is wrong>`, `= note:` lines with the derivation, a `= help:` line with a concrete fix, span on the offending token |
| BuildError | lists every violation; each names the rule id, the Module(s) and the actual vs allowed value; `--publish --dry-run` runs the same checks |
| Options | `derive(Options)` rejects a required field after an optional one (OPT-01); it never reorders |
| Errors | `thiserror` enum per crate, named `<Area>Error`; `HandlerError` is the only type-erased public error |
| Async | tokio only; public async APIs are `async fn` or return `impl Future + Send`; no blocking calls in async |
| Ids and time | twilight `Id<Marker>` everywhere; durations are `std::time::Duration`; timestamps are UTC; deadlines measure from `Inbound::received_at` |
| Serialization | Discord JSON with serde; port values postcard plus schema byte; `custom_id` and Ref records with `noctis-codec` only |
| Config | builder API in code, plus `App::from_env()` reading `NOCTIS_*` variables; no config file required |
| Command line | `App::run()` parses `std::env::args()` with a small built-in parser (no clap): `--publish`, `--publish-guild <id>`, `--publish-clear`, `--dry-run`, `--features a,b\|all`, `--no-resume`. `App::run_with_args(impl IntoIterator<Item = OsString>)` takes explicit arguments (tests, several Apps); `noctis::cli::NoctisArgs::split(args)` returns Noctis's flags plus the remaining arguments for the bot's own parser. An unknown flag reaching `App::run` is a typed error |
| Builds | `--release` for every cargo command; dev profile made release-equivalent by `.cargo/config.toml` (AD-28); toolchain from `rust-toolchain.toml` |
| Features | additive only; default features build a working bot |
| Docs | every public item has rustdoc; runnable examples are doctests |
| Tests | trybuild `compile_fail` fixtures at `tests/ui/<area>/<RULE-ID>_<slug>.rs` with the id in `.stderr`; boundary passes as ordinary integration tests in `tests/boundary/` (modules of `tests/boundary.rs`); ports pass their conformance suite; Views are snapshot-tested with `insta`; Discord I/O is faked only through `noctis_testkit::fake` (AD-24) |

## Stack

| Name | Version |
| --- | --- |
| Rust (MSRV / edition) | 1.91 / 2024 |
| Rust stable for development, clippy and UI tests (`rust-toolchain.toml`) | 1.97.0 |
| Rust nightly (cargo public-api job only) | nightly-2026-10-15 |
| tokio | 1.53 |
| twilight-gateway / -http / -model / -cache-inmemory / -http-ratelimiting | 0.17.1 |
| takumi | =2.17.0 |
| serde / serde_json | 1.0 |
| postcard | 1.1 |
| thiserror | 2.0 |
| futures-util | 0.3 |
| tracing / metrics | 0.1.44 / 0.24 |
| arc-swap | 1.9 |
| moka (memory adapters) | 0.12 |
| rayon | 1.12 |
| blake3 | 1.8 |
| bytes | 1.12 |
| arrayvec | 0.7 |
| getrandom | 0.4 |
| regex (noctis-rules) | 1.13 |
| syn / quote (macros) | 3.0 / 1.0 |
| trybuild / insta / proptest | 1.0 / 1.49 / 1.12 |
| hyper / hyper-util / http-body-util (testkit HTTP fake) | 1.12 / 0.1.21 / 0.1.5 |
| tokio-websockets (testkit gateway fake) | 0.13 |
| yaml-rust2 (locale files, macros) | 0.13 |
| cssparser (theme file, macros; MPL-2.0, allowed in `deny.toml`) | 0.38 |
| criterion / dhat (benches) | 0.8 / 0.3 |
| cargo-public-api | 0.52 |
| cargo-semver-checks / cargo-deny / release-plz | 0.51 / 0.20 / 0.3 |

## Structural Seed

```text
noctis-rs/
  Cargo.toml                 # [workspace] resolver = "3", shared version, [workspace.dependencies], [workspace.lints]
  rust-toolchain.toml        # stable 1.97.0 (AD-28)
  .cargo/config.toml         # release-equivalent [profile.dev] (AD-28)
  deny.toml                  # cargo-deny policy
  release-plz.toml
  crates/
    noctis/                  # facade: re-exports, features, Runtime
    noctis-rules/            # Discord limits and validators (rule ids), Limits, Locale, MessageKey, Translate
    noctis-codec/            # custom_id layout, Route/Payload/Field traits, EncodedId, RefRecord, BoundedStr
    noctis-ui/               # V2 and modal model, Attachment, validator, View/Navbar/Wizard/Paginator
    noctis-ports/            # port traits incl. DiscordCache, Template/RenderJob/Renderer seam, memory defaults
    noctis-gateway/          # shard tasks, lanes, Inbound, identify, SessionStore writer
    noctis-core/             # App, Module, Registry, pipeline, workers, Responder, HTTP client, publish
      benches/               # allocation, memory, latency, render benches + budgets.toml
    noctis-macros/           # command!, user/message_command, derive(Route/Payload/PayloadEnum/Options/Choices/Modal), view!/tw!, i18n!
    noctis-i18n/             # locale runtime, merged catalogs, default noctis.* strings
    noctis-image/            # tw grammar (takumi-free), takumi renderer, render pool, fonts, AssetFetcher
      benches/               # reference card render bench
    noctis-prefix/           # opt-in prefix commands (P2)
    noctis-testkit/          # Discord HTTP/gateway fakes, simulated Inbound, conformance suites, snapshot helpers, assert_memory_flat
  examples/
    project-template/        # standalone bot + AGENTS.md + .claude/skills/{noctis,rust-best-practices}
    <feature>/               # small focused examples; workspace members with publish = false
  docs/
    discord-rules.md         # rules catalog: rule, limit, source, enforcement point
    guides/                  # FR-74 guides: concept map, Cluster deployment
  xtask/                     # deps-check, rules-catalog, api-snapshot, skill-check, tw-grammar, tw-crosscheck, bench
  .github/workflows/         # ci.yml (PR gates), main.yml (benches, latest takumi), release.yml (release-plz)
```

```mermaid
flowchart LR
  subgraph Process["Bot process (one Shard Range)"]
    S1[Shard task 1] --> IL[(Interaction lane)]
    S2[Shard task N] --> IL
    S1 --> EL[(Event lane, bounded)]
    S2 --> EL
    EL --> OV[(Overflow, capped)]
    IL --> W[Worker set]
    EL --> W
    OV --> W
    W --> PL[Pipeline and handlers]
    PL --> RESP[Responder]
    PL --> RP[Render pool]
    PL --> P[Ports]
    RP --> AF[AssetFetcher]
  end
  RESP --> HTTP[App HTTP client]
  HTTP --> PROXY[HTTP proxy, documented setup for Clusters]
  PROXY --> DISCORD[(Discord API)]
  S1 <--> GW[(Discord Gateway)]
  AF --> CDN[(Discord CDN, allow-listed hosts)]
  P --> STORE[(Memory default or user's Redis or SQL)]
```

**Deployment envelope:**
- **Unit of deployment:** one binary. A single process runs every shard; a Cluster
  runs N processes, each with a Shard Range.
- **Shared services in a Cluster:** shared port adapters (Redis/SQL, written by the
  user), a shared HTTP proxy, the `IdentifyQueue` port, and one
  `NOCTIS_PAYLOAD_KEY`.
- **Publishing:** exactly one process has `publisher = true`; `--publish`,
  `--publish-guild` and `--publish-clear` are CLI modes of the same binary.
- **Environments:** dev uses guild-scoped Publish to a test guild; prod uses global
  Publish.
- **Secrets:** bot token and payload key from the environment only; the crates.io
  token only in the release environment.

## Capability → Architecture Map

| Capability / Area | Lives in | Governed by |
| --- | --- | --- |
| FR-1, FR-2 App builder, lifecycle, automatic sharding | noctis-core, noctis-gateway | AD-21, AD-24 |
| FR-3, FR-4, FR-55 Shard Ranges, resume, resumable shutdown | noctis-gateway, noctis-core | AD-21, AD-14 |
| FR-5, FR-6 dispatch, back-pressure, typed event handlers | noctis-gateway (lanes), noctis-core (workers) | AD-10, AD-23 |
| FR-52 shared HTTP rate limiting | noctis-core | AD-24 |
| FR-53 presence and gateway commands | noctis-gateway | AD-21 |
| FR-54 Lifecycle Hooks | noctis-core | AD-21, AD-7 |
| FR-7 to FR-12, FR-56, FR-57 commands, patterns, merge, options, metadata, context menus | noctis-macros, noctis-rules, noctis-core | AD-8, AD-7, AD-4, AD-26 |
| FR-13 Publish | noctis-core | AD-7, AD-26 |
| FR-14 prefix commands (P2) | noctis-prefix | AD-23 |
| FR-15 to FR-17, FR-58 to FR-60 Routes, Payloads, Refs, route-bound components, defaults, evolution | noctis-codec, noctis-ui, noctis-core | AD-5, AD-6, AD-18, AD-19 |
| FR-18 to FR-22 Expired Handler, untrusted input, Middleware, auto-ack, error routing | noctis-core | AD-23, AD-11, AD-13 |
| FR-23, FR-24, FR-29, FR-31, FR-63, FR-64 V2 builders, Limits, modals, raw escape hatch, pre-fill, Radio/Checkbox | noctis-ui, noctis-rules | AD-12, AD-3, AD-11 |
| FR-25 to FR-28, FR-61, FR-62 Views, Navbar, Wizard, Paginator, View responses and overflow | noctis-ui, noctis-core | AD-20, AD-27, AD-11 |
| FR-30 attachments | noctis-ui, noctis-core | AD-22 |
| FR-65 Views over webhooks | noctis-core, noctis-ui | AD-11, AD-12 |
| FR-32 to FR-37, FR-66 to FR-70 Templates, formats, render pool, fonts, cache, Renderer seam, Tailwind, remote images, emoji, animation, caps | noctis-image, noctis-macros, noctis-ports | AD-15, AD-22, AD-24 |
| FR-38 to FR-40, FR-71 Adapters, conformance, Discord cache (`DiscordCache`, `ctx.cache()`), namespacing | noctis-ports, noctis-testkit, noctis-core | AD-1, AD-14, AD-21 |
| FR-41 to FR-43, FR-72 command localization, typed strings, resolution, Module-scoped Locales | noctis-i18n, noctis-macros, noctis-rules | AD-16 |
| FR-44 to FR-47, FR-73 Modules, enable/disable, rate limits, multiple Apps, Boot Flags | noctis-core | AD-7, AD-23, AD-24, AD-14 |
| FR-48 facade and features | noctis | AD-2, AD-28 |
| FR-49, FR-50 Project Template, Usage Skill | examples, xtask | AD-17, AD-27 |
| FR-51 testing kit | noctis-testkit | AD-10, AD-14, AD-25 |
| FR-74 launch documentation | docs, CI | AD-28, AD-26 |
| FR-75 to FR-79 enforcement points, provable length, compile-time commands and trees, rules catalog | noctis-rules, noctis-macros, noctis-codec, docs | AD-3, AD-4, AD-5, AD-6, AD-8 |
| UD-1 to UD-6 UI Doctrine | noctis-ui, Project Template | AD-27, AD-20 |
| UD-7, NFR-2 no per-message state | noctis-core | AD-9, AD-25 |
| UD-8, NFR-1, NFR-9 allocation and memory budgets | noctis-core, noctis-codec | AD-25 |
| NFR-3 latency benchmarks (event mix, reference card) | noctis-core benches | AD-25, AD-28 |
| NFR-4 reliability | all runtime crates | AD-13 |
| NFR-5 observability | all runtime crates | AD-29 |
| NFR-6 security | noctis-codec, noctis-ports, noctis-image | AD-5, AD-14, AD-15, AD-29 |
| NFR-7, NFR-8 toolchain and quality gates | workspace, CI | AD-28, AD-17 |
| NFR-10 rule enforcement coverage | noctis-rules, xtask | AD-3, AD-4 |

## Deferred

- **Official Redis/SQL adapter crates:** v1.x. The port shape (AD-14) and the
  conformance suites make them additive.
- **Cross-process request bus and live resharding:** until a Cluster deployment
  needs them. `IdentifyQueue` and `SessionStore` already cover restarts.
- **Remote render service:** only the `Renderer` seam exists in v1.
- **twilight 0.18 migration:** handled in `noctis-gateway` and the twilight-facing
  parts of core, without facade changes. 0.18 models Checkbox but not Radio Group,
  so AD-12 stands.
- **Strict upstream Tailwind parser in takumi:** an upstream PR is a follow-up; if
  it lands, `tw-grammar` may read it instead of generating from source.
- **HTTP client crate for `AssetFetcher`:** chosen in the image epic; its owner and
  limits are fixed by AD-15.
- **PRD open questions** (each defaults conservatively and is revisited when
  answered):
  - Q1 and Q4: the 4000-character counting unit, the single Text Display maximum
    and media URL length. Defaults sit in `Limits`; tracked in
    `docs/discord-rules.md`.
  - Q2: animated WebP on every client. Default stays WebP with GIF fallback;
    revisit after the mobile check.
  - Q3: NFR-1, NFR-3 and NFR-9 numbers. Benches report until the first benchmark
    confirms them, then gate (AD-28).
  - Q5: legacy → V2 switch on modal submit. `LegacyMessage` → V2 edits are a typed
    error until verified.
  - Q6: ModuleStateStore propagation delay. Default poll interval 5 s; the documented
    delay equals the poll interval.
- **Internal module layout of each crate:** owned by the code.
