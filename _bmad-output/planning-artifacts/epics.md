---
status: draft
stepsCompleted: [1, 2]
inputDocuments:
  - _bmad-output/planning-artifacts/prds/prd-noctis-rs-2026-10-19/prd.md
  - _bmad-output/planning-artifacts/prds/prd-noctis-rs-2026-10-19/addendum.md
  - _bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md
  - docs/discord-rules.md
  - README.md
  - examples/project-template/
---

# noctis-rs - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for noctis-rs, decomposing the requirements from the PRD, UX Design if it exists, and Architecture requirements into implementable stories.

### Workflow decisions (autonomous run)

The user asked for an autonomous run with no questions. Every menu or confirmation
prompt in the workflow was answered with the option that continues; each decision is
recorded here.

- **Step 1, document discovery:** used the final PRD (`prd.md` + `addendum.md`), the
  architecture spine (`ARCHITECTURE-SPINE.md`), the Discord Rules Catalog
  (`docs/discord-rules.md`, rule ids), the README roadmap (milestone order) and the
  Project Template (`examples/project-template/`, including its Noctis Usage Skill).
  Excluded as process artifacts, not requirements: the PRD `reconcile-*.md`,
  `review-rubric.md` and `.memlog.md` files, the architecture `reviews/` folder, the
  product brief (superseded by the PRD) and the raw research reports (already folded
  into the PRD, addendum and catalog). No UX design contract exists
  (`ux-designs/` is absent); the PRD's UI Doctrine (§4.0, UD-1..UD-8) is used as the
  UX design requirements.
- **Step 1, requirements confirmation:** [C] chosen; the extracted inventory below
  was accepted as complete.
- **Step 2, epic structure approval and menu:** approved the 15-epic structure and
  chose [C] Continue (no Advanced Elicitation or Party Mode). Epic order and the
  placement of FR-51 and the V2 model are explained under Epic List.

### Story conventions (apply to every story)

- **Implements** lines name the FR/NFR ids, architecture decisions (AD-n) and Discord
  rule ids (`docs/discord-rules.md`) a story delivers. Tests cite the same rule ids.
- **Release builds only:** every cargo command in a story, in CI and in local
  verification uses `--release` (`cargo build|test|clippy|run --release`); no story
  creates a debug build.
- **Done means** `cargo fmt --check`, `cargo clippy --release --all-targets --all-features -- -D warnings`
  and `cargo test --release` pass, plus every CI gate that exists at that point.
- **Compile-time rules** get trybuild fixtures at `tests/ui/<area>/<RULE-ID>_<slug>.rs`
  whose `.stderr` contains the rule id, with boundary passes in `tests/ui-pass/`
  (run on the 1.91 toolchain). App-build and runtime rules get unit tests that assert
  the typed error variant and name the rule id.
- **Public API changes** (AD-2, AD-17, repo policy): the same story updates
  `examples/project-template/` code that uses the API, its `AGENTS.md` when
  conventions change, and the Noctis Usage Skill
  (`examples/project-template/.claude/skills/noctis/` SKILL.md and `references/`);
  `xtask api-snapshot` and `xtask skill-check` pass. Each such story states this as an
  explicit acceptance criterion.
- **Project Template build gate:** the template is a scaffold written against the
  planned API and uses features from many epics (image, i18n, Module toggles, rate
  limits). Its CI build job runs on every change from Story 1.1 in *tracking* mode
  (reported, non-blocking) and becomes a required gate in Story 14.2 once every API it
  uses has landed. Decision recorded here because FR-49 asks for the build on every
  change while the template cannot compile before Epic 13.

## Requirements Inventory

### Functional Requirements

Priority tier from PRD §8 in brackets.

**4.1 App, Gateway and Sharding**

- FR-1 [P0]: App builder and lifecycle — build an App from token, intents, Modules and Adapters; start; graceful shutdown. Typed errors (not panics) outside a tokio runtime or without a TLS crypto provider (Noctis installs a default provider); invalid token or rejected intents fail startup with a typed error naming the cause; a fatal Shard close is reported as a shard-status Lifecycle Hook and metric; shutdown closes Shards and lets in-flight handlers finish within a configurable grace period.
- FR-2 [P0]: Automatic sharding — start the recommended or an explicit shard total honoring identify concurrency; same-bucket Shards identify in parallel with `max_concurrency = 16`; total from config or Discord; a total that is not a multiple of `max_concurrency` (> 1) warns at startup.
- FR-3 [P1]: Shard Ranges for Clusters — a process runs only its configured Shard Range; Publish runs from exactly one designated process; all processes identify through the IdentifyQueue Adapter (Cluster-wide bucket concurrency); unaligned ranges are accepted with a warning.
- FR-4 [P1]: Session resume — Shards resume sessions after a process restart via the SessionStore Adapter; a rejected stored session falls back to a fresh identify with no operator action.
- FR-5 [P0]: Concurrent event dispatch with backpressure — socket reads and heartbeats never wait on handler queues; interactions use the Interaction Lane (never dropped, never behind events, exempt from the bound, meet the FR-21 deadline with the event queue full); per-event-type policy when the bound is reached (buffer, drop oldest droppable, metered overflow with hard cap, error log past it); per-type drop metric; queue-depth metric per lane; opt-in per-guild ordered dispatch.
- FR-6 [P0]: Typed Event Handlers and derived event filtering — only event types with a handler (or needed by the cache) are deserialized; handlers concurrent by default, one may be marked ordered and runs first; two handlers for one event both kept; identity independent of time/registration timing.
- FR-52 [P1]: Shared HTTP rate limiting — external rate-limit proxy replaces the local limiter (no request bypasses it); Cluster without proxy warns at startup; Cluster guide and Project Template config document proxy mode; configurable HTTP request timeout.
- FR-53 [P2]: Presence and gateway commands — initial and runtime presence for all or one Shard; gateway commands (e.g. request guild members) to the owning Shard; a guild outside the Shard Range and a gateway-command rate limit are typed errors.
- FR-54 [P1]: Lifecycle Hooks — App started; Shard ready/resumed/disconnected/fatally closed; all Shards ready; Publish completed/failed (with diff); Module enabled/disabled (global or per guild); shutdown started. Each fires exactly once per occurrence with typed data and is observable in order by a test App; an erroring hook goes to an Error Sink and doesn't stop the App.
- FR-55 [P1]: Resumable shutdown — graceful shutdown closes each Shard resumably and writes session id, sequence and resume URL to the SessionStore after the final event; a `--no-resume` mode clears stored sessions.

**4.2 Commands and Publish**

- FR-7 [P0]: Command Paths — 1/2/3 words publish as command / subcommand / group + subcommand; > 3 words, empty word or invalid name is a typed error; exceeding the per-command total size is a registration error, not a Publish error.
- FR-8 [P0]: Command Patterns — `(a|b)` and `?` groups expand left to right in declaration order; each expansion validated (FR-7 and Discord structure); a word that is both subcommand and group fails to compile (FR-77) or fails App build when dynamic; expansion cap (default 25) is a typed error; the handler receives a typed value naming the invoked combination.
- FR-9 [P0]: Merge rules — Commands from different Modules sharing a base word merge; conflicting `default_member_permissions`, `contexts`, `integration_types` or `nsfw` is a typed error (no "first wins"); a base with both options and subcommands is a typed error.
- FR-10 [P0]: Typed options and autocomplete — parsed options struct (missing/wrong type is a typed error); required-after-optional and choices+autocomplete are compile errors (derive) or registration errors (dynamic); autocomplete > 25 choices truncated with a warning; autocomplete error/timeout responds with empty choices and routes to the Autocomplete Error Sink; autocomplete receives the focused value and a typed partial struct; failed validators return a localized ephemeral message and the handler doesn't run.
- FR-11 [P0]: Command metadata — `default_member_permissions`, `contexts`, `integration_types`, `nsfw`, guild scope per Command appear in Publish JSON exactly as declared, unset fields omitted; guild-scoped Commands publish only to their guilds.
- FR-12 [P0]: Context-menu Commands — user and message context menus; handler receives the resolved target user (with member) or message as typed values; names localized as whole names.
- FR-13 [P0]: Publish with diff — no write when remote equals local; dry run prints full JSON and a human-readable diff; CLI modes `--publish`, `--publish-guild <id>`, `--publish-clear`; per App with several Apps; never on Shard ready, only in the designated process or CLI mode.
- FR-14 [P2]: Prefix commands (opt-in `prefix` feature, off by default) — with the feature off no prefix code compiles and no message-content intent is needed; with it on, `<prefix><Command Path> <args>` runs the same handler with the same FR-10 typed errors.
- FR-56 [P0]: Unique Command Paths — one handler per Command Path and per context-menu name+type; duplicates (including pattern expansions) are a typed error naming both Modules.
- FR-57 [P0]: Explicit base and group metadata — descriptions (and localizations) of base commands and subcommand groups declared independently; if undeclared, Publish uses a documented deterministic rule and warns; conflicting declarations from two Modules are a typed error.

**4.3 Interaction Routing, Payloads and Refs**

- FR-15 [P0]: Route registration — exactly one Component/Modal Handler per Route; duplicate Route is a typed error; dispatch is O(1) in the number of Routes.
- FR-58 [P0]: Route-bound component construction — components are built from a Route's typed handle; emitting a component for a Route with no handler is a compile error or, for dynamically composed Modules, a typed App-startup error; no codegen/watch step.
- FR-59 [P1]: Route component defaults — style, label, emoji, placeholder, min/max values declared on the Route; per-render overrides; Limits validated after overrides.
- FR-16 [P0]: Typed Payloads — property-tested round trip; separator characters in strings are escaped; `custom_id` never exceeds 100 characters, max length proven at compile time (FR-76), only per-message uniqueness checked at runtime; author types join by implementing a Noctis trait.
- FR-60 [P0]: Payload evolution — every encoded Payload carries a schema version; a Stale Payload goes to the Expired Handler (or the Route's stale handler); documented compatible changes (appending optional fields) decode.
- FR-17 [P0]: Refs with overflow policy — Payloads that don't fit, or by-ref Routes, are stored in the RefStore; Ref = 11 base62 chars with ≥ 64 bits CSPRNG entropy, never sequential or derived; namespaced by App (cross-App Ref treated as missing); per-Route TTL, else App default; one-shot Refs; per-Route overflow policy spill or error.
- FR-18 [P0]: Expired Handler — missing/expired Ref goes to the Expired Handler; default is a short ephemeral localized notice; overridable per App or per Route (e.g. re-render the starting View).
- FR-19 [P0]: Untrusted input — malformed `custom_id`, unknown Route or undecodable Payload is a typed error + logged warning, never a panic; Route Guards (invoking user only, required permission) run before the handler and fail with a localized ephemeral notice; optional integrity tag on inline Payloads, mismatch handled as unknown Route.
- FR-20 [P0]: Middleware with typed Extensions — before/after interactions and events; can stop processing; attaches typed Extensions read by type; deterministic order (registration order, parent before child).
- FR-21 [P0]: Automatic acknowledgement — deferred response if the handler hasn't responded by a configurable deadline (default 2500 ms from gateway receipt); ephemerality declared up front per Command/Route; Commands defer as new deferred message, components/modal submits as in-place update; late responses (> 5 s) still delivered; V2 flag applied on the follow-up edit; modal-opening handlers never auto-deferred (missed deadline logs an error naming the handler), modal-in-response-to-modal-submit is a typed error; after 15 min public Views fall back to bot-token edit, ephemeral ones return `TokenExpired`; user-installed follow-up cap returns a typed error.
- FR-22 [P0]: Error routing — default: short localized ephemeral user error plus full structured trace with interaction context; Error Sinks selected by origin (Command, Component/Modal, Autocomplete, Event, Render).

**4.4 Components V2 UI Kit**

- FR-23 [P0]: Typed V2 builders — Container, Section, Text Display, Thumbnail, Media Gallery, File, Separator, Action Row, Button, all selects; illegal placement doesn't compile (FR-78); `IS_COMPONENTS_V2` on every V2 send/edit, `content`/`embeds`/`poll`/stickers/`shared_client_theme` impossible; editing legacy → View clears legacy fields in one request, V2 → legacy is a typed error; minimal legacy plain-text send path.
- FR-24 [P0]: Limit validation — 40 components counted at every depth; 4000-char Text Display budget; per-field limits (labels, placeholders, option counts, media items, `attachment://` refs, Section shape, gallery counts, link `url` length, premium button rules, accent color, select `default_values`); unique `custom_id` and numeric `id`; error names path, limit and actual value; lenient mode truncates with "…"; all numeric Limits in one configurable value; strict-mode missing alt text is a warning in the cost report.
- FR-25 [P0]: Views — same View for first response and every update; identical render for identical state (except per-render select ids); selects show declared defaults after update; no parsed mentions by default (opt-in per View/message) plus escape helper; optional state version (older version re-renders from current state with a notice); Limit cost available without sending.
- FR-61 [P0]: View responses — Commands, Component and Modal Handlers return a View (optionally with attachments) and Noctis sends the right response type; option-less View-returning Command compiles, publishes with empty options and sends the View with no response code; Component Handler View updates in place (FR-21 deferred variants when late); Project Template has no manual response-type selection.
- FR-62 [P0]: View overflow — default Overflow Policy paginates (10 → 300 items, no code change, no Limit error); Chrome on every page with its cost reserved first; only a single item that can't fit beside the Chrome is a typed error naming it; `Error` and `Truncate` opt-in, `Error` yields the FR-24 error.
- FR-26 [P0]: Navbar — tab click edits the same message; active tab disabled, others enabled, on every page; selected tab travels in the Payload, nothing stored; overflowing tab paginates inside the tab with the Navbar on every page; more tabs than one Action Row is a typed error.
- FR-27 [P0]: Wizard — Steps with "Step n of m: Title" progress (never page numbers); failed validation stays on the Step with an inline message; Back keeps earlier values; Cancel edits to a cancelled state, removes interactive components and deletes the Wizard's Refs; expired Step Ref goes to the Expired Handler, default re-renders Step 1 with a notice; a modal-collecting Step returns to a message Step before any next modal without author code.
- FR-28 [P0]: Paginator — packs as many items per page as fit the Limits, reserving room for surrounding content; 300 items: no page exceeds a Limit, navigation reaches every item; a single item that can't fit is a typed error; current page in the Payload, nothing stored; > 5 pages adds a jump control reaching any page in bounded clicks; optional rendering as chunked String Selects (25 options each).
- FR-29 [P0]: Modals with V2 components — top level Label or Text Display; Labels wrap Text Input, any select, File Upload, Radio Group, Checkbox Group or Checkbox; typed submission struct with selects joined to resolved entities and File Upload values joined to attachments; builder rejects `disabled`, > 5 top-level components, title/Label length violations, Radio options outside 2–10, Checkbox Group outside 1–10, `required` on Checkbox, `min_values = 0` with `required = true`; never emits deprecated Action-Row-wrapped Text Input or Text Input `label`; File Upload exposes metadata + download helper, docs state the URL is ephemeral.
- FR-63 [P1]: Modal pre-fill — Text Input initial values; default selections for selects, Radio Groups and Checkbox Groups; a pre-fill value breaking a field limit is a typed error before sending.
- FR-64 [P1]: Radio Group and Checkbox Group with raw fallback — submits containing Radio Group, Checkbox Group or Checkbox decode into the typed struct even when twilight drops their data; upgrading twilight requires no author change.
- FR-65 [P1]: Views over webhooks — send/edit a View through a webhook; always request component support; interactive components through a non-owned webhook are a typed error; only endpoint-valid flags sent.
- FR-30 [P1]: Attachments — `attachment://` names generated and unique; edit drops unused attachments and keeps referenced ones without re-upload; attachments over the Upload Budget rejected before sending; strict mode: rendered image without alt text in Thumbnail/Media Gallery is an error (lenient: warning).
- FR-31 [P1]: Raw escape hatch — raw component JSON for types twilight doesn't model (counted by the validator); unknown component types in received messages/modal submits still dispatch, exposed as raw JSON; edits never re-send unparsed components; raw REST requests go through the App's HTTP client and rate limiting (or FR-52 proxy).

**4.5 Image and GIF Rendering**

- FR-32 [P1]: Templates — Rust Templates with Tailwind classes and dynamic data; unknown classes in literal class strings are compile errors (same grammar as rendering, inside the proc macro); user text enters only as text content; an App-registered Theme's tokens are usable and accepted by compile-time validation.
- FR-66 [P1]: Tailwind v4 class names and unknown classes — compile error for a known v3 name suggests the v4 replacement (`bg-gradient-to-r` → `bg-linear-to-r`); dynamic values via typed style setters; unknown class in a runtime string warns once per distinct string and fails tests in the testkit.
- FR-33 [P1]: Output formats — PNG, JPEG, WebP stills; animated WebP (default), GIF, APNG; size budget = min(Upload Budget, target ≈ 2 MB); over budget degrades fps → size → quality → optional still, then a typed error; settings used are reported.
- FR-69 [P1]: Animated Templates — CSS/Tailwind animations with Theme keyframes, or one Template state per frame; fps above the format cap is a typed error before rendering; redefining a built-in animation name is a registration error; scenes of different sizes are a typed error.
- FR-34 [P1]: Bounded render pool — dedicated bounded pool; bounded queue, full queue fails fast with a typed error; render-triggering Commands use FR-21 automatically; render parallelism never on tokio workers or a pool shared with the author.
- FR-70 [P1]: Per-job render caps — pixels, frames, duration checked before queueing (over-cap fails, never enters the queue); animated jobs weigh more in admission control; abandoned jobs dropped, per-frame animations stop between frames.
- FR-35 [P1]: Fonts and assets — font and static image registration; no font is a clear startup error unless `default-font`; fallback fonts with priority and Unicode ranges (e.g. CJK) without Template changes.
- FR-68 [P1]: Emoji rendering — opt-in color emoji font (Twemoji); without it, a startup warning when any Template is registered and the Usage Skill documents boxes; Discord custom emoji render as images fetched under FR-67.
- FR-67 [P1]: Remote image fetching — Templates declare remote images, all resolved before render; per-fetch timeout and byte cap; failed/oversized/undecodable images replaced by a fallback and logged (fail only when marked required); host allow-list (default Discord CDN); avatar/icon helpers keyed by id, asset hash and size; animated avatars animate in animated output; fetch/decode off the async runtime under a concurrency bound.
- FR-36 [P1]: Render cache and deduplication — RenderCache Adapter; identity covers Template + version, data, size, format/settings, asset versions, Noctis and renderer versions; N concurrent identical requests cause one render; attachment filename derived from the cache key.
- FR-37 [P2]: Renderer seam — Renderer is a trait; a test Renderer replaces the local one with no change to Templates, Views or handlers.

**4.6 Adapters**

- FR-38 [P0]: Adapter traits with defaults — RefStore, SessionStore, RateLimitStore, RenderCache, AssetStore, LocaleSource, CacheStore, ModuleStateStore, IdentifyQueue, DiscordCache, each with an in-memory default; replacing one needs no handler change; defaults bounded by capacity and TTL.
- FR-71 [P0]: Adapter namespacing per App — keys namespaced by App by default (no cross-App reads/overwrites of Refs, sessions, counters, Module state); opt-in sharing per Adapter (e.g. CacheStore).
- FR-39 [P1]: Adapter conformance tests — reusable suite per Adapter trait (TTL, overwrite, delete, missing key, concurrent writers, App namespacing); each in-memory default passes in CI; a deliberately broken implementation fails.
- FR-40 [P1]: Discord cache configuration — choose cached resources, replace storage via DiscordCache, or disable; default caches only what enabled features need; default intents are the minimum required (interaction-only bot: guild events only); a handler/cache resource needing a non-enabled intent fails startup with a typed error; cache reads return owned/shared values, no guard across `.await`; an out-of-process DiscordCache serves the whole Cluster.

**4.7 Localization**

- FR-41 [P1]: Command localization — names word by word, descriptions, option names/descriptions, choices; Publish sends localization maps; a base-language file (`en`) covers its variants unless a variant file exists (variant overrides base); invalid localized names are a typed error naming the Locale.
- FR-42 [P1]: Typed content strings — Locale strings called as typed functions with named/positional arguments; missing default-Locale key or wrong arguments is a compile error; key missing in another Locale falls back with a build warning; every placeholder occurrence replaced.
- FR-43 [P1]: Locale resolution — User and Guild Locale readable in every handler, Middleware can override; interactions: User → Guild → default, guild events: Guild → default; regional codes resolve to base language first; Usage Skill documents User Locale for ephemeral single-user UI, Guild Locale for shared content.
- FR-72 [P1]: Module-scoped Locales — per-Module Locale files merged per language; a key defined by two Modules fails App build naming both; disabling a Module keeps its strings for Modules still referencing them.

**4.8 Modules, Feature Flags and Rate Limits**

- FR-44 [P1]: Module composition — Module tree mounted on the App, unique path-like ids; duplicate id is a typed error; registrations carry the Module id in logs, metrics and errors.
- FR-73 [P1]: Boot-time feature flags — Boot Flags on Modules/registrations selected at startup from config or CLI (`--features a,b` / `all`); unselected registrations are not registered, dispatched or published; dry-run reflects selected flags only.
- FR-45 [P1]: Runtime enable/disable — globally or per guild through ModuleStateStore; disabled handlers don't run; propagation to every process sharing the store within a documented delay; a disabled Module's global Command in a guild returns a localized ephemeral "disabled here" notice; only guild-scoped Commands drop out of a guild's Publish.
- FR-46 [P1]: Rate limits — per user, member, channel, guild or message on a Command or Route, stored in RateLimitStore; "N uses per window"; checked before the handler and any auto-defer, immediate localized ephemeral notice with time remaining; handlers can set/extend limits dynamically; rate-limited response overridable per App, Command or Route.
- FR-47 [P2]: Multiple Apps per process — several Apps with different tokens; same Module tree mountable on several Apps; Commands restrictable to a subset of Apps for Publish; guild allow/deny list per App resolved through an Adapter.

**4.9 Developer Experience and Project Template**

- FR-48 [P0]: Single facade crate with features — bot depends only on `noctis`; capabilities are cargo features; default features build without the image stack, image feature docs state build requirements; each documented feature combination builds in CI.
- FR-49 [P0]: Project Template — `examples/project-template/` builds and runs standalone; demonstrates an option-less entry Command opening a View, a Wizard, a Navbar View with an overflowing list, a Template in a Media Gallery, a custom Adapter and Locale files; CI builds it against the workspace on every change; follows the UI Doctrine (no embed/plain-content primary output, no collectors or inline listeners).
- FR-50 [P0]: Noctis Usage Skill — documents the whole public API (signatures, examples, one Limits entry per V2/modal component, UI Doctrine + UI quality rulebook, when to use Wizard/Navbar/Paginator, patterns/anti-patterns, common mistakes); every public item appears in it; every FR-23/FR-29 component type has an entry (missing entry fails CI); public-API change without skill update fails CI (snapshot check); no example uses a per-message listener.
- FR-51 [P1]: Testing kit — unit-test Commands, Views and handlers without Discord via simulated interactions; invoke a Command or click by Route and assert View, response type and ephemerality offline; View JSON snapshots change only when JSON changes; Templates pixel-snapshot-tested with tolerance.
- FR-74 [P1]: Launch documentation — docs.rs docs with examples for every public item; tescord/dbi → Noctis concept map plus serenity/poise section; Cluster deployment guide; changelog with migration notes; `missing_docs` denied for `noctis` in CI; each guide exists at release and is linked from the crate README.

**4.10 Compile-time Rule Enforcement**

- FR-75 [P0]: Enforcement points — each rule enforced at the earliest point its input is known; every compile-time rule has a trybuild compile-fail test whose error names rule, limit and offending value at the offending span; cross-Module conflicts (duplicate paths, conflicting base metadata, options mixed with subcommands, duplicate Routes, Locale key clashes) fail App build before any Shard connects; a runtime-only check for static input fails the NFR-10 audit.
- FR-76 [P0]: Provable `custom_id` length — Payload derive computes each Route's max encoded length; Refs fixed at 11 chars; every field fixed-width or bounded; > 100 is `compile_error!`; unbounded field is a compile error unless by-ref; trybuild shows 101 failing and 100 compiling.
- FR-77 [P0]: Command declarations checked at compile time — word count, name format, subcommand-and-group word, base mixing options and subcommands, duplicate paths in one invocation are compile errors (`"config (set|get) (prefix|welcome)?"` fails, without `?` compiles); literal descriptions, option names, choice counts and required-before-optional checked at compile time.
- FR-78 [P0]: Static component trees and text checked at compile time — illegal nesting, > 5 buttons per row, Section with < 1 or > 3 Text Displays, over-length literal label/placeholder, static modal rules (FR-29) are compile errors with trybuild tests; the same rules on runtime trees give FR-24 typed errors.
- FR-79 [P0]: Discord Rules Catalog — `docs/discord-rules.md` lists every enforced rule with limit, source and Enforcement Point; every Limit and every rule in FR-7..FR-10, FR-16, FR-23, FR-24, FR-29 has a row; CI fails when a compile-time row lacks a trybuild test or a runtime row lacks a unit test; the Usage Skill links the catalog.

### NonFunctional Requirements

- NFR-1: Memory — framework memory per idle guild within a documented budget measured by a CI benchmark with a stated default cache config (no members, no messages); Discord cache and Renderer cache memory reported separately with configurable byte budgets; initial goal ≤ 1 KiB per idle guild excluding the Discord cache [ASSUMPTION, confirmed by first benchmark].
- NFR-2: No per-message state — no framework feature holds a listener, timer or closure per message or component instance (UD-7).
- NFR-3: Latency — event arrival → handler start p99 ≤ 1 ms at 10k events/s on 4 cores (benchmark event mix defined in AD-25); reference card (≈934×282 PNG, warm caches) p50 ≤ 10 ms, p99 ≤ 50 ms with the pool saturated [targets until the first benchmark].
- NFR-4: Reliability — no panics on any Discord or user input; under `panic = "unwind"` a panic in a handler or Render Job becomes an error response and a logged event, and the Shard keeps running.
- NFR-5: Observability — structured tracing spans for every interaction, event and render; metrics hooks for per-lane queue depth, dropped events, render times, Ref hits/misses and shard status.
- NFR-6: Security — Payloads and Refs untrusted; Refs ≥ 64 bits CSPRNG entropy, namespaced per App; Views ping nobody by default; remote image fetches host-allow-listed; tokens never logged; Adapters receive only serialized bytes and keys, never secrets.
- NFR-7: Toolchain — current edition (2024), MSRV no lower than dependencies require (1.91), dependency versions pinned in the workspace; public types don't force exhaustive matches on upstream enums.
- NFR-8: Quality gates — every workspace crate and the Project Template pass fmt, lints with warnings denied, tests and doc tests in CI; renderer upgrades merge only after the Project Template's pixel snapshots pass.
- NFR-9: Allocation — dispatch path (event → handler start, `custom_id` → Route → decoded inline Payload) has a documented allocation budget measured by a CI benchmark; goal: no heap allocation for Route lookup and fixed-width Payload decode [ASSUMPTION].
- NFR-10: Rule enforcement coverage — every enforced Discord rule has a documented Enforcement Point in the catalog (FR-79), and static input is always checked at compile time.

### Additional Requirements

From the architecture spine (29 ADs), the PRD addendum and repo policy.

**Starter / greenfield setup (Epic 1 Story 1).** No starter template; the architecture
gives a **Structural Seed** instead. The first story creates the Cargo workspace
exactly as seeded: `Cargo.toml` (`[workspace]`, shared `version.workspace = true`,
`[workspace.dependencies]` with the Stack pins, `[workspace.lints]`,
`exclude = ["examples/project-template"]`), `deny.toml`, `release-plz.toml`,
`crates/{noctis, noctis-rules, noctis-codec, noctis-ui, noctis-ports, noctis-gateway,
noctis-core, noctis-macros, noctis-i18n, noctis-image, noctis-prefix, noctis-testkit}`,
`xtask/`, `docs/guides/`, `.github/workflows/{ci.yml, main.yml, release.yml}`.

- **Toolchain and pins (AD-28, Stack):** edition 2024, MSRV 1.91; tokio 1.53;
  twilight-* 0.17.1 (`twilight-http-ratelimiting >= 0.17.1`); takumi `=2.17.0`;
  serde 1.0, postcard 1.1, thiserror 2.0, futures-util 0.3, tracing 0.1.44,
  metrics 0.24, arc-swap 1.9, moka 0.12, rayon 1.12, blake3 1.8, bytes 1.12,
  arrayvec 0.7, getrandom 0.4, regex 1.13, syn 3.0 / quote 1.0, trybuild 1.0,
  insta 1.49, proptest 1.12, criterion 0.8, dhat 0.3, cargo-public-api 0.52 on
  nightly-2026-10-15, cargo-semver-checks 0.51, cargo-deny 0.20, release-plz 0.3.
- **Repo rule:** every cargo build/test/check/clippy/run uses `--release`; no debug
  builds (disk limits). CI follows the same rule.
- **CI gates (AD-28):** fmt; clippy `-D warnings` on all targets; tests and doctests;
  `missing_docs` denied for `noctis`; each facade feature alone and all together;
  `cargo +1.91 check` (MSRV); trybuild UI tests on the 1.91 toolchain only;
  `cargo-semver-checks` on `noctis`; `cargo-deny` (advisories, licenses, bans,
  sources); `xtask deps-check`, `rules-catalog`, `api-snapshot` (nightly),
  `skill-check`; `tw-crosscheck`; the allocation test; `insta` View snapshots;
  Project Template pixel snapshots; template build. `main.yml`: latency/memory
  benches reporting against `budgets.toml`, latest-takumi build. `release.yml`:
  release-plz publishing in layer order, crates.io token only in the protected
  release environment.
- **Crate layering (Invariants graph):** only the drawn edges are allowed
  (`xtask deps-check`); every edge points to a lower layer; no crate depends on
  `noctis` except via dev-dependencies; testkit reachable only via the facade's
  `testkit` feature in `[dev-dependencies]`.
- **AD-1 / AD-14 ports:** every storage concern is a `noctis-ports` trait,
  `Send + Sync + 'static`, dyn-compatible, `BoxFuture<'_, Result<T, PortError>>`,
  `Arc<dyn Port>`; op vocabulary `get`, `put(ttl)`, `put_if_absent`, `take`,
  `delete`, `touch`, `incr_with_ttl`, `version()`; `IdentifyQueue::acquire(bucket) -> Lease`;
  v1 ports: RefStore, SessionStore, IdentifyQueue, ModuleStateStore, RateLimitStore,
  CacheStore (Discord cache backend), RenderCache, AssetStore, LocaleSource,
  GuildAccessList, plus the `Renderer` driven port; bounded memory defaults in
  `noctis_ports::memory` (moka, byte weighers); `Namespaced<P>` (`<app>:<port>:`) and
  `Shared<P>` (`_shared:<port>:`) wrappers only in core; postcard + schema byte for
  typed values.
- **AD-2 facade:** bot code depends only on `noctis`; `noctis::twilight` re-export;
  `noctis::image::unstable` outside semver; macro output uses
  `::noctis::__private::<crate>` paths; sub-crate docs say "internal; use `noctis`".
- **AD-3 / AD-4 rules:** one sync, I/O-free `noctis-rules` crate with stable rule ids,
  `Limits` (Strict/Lenient), `discord_len`; macros call it directly; compile-time →
  App-build → runtime ladder; every test names its rule id; `xtask rules-catalog`
  fails CI when a Compile-time or App-build row has no citing test.
- **AD-5 / AD-6 / AD-18 / AD-19 / AD-20 codec:** normative `custom_id` layout
  (`F route ":" [S] [E] [N] body [T]`), `pub const` layout constants checked against
  the catalog, total decoding, `Field::MAX_LEN` widths, `#[derive(Route)]` with
  `assert_fits`, `EncodedId = Inline | RefSlot(PendingRef)`, two-phase Ref minting by
  the Responder, `R: Handled` component constructors, View envelope `E` for widget
  actions, integrity tag keyed by `NOCTIS_PAYLOAD_KEY` (32 bytes, rotation with one
  previous key, never logged).
- **AD-7 Registry:** Modules produce descriptors; Boot Flags and per-App filters apply
  before the fold; `App::build` folds into one immutable Registry with every App-build
  check (returns all violations); O(1) dispatch maps; Publish JSON only from the
  Registry; Module enablement is an `arc-swap` bitset snapshot.
- **AD-9 stateless handlers**, `State<T>`, `Ext<T>`, no globals.
- **AD-10 lanes:** `Inbound { shard, received_at, kind }`, shard tasks only `try_send`,
  interaction lane priority, bounded event lane with droppable/overflow policy,
  semaphore-bounded worker set in core, guild-keyed ordered sub-queues.
- **AD-11 Responder:** per-kind ctx types, `Responder<Pending>` → `Responder<Acked>`
  typestate, atomic runtime state machine shared with auto-defer, follow-up counting,
  `TokenExpired`, typed flag sets, webhook serializer with `with_components=true`.
- **AD-12 V2 model:** Noctis's own component types serialized directly (raw
  twilight-http body); twilight-model only for ids, interactions, events, commands;
  modal submissions decoded from `RawJson`; unknown data kept as `serde_json::Value`.
- **AD-13 errors:** per-crate `thiserror` enums, `noctis::Error` `#[non_exhaustive]`,
  `HandlerError` the only type-erased public error, `catch_unwind` around every
  handler/hook/event future and Render Job, no `unwrap`/`expect` on external input.
- **AD-15 rendering:** takumi `=2.17.0` on a process-scoped rayon pool, bounded queue,
  frame-weighted admission, per-job caps; `noctis_image::tw` grammar generated by
  `xtask tw-grammar`, takumi-free; `tw-crosscheck` CI; blake3 cache key; deterministic
  Template data (no `HashMap`); `AssetFetcher` the only non-Discord HTTP.
- **AD-16 i18n:** `i18n!("locales")`, `Locale` (32 codes) / `MessageKey` / `Translate`
  in `noctis-rules`, reserved `noctis.*` keys, fixed `cmd.*` / `cmdword.*` keys,
  overlays content-only.
- **AD-17 template tracking:** `xtask api-snapshot` (cargo public-api on the pinned
  nightly) and `xtask skill-check` (every public item mentioned in
  `examples/project-template/.claude/skills/noctis/`, recorded API hash updated);
  CI builds the template against the workspace. Repo policy: every public API change
  updates the template code, its `AGENTS.md` and the Noctis Usage Skill in the same
  commit.
- **AD-21 gateway lifecycle:** runtime and crypto-provider checks, `GET /gateway/bot`,
  `IdentifyQueue` buckets, `session_start_limit` check, own parsing via
  `twilight_gateway::parse`, `ShardEvent` channel, SessionStore written only by the
  gateway, core-owned shutdown order.
- **AD-22 attachments:** content-addressed names, `(MessageTree, AttachmentSet)`,
  Upload Budget and edit diff only in the Responder, `ctx.render(...).await`.
- **AD-23 pipeline:** fixed 10-stage interaction pipeline; Extensions type map with
  App-build check of `Ext<T>` providers.
- **AD-24 resource scoping:** one `Arc<twilight_http::Client>` per App (proxy mode,
  timeout 10 s default); process-scoped `noctis::Runtime` (render pool, takumi
  caches, fonts, emoji font, metrics recorder) frozen by the first App.
- **AD-25 allocation:** zero-allocation Route lookup and inline decode (dhat test),
  bounded caches, fixed-size per-guild state, benches in `noctis-core/benches` with
  `budgets.toml`, testkit `assert_memory_flat`.
- **AD-26 authoring surface:** `#[command]`, `#[user_command]`, `#[message_command]`,
  `Module::new("a/b")`, Route-value component construction, tescord naming.
- **AD-27 UI Doctrine in `noctis-ui`** (see UX-DRs) and `xtask skill-check` doctrine
  lint of the Project Template.
- **AD-28 release:** semver, release-plz, changelog with migration notes,
  `#[non_exhaustive]` upstream wrappers, MSRV policy, takumi bumps as their own PR.
- **AD-29 observability:** span names `noctis.shard|interaction|event|render|publish`,
  fields, `noctis_*` metric names with `app` label, no ids/free text in labels,
  redacted token and key newtypes, Error Sinks receive every error/panic/drop.
- **Conventions:** crate/feature naming (`image`, `prefix`, `i18n`, `testkit`,
  `image-gif`, `image-emoji`, `default-font`), Route/Module/App id formats, compile
  error format `[<RULE-ID>] …` with note/help lines, `BuildError` lists all violations,
  `App::from_env()` with `NOCTIS_*`, additive features, trybuild fixtures at
  `tests/ui/<area>/<RULE-ID>_<slug>.rs` and `tests/ui-pass/`, `insta` View snapshots.
- **Deferred decisions to settle in stories:** HTTP client crate for `AssetFetcher`
  (image epic); PRD open questions Q1–Q6 keep their conservative defaults
  (configurable Limits, WebP default, benches report until confirmed, legacy → V2 on
  modal submit is a typed error, ModuleStateStore poll 5 s).
- **Project Template (FR-49, AD-17):** `examples/project-template/` already exists as
  a scaffold written against the planned API (modules `setup`, `dashboard`,
  `profile`, `adapters/bounded_cache.rs`, `strings.rs`, tests `flows.rs`,
  `adapters.rs`, `templates.rs`, `noctis-theme.css`, `locales/{en,tr}.yaml`, the
  Noctis Usage Skill at `.claude/skills/noctis/` with `references/`). Stories
  reconcile it with the real API as each API lands, and it must build in CI once the
  features it uses exist.

### UX Design Requirements

No UX design contract exists. The PRD's UI Doctrine (§4.0) is normative for every
built-in widget, the Project Template and the Usage Skill, and is treated as the UX
design requirements.

- UX-DR1 (UD-1): Components V2 for anything interactive — built-in widgets produce only V2 messages; plain `content` and embeds exist only behind `LegacyMessage`.
- UX-DR2 (UD-2): Images for read-only visuals — rendered images enter Views only as Thumbnail or Media Gallery attachments with alt text and never carry actionable information.
- UX-DR3 (UD-3): Commands are entry points — option-less View-returning Commands need no response code; configuration happens inside Views.
- UX-DR4 (UD-4): Steps for setup flows — Wizard progress reads "Step n of m: Title", never page numbers.
- UX-DR5 (UD-5): Navbar for general views that don't fit — top Action Row of View-Route buttons, active tab disabled, over-capacity is a typed error.
- UX-DR6 (UD-6): Pagination on overflow — homogeneous lists paginate automatically, inside the active Navbar tab when present, Chrome cost reserved.
- UX-DR7 (UD-7): No listener per message or component — one handler per Route; no collector, await-component loop or inline per-message closure anywhere (framework, template, skill, docs).
- UX-DR8 (UD-8): Allocation-conscious by design — no per-message state, bounded caches, no avoidable heap allocation on the dispatch path.
- UX-DR9 (FR-50 UI quality rulebook): one primary action per View; visible active state; confirmation for destructive actions; ephemeral for single-user UI, public for shared content; a way back or cancel on every interactive View; designed empty and error states — taught by the Usage Skill and followed by the Project Template.

### FR Coverage Map

Primary epic first; "+" lists epics that extend or complete the FR. Every FR and NFR
is mapped.

| Req | Tier | Epic(s) | What is delivered there |
| --- | --- | --- | --- |
| FR-1 | P0 | E5 | App builder, startup safety, fatal-close reporting, graceful shutdown |
| FR-2 | P0 | E5 | Recommended/fixed shard count, bucketed parallel identify, multiple warning |
| FR-3 | P1 | E12 | Shard Ranges, Cluster-wide IdentifyQueue, single publisher process |
| FR-4 | P1 | E12 | Session resume through SessionStore, fallback to identify |
| FR-5 | P0 | E5 | Interaction Lane, bounded event lane, drop/overflow policy, lane metrics, per-guild ordering |
| FR-6 | P0 | E5 | `#[event]` handlers, derived event filtering and intents, ordered handlers |
| FR-7 | P0 | E6 | Command Paths validated at compile time |
| FR-8 | P0 | E6 | Command Pattern expansion, cap, typed variant enum |
| FR-9 | P0 | E6 | Merge by base word, metadata and options/subcommand conflicts at App build |
| FR-10 | P0 | E6 | `derive(Options)`/`derive(Choices)`, validators, autocomplete |
| FR-11 | P0 | E6 | Command metadata in Publish JSON, guild scope |
| FR-12 | P0 | E6 | User and message context-menu Commands (+E11 whole-name localization) |
| FR-13 | P0 | E6 | Publish diff, dry run, CLI modes, designated publisher (+E12 Cluster publisher, +E15 per App) |
| FR-14 | P2 | E15 | Prefix commands behind the `prefix` feature |
| FR-15 | P0 | E7 | One handler per Route, O(1) dispatch |
| FR-16 | P0 | E2 | Field encodings, escaping, round-trip property tests (+E7 runtime use) |
| FR-17 | P0 | E2 + E7 | Ref format and `EncodedId` (E2); RefStore minting, TTL, one-shot, overflow policy (E7) |
| FR-18 | P0 | E7 | Expired Handler, defaults and overrides |
| FR-19 | P0 | E2 + E7 | Total decoding and integrity tag (E2); pipeline handling and Route Guards (E7) |
| FR-20 | P0 | E7 | Middleware and typed Extensions |
| FR-21 | P0 | E6 + E7 | Responder typestate and auto-defer for Commands (E6); components and modals (E7) |
| FR-22 | P0 | E5 + E6 | Error Sinks by origin (E5); user-facing localized errors and panics as responses (E6) |
| FR-23 | P0 | E4 | Typed V2 builders, flags, legacy path |
| FR-24 | P0 | E4 | Runtime Limit validator, strict/lenient, configurable `Limits` |
| FR-25 | P0 | E8 | View trait, determinism, select freshness, mentions, state version, cost |
| FR-26 | P0 | E8 | Navbar |
| FR-27 | P0 | E8 | Wizard |
| FR-28 | P0 | E8 | Paginator |
| FR-29 | P0 | E4 + E7 | Modal model and `derive(Modal)` (E4); modal Routes and typed submission decoding (E7) |
| FR-30 | P1 | E9 | Attachment tracking, edit diff, Upload Budget, alt text |
| FR-31 | P1 | E9 | Raw component JSON, unknown received components, raw REST |
| FR-32 | P1 | E10 | Templates, compile-time class validation, Theme |
| FR-33 | P1 | E10 | Output formats, size budget and degradation |
| FR-34 | P1 | E10 | Bounded render pool |
| FR-35 | P1 | E10 | Fonts, fallback fonts, static assets |
| FR-36 | P1 | E10 | RenderCache key and single-flight deduplication |
| FR-37 | P2 | E15 | Renderer seam proven with a test Renderer (port defined in E3) |
| FR-38 | P0 | E3 | Port traits with bounded in-memory defaults (+E12 DiscordCache configuration) |
| FR-39 | P1 | E14 | Conformance suites in the testkit |
| FR-40 | P1 | E12 | Discord cache configuration and minimal intents |
| FR-41 | P1 | E11 | Command localization in Publish |
| FR-42 | P1 | E11 | `i18n!` typed content strings |
| FR-43 | P1 | E11 | User/Guild Locale resolution |
| FR-44 | P1 | E13 | Module tree, unique ids, ids in logs/metrics/errors (basic `Module` from E5) |
| FR-45 | P1 | E13 | Runtime enable/disable through ModuleStateStore |
| FR-46 | P1 | E13 | Rate limits through RateLimitStore |
| FR-47 | P2 | E15 | Multiple Apps per process, GuildAccessList |
| FR-48 | P0 | E1 | Facade crate, features, feature-matrix CI (+E14 final matrix) |
| FR-49 | P0 | E1 + all + E14 | CI template job (E1); each API story reconciles the template; template green and gate required (E14) |
| FR-50 | P0 | E1 + all + E14 | `api-snapshot`/`skill-check` gates (E1); each API story updates the Usage Skill; completeness audit (E14) |
| FR-51 | P1 | E8 + E10 | Testkit simulated interactions and View snapshots (E8, per roadmap M2); pixel snapshots (E10) |
| FR-52 | P1 | E12 | HTTP proxy mode, Cluster warning, timeout |
| FR-53 | P2 | E15 | Presence and gateway commands |
| FR-54 | P1 | E13 | Lifecycle Hooks API (shard status events produced in E5) |
| FR-55 | P1 | E12 | Resumable shutdown, `--no-resume` |
| FR-56 | P0 | E6 | Unique Command Paths and context-menu names |
| FR-57 | P0 | E6 | Explicit base/group metadata, deterministic fallback |
| FR-58 | P0 | E4 + E7 | `R: Handled` component constructors (E4); handler macro and App-build check (E7) |
| FR-59 | P1 | E9 | Route component defaults and per-render overrides |
| FR-60 | P0 | E2 + E7 | Schema version and Stale decoding (E2); stale routing (E7) |
| FR-61 | P0 | E8 | View responses without response code |
| FR-62 | P0 | E8 | Default paginate Overflow Policy, Chrome reservation |
| FR-63 | P1 | E9 | Modal pre-fill |
| FR-64 | P1 | E7 | Radio/Checkbox decoding from raw JSON (built into the modal decoder per AD-12) |
| FR-65 | P1 | E9 | Views over webhooks |
| FR-66 | P1 | E10 | Tailwind v4 grammar, v3 suggestions, runtime class warnings |
| FR-67 | P1 | E10 | AssetFetcher: allow-list, caps, avatar helpers |
| FR-68 | P1 | E10 | Emoji font and custom emoji images |
| FR-69 | P1 | E10 | Animated Templates |
| FR-70 | P1 | E10 | Per-job caps and admission weights |
| FR-71 | P0 | E7 | `Namespaced<P>` and `Shared<P>` wrappers in core (port keys opaque since E3) |
| FR-72 | P1 | E11 | Module-scoped Locale catalogs |
| FR-73 | P1 | E13 | Boot Flags |
| FR-74 | P1 | E14 | Launch documentation |
| FR-75 | P0 | E1 + all | trybuild harness and catalog check (E1); each rule story adds its fixtures |
| FR-76 | P0 | E2 | `derive(Route)` compile-time length proof |
| FR-77 | P0 | E6 | `#[command]` compile-time validation |
| FR-78 | P0 | E4 | Static component tree and literal checks |
| FR-79 | P0 | E1 | Rules catalog as code and CI cross-check (+E14 pending list empty) |
| NFR-1 | — | E3 + E12 + E14 | Bounded defaults (E3); memory bench over 10k idle guilds (E12); gating in CI (E14) |
| NFR-2 | — | E7 + E8 | No per-message state by construction (E7); `assert_memory_flat` (E8) |
| NFR-3 | — | E14 (+E10) | Latency bench (E14); reference card bench (E10) |
| NFR-4 | — | E5 (+E6, E10) | `catch_unwind` around every future (E5); panic → error response (E6); render jobs (E10) |
| NFR-5 | — | E5 (+all) | Span and metric contract (E5); each runtime story emits its spans/metrics |
| NFR-6 | — | E2 + E3 + E7 + E10 | Ref entropy, key newtypes (E2); bytes-only ports (E3); namespacing, mentions (E7, E8); host allow-list (E10) |
| NFR-7 | — | E1 | Edition, MSRV, workspace pins, `#[non_exhaustive]` policy |
| NFR-8 | — | E1 (+E10) | CI quality gates (E1); pixel-snapshot gate for renderer upgrades (E10) |
| NFR-9 | — | E2 + E7 + E14 | Zero-alloc decode (E2); zero-alloc Route lookup (E7); allocation bench budget (E14) |
| NFR-10 | — | E1 + E14 | `xtask rules-catalog` (E1); empty pending list audit (E14) |
| UX-DR1..9 | — | E4 + E8 + E14 | V2-only widgets (E4, E8); doctrine lint and rulebook in the skill (E1 gate, E8, E14) |

## Epic List

Fifteen epics, ordered by priority tier (P0 → P1 → P2) and, inside a tier, by the
crate layers (rules → codec → ports/ui → gateway → core → macro-heavy features →
image → i18n → scale). Each epic builds only on earlier epics. The README roadmap
milestone is given per epic.

**Epic order decision (autonomous):** the Components V2 model (E4, roadmap M2) is
pulled ahead of the gateway and core epics because `noctis-core` depends on
`noctis-ui` (layer 2) and the Responder must send V2 messages; Views and widgets stay
in M2 (E8). The P1 UI completions (E9) sit at the head of the P1 block because they
extend the same `noctis-ui`/Responder files right after E8. The testing kit (FR-51,
P1) lands in E8 as the roadmap places it in Milestone 2.

### Epic 1: Project foundation and Discord rules as code (P0, roadmap M1)
Contributors get a green, gated Cargo workspace laid out as the structural seed, and
bot authors get every Discord limit and rule as one tested crate (`noctis-rules`) whose
catalog CI keeps honest. Usage Skill and Project Template drift is caught from day one.
**FRs covered:** FR-48, FR-49 (CI job), FR-50 (gates), FR-75 (harness), FR-79; NFR-7, NFR-8, NFR-10.

### Epic 2: Typed, length-proven `custom_id`s (P0, roadmap M1)
Bot authors declare Routes and Payloads whose encoded `custom_id` is proven ≤ 100
characters at compile time, with Refs, schema versions and integrity tags in one codec.
**FRs covered:** FR-16, FR-17 (format), FR-19 (decoding, tag), FR-60 (versioning), FR-76; NFR-6, NFR-9.

### Epic 3: Pluggable storage ports (P0, roadmap M1)
Bot authors get every storage concern as a dyn-compatible port with a bounded
in-memory default, and can hand their own implementation to the App.
**FRs covered:** FR-38; NFR-1 (bounded defaults), NFR-6.

### Epic 4: Components V2 and modal model with limit validation (P0, roadmap M2)
Bot authors build any Components V2 message or modal through typed builders where
illegal structures don't compile and every Limit is checked before sending.
**FRs covered:** FR-23, FR-24, FR-29 (model and derive), FR-58 (constructor bound), FR-78; UX-DR1.

### Epic 5: A running sharded bot (P0, roadmap M1)
A bot author builds an App, runs it sharded across all cores with correct identify
concurrency, and handles gateway events without ever stalling heartbeats or dropping
interactions.
**FRs covered:** FR-1, FR-2, FR-5, FR-6, FR-22 (Error Sinks); NFR-4, NFR-5.

### Epic 6: Slash commands and Publish (P0, roadmap M1)
A bot author declares tescord-style Commands, Patterns, typed options, autocomplete and
context menus, gets compile-time and App-build rule checks, responds safely through the
Responder, and publishes only when something changed.
**FRs covered:** FR-7, FR-8, FR-9, FR-10, FR-11, FR-12, FR-13, FR-21 (Commands), FR-22 (user errors), FR-56, FR-57, FR-77.

### Epic 7: Routed components, Refs and modals (P0, roadmap M1)
A bot author handles every button, select and modal through one stateless handler per
Route, with Refs for large state, expiry handling, guards, middleware and auto-defer.
**FRs covered:** FR-15, FR-17 (store), FR-18, FR-19 (pipeline, guards), FR-20, FR-21 (components, modals), FR-29 (routing, submissions), FR-58 (handler side), FR-60 (routing), FR-64, FR-71; NFR-2, NFR-9.

### Epic 8: Views, navigation widgets and the testing kit (P0 + FR-51, roadmap M2)
A bot author returns Views from any handler and gets Navbar tabs, Wizards and
automatic pagination with no state storage, and can test flows without Discord.
**FRs covered:** FR-25, FR-26, FR-27, FR-28, FR-51 (simulated interactions, View snapshots), FR-61, FR-62; NFR-2; UX-DR1..UX-DR9.

### Epic 9: Attachments, webhooks and advanced component surfaces (P1, roadmap M2 completion)
A bot author attaches files to Views, sends Views over webhooks, pre-fills modals,
declares Route component defaults and falls back to raw JSON where twilight lags.
**FRs covered:** FR-30, FR-31, FR-59, FR-63, FR-65.

### Epic 10: Generated images and GIFs (P1, roadmap M3)
A bot author writes Tailwind-styled Templates in Rust, validated at compile time, and
gets cached PNG/WebP/GIF/APNG output from a bounded pool, embedded in Views.
**FRs covered:** FR-32, FR-33, FR-34, FR-35, FR-36, FR-51 (pixel snapshots), FR-66, FR-67, FR-68, FR-69, FR-70; NFR-3 (card bench), NFR-8 (pixel gate).

### Epic 11: Localization (P1, roadmap M4)
A bot author ships Locale files compiled into typed strings, localizes Commands word
by word, and every handler receives the right User or Guild Locale.
**FRs covered:** FR-41, FR-42, FR-43, FR-72.

### Epic 12: Clusters and scale (P1, roadmap M4)
A bot author scales from one process to a Cluster: Shard Ranges, shared identify,
session resume across restarts, HTTP proxy mode and a configurable Discord cache.
**FRs covered:** FR-3, FR-4, FR-40, FR-52, FR-55; NFR-1 (memory bench).

### Epic 13: Module operations (P1, roadmap M4)
A bot author composes Module trees, selects features at boot, enables or disables
Modules at runtime per guild, rate-limits Commands and Routes, and observes lifecycle
events.
**FRs covered:** FR-44, FR-45, FR-46, FR-54, FR-73.

### Epic 14: v1 launch readiness (P1, roadmap M4 → v0.1.0)
Third-party adapters can prove correctness, benchmarks guard memory, latency and
allocation, the Project Template and Usage Skill are complete and gated, the launch
docs exist, and `noctis` 0.1.0 is published.
**FRs covered:** FR-39, FR-48 (final matrix), FR-49 (completion), FR-50 (completion), FR-74, FR-79 (completion); NFR-1, NFR-3, NFR-9, NFR-10.

### Epic 15: Opt-in extras (P2, roadmap "Opt-in extras")
A bot author can opt into prefix commands, a remote Renderer, several Apps per process
and presence/gateway commands, each behind a feature or as an extension.
**FRs covered:** FR-14, FR-37, FR-47, FR-53.

## Epic 1: Project foundation and Discord rules as code

Contributors get a green, gated Cargo workspace laid out as the architecture's
structural seed, and bot authors get every Discord limit and rule as one tested crate
(`noctis-rules`) whose catalog CI keeps honest. Usage Skill and Project Template drift
is caught from the first public item onwards.

### Story 1.1: Workspace skeleton and baseline CI

As a Noctis contributor,
I want the Cargo workspace, crate skeletons and baseline CI laid out exactly as the structural seed,
So that every later story lands in the right crate behind the same quality gates.

**Implements:** FR-48, FR-49 (CI job), NFR-7, NFR-8; AD-2, AD-28, Stack, Structural Seed.

**Acceptance Criteria:**

**Given** an empty repository apart from docs, planning artifacts and `examples/project-template/`
**When** the story is done
**Then** the root `Cargo.toml` declares a `[workspace]` with members `crates/{noctis, noctis-rules, noctis-codec, noctis-ui, noctis-ports, noctis-gateway, noctis-core, noctis-macros, noctis-i18n, noctis-image, noctis-prefix, noctis-testkit}` and `xtask`, `exclude = ["examples/project-template"]`, `version.workspace = true` in every crate, `edition = "2024"`, `rust-version = "1.91"`, `[workspace.dependencies]` pinned to the Stack table and shared `[workspace.lints]`
**And** each sub-crate is a compiling `lib` whose crate docs say "internal; use `noctis`", `noctis-macros` is a `proc-macro` crate, and `deny.toml`, `release-plz.toml` and `docs/guides/` exist

**Given** the facade crate `noctis`
**When** its `Cargo.toml` is inspected
**Then** it exposes the additive features `image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `prefix` and `testkit` (initially empty), default features build without any image dependency, and `#![deny(missing_docs)]` is set

**Given** a pull request
**When** `.github/workflows/ci.yml` runs
**Then** it executes `cargo fmt --check`, `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`, `cargo test --release --workspace` (tests and doctests), `cargo +1.91 check --release --workspace` (MSRV), `cargo deny check` (advisories, licenses, bans, sources), and a build of each facade feature alone plus all features together, every cargo invocation using `--release`
**And** a `template-build` job runs `cargo build --release` in `examples/project-template/` in tracking mode (non-blocking, result reported) as decided in Story conventions
**And** `main.yml` and `release.yml` exist as stubs referencing the jobs later epics fill in

**Given** the root `AGENTS.md`
**When** the workspace exists
**Then** its TODO line about missing `Cargo.toml` is replaced by the real verification commands (all `--release`)

### Story 1.2: Crate layer enforcement with `xtask deps-check`

As a Noctis contributor,
I want CI to reject any workspace dependency edge the architecture does not allow,
So that the hexagonal layering cannot erode as crates grow.

**Implements:** AD-1, AD-2, Invariants graph, AD-28.

**Acceptance Criteria:**

**Given** the allowed-edge list from the spine's Invariants graph encoded in `xtask`
**When** `cargo run --release -p xtask -- deps-check` runs on the workspace
**Then** it passes when only drawn edges exist (the facade may depend on any crate)

**Given** a crate adds a forbidden edge (for example `noctis-ui → noctis-core`, or any crate → `noctis` outside `[dev-dependencies]`)
**When** `deps-check` runs
**Then** it fails naming the offending edge and the rule it breaks
**And** `noctis-testkit` reachable from the facade other than through the `testkit` feature also fails

**Given** CI
**When** a pull request runs
**Then** `deps-check` is a required job

### Story 1.3: `noctis-rules` foundations — Limits, length counting, rule ids and locales

As a bot author,
I want one source of truth for Discord limits, text length and locale codes,
So that macros, validators and packers can never disagree about a limit.

**Implements:** FR-24 (configurable Limits), FR-79, NFR-10; AD-3, AD-16 (types); rules LOC-01, CV2-04 (counting unit).

**Acceptance Criteria:**

**Given** `noctis-rules`
**When** it is built
**Then** it is sync, has no I/O and no async, and depends only on small pure crates (`regex` with Unicode tables)
**And** it exposes a `RuleId` type whose values match the catalog ids (`<AREA>-<nn>`) and a violation type carrying rule id, path, limit and actual value

**Given** the `Limits` struct
**When** `Limits::default()` is created
**Then** it holds every numeric limit from the catalog with Discord's defaults and `mode: Strict`
**And** `Lenient` mode is documented to truncate text with `…` and never applies to CV2-03, CV2-05, CV2-06, CID-01 or CID-02 (unit tests cite those ids)

**Given** user text containing astral-plane characters
**When** `noctis_rules::len::discord_len` measures it
**Then** it returns the larger of Unicode scalar count and UTF-16 unit count (unit and property tests)

**Given** the 32 Discord locale codes
**When** `Locale` is used
**Then** it is a closed enum with exactly those codes (LOC-01), parses from and prints to the Discord code, maps regional codes to their base language, and `MessageKey` and `trait Translate` live in this crate

**Given** public items were added to the facade re-exports (`noctis::rules::Limits`, `Locale`)
**When** the story is done
**Then** the Usage Skill's limits reference documents `Limits`, strict/lenient mode and `Locale`, and the template code referring to them is reconciled

### Story 1.4: Command naming and tree rules

As a bot author,
I want command names, patterns and command trees validated by shared rule functions,
So that the same checks run in the `#[command]` macro and at App build.

**Implements:** FR-7, FR-8, FR-9 (rule functions), FR-77 (shared logic); AD-3, AD-8; rules CMD-01, CMD-02, CMD-03, CMD-04, CMD-05, CMD-10, CMD-11, CMD-12, CMD-13, CMD-14, CMD-15, REG-01, REG-03, REG-04.

**Acceptance Criteria:**

**Given** `noctis_rules::command::expand`
**When** it expands `"config (set|get) (prefix|welcome)"`
**Then** it yields `config set prefix`, `config set welcome`, `config get prefix`, `config get welcome` in that order (left to right, declaration order)
**And** a `?` group also yields the path without it, whitespace collapses, duplicate expansions fail with CMD-15, and more than the cap (default 25, configurable) fails with CMD-15

**Given** `noctis_rules::command::validate_tree`
**When** it receives the expansions of `"config (set|get) (prefix|welcome)?"`
**Then** it reports CMD-11 naming `set` as both subcommand and group of `/config`, with the two contributing paths
**And** it reports CMD-01/02/03 for bad words (including ASCII `'` rejected, `ʼ` accepted, uppercase with a lowercase suggestion), CMD-10 for more than 3 words, CMD-12 for an invokable base with children, CMD-13 for more than 25 children and CMD-05 for the 8000 budget using the longest variant per field
**And** sibling groups and subcommands under one base pass (CMD-14)

**Given** a set of trees from several sources
**When** the merge validator runs
**Then** it returns every violation at once (REG-01 counts, REG-03 duplicate context-menu names, REG-04 merge conflicts), each naming the rule id
**And** every unit test name or message cites its rule id

### Story 1.5: Option, context-menu and command metadata rules

As a bot author,
I want option, choice, context-menu and metadata constraints available as shared rule functions,
So that option derives and App build reject illegal declarations identically.

**Implements:** FR-10, FR-11, FR-12 (rule functions); AD-3; rules OPT-01..OPT-14, CTX-01, CTX-02, CTX-03, META-01, META-02, META-03, META-05.

**Acceptance Criteria:**

**Given** an option list descriptor
**When** the option validator runs
**Then** it rejects a required option after an optional one (OPT-01), duplicate names (CMD-06), out-of-range integer/number bounds or `min > max` (OPT-03..05), invalid string length bounds (OPT-06), `channel_types` on non-channel options (OPT-07), more than 25 choices (OPT-08), choice name/value over 100 (OPT-09), choices with autocomplete (OPT-10), autocomplete on unsupported types (OPT-11) and invalid `file_types` (OPT-13)
**And** each rejection names its rule id and the offending option

**Given** a context-menu name
**When** it is validated
**Then** only the 1–32 length applies (CTX-01), mixed case and spaces are accepted, and descriptions/options are not representable (CTX-02)

**Given** command metadata
**When** two declarations target the same base command
**Then** differing `default_member_permissions`, `contexts`, `integration_types` or `nsfw` are a conflict (META-01, META-02), and `contexts`/`integration_types` on a guild-scoped command are rejected (META-03)

### Story 1.6: Component, modal, message and attachment rules

As a bot author,
I want every Components V2, modal, message, webhook and attachment limit available as shared rule functions,
So that compile-time checks and the runtime validator in later epics use the same code.

**Implements:** FR-23, FR-24, FR-29 (rule functions), FR-79; AD-3; rules CV2-03..CV2-16, MOD-01..MOD-07, MSG-01..MSG-06, ATT-01..ATT-06, WH-04, INT-07.

**Acceptance Criteria:**

**Given** the rule functions in `noctis_rules::component`, `::modal`, `::message` and `::attachment`
**When** they are called with boundary values
**Then** each accepts the limit exactly and rejects limit + 1 for: 40 components (CV2-03), 4000 text characters via `discord_len` (CV2-04), 1–5 buttons or exactly one select per row (CV2-06), 1–3 Text Displays plus one accessory per Section (CV2-07), button label 80 and link `url` 512 and premium rules (CV2-08), select placeholder 150 / options 25 / min-max / `default_values` (CV2-09), gallery 1–10 items and descriptions 1024 (CV2-10, CV2-11), separator spacing and accent color range (CV2-13), legacy 5 rows (CV2-15)
**And** modal rules MOD-01..MOD-07, message rules MSG-01..MSG-06, attachment rules ATT-01..ATT-06 (20 MiB default budget, 25 MiB request, 10 files configurable), webhook name WH-04 and autocomplete choice cap INT-07 are covered the same way

**Given** a rule function that can be evaluated at compile time (fixed lengths, counts, arity)
**When** it is used from a `const` context
**Then** it is a `const fn` so later builders can assert literals with `const { … }`

**Given** a lowered runtime `Limits` override
**When** a rule function receives `&Limits`
**Then** it checks against the override, and no function reads `Limits::default()` internally (AD-3)

### Story 1.7: trybuild harness and rules-catalog cross-check

As a Noctis maintainer,
I want CI to prove every catalog rule has a test at its enforcement point,
So that the catalog and the code can never silently diverge.

**Implements:** FR-75, FR-79, NFR-10; AD-3, AD-4, AD-5 (constants quote), Conventions (Tests).

**Acceptance Criteria:**

**Given** the workspace
**When** the trybuild harness is added
**Then** a single `compile_fail("tests/ui/**/*.rs")` and `pass("tests/ui-pass/**/*.rs")` pair exists in the facade's test suite, runs on the 1.91 toolchain only in CI, and requires each `.stderr` to contain the rule id of its file name

**Given** `docs/discord-rules.md`
**When** `cargo run --release -p xtask -- rules-catalog` runs
**Then** it parses every rule row and its Enforcement column, and fails when a Compile-time row has no trybuild fixture citing its id, or an App-build or Runtime row has no unit test citing its id
**And** rows whose enforcement lands in a later epic are listed in `xtask/rules-pending.toml` with the story id that will add the test; a pending entry that already has a test also fails (the list only shrinks)

**Given** the `custom_id` layout constants table in the catalog
**When** `rules-catalog` runs after Epic 2 adds the constants
**Then** it asserts the catalog values equal the `pub const`s in `noctis-codec` (until then the check is listed as pending for Story 2.2)

**Given** CI
**When** a pull request runs
**Then** `rules-catalog` is a required job

### Story 1.8: Public API snapshot and Usage Skill gate

As a coding agent writing a bot from the Project Template,
I want CI to fail whenever the public API changes without the Usage Skill changing,
So that the skill always describes the real API and I never need Noctis source.

**Implements:** FR-49, FR-50, UX-DR1..UX-DR9 (doctrine lint), NFR-8; AD-17, AD-27, AD-28.

**Acceptance Criteria:**

**Given** the facade
**When** `cargo run --release -p xtask -- api-snapshot` runs
**Then** it records the public API with `cargo public-api` 0.52 on `nightly-2026-10-15` (the only nightly use in CI) into a committed snapshot file, and fails if the committed snapshot is stale

**Given** the snapshot and `examples/project-template/.claude/skills/noctis/`
**When** `cargo run --release -p xtask -- skill-check` runs
**Then** it fails if any public item in the snapshot is not mentioned in the skill, or if the snapshot changed while the skill's recorded API hash (the `API hash:` line in `SKILL.md`) did not
**And** it lints the Project Template for UI Doctrine violations: embeds or `content` as primary output of an interactive handler, manual response-type selection, and collector/await-component/inline-listener patterns (UD-1, UD-3, UD-7)

**Given** the skill's component entries
**When** `skill-check` runs after Epic 4
**Then** every FR-23/FR-29 component builder without a skill entry fails CI (registered as pending until Story 4.1 exists)

**Given** CI
**When** a pull request runs
**Then** `api-snapshot` and `skill-check` are required jobs, and the skill's `API hash` line is updated from `pending` to the current hash in this story
