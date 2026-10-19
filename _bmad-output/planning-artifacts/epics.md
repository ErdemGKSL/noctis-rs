---
status: final
stepsCompleted: [1, 2, 3, 4]
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
- **Step 3, story review and final menu:** each epic's stories were accepted as
  written (96 stories in 15 epics) and [C] Continue was chosen (no Advanced
  Elicitation or Party Mode). Story-level decisions: the Project Template build job
  runs in tracking mode until Story 14.2 (see Story conventions); `xtask/rules-pending.toml`
  lets the rules-catalog gate be required from Story 1.7 while rule tests land epic by
  epic, and must be empty by Story 14.4; the HTTP client for `AssetFetcher` (deferred
  by the architecture) is chosen in Story 10.8.
- **Step 4, final validation:** all checks passed after three fixes made during
  validation (rule ids CMD-07, RL-04 and REG-06 were not cited by any story; the
  `Attachment` type moved into Story 4.1 because Story 4.3 validates
  `attachment://` references; the reference-card bench and pixel gates no longer rely
  on the Project Template compiling before Story 14.2). [C] Complete Workflow chosen;
  status set to `final`.

### Validation summary (step 4)

- **FR coverage:** FR-1..FR-79 each appear in at least one story's Implements line
  and acceptance criteria; NFR-1..NFR-10, UX-DR1..UX-DR9 and AD-1..AD-29 are all
  cited; every rule id in `docs/discord-rules.md` is cited by a story (documentation-only
  rows are confirmed in Story 14.4).
- **Starter template:** none in the architecture; Story 1.1 lays out the structural
  seed. Creating all twelve crate skeletons up front is deliberate: the seed and the
  `deps-check` layer graph need every crate to exist, and each skeleton is an empty,
  documented `lib`; real code lands only in the story that needs it.
- **Story quality:** every story has a user story, an Implements line (FR/NFR, AD,
  rule ids), BDD acceptance criteria with typed-error, boundary and edge cases,
  trybuild fixtures for every compile-time rule, and an explicit template/Usage Skill
  criterion when it changes the public API.
- **Dependencies:** no story depends on a later story in its own epic; the five
  cross-epic mentions of later stories (1.7 → 2.2, 1.8 → 4.1, 2.6 → 7.1, 5.6 → 13.5,
  12.6 → 14.3) are "later extended by" notes, not prerequisites. Each epic uses only
  earlier epics' outputs.
- **File churn:** `noctis-core` is touched by Epics 5–9, 12 and 13. Consolidation was
  considered and rejected: each epic owns a distinct core subsystem (lanes and
  workers; command resolution and Responder; Route dispatch and Refs; View
  re-rendering; attachments; Cluster configuration; Module state), follows the crate
  layer order the architecture mandates, and is a feedback point (P0 → P1 → P2).

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

## Epic 2: Typed, length-proven `custom_id`s

Bot authors declare Routes and Payloads whose encoded `custom_id` is proven at compile
time to fit 100 characters, with Refs, schema versions and integrity tags handled by
one codec that is the only code allowed to build or parse a `custom_id`.

### Story 2.1: Payload field encodings

As a bot author,
I want integers, snowflakes, enums, booleans, options and bounded strings to encode compactly with a known worst-case width,
So that my Payload types fit in a `custom_id` and round-trip exactly.

**Implements:** FR-16, NFR-9; AD-5, AD-6, AD-25; rules CID-01, CID-07.

**Acceptance Criteria:**

**Given** `noctis_codec::Field { const MAX_LEN: Option<usize> }`
**When** it is implemented for the built-in types
**Then** the widths are `bool` 1, `u8`/`i8` 2, `u16`/`i16` 3, `u32`/`i32` 6, `u64`/`i64`/`Id<T>` 11, `Option<T>` `T + 1`, `BoundedStr<N>` `2N`, `String`/`Vec<T>` `None`, signed values zig-zag encoded in base62
**And** a unit test per type proves the worst-case value encodes to exactly `MAX_LEN` characters

**Given** `BoundedStr<N>` containing the field separator `,`, the escape `!`, `:` or `~`
**When** it is encoded and decoded
**Then** the escape character `!` makes the round trip exact, non-ASCII input is rejected at construction, and the encoded form is ASCII only (CID-07)

**Given** proptest strategies for every field type and for composite field lists
**When** the property tests run
**Then** encode-then-decode returns an equal value for every generated input (FR-16)
**And** decoding from `&str` slices allocates nothing for fixed-width fields and returns `Cow<'a, str>` for strings (AD-25)

**Given** a bot type
**When** the author implements `Field` for it
**Then** the same round-trip helper and width check are available to the author's tests (FR-16 last consequence), documented in the Usage Skill's routes reference

### Story 2.2: `custom_id` layout encoder and total decoder

As a bot author,
I want every `custom_id` to follow one normative layout with a total decoder,
So that malformed or stale ids are typed outcomes and never panics.

**Implements:** FR-16, FR-19 (decoding), FR-60 (schema versions); AD-5, AD-13; rules CID-01, CID-03, CID-06, CID-07.

**Acceptance Criteria:**

**Given** the layout `F route ":" [S] [E] [N] body [T]`
**When** the codec encodes a Route value
**Then** `F` is a base62 digit `16 * generation + flags` with generation 0, the `S`, `E`, `N`, `T` sections appear only when their flags are set, and the output is written into `ArrayString<100>`
**And** the constants `FMT_LEN = 1`, `SEP_LEN = 1`, `SCHEMA_LEN = 1`, `VIEW_ENV_LEN = 5`, `NONCE_LEN = 2`, `REF_MARKER_LEN = 1`, `REF_LEN = 11`, `TAG_LEN = 6` are `pub const`, and `xtask rules-catalog` now asserts the catalog quotes the same values (pending entry removed)

**Given** arbitrary input strings (fuzzed with proptest, including non-ASCII, empty, 101+ chars, `F >= 16`)
**When** the decoder runs
**Then** it returns `Ok(Decoded)` or `Err(CodecError)` and never panics (CID-06)

**Given** an id whose flag set does not match the Route's declared shape, or whose `S` differs from the Route's schema
**When** it is decoded against that Route
**Then** the result is `Stale` (FR-60), distinct from `CodecError`

**Given** a Payload type gains a trailing `Option<T>` field
**When** an id encoded by the earlier version is decoded
**Then** the missing trailing field decodes as `None` with no schema bump (FR-60 compatible change)

### Story 2.3: Refs and two-phase `EncodedId`

As a bot author,
I want oversized or by-ref Payloads replaced by short unguessable Refs,
So that large state still fits the 100-character limit without ever being guessable.

**Implements:** FR-17 (format), NFR-6; AD-5, AD-18; rules CID-01, CID-05.

**Acceptance Criteria:**

**Given** the Ref generator
**When** a Ref is minted
**Then** it is exactly 11 base62 characters drawn from the OS CSPRNG via `getrandom` (≥ 64 bits of entropy), never sequential and never derived from interaction data
**And** a statistical test over 100k Refs finds no duplicates and a uniform character distribution

**Given** a Payload that must be stored
**When** the codec encodes it
**Then** it produces `EncodedId::RefSlot(PendingRef { route, schema, payload: Bytes, ttl, one_shot })`, while a fitting inline Payload produces `EncodedId::Inline(ArrayString<100>)`
**And** `codec::finish(slot, ref)` yields the final `custom_id` with body `"~" + ref`, whose length is at most 46 for a 24-character route

**Given** a `RefRecord { route, schema, payload }`
**When** it is encoded and decoded with the codec
**Then** it round-trips (property test), and decoding a record whose route differs from the id's route is reported as forged

### Story 2.4: Integrity tags for signed inline Payloads

As a bot author,
I want to optionally sign inline Payloads,
So that a forged `custom_id` is rejected like an unknown Route.

**Implements:** FR-19 (integrity tag), NFR-6; AD-5, AD-29; rules CID-06.

**Acceptance Criteria:**

**Given** a 32-byte `PayloadKey` newtype
**When** it is formatted with `Debug` or `Display`
**Then** it prints a redacted placeholder, never key material (AD-29)

**Given** a Route declared signed with an inline body
**When** it is encoded
**Then** `T` is 6 base62 characters of truncated keyed BLAKE3 over all preceding characters and the `T` flag is set

**Given** a tampered body, a wrong tag or a missing tag
**When** it is verified
**Then** the decoder reports `Forged`, and an id signed with the configured previous key still verifies during rotation

### Story 2.5: `#[derive(Route)]`, `Payload` and `PayloadEnum` with compile-time length proof

As a bot author,
I want deriving a Route to prove at compile time that its `custom_id` can never exceed 100 characters,
So that a length bug fails `cargo check` instead of failing in production.

**Implements:** FR-16, FR-17 (by_ref/spill), FR-60, FR-75, FR-76; AD-2, AD-6, AD-19; rules CID-01, CID-03, CID-05.

**Acceptance Criteria:**

**Given** `#[derive(Route)] #[route("shop.buy")] struct BuyButton { item: Id<ItemMarker>, qty: u8 }`
**When** it compiles
**Then** it implements `Route { const ID; const SHAPE; const SCHEMA; type Payload }` and `Payload { MAX_ENCODED_LEN }` by summing `<F as Field>::MAX_LEN` plus separators through codec `const fn`s, and emits `const _: () = ::noctis::__private::codec::assert_fits::<BuyButton>();` plus a `#[test]` printing the width breakdown
**And** macro output names runtime items only through `::noctis::__private::<crate>` paths

**Given** trybuild fixtures under `tests/ui/cid/`
**When** the UI tests run
**Then** a Route whose worst case is 101 characters fails to compile and one of exactly 100 compiles (`tests/ui-pass/`), an unbounded `String` field without `by_ref`/`overflow = spill` fails with CID-01, `by_ref` together with `signed` fails, a generic Route fails, and a route id violating `[a-z0-9._-]{1,24}` or starting with `_` fails with CID-03
**And** every `.stderr` follows `[<RULE-ID>] <what is wrong>` with a `help:` line

**Given** the attribute options `by_ref`, `overflow = spill`, `schema = n` (1..=61), `view`, `signed`
**When** they are combined legally
**Then** the derived `SHAPE` and the length bound include `S`, `E`, `N`, `T` only when declared, and `spill` Routes encode inline when the value fits and by Ref otherwise

**Given** `#[derive(PayloadEnum)]` on an enum
**When** it has at most 62 unit variants
**Then** it encodes as one character; a 63rd variant is a compile error

**Given** the new public derives and traits
**When** the story is done
**Then** `noctis` re-exports them in the prelude, the Usage Skill's `references/routes-payloads-refs.md` documents the attribute syntax, widths and errors, the template's Route declarations are reconciled with the real syntax, and `api-snapshot`/`skill-check` pass

### Story 2.6: Zero-allocation inline decode guard

As a bot author running at millions of users,
I want decoding fixed-width inline Payloads to allocate nothing,
So that interaction dispatch stays cheap at scale.

**Implements:** NFR-9, UX-DR8; AD-25.

**Acceptance Criteria:**

**Given** a dhat counting-allocator test in `noctis-codec`
**When** it decodes 10k inline `custom_id`s of a fixed-width Route
**Then** the test asserts zero heap allocations and fails otherwise

**Given** the test
**When** CI runs
**Then** it is part of the required allocation test job (later extended by Story 7.1 for Route lookup)

## Epic 3: Pluggable storage ports

Bot authors get every storage-like concern as a dyn-compatible port with a bounded
in-memory default, and can hand their own implementation (Redis, SQL, anything) to the
App without touching handlers.

### Story 3.1: Port traits and the shared op vocabulary

As a bot author,
I want every storage concern defined as a small dyn-compatible trait with a shared op vocabulary,
So that I can implement Redis or SQL versions with clear, uniform semantics.

**Implements:** FR-38, NFR-6; AD-1, AD-14, AD-22 (Renderer port types).

**Acceptance Criteria:**

**Given** `noctis-ports`
**When** it is built
**Then** it defines `RefStore`, `SessionStore`, `IdentifyQueue`, `ModuleStateStore`, `RateLimitStore`, `CacheStore`, `RenderCache`, `AssetStore`, `LocaleSource` and `GuildAccessList`, each `Send + Sync + 'static` and dyn-compatible, with async methods returning `BoxFuture<'_, Result<T, PortError>>`
**And** each store port exposes only its subset of `get`, `put(ttl)`, `put_if_absent(ttl) -> bool`, `take`, `delete`, `touch`, `incr_with_ttl(key, by, ttl) -> u64` as listed in AD-14, `ModuleStateStore` adds `version() -> u64`, and `IdentifyQueue` is `acquire(bucket) -> Lease`

**Given** the `Renderer` driven port
**When** it is declared
**Then** it takes a `RenderJob` and returns `RenderOutput { bytes, digest, format, settings_used }`, without depending on takumi

**Given** port values
**When** typed values are stored
**Then** a helper serializes with postcard plus a leading schema byte, and a schema mismatch reads as a miss (unit test)
**And** ports accept only opaque keys and `Bytes` values; no port method takes a token, payload key or other secret (NFR-6)

**Given** the new public traits
**When** the story is done
**Then** `noctis::ports` re-exports them, the Usage Skill gains an Adapters reference covering each trait and its op semantics, and `api-snapshot`/`skill-check` pass

### Story 3.2: Bounded in-memory store defaults

As a bot author,
I want working in-memory implementations of every store port that never grow without bound,
So that my bot runs out of the box and stays within memory budgets.

**Implements:** FR-38, NFR-1 (bounded), UX-DR8; AD-1, AD-14, AD-25.

**Acceptance Criteria:**

**Given** `noctis_ports::memory`
**When** each default store is constructed
**Then** it has a configurable entry or byte capacity (moka weighers by bytes) and TTL with documented defaults, and no map grows unbounded

**Given** concurrent callers (100 tasks)
**When** they race `put_if_absent` or `take` on one key, or `incr_with_ttl` on one counter
**Then** exactly one `put_if_absent`/`take` wins, no increment is lost, and TTL is set only on create (unit tests)

**Given** an entry past its TTL
**When** it is read
**Then** it is a miss; `touch` extends it; `delete` removes it

**Given** `ModuleStateStore`
**When** state changes
**Then** `version()` increases monotonically

**Given** the Project Template's `adapters/bounded_cache.rs` custom `CacheStore`
**When** the story is done
**Then** it is reconciled with the real `CacheStore` signature, and the Usage Skill documents how to implement a custom Adapter with that file as the example

### Story 3.3: In-memory IdentifyQueue with identify buckets

As a bot author,
I want the default identify queue to honor Discord's identify buckets,
So that shards with `max_concurrency = 16` identify in parallel rather than one at a time.

**Implements:** FR-2 (queue), FR-38; AD-14, AD-21; rules GW-01.

**Acceptance Criteria:**

**Given** a memory `IdentifyQueue` built from `max_concurrency`
**When** shards `0..32` acquire leases with `bucket = shard_id % max_concurrency` (GW-01)
**Then** leases in different buckets are granted concurrently and each bucket grants at most one lease per 5 s (tested with a paused tokio clock)

**Given** `max_concurrency = 1`
**When** shards acquire
**Then** they are serialized 5 s apart

**Given** the port docs
**When** a Cluster implementer reads them
**Then** they state that a Cluster implementation must serialize per bucket across processes, linking the (future) Cluster guide

## Epic 4: Components V2 and modal model with limit validation

Bot authors build any Components V2 message or modal through Noctis's own typed model,
where illegal structures don't compile, literals are checked at compile time and every
Limit is validated before anything is sent. This epic is pure (`noctis-ui` over
`noctis-codec` and `noctis-rules`) and is verified with JSON snapshots.

### Story 4.1: Typed V2 message components with placement in the type system

As a bot author,
I want typed builders for every Components V2 message component where illegal placements are type errors,
So that I can't build a structure Discord will reject.

**Implements:** FR-23, FR-58 (constructor bound), FR-78 (placement), UX-DR1; AD-12, AD-19; rules CV2-05, CV2-08, CV2-12, CV2-14.

**Acceptance Criteria:**

**Given** `noctis-ui`
**When** the component model is built
**Then** it defines Container, Section, Text Display, Thumbnail, Media Gallery, File, Separator, Action Row, `RouteButton<R>`, `LinkButton`, `PremiumButton`, String/User/Role/Mentionable/Channel selects, with marker traits `TopLevel`, `ContainerChild`, `SectionAccessory` (Accessory) and `RowItem`
**And** each type serializes directly to Discord JSON with unset fields omitted (no `"content_type": null` on media), verified by `insta` snapshots

**Given** trybuild fixtures under `tests/ui/cv2/`
**When** the UI tests run
**Then** a Container inside a Container, a Thumbnail outside a Section accessory, a Button outside an Action Row or accessory, and a Section without an accessory fail to compile citing CV2-05 or CV2-07

**Given** a Route `R`
**When** a component is built from it
**Then** the only constructor is from an `R` value (`BuyButton { .. }.button()`, `.select()`), it requires `R: Handled`, and there is no string `custom_id` constructor (AD-19); a fixture shows a Route without `Handled` failing to compile (FR-58 compile-time half)
**And** `noctis-ui` defines `Attachment { name, bytes: Bytes, content_type, alt }` named `<first 16 hex of blake3(bytes)>.<ext>` (or `Attachment::with_digest`), an `AttachmentSet` keyed by name, and `AttachmentRef`, obtainable only by adding an attachment; `File` accepts only an `AttachmentRef` (CV2-12), and numeric component ids are assigned deterministically from the tree (CV2-14)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill has one entry per component (where it may appear, its Limits, its builder), the `skill-check` component-entry check is enabled (pending entry removed), and the template's component code is reconciled

### Story 4.2: V2 messages, legacy messages, flags and mentions

As a bot author,
I want V2 messages that always carry the right flag and can never carry legacy fields,
So that sends and edits never fail with a 400 for flag or field misuse.

**Implements:** FR-23, FR-25 (mention default), NFR-6, UX-DR1; AD-12, AD-27; rules CV2-01, CV2-02, CV2-15, MSG-01, MSG-03, MSG-05, MSG-07, INT-09.

**Acceptance Criteria:**

**Given** a `V2Message`
**When** it is serialized for create or edit
**Then** `IS_COMPONENTS_V2` is always set and the type has no `content`, `embeds`, `poll`, `sticker_ids` or `shared_client_theme` fields (CV2-01, CV2-02)
**And** `allowed_mentions.parse` defaults to `[]`, with opt-in via a typed `Users::All | Users::Only(ids)` (and roles) enum so `parse` plus explicit ids can't be built (MSG-03, MSG-05)

**Given** typed flag sets `CreateFlags`, `EditFlags` and `CallbackFlags`
**When** a flag invalid for an endpoint is used
**Then** it does not compile (MSG-07, INT-09), shown by a trybuild fixture

**Given** an edit that turns a legacy message into a V2 message
**When** it is serialized
**Then** `content`, `embeds`, `poll` and stickers are explicitly cleared in the same body; turning a V2 message back into a `LegacyMessage` is a typed error

**Given** `LegacyMessage`
**When** it is built
**Then** it supports plain text (MSG-01) and at most 5 Action Rows (CV2-15), and is documented as the only legacy path (FR-23 minimal legacy send)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `V2Message`, `LegacyMessage`, flags and mention opt-in, and `api-snapshot`/`skill-check` pass

### Story 4.3: Runtime Limit validator with path-precise errors

As a bot author,
I want every message validated against all Limits before it is sent, with errors naming the exact path,
So that dynamic content never produces a Limit-related 400 from Discord.

**Implements:** FR-24, FR-78 (runtime half); AD-3, AD-4; rules CV2-03, CV2-04, CV2-06..CV2-11, CV2-13, CID-02, CV2-14, ATT-05, ATT-06.

**Acceptance Criteria:**

**Given** a tree built from runtime data
**When** `validate(&tree, &limits)` runs
**Then** every component at every depth counts toward 40 (CV2-03), all Text Display text counts toward 4000 via `discord_len` (CV2-04), and per-field limits (labels, placeholders, option counts, media items, Section shape, gallery counts, link `url`, premium buttons, accent color, select `default_values`) are enforced
**And** duplicate `custom_id`s or numeric ids in one message are errors (CID-02), and every `attachment://` reference has a member in the message's attachment set (ATT-05)

**Given** a violation
**When** the error is returned
**Then** it names the path (for example `container[0].section[2].accessory.label`), the rule id, the limit and the actual value

**Given** `Limits` in Lenient mode
**When** text exceeds a limit
**Then** it is truncated with `…` and a warning is logged, while structural rules (CV2-03, CV2-05, CV2-06, CID-01, CID-02) still fail

**Given** Strict mode
**When** a Thumbnail or gallery item has no alt text
**Then** a warning is emitted and appears in the cost report

**Given** any tree
**When** `cost(&tree)` is called
**Then** it returns component count, text characters and warnings without sending (used by FR-25)

### Story 4.4: Compile-time checks for literals, arity and static budgets

As a bot author,
I want literal labels, placeholders and fixed-shape component trees checked by `cargo check`,
So that static mistakes never reach runtime.

**Implements:** FR-75, FR-78, NFR-10; AD-3, AD-4; rules CV2-03, CV2-04, CV2-06, CV2-07, CV2-08, CV2-09, CV2-10, CV2-11, CV2-13, CV2-15.

**Acceptance Criteria:**

**Given** builders that take `Label`, `Placeholder` and similar newtypes
**When** a literal is passed through `const { Label::lit("…") }`
**Then** an over-length literal (81-char button label, 151-char placeholder, 1025-char description) fails to compile, and the runtime `try_from` path enforces the same limit via the same `noctis-rules` function

**Given** const-generic arity
**When** `ActionRow::buttons([b; N])`, `Section::new([t; N], accessory)` or `MediaGallery::new([i; N])` is built
**Then** `N` outside 1–5, 1–3 or 1–10 fails to compile (CV2-06, CV2-07, CV2-10), and `Rgb(u32)` outside 0x000000–0xFFFFFF fails in `const fn` (CV2-13)

**Given** node types with `const COST: usize` and `const TEXT: Option<usize>`
**When** a fully static tree exceeds 40 components or its literal text exceeds 4000
**Then** a `const { assert!(..) }` fails to compile (CV2-03, CV2-04); trees containing `Vec`s assert their static lower bound and leave the rest to the runtime validator

**Given** `tests/ui/cv2/` and `tests/ui-pass/`
**When** the UI tests run
**Then** each rule above has a failing fixture and a boundary-pass fixture (exactly 5 buttons, exactly 40 components, 80-char label), each `.stderr` names the rule id, and the matching `rules-pending.toml` entries are removed

### Story 4.5: Modal component model and modal validation

As a bot author,
I want typed modal components including Radio Group, Checkbox Group and Checkbox,
So that I can build modern modals without deprecated forms or invalid combinations.

**Implements:** FR-29, FR-78 (modal), UX-DR1; AD-12; rules MOD-01..MOD-08.

**Acceptance Criteria:**

**Given** `noctis-ui` modal types
**When** a modal is built
**Then** its top level accepts only Label or Text Display (MOD-02), Labels wrap Text Input, modal selects, File Upload, Radio Group, Checkbox Group or Checkbox, and message components (Buttons, Sections, Containers, galleries) are not accepted (MOD-03, type error fixture)
**And** modal select types have no `disabled` field, and Noctis never emits an Action-Row-wrapped Text Input or a Text Input `label` (snapshot test)

**Given** a modal built at runtime
**When** it is validated
**Then** more than 5 top-level components, title over 45 (MOD-01), Label over 45 or description over 100 (MOD-04), Text Input bounds (MOD-05), File Upload bounds (MOD-06), Radio Group options outside 2–10, Checkbox Group options outside 1–10, `required` on a Checkbox, and `min_values = 0` with `required = true` (MOD-07) are typed errors naming the path and rule id
**And** the modal's `custom_id` comes from a Route (MOD-08)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill has one entry per modal component with Limits and builder, and `skill-check` passes

### Story 4.6: `derive(Modal)` and typed modal submissions from raw JSON

As a bot author,
I want to declare a modal as a struct and receive its submission as that typed struct,
So that modal input is parsed for me, including component types twilight doesn't model.

**Implements:** FR-29, FR-64, FR-77/FR-78 (static modal rules); AD-12; rules MOD-01, MOD-02, MOD-04, MOD-05, MOD-06, MOD-07, MOD-09.

**Acceptance Criteria:**

**Given** `#[derive(Modal)]` on a struct with a modal Route and a literal title
**When** it compiles
**Then** one field maps to one top-level component, and more than 5 fields, an over-length literal title or Label, invalid literal Text Input bounds and a `Choices` enum with 1 or 11 variants on a Radio Group fail to compile (trybuild fixtures for MOD-01, MOD-02, MOD-04, MOD-05, MOD-07)

**Given** a modal submission as raw JSON (`RawJson`) containing Text Input, selects, File Upload, Radio Group, Checkbox Group and Checkbox values
**When** it is decoded into the derived struct
**Then** every field is populated, select values join to resolved entities, File Upload values join to their attachments with metadata, and Radio/Checkbox values decode even from fixtures where twilight's model drops them (FR-64)
**And** unknown component data is kept as `serde_json::Value` and a missing or ill-typed field is a typed error, never a panic (MOD-09)

**Given** File Upload values
**When** the docs are read
**Then** the attachment type exposes a download helper and states that the CDN URL is ephemeral

**Given** the new public derive
**When** the story is done
**Then** the Usage Skill's modal recipe uses the real `derive(Modal)` syntax, the template's profile bio modal is reconciled, and `skill-check` passes

## Epic 5: A running sharded bot

A bot author builds an App, runs it sharded across every core with correct identify
concurrency, and handles gateway events without ever stalling heartbeats or dropping
interactions. Tests use a local mock of Discord (an HTTP mock server plus an in-process
gateway stub) so no network is needed.

### Story 5.1: App builder, process Runtime and startup safety

As a bot author,
I want to build an App from a token, Modules and Adapters and get typed errors for every startup mistake,
So that misconfiguration fails clearly instead of panicking.

**Implements:** FR-1, FR-38 (Adapter injection), NFR-4, NFR-6; AD-9, AD-13, AD-21, AD-24, AD-29; Conventions (Config).

**Acceptance Criteria:**

**Given** `App::builder()` and `App::from_env()` (reading `NOCTIS_*` variables)
**When** a bot author sets the token, Modules, `State<T>` values and any port implementation (`.ref_store(..)`, `.cache_store(..)`, …)
**Then** `.build()` returns an `App` or a `BuildError` listing every violation at once, and unset ports default to the bounded memory implementations
**And** the token is a newtype whose `Debug`/`Display` are redacted and that is never logged (NFR-6)

**Given** `App::run` is called outside a multi-thread tokio runtime
**When** it starts
**Then** it returns a typed error instead of panicking; when no rustls crypto provider is installed, Noctis installs one, and a failure to do so is a typed error

**Given** the App
**When** it is built
**Then** core constructs exactly one `Arc<twilight_http::Client>` per App with a configurable request timeout (default 10 s), and no other crate constructs a Discord HTTP client (AD-24)
**And** process-scoped resources live in `noctis::Runtime`, the first App's configuration freezes them, and a later conflicting configuration is a `BuildError`

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's `references/app-and-sharding.md` documents the builder, `from_env` variables and errors, the template's `main.rs`/`lib.rs` `configure()` are reconciled with the real builder, and `skill-check` passes

### Story 5.2: Automatic sharding with bucketed identify

As a bot author,
I want Noctis to start the recommended number of shards and identify them as fast as Discord allows,
So that a large bot boots quickly without identify storms.

**Implements:** FR-1 (token/intent errors), FR-2; AD-10, AD-21; rules GW-01, GW-02, GW-03, GW-05.

**Acceptance Criteria:**

**Given** a mocked `GET /gateway/bot` returning a shard count and `max_concurrency = 16`
**When** the App starts without a fixed shard total
**Then** it starts the recommended number of shards, each in its own tokio task, identifying through the `IdentifyQueue` port with buckets `shard_id % 16` so same-bucket shards identify in parallel (GW-01)

**Given** a fixed shard total
**When** it is too small for the guild count (GW-03)
**Then** `build`/start fails with a typed error; when it isn't a multiple of `max_concurrency > 1`, startup logs a warning

**Given** `session_start_limit.remaining` lower than the shards to identify
**When** startup runs
**Then** it refuses to boot with a typed error (GW-02)

**Given** an invalid token (401) or a fatal close for disallowed intents (4014)
**When** startup runs
**Then** it fails with a typed error naming the cause (FR-1), never a silent stop

**Given** shard tasks
**When** they receive gateway messages
**Then** they parse raw messages themselves with `twilight_gateway::parse`, keeping the raw `d` JSON for interactions as `RawJson`, and produce `Inbound { shard, received_at, kind }`

### Story 5.3: Interaction Lane and bounded event lane

As a bot author,
I want interactions on a priority lane that never drops and events on a bounded lane with explicit drop policy,
So that slow handlers never cost heartbeats or user-visible interactions.

**Implements:** FR-5, NFR-3 (structure), NFR-5, UX-DR8; AD-10, AD-25, AD-29.

**Acceptance Criteria:**

**Given** shard tasks
**When** they enqueue
**Then** they only `try_send`; they never await lane capacity or a handler, so a handler sleeping 10 s doesn't delay heartbeats (test with the gateway stub's heartbeat timing)

**Given** the event lane is full
**When** a droppable event type (presence, typing, others marked droppable) arrives
**Then** the oldest queued droppable event is dropped and `noctis_events_dropped_total{event_type}` increments
**And** a non-droppable type goes to the metered overflow buffer (`noctis_event_overflow_depth`) and is dropped with an error log only beyond its hard cap

**Given** the event lane and overflow are full
**When** an interaction arrives
**Then** it enters the interaction lane (exempt from the bound) and reaches its handler within the FR-21 deadline (test with a paused clock)

**Given** the lanes
**When** metrics are scraped
**Then** `noctis_lane_depth{lane}` reports each lane's depth with the `app` label

### Story 5.4: Modules and typed Event Handlers with derived filters and intents

As a bot author,
I want to register typed Event Handlers in a Module and have Noctis derive the event filter and required intents,
So that unhandled events are never deserialized and missing intents fail before connecting.

**Implements:** FR-5 (worker set, per-guild ordering), FR-6, FR-44 (basic Module), NFR-4; AD-7, AD-9, AD-10, AD-13, AD-21; rules GW-05.

**Acceptance Criteria:**

**Given** `Module::new("a/b")` with `#[event]` handlers added
**When** the App is built
**Then** the Registry is folded once from descriptors, and Noctis asks the gateway to deserialize only event types with a handler (or needed by the cache), so an unhandled type is never deserialized (test with counting deserialization)

**Given** a handler needing an intent the App did not request
**When** `App::build` runs
**Then** it fails with a `BuildError` naming GW-05, the handler and the intent; with no event handlers the default intents are `Intents::GUILDS`

**Given** two handlers for the same event, one marked ordered
**When** the event arrives
**Then** both run; the ordered one runs before the concurrent ones, and ordered handlers run in per-guild order through guild-keyed sub-queues

**Given** the worker set
**When** lanes have items
**Then** a semaphore-bounded worker set drains the interaction lane first, and every handler future runs under `catch_unwind`, so a panicking handler is logged and the shard keeps running (NFR-4)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `Module`, `#[event]`, ordering and intent derivation, the template's event handler (profile XP on message) is reconciled, and `skill-check` passes

### Story 5.5: Observability contract and Error Sinks

As a bot operator,
I want consistent spans, metrics and a configurable Error Sink per error origin,
So that I can monitor and alert on a Noctis bot without reading its code.

**Implements:** FR-22 (sinks), NFR-5, NFR-6; AD-13, AD-29.

**Acceptance Criteria:**

**Given** a running App
**When** shards, events and interactions are processed
**Then** spans `noctis.shard`, `noctis.event` and `noctis.interaction` carry the fields `app`, `shard`, `guild_id`, `module` and the route or command path, and metrics go through the `metrics` facade with the `noctis_` prefix and an `app` label

**Given** any metric
**When** its labels are inspected (unit test over the registered metric set)
**Then** no label carries user, guild, channel or message ids or free text

**Given** an `ErrorSink` trait with origins Command, Component/Modal, Autocomplete, Event and Render
**When** a handler, hook or event errors or panics, or an overflow drop happens
**Then** the sink for that origin receives it; the default sink logs at `error`, and a bot author can set a different sink per origin

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents spans, metrics and Error Sinks, and `skill-check` passes

### Story 5.6: Graceful shutdown and fatal-close reporting

As a bot operator,
I want shutdown to finish in-flight work within a grace period and fatal shard closes to be reported,
So that deploys don't lose interactions and dead shards never fail silently.

**Implements:** FR-1 (shutdown, fatal close); AD-21, AD-29.

**Acceptance Criteria:**

**Given** a shutdown signal (Ctrl-C/SIGTERM)
**When** core shuts down
**Then** it follows the AD-21 order: mark shutdown started, `gateway.close_all(..)` so shards stop enqueuing, drain both lanes until empty or the configurable grace period ends with HTTP still live, await `gateway.closed()`, then drop ports
**And** handlers still running at the deadline are cancelled and reported to the Error Sink

**Given** a shard closes fatally after startup
**When** the gateway reports it as a `ShardEvent`
**Then** core logs it, sets `noctis_shard_status{shard}`, and emits an internal shard-status event that Story 13.5 exposes as a Lifecycle Hook; it is never a silent stop

## Epic 6: Slash commands and Publish

A bot author declares tescord-style Commands, Command Patterns, typed options,
autocomplete and context menus; gets compile-time and App-build rule checks; responds
safely through the Responder with automatic deferral; and publishes only when
something changed.

### Story 6.1: `#[command]` with compile-time Command Path and Pattern validation

As a bot author,
I want to declare a Command as an `async fn` with a Command Path or Pattern that is validated at compile time,
So that illegal command structures fail `cargo check` and my handler knows which variant ran.

**Implements:** FR-7, FR-8, FR-75, FR-77, UX-DR3; AD-4, AD-8, AD-26; rules CMD-01, CMD-02, CMD-03, CMD-04, CMD-05, CMD-10, CMD-11, CMD-12, CMD-13, CMD-15, OPT-14.

**Acceptance Criteria:**

**Given** `#[command("settings logs channel", description = "…")]` on an `async fn`
**When** it compiles
**Then** it produces a command descriptor whose word count maps to command, subcommand, or group plus subcommand (FR-7), with subcommand `required` not settable (OPT-14)

**Given** `#[command("config (set|get) (prefix|welcome)")]`
**When** it compiles
**Then** the macro expands it with `noctis_rules::command::expand`, validates it with `validate_tree`, and generates a typed enum (for example `ConfigPath::SetPrefix`) that the handler receives telling it which combination ran (FR-8)
**And** `#[command(max_expansions = n)]` changes the default cap of 25

**Given** trybuild fixtures under `tests/ui/cmd/`
**When** the UI tests run
**Then** `"config (set|get) (prefix|welcome)?"` fails with `[CMD-11]` naming `set`, the two contributing paths and a `help:` to remove the `?`, while `"config (set|get) (prefix|welcome)"` compiles
**And** fixtures fail for CMD-01 (invalid character, ASCII `'`), CMD-02 (33-char word, empty word), CMD-03 (uppercase, with lowercase suggestion), CMD-04 (missing or 101-char description), CMD-10 (4 words), CMD-12 (`config (set)?`), CMD-13 (26 children), CMD-15 (duplicate expansion, over cap) and CMD-05 (8000 budget), and boundary passes exist (32-char name, 100-char description, 25 children)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's `references/commands.md` documents `#[command]`, patterns and the variant enum, the template's commands are reconciled, and `api-snapshot`/`skill-check` pass

### Story 6.2: Typed options with `derive(Options)` and `derive(Choices)`

As a bot author,
I want to declare a Command's options as a typed struct and receive it parsed,
So that missing or wrong option values are typed errors, never panics.

**Implements:** FR-10 (options, validators), FR-77 (option rules); AD-4, AD-26; rules OPT-01..OPT-11, OPT-13, CMD-06.

**Acceptance Criteria:**

**Given** `#[derive(Options)]` with field types implementing the option trait (`i64`, `f64`, `bool`, `String`, `Id<UserMarker>`, channel, role, mentionable, attachment)
**When** the Command is invoked
**Then** the handler receives the parsed struct; a missing required option or a wrong option type is a typed error (OPT-02, OPT-03 runtime decode)

**Given** trybuild fixtures under `tests/ui/opt/`
**When** the UI tests run
**Then** a required field after an optional one fails with OPT-01 (the derive never reorders), and fixtures fail for duplicate names (CMD-06), bounds on non-numeric fields or `min > max` (OPT-05), invalid string lengths (OPT-06), `channel_types` on a non-channel field (OPT-07), a `Choices` enum with 26 variants (OPT-08), over-length choice name/value (OPT-09), choices plus autocomplete (OPT-10), autocomplete on an unsupported type (OPT-11) and invalid `file_types` (OPT-13), with boundary passes (25 choices, 25 options)

**Given** an option with a validator
**When** validation fails at runtime
**Then** the user gets an ephemeral message from the validator's `MessageKey` and the handler doesn't run

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `derive(Options)`, `derive(Choices)` and validators, and `skill-check` passes

### Story 6.3: Command metadata and context-menu Commands

As a bot author,
I want to set permissions, contexts, integration types, nsfw and guild scope per Command, and register user and message context menus,
So that each Command appears exactly where and for whom I intend.

**Implements:** FR-11, FR-12; AD-26; rules META-01, META-02, META-03, META-05, CMD-07, CTX-01, CTX-02, CTX-03.

**Acceptance Criteria:**

**Given** `#[command(.., permissions = MANAGE_GUILD | BAN_MEMBERS, contexts = [Guild], integration_types = [..], nsfw = true, guilds = [..])]`
**When** it compiles
**Then** permissions parse into typed bitflags and an unknown permission name fails to compile (META-01), `nsfw` is a typed `bool` attribute (CMD-07; the discovery restriction is documented only), and `contexts`/`integration_types` on a guild-scoped Command fail to compile (META-03); `dm_permission` is not exposed (META-05)

**Given** `#[user_command("Show Profile")]` and `#[message_command("…")]`
**When** they compile
**Then** only the 1–32 length is checked (CTX-01, trybuild fixture for 33 chars), they have no description or options parameters (CTX-02), and their type comes from the macro (CTX-03)
**And** at runtime the handler receives the resolved target user with member when present, or the target message, as typed values

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents metadata and context menus, the template's "Show profile" user command is reconciled, and `skill-check` passes

### Story 6.4: App-build merge and conflict checks for Commands

As a bot author composing several Modules,
I want Commands sharing a base word merged and every cross-Module conflict reported before connecting,
So that Publish never silently drops or overrides a declaration.

**Implements:** FR-9, FR-56, FR-57, FR-75; AD-4, AD-7, AD-8; rules REG-01, REG-02, REG-03, REG-04, CMD-05, CMD-06, CMD-11, CMD-12, CMD-13, META-02.

**Acceptance Criteria:**

**Given** Module A registering `config set` and Module B registering `config get prefix`
**When** `App::build` runs
**Then** both merge into one `/config` Discord command in the Registry

**Given** conflicting declarations across Modules
**When** `App::build` runs
**Then** it returns one `BuildError` listing every violation, each naming the rule id, both Module ids and the values: conflicting `default_member_permissions`/`contexts`/`integration_types`/`nsfw` on one base (FR-9, META-02), a base with its own options and subcommands (CMD-12), a word that is both subcommand and group across Modules (CMD-11), more than 25 children (CMD-13), the merged 8000 budget (CMD-05), duplicate paths including pattern expansions or duplicate context-menu name and type (FR-56, REG-03), and global/guild counts over 100/15/15 (REG-01, REG-02)

**Given** base command and subcommand group descriptions
**When** a Module declares them explicitly
**Then** they are used independently of any leaf (FR-57); two Modules declaring different values fail with an error naming both; undeclared metadata takes the value from the lexicographically first Module id and logs a warning

**Given** each check above
**When** tests run
**Then** each has a unit test asserting the exact `BuildError` variant and rule id, and `rules-pending.toml` loses the corresponding App-build entries

### Story 6.5: Command pipeline and the Responder typestate

As a bot author,
I want my Command handler to reply through a Responder that only offers legal next steps,
So that double acknowledgements and illegal callback types can't happen.

**Implements:** FR-21 (callback rules), FR-61 (Command half, with `V2Message`); AD-11, AD-12, AD-23; rules INT-03, INT-04, INT-06, INT-08, INT-09, INT-10, INT-12.

**Acceptance Criteria:**

**Given** a Command interaction from the interaction lane
**When** core dispatches it
**Then** it resolves the Command from the Registry's O(1) map by default (unlocalized) names (LOC-06) and runs the AD-23 stages that apply to Commands at this point (resolve, locale default, auto-defer arm point, handler, error routing)

**Given** `CommandCtx`
**When** the handler responds
**Then** `Responder<Pending>` offers `reply` and `defer` (and `modal` only for `opens_modal` handlers), each consuming `self` and returning `Responder<Acked>`, which offers only `edit_original`, `followup` and `delete_original` (INT-03)
**And** `CommandCtx` has no `update` (INT-04, trybuild fixture), `defer` takes only an `Ephemerality` value (INT-08), and every V2 send or edit carries `IS_COMPONENTS_V2`

**Given** a handler that returns a `V2Message`
**When** it completes
**Then** core sends it as the initial response with the Command's declared ephemerality (default public, INT-10), validated by the runtime validator before the HTTP request, and after a defer it uses `edit_original` explicitly (INT-12)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `CommandCtx`, `Responder` states and ephemerality, and `skill-check` passes

### Story 6.6: Automatic deferral, token lifetime and follow-up cap

As a bot author,
I want slow handlers deferred automatically and expired tokens handled,
So that users never see "interaction failed" and late updates still land.

**Implements:** FR-21; AD-11, AD-23, AD-29; rules INT-01, INT-02, INT-05, INT-08, INT-11.

**Acceptance Criteria:**

**Given** a Command handler that hasn't responded by the deadline (default 2500 ms, configurable, measured from `Inbound::received_at` so queueing time counts)
**When** the deadline passes
**Then** core sends a type-5 deferred response with only the declared `EPHEMERAL` flag (INT-08), and the handler's later `reply` becomes `edit_original` carrying the V2 flag (runtime state machine shared by the handler and the auto-defer task)
**And** a handler responding after 5 s still delivers its message (test with paused clock), and `noctis_autodefer_total` and `noctis_ack_latency_seconds` are recorded

**Given** a handler declared `opens_modal`
**When** it is dispatched
**Then** auto-defer is never armed (INT-05); if it misses the deadline, an error naming the handler is logged

**Given** 15 minutes have passed since `received_at`
**When** the handler edits a public response
**Then** the edit falls back to a bot-token message edit; for an ephemeral response the call returns `TokenExpired` without an HTTP request (INT-02)

**Given** a user-installed app not installed in the guild
**When** a sixth follow-up is attempted
**Then** a typed error is returned instead of sending (INT-11)

### Story 6.7: Autocomplete

As a bot author,
I want autocomplete handlers that receive the focused value and the other filled options as a typed partial struct,
So that suggestions are easy to write and never break the interaction.

**Implements:** FR-10 (autocomplete); AD-11; rules INT-06, INT-07, OPT-12.

**Acceptance Criteria:**

**Given** `#[option(autocomplete = handler)]` on a supported field
**When** an autocomplete interaction arrives for an option of a 3-word Command Path
**Then** the handler receives an `AutocompleteCtx` (which offers only `respond`, no defer: INT-06) with the focused value and a `Partial<Options>` whose fields are all `Option<T>` (OPT-12)

**Given** a handler returning 30 choices
**When** it responds
**Then** exactly 25 are sent and a warning is logged, in both Strict and Lenient mode (INT-07)

**Given** a handler that errors or misses the response deadline
**When** the deadline passes
**Then** Noctis responds with an empty choice list and routes the error to the Autocomplete Error Sink

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents autocomplete with an example, and `skill-check` passes

### Story 6.8: User-facing errors, built-in notices and panics as responses

As a bot author,
I want handler errors and panics to become short localized ephemeral messages while the full error is logged,
So that users get a clear reply and I get full diagnostics.

**Implements:** FR-22, FR-10 (validator notice), NFR-4, NFR-5; AD-13, AD-16 (`noctis.*` keys), AD-23; rules LOC-04.

**Acceptance Criteria:**

**Given** `noctis-i18n`
**When** it is built
**Then** it implements `Translate` with default English strings for every reserved `noctis.*` key used by built-in notices (error, expired, disabled, rate limited, guard failed, validation failed), and core uses them through `&dyn Translate`

**Given** a handler returning `Err(HandlerError)` with or without a user `MessageKey`
**When** the error is routed
**Then** the user sees a short ephemeral message (the given key, else `noctis.error.generic`) and the full error is logged through structured tracing with interaction context, then passed to the origin's Error Sink

**Given** a handler that panics
**When** it is dispatched under `catch_unwind`
**Then** the user receives the generic error response, the panic is logged and sent to the Error Sink, and the shard keeps running (NFR-4)

**Given** a notice that must be sent after an auto-defer already happened
**When** it is sent
**Then** it goes out as an ephemeral follow-up and the deferred original is deleted (AD-23)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `HandlerError`, `HandlerResult` and error handling patterns, and `skill-check` passes

### Story 6.9: Publish with diff, dry run and CLI modes

As a bot author,
I want to publish my Commands globally or to a test guild only when they changed, and preview the diff,
So that iteration is fast and I never burn Discord's command-create quota.

**Implements:** FR-11 (JSON), FR-13, FR-61 (empty option list); AD-7, AD-26, AD-29; rules META-04, REG-05.

**Acceptance Criteria:**

**Given** the Registry
**When** Publish builds the command JSON
**Then** it is generated only from the Registry; each metadata field set on a Command appears exactly as declared and unset fields are omitted; an option-less Command publishes an empty option list; guild-scoped Commands are published only to their guilds

**Given** remote commands equal to the local definition (mocked HTTP)
**When** Publish runs
**Then** it makes no write request; when they differ it writes once and logs the diff

**Given** `--publish --dry-run`
**When** the binary runs
**Then** it prints the full JSON and a human-readable diff, the number of creates a write would cost (REG-05), runs the same App-build checks, and writes nothing

**Given** the CLI modes `--publish`, `--publish-guild <id>` and `--publish-clear`
**When** `App::run` sees them
**Then** it performs that Publish mode and returns without connecting shards; Publish never runs on shard ready, and in normal runs it runs only when the App is configured as the publisher
**And** before writing, Publish reads the application object and reports a typed error when a declared `integration_types` value isn't enabled for the app (META-04), and emits a `noctis.publish` span

**Given** the new public API
**When** the story is done
**Then** the Usage Skill and the template README document the publish workflow (`cargo run --release -- --publish-guild <id>`), and `skill-check` passes

## Epic 7: Routed components, Refs and modals

A bot author handles every button, select and modal through exactly one stateless
handler per Route, across all messages, shards and restarts, with Refs for large state,
expiry and stale handling, App-namespaced storage, guards, middleware and automatic
deferral. No listener, closure or timer is ever kept per message.

### Story 7.1: Route handlers and O(1) component dispatch

As a bot author,
I want to register exactly one handler per Route and have components dispatched to it in constant time,
So that one function handles a button on every message, shard and restart.

**Implements:** FR-15, FR-58, NFR-2, NFR-9, UX-DR7; AD-7, AD-9, AD-19, AD-25; rules CID-04.

**Acceptance Criteria:**

**Given** `#[handler]` on an `async fn(ctx: ComponentCtx, payload: R)` for Route `R`
**When** it compiles
**Then** it implements `Handled` for `R` (so components for `R` can now be constructed) and produces a handler descriptor bound by `R::ID`

**Given** two Modules registering handlers for the same Route id
**When** `App::build` runs
**Then** it fails with a `BuildError` naming CID-04 and both Modules (unit test)
**And** a Route referenced by a mounted Module's descriptors without a mounted handler fails App build (FR-58 dynamic half)

**Given** 10 or 10 000 registered Routes
**When** a component interaction is dispatched
**Then** lookup uses the Registry's map keyed by `&'static str` with a borrowed key, and the dhat allocation test (extended from Story 2.6) shows zero allocations for Route lookup plus fixed-width decode

**Given** the framework
**When** the dispatch code is reviewed and tested with `N` messages carrying the same Route
**Then** no per-message or per-component closure, task or timer exists (NFR-2), and memory does not grow with `N`

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's routes reference documents `#[handler]` and the one-handler-per-Route rule, the template's handlers are reconciled, and `skill-check` passes

### Story 7.2: Component pipeline, untrusted input and in-place updates

As a bot author,
I want component interactions decoded defensively and answered by updating the message in place,
So that forged or malformed ids never crash the bot and clicks feel instant.

**Implements:** FR-19, FR-21 (components), NFR-4, NFR-6; AD-5, AD-11, AD-23, AD-29; rules CID-06, INT-01, INT-04.

**Acceptance Criteria:**

**Given** a component interaction whose `custom_id` is malformed, names an unknown Route, has a bad integrity tag or an undecodable Payload
**When** stage 1 of the pipeline runs
**Then** it produces a typed error and a logged warning, never a panic, and the user gets the generic ephemeral notice (CID-06)

**Given** `NOCTIS_PAYLOAD_KEY` (and an optional previous key) from the environment
**When** a signed Route's id is verified
**Then** a valid tag passes, a tag from the previous key passes during rotation, a bad tag takes the unknown-Route path, and the key is never logged or passed to a port

**Given** `ComponentCtx`
**When** the handler responds
**Then** `Responder<Pending>` offers `update` (type 7), `defer` (type 6 in-place) and `reply`; the declared default for components is update-in-place; the auto-defer uses type 6 at the deadline and later updates become `edit_original` with the V2 flag
**And** spans `noctis.interaction` cover every stage, and `noctis_handler_duration_seconds{kind}` and `noctis_handler_errors_total{kind}` are recorded

### Story 7.3: Expired Handler and stale Payload routing

As a bot author,
I want clicks on expired or pre-deploy components to reach an Expired Handler instead of failing,
So that months-old messages degrade gracefully.

**Implements:** FR-18, FR-60; AD-5, AD-23.

**Acceptance Criteria:**

**Given** a `custom_id` that decodes as `Stale` (schema or shape mismatch)
**When** it is dispatched
**Then** it goes to the Route's stale handler when declared, else to the Expired Handler, never to a generic decode error

**Given** the default Expired Handler
**When** it runs
**Then** it replies with a short ephemeral notice from `noctis.expired`

**Given** an App-level or Route-level override
**When** a Route's interaction expires
**Then** the override runs instead, and can return a View-like response such as re-rendering a starting message (test with a `V2Message`)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents stale and expired handling and Payload evolution rules (append `Option` fields; otherwise bump `schema`), and `skill-check` passes

### Story 7.4: App-namespaced ports

As a bot author running several bots on shared storage,
I want every port key namespaced by App unless I opt into sharing,
So that two Apps never read or overwrite each other's Refs, sessions, counters or Module state.

**Implements:** FR-71, NFR-6; AD-14; Conventions (App ids).

**Acceptance Criteria:**

**Given** an `AppId` (`[a-z0-9-]{1,32}`, default the application id in base62)
**When** core wraps a port
**Then** `Namespaced<P>` prefixes keys with `<app>:<port>:`, escaping `:` in key segments, and adapters receive keys verbatim (they never prefix)

**Given** two Apps sharing one `RefStore` instance
**When** each writes and reads the same logical key
**Then** neither sees the other's value (unit test for RefStore, SessionStore, RateLimitStore and ModuleStateStore)

**Given** an Adapter opted into sharing (for example `CacheStore`)
**When** it is configured with `.share(..)`
**Then** it is wrapped in `Shared<P>` with the prefix `_shared:<port>:` and both Apps see the same keys

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `app_id`, namespacing and sharing, the template's `app_id("noctis-template")` is reconciled, and `skill-check` passes

### Story 7.5: Ref minting and resolution

As a bot author,
I want large or by-ref Payloads stored and resolved for me with TTLs and one-shot semantics,
So that I can carry rich state in components without hitting the 100-character limit.

**Implements:** FR-17, FR-18 (missing Ref), NFR-5, NFR-6; AD-18, AD-23; rules CID-05.

**Acceptance Criteria:**

**Given** a response containing `EncodedId::RefSlot`s
**When** the Responder finalizes it before validation and sending
**Then** it stores each slot with `RefStore::put_if_absent` (retrying on collision), calls `codec::finish` for the final `custom_id`, and identical slots in one message share one Ref; no crate below `noctis-core` holds a `RefStore` handle

**Given** per-Route TTL and the App default TTL
**When** Refs are stored
**Then** the Route's TTL applies when declared, else the App default; a Route marked `one_shot` resolves with `take` so the Ref is deleted on use

**Given** a Route with `overflow = error`
**When** its Payload doesn't fit inline
**Then** the render returns a typed error instead of spilling

**Given** a Ref that is missing, expired, or created by another App
**When** stage 2 resolves it
**Then** the interaction goes to the Expired Handler; a record whose route differs from the id's route is treated as forged; a schema mismatch is Stale
**And** `noctis_ref_lookups_total{result}` counts hit, miss, stale and forged

**Given** a response carrying a `RefOps` deletion list (from pure code such as a Wizard cancel)
**When** core sends it
**Then** core executes the deletions after a successful send

### Story 7.6: Middleware and typed Extensions

As a bot author,
I want Middleware that runs before and after handlers and can attach typed Extensions,
So that cross-cutting concerns (auth, settings, metrics) live in one place.

**Implements:** FR-20; AD-23; Conventions (BuildError).

**Acceptance Criteria:**

**Given** before- and after-Middleware registered on a parent Module and a child Module
**When** an interaction or event runs
**Then** before-Middleware run parent first, then registration order; after-Middleware run in reverse order; and a before-Middleware can stop processing so the handler doesn't run

**Given** a Middleware that inserts an Extension of type `T`
**When** the handler extracts `Ext<T>`
**Then** it receives the value; inserting an existing type replaces it (child wins) with a debug trace; Extensions reach after-Middleware and Error Sinks and are never written to a port

**Given** a handler extracting `Ext<T>` that no before-Middleware on every ancestor path declares
**When** `App::build` runs
**Then** it fails with a `BuildError` naming the handler and `T`; `Option<Ext<T>>` opts out

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Middleware, `Ext<T>` and ordering, and `skill-check` passes

### Story 7.7: Route Guards

As a bot author,
I want to declare guards such as "invoking user only" or "requires MANAGE_GUILD" on a Route,
So that unauthorized clicks are rejected before my handler runs.

**Implements:** FR-19 (guards); AD-23.

**Acceptance Criteria:**

**Given** `#[route("dash.mod", permissions = MANAGE_GUILD)]` or `owner = user` (Payload-carried invoking user)
**When** a user without the permission, or a different user, clicks
**Then** stage 8 rejects it with a localized ephemeral notice (`noctis.guard.*`) and the handler doesn't run

**Given** an auto-defer already happened before the guard decided
**When** the notice is sent
**Then** it is an ephemeral follow-up and the deferred original is deleted

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents guards and the template's guarded Routes are reconciled, and `skill-check` passes

### Story 7.8: Modal Routes and typed submissions end to end

As a bot author,
I want to open a modal from a Command or component and receive its submission as a typed struct in one handler,
So that data entry works across restarts with no per-message state.

**Implements:** FR-21 (modals), FR-29, FR-64; AD-11, AD-12; rules INT-04, INT-05, MOD-08, MOD-09.

**Acceptance Criteria:**

**Given** a Command or Route declared `opens_modal`
**When** its handler runs
**Then** only its context's `Responder<Pending>` offers `modal(..)`, auto-defer is disabled for it, and a non-`opens_modal` handler calling `modal` fails to compile (INT-05 trybuild fixture)

**Given** a modal-submit interaction
**When** it is dispatched to the modal Route's handler
**Then** the handler receives `ModalSubmitCtx` and the typed `derive(Modal)` struct decoded from the interaction's `RawJson`, including Radio Group, Checkbox Group and Checkbox values that twilight drops (FR-64 end-to-end test through the gateway stub)
**And** `ModalSubmitCtx` has no `modal` method (INT-04), so responding to a submit with another modal can't be expressed; a dynamic attempt is a typed error

**Given** a modal submit
**When** the handler is slow
**Then** the auto-defer uses type 6 (in-place update), as for components

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's modal recipe shows the full open/submit flow, the template's profile bio modal compiles against it in tracking mode, and `skill-check` passes

## Epic 8: Views, navigation widgets and the testing kit

A bot author returns Views from any handler and gets Navbar tabs, step Wizards and
automatic pagination that need no handler code and no storage, and can test complete
flows without Discord. Built-in widgets follow the UI Doctrine by construction.

### Story 8.1: Views and View responses

As a bot author,
I want to write a View once as a pure function from state to a message and return it from any handler,
So that the first response and every update come from the same code with no response plumbing.

**Implements:** FR-25, FR-61, NFR-6, UX-DR1, UX-DR3; AD-11, AD-20, AD-27; rules MSG-05.

**Acceptance Criteria:**

**Given** `impl View for T { fn render(&self, cx) -> UiResult<V2Message> }`
**When** the same state is rendered twice
**Then** the output is identical except for per-render select identifiers (snapshot test)

**Given** an option-less `#[command]` whose handler returns a View
**When** it is invoked
**Then** it compiles, publishes with an empty option list, and sends the View as the initial response with no response code from the author (FR-61)
**And** a Component or Modal Handler returning a View updates the message in place, using the FR-21 deferred variant when the deadline passed

**Given** a View listing `<@id>` mentions
**When** it is sent
**Then** nobody is pinged unless the author opts in per View or per message, and `escape_markdown`/`escape_mentions` helpers neutralize user-supplied text

**Given** a View
**When** `view.cost(&limits)` is called
**Then** its component count, text characters and warnings are returned without sending

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `View`, returning Views and the response rules, the template contains no manual response-type selection (`skill-check` doctrine lint), and `api-snapshot`/`skill-check` pass

### Story 8.2: View Routes, widget envelope and state versions

As a bot author,
I want widget controls to ride on my View's own Route and stale clicks to re-render from current state,
So that tabs, pages and steps need no handlers or storage and never apply outdated actions.

**Implements:** FR-25 (selects, state version), FR-62 (envelope), UX-DR7; AD-5, AD-20; rules CID-02.

**Acceptance Criteria:**

**Given** `#[route("dash", view)]`
**When** a control is rendered
**Then** it encodes the `E` envelope (action, tab, page, state version) into the View Route's id; actions `t`, `p`, `j`, `n`, `b`, `c`, `i` are handled by core by re-rendering the View, and action `_` goes to the author's handler
**And** widget controls in one message have distinct envelopes so their ids are unique, and the runtime validator still checks CID-02

**Given** a select rendered by a View
**When** the message is updated after a user selection
**Then** the per-render nonce `N` changes, so every select shows the View's declared defaults, not the user's previous local selection

**Given** a View that opts into a state version
**When** an interaction carries an older version
**Then** core re-renders from current state instead of applying the action and shows a short `noctis.view.stale` notice

### Story 8.3: Paginator and the default Overflow Policy

As a bot author,
I want long lists to paginate automatically under the Limits while my header and footer stay on every page,
So that a list growing from 10 to 300 items never needs a code change or causes a Limit error.

**Implements:** FR-28, FR-62, FR-24, UX-DR6; AD-20, AD-27; rules CV2-03, CV2-04, CV2-09.

**Acceptance Criteria:**

**Given** a View with Chrome (header, footer actions) and a list rendered through `Paginator` with the default Overflow Policy
**When** the list has 300 items
**Then** the Chrome's cost is reserved first, items are packed against the App's `Limits`, no page exceeds any Limit, and next/previous navigation reaches every item (property test over item sizes)

**Given** more than 5 pages
**When** the page is rendered
**Then** a jump String Select (never a modal) reaches any page in a bounded number of clicks

**Given** the current page
**When** navigation happens
**Then** the page travels in the envelope and nothing is stored

**Given** a single item that can't fit beside the Chrome
**When** the View renders
**Then** it returns a typed error naming the item; with `Overflow::Error` any overflow returns the FR-24 error naming path and Limit; `Overflow::Truncate` truncates

**Given** the chunked-select mode
**When** items render as String Selects
**Then** each select has at most 25 options

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Paginator and Overflow Policies, the template dashboard's paginated tab is reconciled, and `skill-check` passes

### Story 8.4: Navbar

As a bot author,
I want a Navbar of tabs at the top of a View that swaps content in the same message,
So that a general View that doesn't fit one Container stays easy to navigate.

**Implements:** FR-26, UX-DR5; AD-20, AD-27; rules CV2-06.

**Acceptance Criteria:**

**Given** a View with a Navbar of N tabs
**When** a tab is clicked
**Then** the same message is edited in place with that tab's content, no new message is sent, and the selected tab travels in the envelope with nothing stored

**Given** any page of any tab
**When** it renders
**Then** the active tab's button is disabled and the others are enabled

**Given** a tab whose content overflows
**When** it renders
**Then** it paginates inside the tab with the Navbar on every page

**Given** more tabs than fit one Action Row
**When** the View renders
**Then** it returns a typed error (CV2-06)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Navbar, the template's dashboard tabs are reconciled, and `skill-check` passes

### Story 8.5: Wizard

As a bot author,
I want setup flows as Wizards with Steps, progress, validation and back/next/cancel,
So that users configure the bot in one message with a clear sense of progress.

**Implements:** FR-27, FR-18, UX-DR4, UX-DR9; AD-18, AD-20, AD-27.

**Acceptance Criteria:**

**Given** a Wizard of 3 Steps
**When** Step 2 renders
**Then** the progress reads "Step 2 of 3: Roles" (from `noctis.wizard.progress`), never page numbers

**Given** a Step whose validation fails
**When** the user clicks Next
**Then** the Wizard stays on that Step and shows the validation message inline

**Given** Step 3
**When** the user clicks Back
**Then** Step 2 renders with its earlier values kept (state in the Payload, or by Ref through `overflow = spill` when it doesn't fit)

**Given** Cancel
**When** it is clicked
**Then** the message is edited to a cancelled state with no interactive components, and the response carries `RefOps` deleting the Wizard's Refs

**Given** a Step's Ref has expired
**When** the user clicks
**Then** the Expired Handler by default re-renders Step 1 with a notice (UJ-1)

**Given** a Step that collects input through a modal
**When** the modal is submitted
**Then** the Wizard returns to a message Step before any next modal, without author code

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Wizard, the template's setup Wizard is reconciled, and `skill-check` passes

### Story 8.6: Testing kit — simulated interactions and View snapshots

As a bot author,
I want to invoke Commands and click components in tests without Discord,
So that I can verify flows, response types and Limit costs in CI.

**Implements:** FR-51 (simulated interactions, snapshots), NFR-2; AD-10, AD-25; Conventions (Tests).

**Acceptance Criteria:**

**Given** `noctis::testkit` enabled through the facade's `testkit` feature in `[dev-dependencies]`
**When** a test builds `TestApp` from an `AppBuilder` with test guilds, channels, roles and users
**Then** it injects simulated `Inbound` interactions into the same lanes the gateway uses, with a mocked Discord HTTP layer, and no network access

**Given** a test
**When** it calls `app.command("setup start").by(user).in_guild(guild).run()` or clicks a component by Route or label
**Then** it can assert the resulting View, response kind (reply, update, deferred, modal), ephemerality and Limit cost

**Given** a View
**When** it is snapshot-tested with `insta`
**Then** the snapshot is its rendered JSON and changes only when that JSON changes

**Given** `assert_memory_flat`
**When** it runs N interactions against a handler
**Then** it fails if framework memory grows with N (NFR-2)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's testing reference documents `TestApp` and assertions, the template's `tests/flows.rs` is reconciled with the real testkit API, and `skill-check` passes

### Story 8.7: UI Doctrine example and the UI quality rulebook

As a coding agent learning Noctis from the Usage Skill,
I want a compiling example of the doctrine patterns and a rulebook that tells me which surface to use,
So that my first bot follows the UI Doctrine and compiles on the first attempt.

**Implements:** FR-49 (P0 demonstrations), FR-50, UX-DR1..UX-DR9, SM-3 groundwork; AD-17, AD-27.

**Acceptance Criteria:**

**Given** `examples/views/` (a small focused example built in CI)
**When** it builds and its testkit tests run (`cargo test --release`)
**Then** it demonstrates an option-less entry Command opening a View, a 3-Step Wizard, and a Navbar View with a list that overflows into pages, using only Payloads and Refs (no collectors or inline listeners)

**Given** the Usage Skill
**When** the story is done
**Then** it contains the UI quality rulebook (one primary action per View, visible active state, confirmation for destructive actions via a one-shot Route, ephemeral for single-user UI and public for shared content, a way back or cancel on every interactive View, designed empty and error states), the decision table for Wizard vs Navbar vs Paginator (UD-4..UD-6), and patterns/anti-patterns, with no example using a per-message listener (UD-7)

**Given** `xtask skill-check`
**When** it runs
**Then** its doctrine lint covers `examples/` as well as the template, and passes

## Epic 9: Attachments, webhooks and advanced component surfaces

A bot author attaches files to Views with automatic naming and edit diffs, sends Views
over webhooks, pre-fills modals, declares Route component defaults, and falls back to
raw JSON wherever twilight lags behind Discord.

### Story 9.1: Content-addressed attachments with edit diffs

As a bot author,
I want files attached to a View to be named, tracked and diffed automatically,
So that edits keep unchanged images without re-uploading and drop unused ones.

**Implements:** FR-30, UX-DR2; AD-22, AD-11; rules ATT-01, ATT-02, ATT-03, ATT-04, ATT-05, ATT-06, ATT-07, CV2-12, MSG-08.

**Acceptance Criteria:**

**Given** the content-addressed `Attachment` and `AttachmentSet` from Story 4.1
**When** a View with attachments is rendered and sent
**Then** it is `(MessageTree, AttachmentSet)` keyed by name, so one image used twice uploads once, and the multipart body is built by the Responder

**Given** an edit of a View
**When** the Responder computes the attachment diff
**Then** names already on the message are kept by id without re-upload, new ones are uploaded as `files[n]` matching `attachments[].id` (ATT-04), and names no longer referenced are dropped (MSG-08)

**Given** an attachment larger than the Upload Budget (the interaction's `attachment_size_limit`, else 20 MiB), a request over 25 MiB, or more than the configured file count
**When** the response is prepared
**Then** it is rejected with a typed error before any HTTP request (ATT-01, ATT-02, ATT-03)

**Given** an image placed in a Thumbnail or Media Gallery without alt text
**When** it is validated
**Then** Strict mode returns a validation error and Lenient mode a warning; alt text over 1024 is rejected (ATT-06)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents attachments and alt text, and `skill-check` passes

### Story 9.2: Raw escape hatch for components and REST

As a bot author,
I want to include raw component JSON and send raw REST requests for things twilight doesn't model,
So that I'm never blocked by an upstream gap.

**Implements:** FR-31, FR-52 (raw requests honor proxy); AD-12, AD-24; rules CV2-16, CV2-03.

**Acceptance Criteria:**

**Given** a raw component JSON node inside a View
**When** the message is validated
**Then** the raw node counts toward the component total (CV2-03) and is serialized verbatim

**Given** a received message or modal submit with an unknown component type
**When** it is dispatched
**Then** the handler still runs and the unknown part is available as raw JSON (CV2-16)

**Given** a View edit
**When** the original message contained components Noctis couldn't parse
**Then** the edit uses only the View's own rendered tree and never re-sends the unparsed component

**Given** `ctx.http().raw(..)`
**When** a raw REST request is sent
**Then** it goes through the App's single HTTP client and its rate limiting (or the configured proxy)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents the raw escape hatch with its stability caveats, and `skill-check` passes

### Story 9.3: Route component defaults with per-render overrides

As a bot author,
I want to declare a Route's default label, style, emoji, placeholder and value bounds once,
So that every component for that Route looks consistent and I override only what changes.

**Implements:** FR-59; AD-19; rules CV2-08, CV2-09.

**Acceptance Criteria:**

**Given** a Route declaring component defaults (associated const on `ComponentRoute`)
**When** a component is built with no overrides
**Then** it uses the defaults; literal defaults are checked at compile time (CV2-08, CV2-09 fixtures)

**Given** a render with overrides (for example `disabled` or a localized label)
**When** the component is built
**Then** the overrides apply to that render only, and the result is validated against the Limits after overrides apply

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Route defaults and overrides, and `skill-check` passes

### Story 9.4: Modal pre-fill

As a bot author,
I want to open a modal with values pre-filled from the entity being edited,
So that editing feels natural instead of starting from blank fields.

**Implements:** FR-63; AD-12; rules MOD-05, MOD-07, CV2-09.

**Acceptance Criteria:**

**Given** a `derive(Modal)` struct
**When** it is opened with `prefill(..)`
**Then** Text Inputs receive initial values and selects, Radio Groups and Checkbox Groups receive default selections

**Given** a pre-filled value that breaks a field's limit (Text Input `value` over 4000 or over its `max_length`, more defaults than `max_values`)
**When** the modal is prepared
**Then** a typed error is returned before sending

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's modal recipe shows pre-fill, the template's bio modal pre-fill is reconciled, and `skill-check` passes

### Story 9.5: Views over webhooks

As a bot author,
I want to send and edit Views through webhooks for log channels and personas,
So that webhook messages get the same V2 safety as interaction replies.

**Implements:** FR-65; AD-11, AD-12; rules WH-01, WH-02, WH-03, WH-04, WH-05, MSG-07.

**Acceptance Criteria:**

**Given** an application-owned webhook
**When** a View is executed or edited through it
**Then** the request always carries `with_components=true` (WH-01), only flags valid for that endpoint are sent (MSG-07), and legacy fields are nulled on edit (WH-03)

**Given** `ForeignWebhook::send`
**When** it is called
**Then** it accepts only a non-interactive `StaticView`, so interactive components through a non-owned webhook fail to compile (WH-02 trybuild fixture), and a dynamic attempt is a typed error

**Given** a forum or media channel webhook
**When** no `thread_id` or `thread_name` is given
**Then** a typed error is returned (WH-05); a webhook name outside 1–80 is rejected (WH-04)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents webhooks, and `skill-check` passes

## Epic 10: Generated images and GIFs

A bot author writes Tailwind-styled Templates in Rust, validated at compile time with
no Node or Tailwind CLI, and gets cached PNG, JPEG, WebP, animated WebP, GIF or APNG
output from a bounded pool, embedded in Views as Thumbnail or Media Gallery images.
Everything here sits behind the `image` facade feature (off by default).

### Story 10.1: Strict Tailwind v4 grammar and takumi cross-check

As a bot author,
I want Noctis to own a strict Tailwind v4 class grammar generated from the renderer's parser,
So that every class the macros accept is one the renderer actually draws.

**Implements:** FR-32, FR-66; AD-15, AD-28.

**Acceptance Criteria:**

**Given** `xtask tw-grammar`
**When** it runs against takumi `=2.17.0` source
**Then** it generates `noctis_image::tw` (takumi-free, builds with the `render` feature off) containing the class grammar, theme-token hooks and a v3 → v4 rename table (for example `bg-gradient-to-r` → `bg-linear-to-r`)

**Given** the generated grammar
**When** the CI job `tw-crosscheck` renders every grammar entry with takumi
**Then** it fails on any drift between the grammar and takumi's output

**Given** a runtime class string
**When** it is checked by `tw::validate`
**Then** unknown classes are reported with a v4 suggestion when the rename table knows one

### Story 10.2: Templates with compile-time class validation and a Theme

As a bot author,
I want to write image Templates in Rust with Tailwind classes that are checked by `cargo check`,
So that a typo in a class fails the build instead of rendering nothing.

**Implements:** FR-32, FR-66, FR-75, UX-DR2; AD-2, AD-15; rules ATT-07.

**Acceptance Criteria:**

**Given** `#[derive(Template)]` with `#[template(id, version, size, format)]` and a `view!`/`tw!` body
**When** it compiles
**Then** literal class strings are validated against `noctis_image::tw` and the tokens declared in the crate's `noctis-theme.css`; an unknown class is a compile error, and a known v3 name's error suggests its v4 replacement (trybuild fixtures under `tests/ui/tw/`)

**Given** user-supplied data
**When** it enters a Template
**Then** it can only be text content, never a class string or markup, and dynamic values (widths, colors, percentages) go through typed style setters

**Given** a class string built at runtime with an unknown class
**When** it renders
**Then** a warning is logged once per distinct string, and the same string fails the test under the testkit

**Given** Template data types
**When** a field is a `HashMap`
**Then** the derive rejects it (deterministic serialization for cache keys)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's Templates reference documents `derive(Template)`, `view!`, `tw!`, the Theme file and typed setters, the template's profile card Template is reconciled, and `skill-check` passes

### Story 10.3: Renderer and bounded render pool

As a bot author,
I want rendering to happen on a dedicated bounded pool and be awaited from my handler,
So that renders never block the gateway or the async runtime and slow renders are auto-deferred.

**Implements:** FR-34, FR-61 (with images), NFR-4, NFR-5, UX-DR2; AD-15, AD-22, AD-24, AD-29.

**Acceptance Criteria:**

**Given** the `image` feature
**When** the App is built
**Then** `noctis-image` implements the `Renderer` port with takumi `=2.17.0` on the process-scoped rayon pool owned by `noctis::Runtime`, never on tokio workers or a pool shared with the author

**Given** a handler calling `ctx.render(template, data).await`
**When** the render takes longer than the ack deadline
**Then** the FR-21 auto-defer covers it, and the returned image becomes an `Attachment` placed in a Thumbnail or Media Gallery
**And** without a Renderer configured, `ctx.render` returns a typed error

**Given** the render queue is full
**When** another job is submitted
**Then** it fails fast with a typed error and `noctis_render_rejected_total{reason}` increments; `noctis_render_queue_depth` and `noctis_render_duration_seconds{template}` are recorded under a `noctis.render` span

**Given** a Render Job that panics
**When** it runs under `catch_unwind` on the pool
**Then** the requester receives an error, the Render Error Sink is notified and the pool keeps running

**Given** default features
**When** a bot builds without `image`
**Then** no image dependency is compiled, and the `image` feature docs state its extra build requirements (C compiler for the WebP encoder)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `ctx.render`, attachments of renders and the feature flags, and `skill-check` passes

### Story 10.4: Output formats and size budget

As a bot author,
I want PNG, JPEG, WebP, animated WebP, GIF and APNG output that always fits the upload budget,
So that rendered images never fail to upload.

**Implements:** FR-33; AD-15, AD-22; rules ATT-01, ATT-07.

**Acceptance Criteria:**

**Given** a Render Job
**When** it requests a still format
**Then** PNG, JPEG and WebP are produced; animated output defaults to animated WebP, with GIF and APNG behind `image-gif` and configurable as the animated format

**Given** an output larger than the size budget (the smaller of the Upload Budget and a configurable target, default about 2 MB)
**When** the encoder finishes
**Then** settings degrade in the documented order — frame rate, size, quality, then optionally a still frame — and a typed error is returned if it still doesn't fit

**Given** a finished render
**When** it is returned
**Then** `RenderOutput.settings_used` reports the settings actually used

### Story 10.5: Per-job caps and admission control

As a bot operator,
I want each Render Job checked against caps before queueing and animations weighted in admission,
So that a burst of heavy animations can't starve stills or exhaust memory.

**Implements:** FR-70, NFR-1 (render memory); AD-15.

**Acceptance Criteria:**

**Given** per-job caps (defaults ≤ 4 MP per job, ≤ 1 MP per frame, ≤ 90 frames, ≤ 6 s)
**When** a job exceeds any cap
**Then** it fails with a typed error and never enters the queue

**Given** admission weighted by frame count
**When** many animated jobs and a still job are submitted
**Then** the still job is admitted without waiting for all animations (test)

**Given** a requester that gives up (interaction expired or future dropped)
**When** its job is still queued
**Then** the job is skipped; a running per-frame animation stops between frames

### Story 10.6: Animated Templates

As a bot author,
I want animations either from Tailwind/CSS keyframes in my Theme or from one Template state per frame,
So that I can make animated cards without a separate tool.

**Implements:** FR-69; AD-15.

**Acceptance Criteria:**

**Given** a Template using `animate-*` classes or `animate-[...]` with `@keyframes` from `noctis-theme.css`
**When** it renders to an animated format
**Then** the animation plays as declared

**Given** a code-driven animation (one Template state per frame)
**When** scenes differ in size
**Then** a typed error is returned

**Given** a frame rate above the format's cap (GIF 50, WebP/APNG 90)
**When** the job is created
**Then** a typed error is returned before rendering

**Given** a Theme that redefines a built-in animation name such as `spin`
**When** it is registered
**Then** registration fails with a typed error

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents both animation styles and caps, and `skill-check` passes

### Story 10.7: Fonts and static assets

As a bot author,
I want to register fonts, fallback fonts and static images once,
So that text always renders visibly in every script I support.

**Implements:** FR-35; AD-15, AD-24.

**Acceptance Criteria:**

**Given** no registered font and the `default-font` feature off
**When** the App starts with a Template registered
**Then** startup fails with a clear typed error; with `default-font` on, the bundled default font is used

**Given** fallback fonts registered with a priority order and Unicode ranges (for example CJK)
**When** a Template renders mixed-script text
**Then** glyphs come from the matching fallback without any Template change (pixel test)

**Given** fonts and the font registry
**When** a second App configures a conflicting font registry
**Then** it is a `BuildError`, because the registry is process-scoped

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents font and asset registration, and `skill-check` passes

### Story 10.8: Remote image fetching through the AssetFetcher

As a bot author,
I want avatars and other remote images fetched safely and cached before rendering,
So that Templates can show user avatars without letting data trigger arbitrary fetches.

**Implements:** FR-67, NFR-6; AD-15, AD-24, AD-1.

**Acceptance Criteria:**

**Given** the deferred choice of HTTP client for `AssetFetcher`
**When** the story starts
**Then** the chosen crate is recorded in the story's dev notes and pinned in `[workspace.dependencies]`, and `AssetFetcher` remains the only non-Discord HTTP in the workspace

**Given** a Template declaring its remote images
**When** a render is requested
**Then** all images are resolved before the render starts, fetch and decode run off the async runtime under a concurrency bound, and results are cached through `AssetStore`

**Given** a URL outside the host allow-list (default: Discord CDN)
**When** it is requested
**Then** it is not fetched and the fallback image is used with a log

**Given** a fetch that times out, exceeds the byte cap or doesn't decode
**When** the render runs
**Then** a configurable fallback image is used and the failure is logged; the render fails only if the Template marks that image required

**Given** avatar and guild icon helpers
**When** they build a cache key
**Then** it includes the user or guild id, asset hash and size, so a changed avatar invalidates naturally, and animated avatars animate in animated output

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents remote images and the allow-list, and `skill-check` passes

### Story 10.9: Emoji rendering

As a bot author,
I want emoji and Discord custom emoji in Template text to render as glyphs or images,
So that cards never show tofu boxes or raw `<:name:id>` markup.

**Implements:** FR-68; AD-15.

**Acceptance Criteria:**

**Given** the `image-emoji` feature (Twemoji COLR font)
**When** Template text contains Unicode emoji
**Then** they render as color glyphs (pixel test)

**Given** `image-emoji` is off and a Template is registered
**When** the App starts
**Then** a startup warning says emoji will render as boxes, and the Usage Skill documents this

**Given** text containing `<:name:id>` or `<a:name:id>`
**When** it renders
**Then** the custom emoji is fetched as an image through the AssetFetcher and laid out inline, never shown as raw text

### Story 10.10: Render cache and single-flight deduplication

As a bot operator,
I want identical renders served from cache and concurrent identical requests rendered once,
So that popular cards cost one render, and upgrades never serve stale images.

**Implements:** FR-36, NFR-1; AD-15, AD-22, AD-1.

**Acceptance Criteria:**

**Given** a render request
**When** its cache key is computed
**Then** it is `blake3(KEY_SCHEMA, NOCTIS_VERSION, TAKUMI_VERSION, template_id, template_version, theme_digest, postcard(data), sorted asset (id, version) pairs, size, format, encode_settings)`, so a change to any of them misses the cache

**Given** N concurrent identical requests
**When** they run
**Then** exactly one render happens (process-local single-flight) and all receive the result

**Given** a cached result
**When** it is attached
**Then** its attachment filename is derived from the cache-key digest (`Attachment::with_digest`)

### Story 10.11: Pixel snapshots and the reference card benchmark

As a bot author,
I want to pixel-snapshot my Templates in tests and know the render cost of a standard card,
So that renderer upgrades never change my images unnoticed.

**Implements:** FR-51 (pixel snapshots), FR-49 (Template in a Media Gallery), NFR-3 (card), NFR-8; AD-25, AD-28.

**Acceptance Criteria:**

**Given** the testkit with the `image` feature
**When** a test pixel-snapshots a Template
**Then** it compares against a stored PNG with a configurable tolerance and fails beyond it

**Given** the reference card Template (a copy of the Project Template's profile card, ≈934×282, PNG, warm caches, kept with the benches)
**When** the reference card bench in `noctis-core/benches` runs on `main`
**Then** it reports p50 and p99 against `budgets.toml` (targets p50 ≤ 10 ms, p99 ≤ 50 ms with the pool saturated), reporting only until PRD Q3 confirms the numbers

**Given** a pull request that bumps takumi
**When** CI runs
**Then** `tw-crosscheck` and the pixel snapshots of the reference card and `examples/` Templates are required to pass (NFR-8; the Project Template's own pixel snapshots join this gate in Story 14.2), and `main.yml` builds against the latest takumi and reports

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents pixel snapshots, the template's `tests/templates.rs` is reconciled, and `skill-check` passes

## Epic 11: Localization

A bot author ships Locale files that compile into typed string accessors, localizes
Commands word by word with Discord's localization maps, and every handler receives the
right User or Guild Locale. Built on `Locale`, `MessageKey` and `Translate` from
Epic 1 and the default `noctis.*` strings from Epic 6.

### Story 11.1: `i18n!` typed content strings

As a bot author,
I want my Locale files compiled into typed functions with checked arguments,
So that a missing key or wrong placeholder fails the build instead of showing a raw key to users.

**Implements:** FR-42, FR-75; AD-16; rules LOC-01, LOC-04.

**Acceptance Criteria:**

**Given** `noctis::i18n!(default = "en", dir = "locales")` and YAML files named by Discord locale code
**When** the crate compiles
**Then** it generates `Strings::of(locale)` with nested accessors mirroring the YAML tree (one method per leaf, arguments in placeholder order, named or positional), a `MessageKey` const per key in `keys::*`, and a `catalog()` descriptor

**Given** trybuild fixtures under `tests/ui/loc/`
**When** the UI tests run
**Then** a key missing from the default locale, a call with the wrong number or type of arguments, and a locale file named with an unknown code (LOC-01) fail to compile

**Given** a key missing in a non-default locale
**When** the crate compiles
**Then** a build warning is emitted and the default value is used at runtime (LOC-04)

**Given** a string with the same placeholder twice
**When** it is formatted
**Then** every occurrence is replaced

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's i18n reference documents `i18n!`, the template's `strings.rs` and `locales/{en,tr}.yaml` are reconciled, and `skill-check` passes

### Story 11.2: Module-scoped Locale catalogs

As a bot author composing Modules,
I want each Module to ship its own Locale files merged with the App's,
So that Modules are self-contained and key clashes are caught before connecting.

**Implements:** FR-72, FR-75; AD-7, AD-16.

**Acceptance Criteria:**

**Given** two Modules each providing a catalog descriptor
**When** `App::build` merges them per language
**Then** the merged catalog serves both Modules' keys

**Given** a key defined by two Modules
**When** `App::build` runs
**Then** it fails with a `BuildError` naming the key and both Modules (unit test)

**Given** a Module disabled at runtime
**When** another Module references its strings
**Then** those strings remain available

### Story 11.3: User and Guild Locale resolution

As a bot author,
I want every handler to know the User Locale and Guild Locale with sensible fallbacks, and to override them,
So that each message appears in the language its audience expects.

**Implements:** FR-43, FR-20 (override in Middleware); AD-16, AD-23.

**Acceptance Criteria:**

**Given** an interaction with `locale = "pt-BR"` and no `pt-BR` file, or a `tr` user in an `en-US` guild
**When** stage 4 resolves locales
**Then** interactions resolve User → Guild → default, with a regional code falling back to its base language before the default, and handlers read both via `ctx.user_locale()` and `ctx.guild_locale()`

**Given** a guild event
**When** an Event Handler runs
**Then** it resolves Guild → default

**Given** an App `LocaleResolver` or a before-Middleware overriding the locale (for example a per-guild setting)
**When** the handler runs
**Then** it sees the overridden locale

**Given** the Usage Skill
**When** the story is done
**Then** it documents the rule "User Locale for ephemeral single-user UI, Guild Locale for shared content" and the resolution order, and `skill-check` passes

### Story 11.4: Command localization in Publish

As a bot author,
I want Command names translated word by word and descriptions, options and choices localized from my Locale files,
So that users see commands in their own language.

**Implements:** FR-41, FR-12 (whole-name context menus), FR-13; AD-16; rules LOC-02, LOC-03, LOC-05, LOC-06, CMD-01..CMD-05, CTX-01, OPT-09.

**Acceptance Criteria:**

**Given** keys `cmd.<path>.name`, `cmd.<path>.desc`, `cmd.<path>.opt.<opt>.name|desc|choice.<value>` and per-word `cmdword.<word>`
**When** Publish builds JSON from the merged catalog
**Then** it emits Discord's `name_localizations`/`description_localizations`, a full-path key overrides the per-word translation, and context-menu names localize as whole names

**Given** an `en` file and no `en-GB` file
**When** Publish runs
**Then** `en` values are emitted for both `en-US` and `en-GB`; a variant file overrides its base

**Given** a localized name breaking CMD-01..03, a description breaking CMD-04, a choice name breaking OPT-09, or a localized option name colliding (LOC-03)
**When** `App::build` validates the merged catalog
**Then** it fails with a typed error naming the Locale, key and rule id; the CMD-05 budget counts the longest variant per field (LOC-05)

**Given** a localized Command invocation
**When** it is dispatched
**Then** routing uses default names only (LOC-06)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents command localization keys, and `skill-check` passes

### Story 11.5: Runtime Locale overlays through LocaleSource

As a bot operator,
I want to load or fix content translations at runtime from my own source,
So that translators can ship text without a redeploy, without ever changing published command names.

**Implements:** FR-42, FR-38 (LocaleSource); AD-16, AD-1.

**Acceptance Criteria:**

**Given** a `LocaleSource` implementation returning overlay values
**When** the App loads it
**Then** overlays apply to content keys only; overlaying any `cmd.*` or `cmdword.*` key is rejected with a typed error

**Given** an overlay value whose placeholders differ from the compiled placeholder set
**When** it is loaded
**Then** it is rejected with a typed error naming the key

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents overlays, and `skill-check` passes

## Epic 12: Clusters and scale

A bot author scales from one process to a Cluster by configuration only: Shard Ranges,
Cluster-wide identify, session resume across restarts, resumable shutdown, a shared
HTTP proxy and a configurable Discord cache, with memory per guild measured in CI.
Handler code never changes (UJ-3).

### Story 12.1: Shard Ranges and Cluster-wide identify

As a bot operator,
I want each process to run one Shard Range while identifies are serialized per bucket across the whole Cluster,
So that I can split a large bot across processes without identify storms.

**Implements:** FR-3, FR-13 (designated publisher); AD-21, AD-14; rules GW-01.

**Acceptance Criteria:**

**Given** a configured Shard Range (for example shards 32–63 of 128) via the builder or `NOCTIS_SHARD_RANGE`
**When** the process starts
**Then** it runs only the shards in its range; a range not aligned to `max_concurrency` is accepted and logs a warning

**Given** two in-process Apps standing in for two processes and sharing one `IdentifyQueue` implementation
**When** both boot their ranges
**Then** per-bucket identify concurrency holds across both (one identify per bucket per 5 s, paused-clock test)

**Given** a Cluster configuration
**When** exactly one process sets `publisher = true`
**Then** only that process runs Publish on start; the others never publish

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's app-and-sharding reference documents Shard Ranges and the publisher flag, and `skill-check` passes

### Story 12.2: Session resume through the SessionStore

As a bot operator,
I want restarted processes to resume their shards' sessions,
So that restarts within Discord's resume window don't spend identifies or miss events.

**Implements:** FR-4; AD-21, AD-14; rules GW-02.

**Acceptance Criteria:**

**Given** a SessionStore holding `(session_id, seq, resume_url)` for a shard
**When** the process restarts within the resume window (gateway stub)
**Then** the shard resumes instead of identifying, and the identify budget is not consumed (GW-02)

**Given** a stored session that Discord rejects (invalid session)
**When** the shard starts
**Then** it falls back to a fresh identify with no operator action and clears the stale entry

**Given** the gateway
**When** sessions are read or written
**Then** `noctis-gateway` is the only reader and writer of the SessionStore

### Story 12.3: Resumable shutdown

As a bot operator,
I want a graceful shutdown to leave every shard resumable, with an explicit fresh mode,
So that rolling deploys resume and full resets are still possible.

**Implements:** FR-55, FR-54 (shutdown order); AD-21.

**Acceptance Criteria:**

**Given** a graceful shutdown in default mode
**When** shards close
**Then** each closes with `CloseFrame::RESUME`, and after observing `GatewayClose` its session id, sequence (last received) and resume URL are written to the SessionStore before the process exits

**Given** `--no-resume`
**When** the process shuts down or starts
**Then** shards close fresh and the stored sessions are cleared

**Given** events dropped under back-pressure before shutdown
**When** the session resumes
**Then** they are not replayed (documented at-most-once semantics)

### Story 12.4: Shared HTTP rate limiting through a proxy

As a bot operator running a Cluster,
I want every Discord HTTP request to go through a shared rate-limit proxy,
So that processes sharing one token never exceed Discord's per-token limits.

**Implements:** FR-52; AD-24; rules RL-01, RL-02, RL-03, RL-04, INT-13.

**Acceptance Criteria:**

**Given** a proxy URL configured (builder or `NOCTIS_HTTP_PROXY`)
**When** the App's single HTTP client is built
**Then** it uses `proxy(url, true)` and `ratelimiter(None)`, and a test with a mock proxy shows no request bypasses it (including raw REST and webhook requests)
**And** 429 responses with `X-RateLimit-Scope: shared` are surfaced as typed errors and counted, never hard-coded as limits (RL-04), and 401/403/429 counts feed a metric toward the invalid-request limit (RL-03)

**Given** a Cluster configuration (a Shard Range smaller than the total) without a proxy
**When** the App starts
**Then** a warning names the risk of exceeding the per-token global limit (RL-02)

**Given** the HTTP client
**When** a request timeout is configured
**Then** it applies to every request (default 10 s)

**Given** the Project Template
**When** the story is done
**Then** its `.env.example` and README document proxy mode as the Cluster setup, and the Usage Skill documents it, with `skill-check` passing

### Story 12.5: Discord cache configuration and minimal intents

As a bot author,
I want to choose which Discord resources are cached, back the cache with my own storage, or disable it,
So that memory stays proportional to what my features actually use.

**Implements:** FR-40, FR-38 (Discord cache backend), FR-6 (intents); AD-1, AD-21, AD-25; rules GW-05.

**Acceptance Criteria:**

**Given** default configuration
**When** the App is built
**Then** only resources needed by enabled framework features are cached, and the default intents are the minimum required by registered Event Handlers and cache resources (`Intents::GUILDS` for an interaction-only bot)

**Given** a cache resource or handler needing an intent that isn't enabled
**When** `App::build` runs
**Then** it fails with a typed error naming the resource and intent (GW-05)

**Given** the cache API
**When** a handler reads it
**Then** it returns owned or shared values; no guard that could be held across `.await` is exposed (clippy lint test or compile-fail fixture)

**Given** a Discord cache backed by an out-of-process implementation of the cache port
**When** two in-process Apps standing in for Cluster processes read it
**Then** they see the same data

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents cache configuration, and `skill-check` passes

### Story 12.6: Memory-per-guild benchmark

As a bot operator,
I want framework memory per idle guild measured in CI with the Discord and render caches reported separately,
So that I can size deployments and regressions are visible.

**Implements:** NFR-1, UX-DR8; AD-25, AD-28.

**Acceptance Criteria:**

**Given** the memory bench in `noctis-core/benches` with a stated default cache configuration (no members, no messages)
**When** it loads 10k idle guilds through the gateway stub
**Then** it reports framework bytes per guild, Discord cache bytes and Renderer cache bytes separately against `budgets.toml` (initial goal ≤ 1 KiB per idle guild excluding the Discord cache)

**Given** per-guild framework state
**When** it is inspected
**Then** it is a fixed-size record (Module bitset, locale) and no framework map is keyed unboundedly by guild, user, channel, message or interaction

**Given** `main.yml`
**When** it runs
**Then** the bench reports on every push to `main` (gating is enabled in Story 14.3)

## Epic 13: Module operations

A bot author composes Module trees, selects features at boot, enables or disables
Modules at runtime globally or per guild across a Cluster, rate-limits Commands and
Routes, and observes the App's lifecycle through hooks.

### Story 13.1: Module trees with unique path ids

As a bot author,
I want to nest Modules into a tree with unique path-like ids,
So that features are organized like tescord Packs and every log line says which Module it came from.

**Implements:** FR-44, NFR-5; AD-7, AD-26, AD-29; Conventions (Module ids).

**Acceptance Criteria:**

**Given** `Module::new("admin").module(Module::new("admin/logs"))`
**When** the tree is mounted on the App
**Then** registrations of every nested Module are folded into the Registry, and parent-before-child ordering applies to Middleware

**Given** two Modules with the same id
**When** `App::build` runs
**Then** it fails with a typed error naming the id

**Given** any registration
**When** it logs, records a metric or returns an error
**Then** the `module` span field and error context carry its Module id (metric labels carry it only where AD-29 allows)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Module trees, and `skill-check` passes

### Story 13.2: Boot-time feature flags

As a bot operator,
I want to select which Modules and registrations exist in a run via Boot Flags,
So that one binary can run different feature sets without code changes.

**Implements:** FR-73, FR-13; AD-7.

**Acceptance Criteria:**

**Given** Modules and individual registrations carrying Boot Flags
**When** the App starts with `--features a,b` (or `all`, or the equivalent config)
**Then** registrations whose flags aren't selected are filtered out of the descriptors before the Registry fold, so they are not registered, dispatched or published

**Given** `--publish --dry-run --features a`
**When** it runs
**Then** the printed JSON and diff reflect only the selected flags

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents Boot Flags, and `skill-check` passes

### Story 13.3: Runtime Module enable and disable

As a bot operator,
I want to enable or disable a Module at runtime, globally or for one guild, across every process,
So that I can switch features off without a deploy.

**Implements:** FR-45, FR-13 (per-guild Publish), FR-38 (ModuleStateStore); AD-7, AD-14, AD-23; rules REG-02.

**Acceptance Criteria:**

**Given** `ctx.modules().set_enabled(module, guild, enabled)` writing through the `ModuleStateStore`
**When** another process sharing the store polls its `version()` (default every 5 s)
**Then** it refreshes its per-Module bitset snapshot (`arc-swap`), and the change takes effect within the documented propagation delay equal to the poll interval

**Given** a Module disabled in a guild
**When** a user invokes one of its global Commands there
**Then** stage 3 replies with a localized ephemeral `noctis.module.disabled` notice and the handler doesn't run; its components and Event Handlers don't run either

**Given** per-guild Publish
**When** a Module is disabled for a guild
**Then** only its guild-scoped Commands are left out of that guild's Publish (re-counted against REG-02); global Commands stay published

**Given** `ctx.modules().is_enabled(module, guild)`
**When** a handler reads it
**Then** it answers from the cached snapshot without port I/O

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents runtime enable/disable, the template dashboard's Module toggles are reconciled, and `skill-check` passes

### Story 13.4: Rate limits on Commands and Routes

As a bot author,
I want to declare "N uses per window" limits per user, member, channel, guild or message,
So that abuse is throttled before my handler runs, consistently across a Cluster.

**Implements:** FR-46; AD-14, AD-23.

**Acceptance Criteria:**

**Given** `#[command("profile", rate_limit(user, 3, per = "10s"))]` (or on a Route)
**When** a user invokes it a fourth time within 10 s
**Then** stage 5 increments through `RateLimitStore::incr_with_ttl` before any auto-defer and the handler, and the user gets an immediate localized ephemeral notice including the time remaining

**Given** scopes user, member, channel, guild and message
**When** limits are keyed
**Then** each scope uses its own key and the store is App-namespaced

**Given** a handler that sets or extends a limit dynamically (for example only after a successful action)
**When** it calls the limit API
**Then** the next invocation sees the updated limit

**Given** a custom rate-limited response configured per App, Command or Route
**When** a limit is hit
**Then** the most specific override runs instead of the default notice

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents rate limits, the template's `/profile` rate limit is reconciled, and `skill-check` passes

### Story 13.5: Lifecycle Hooks

As a bot author,
I want hooks for App start, shard state, Publish, Module toggles and shutdown,
So that I can react to framework events (alerts, warm-ups, audits) without polling.

**Implements:** FR-54, FR-1 (fatal close as a hook), NFR-5; AD-21, AD-7, AD-13.

**Acceptance Criteria:**

**Given** `#[hook]` registrations on a Module
**When** a test App starts, publishes and shuts down
**Then** it observes, in order and exactly once each: App started; Shard ready (and resumed or disconnected where simulated); all Shards ready; Publish completed or failed with the diff; shutdown started — each with typed data

**Given** a Module enabled or disabled (global or per guild)
**When** the change is applied
**Then** the Module enabled/disabled hook fires once with the scope

**Given** a shard closing fatally
**When** core receives the `ShardEvent` from the gateway
**Then** the fatally-closed hook fires (gateway never fires hooks itself)

**Given** a hook that returns an error or panics
**When** it runs
**Then** the error goes to an Error Sink and the App keeps running

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents every hook and its data, and `skill-check` passes

## Epic 14: v1 launch readiness

Third-party adapters can prove correctness, benchmarks guard memory, latency and
allocation, the Project Template and Usage Skill are complete and gated, the rules
catalog has no pending rows, the launch documents exist, and `noctis` 0.1.0 is
published to crates.io (PRD §8.3 launch criteria).

### Story 14.1: Adapter conformance suites

As an author of a Redis or SQL adapter,
I want a reusable conformance suite per port,
So that I can prove my implementation has the semantics Noctis relies on.

**Implements:** FR-39, FR-71; AD-14.

**Acceptance Criteria:**

**Given** `noctis::testkit::conformance::<port>` suites for every store port and `IdentifyQueue`
**When** an implementation runs its suite
**Then** it is tested for TTL, overwrite, delete, missing key, atomicity under concurrent callers (exactly one `take`/`put_if_absent` winner, no lost increments), `version()` monotonicity where applicable, and App namespacing through `Namespaced<P>`

**Given** each in-memory default
**When** CI runs
**Then** it passes its own suite

**Given** deliberately broken implementations (one ignoring TTL, one with a non-atomic `take`)
**When** they run the suite
**Then** the suite fails them (meta-tests)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents running the suites, the template's `tests/adapters.rs` is reconciled to run the suite against `BoundedCacheStore` (it runs as a gate from Story 14.2), and `skill-check` passes

### Story 14.2: Project Template builds, runs and gates CI

As a bot author starting a new bot,
I want the Project Template to build, pass its tests and follow the UI Doctrine,
So that copying it gives me a working, idiomatic bot.

**Implements:** FR-49, FR-61, UX-DR1..UX-DR9, NFR-8, SM-1; AD-17, AD-27.

**Acceptance Criteria:**

**Given** `examples/project-template/`
**When** CI runs `cargo build --release`, `cargo clippy --release --all-targets -- -D warnings` and `cargo test --release` in it against the workspace
**Then** all pass, covering: an option-less entry Command opening a View (`/dashboard`), a Wizard (`/setup start`), a Navbar View with an overflowing list, a Template rendered into a Media Gallery (`/profile`), a custom Adapter (`BoundedCacheStore`) and `en`/`tr` Locale files
**And** the `template-build` job switches from tracking mode to a required gate, the template's Template pixel snapshots and Adapter conformance test join the required gates (NFR-8), and the "written against the planned API" notes are removed from the template sources

**Given** the template
**When** `xtask skill-check` lints it
**Then** no handler responds with an embed or plain `content` as primary output, there is no manual response-type selection, and no per-message collector, await-component loop or inline listener exists

**Given** the template's user code
**When** its lines are counted (excluding tests, locales and the theme)
**Then** the count for sharding, auto-published Commands, the 3-Step Wizard and the rendered card is reported against the SM-1 goal of ≤ 150 lines

### Story 14.3: Dispatch latency, allocation and memory gates

As a bot operator,
I want Noctis's performance budgets measured and enforced in CI,
So that regressions in latency, allocation or memory are caught before release.

**Implements:** NFR-1, NFR-3, NFR-9, SM-4; AD-25, AD-28.

**Acceptance Criteria:**

**Given** the NFR-3 latency bench in `noctis-core/benches`
**When** it runs at 10k events/s on 4 cores with the AD-25 mix (60% `MESSAGE_CREATE` with no handler, 25% handled guild events, 10% component interactions with inline Payloads, 5% command interactions, no-op handlers)
**Then** it reports p99 event-arrival-to-handler-start against `budgets.toml` (target ≤ 1 ms)

**Given** the dhat allocation test
**When** it runs over the dispatch path
**Then** Route lookup and fixed-width inline decode allocate nothing, and the documented per-interaction budget (at most one port call for the Payload and one boxed handler future) is asserted

**Given** the first benchmark results (PRD Q3)
**When** the maintainers record the confirmed or re-set targets in `budgets.toml`
**Then** the latency, memory (Story 12.6) and allocation jobs switch from reporting to gating on `main`, and the PRD assumptions for NFR-1/3/9 are noted as confirmed in the changelog

### Story 14.4: Rules catalog completeness audit

As a Noctis maintainer,
I want every catalog rule enforced and tested at its declared point before release,
So that the NFR-10 promise "static input is always checked at compile time" holds for v1.

**Implements:** FR-75, FR-79, NFR-10; AD-3, AD-4.

**Acceptance Criteria:**

**Given** `xtask/rules-pending.toml`
**When** this story is done
**Then** it is empty (P2-only rows such as prefix-command rules may remain only if tagged `p2`), and `xtask rules-catalog` passes with every Compile-time row covered by a trybuild fixture and every App-build or Runtime row by a unit test citing its id

**Given** the audit
**When** each rule with static input is reviewed
**Then** none is checked only at runtime; any found is moved to compile time or recorded in the catalog with the reason its input is dynamic
**And** documentation-only rows are confirmed as such in the catalog: the discovery restriction of CMD-07, REG-06 (out of scope for v1) and CMD-14/LOC-04 (permitted, no check)

**Given** the catalog's Unconfirmed values table
**When** PRD open questions Q1, Q4 and Q5 have answers
**Then** the corresponding `Limits` defaults and catalog rows are updated; otherwise the conservative defaults remain documented

### Story 14.5: Usage Skill completeness and agent trial

As a coding agent given only the Project Template,
I want the Usage Skill to cover the whole public API with working examples,
So that I can write a leaderboard feature that compiles without reading Noctis source.

**Implements:** FR-50, FR-43 (documented rule), UX-DR9, SM-3; AD-17, AD-26.

**Acceptance Criteria:**

**Given** the Usage Skill
**When** `skill-check` runs
**Then** every public item is mentioned, every FR-23/FR-29 component has an entry (where it may appear, its Limits, its builder), the Limits entries link `docs/discord-rules.md`, and the `[planned]` markers and `v0` status are removed

**Given** the skill's references
**When** they are reviewed
**Then** they include the tescord → Noctis concept map, a limits cheatsheet and a troubleshooting page indexed by rule id, patterns and anti-patterns, and common mistakes, with no per-message listener in any example (UD-7)

**Given** the UJ-5 task ("add a leaderboard: a Template, a View and a Command")
**When** a coding agent with only the Project Template directory runs it 5 times
**Then** the result compiles without the agent reading Noctis source in at least 4 of 5 trials, and the trial transcript summary is stored under `_bmad-output/implementation-artifacts/`

### Story 14.6: Launch documentation

As a bot author evaluating Noctis,
I want complete API docs, a migration concept map, a Cluster deployment guide and a changelog,
So that I can adopt Noctis from tescord, dbi, serenity or poise and deploy it at scale.

**Implements:** FR-74, FR-52 (Cluster guide), FR-3; AD-28, AD-26.

**Acceptance Criteria:**

**Given** the `noctis` crate
**When** CI builds docs
**Then** `missing_docs` is denied, every public item has rustdoc, runnable examples are doctests, and sub-crate docs say "internal; use `noctis`"

**Given** `docs/guides/`
**When** the story is done
**Then** it contains the tescord/dbi → Noctis concept map (Pack → Module, `buildComponent` → typed Payload, `locale.user` → User Locale, …) with a short serenity/poise → Noctis section, and a Cluster deployment guide (Shard Ranges, IdentifyQueue, HTTP proxy, shared Adapters, `NOCTIS_PAYLOAD_KEY`, single publisher)
**And** `CHANGELOG.md` has migration notes, and the crate README links every guide (a CI link check verifies them)

### Story 14.7: Release `noctis` 0.1.0

As a bot author,
I want `noctis` 0.1.0 on crates.io with all sub-crates published consistently,
So that I can depend on it from my own bot.

**Implements:** FR-48, NFR-7, NFR-8; AD-2, AD-28.

**Acceptance Criteria:**

**Given** the facade features
**When** CI runs the final feature matrix
**Then** each documented feature alone and all features together build and test (`image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `testkit`; `prefix` once Epic 15 lands), default features build without the image stack, and `cargo-semver-checks` runs on `noctis`

**Given** release-plz on `main`
**When** the release PR is merged
**Then** all workspace crates are published at the shared version in layer order from the protected release environment holding the crates.io token, and the changelog entry is included

**Given** the published crate
**When** the Project Template's `noctis` dependency is switched to `version = "0.1"`
**Then** the copied-out template builds with `cargo build --release`, and the README roadmap marks Milestones 1–4 done

## Epic 15: Opt-in extras

A bot author can opt into a remote-capable Renderer seam, several Apps per process for
white-label bots, presence and gateway commands, and prefix commands that reuse slash
handlers. Each ships behind a feature or as an extension and may slip without blocking
v1 (PRD §8.1, P2).

### Story 15.1: Renderer seam

As a bot author,
I want to replace local rendering with my own Renderer implementation,
So that I can move rendering to a separate service later without touching Templates, Views or handlers.

**Implements:** FR-37; AD-22, AD-1.

**Acceptance Criteria:**

**Given** the `Renderer` port from Epic 3
**When** an App is configured with `.renderer(MyRenderer)`
**Then** `ctx.render` routes Render Jobs to it with no change to Templates, Views or handlers (test Renderer returning fixed bytes)

**Given** a custom Renderer
**When** it returns `RenderOutput`
**Then** the cache key, attachment naming and size budget logic still apply

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents the seam and its stability (renderer types only via `noctis::image::unstable`), and `skill-check` passes

### Story 15.2: Multiple Apps per process

As a bot operator running white-label bots,
I want several Apps with different tokens in one process, sharing Adapters where I choose,
So that customer bots and the main bot run on one deployment without interfering.

**Implements:** FR-47, FR-71, FR-13 (per-App Publish); AD-24, AD-14, AD-23, AD-1.

**Acceptance Criteria:**

**Given** two Apps attached to one `noctis::Runtime`
**When** both run
**Then** each has its own HTTP client, shards, Registry, Responder, `Limits` and namespaced ports, while process-scoped resources (render pool, fonts, metrics recorder) are shared; metrics carry the `app` label

**Given** the same Module tree
**When** it is mounted on both Apps
**Then** both work, and a Command restricted to a subset of Apps is published only for those Apps

**Given** a `GuildAccessList` allow-list on App A and deny-list on App B
**When** an interaction from the customer guild arrives on each
**Then** App A serves it and App B skips it at stage 3

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents multiple Apps, and `skill-check` passes

### Story 15.3: Presence and gateway commands

As a bot author,
I want to set presence and send gateway commands such as request guild members,
So that my bot shows a status and can fetch members on demand.

**Implements:** FR-53; AD-21; rules GW-04, GW-06, GW-07.

**Acceptance Criteria:**

**Given** an initial presence in the builder
**When** shards identify
**Then** they use it; `app.presence().set(..)` updates all shards or one shard at runtime

**Given** a gateway command for a guild
**When** it is sent
**Then** it goes to the owning shard's sender; a guild outside this process's Shard Range returns a typed error

**Given** the gateway send limit (120 per 60 s per shard, GW-04) or `Event::RateLimited` for request guild members (GW-07)
**When** it is hit
**Then** the caller receives a typed error, never a silent drop
**And** `LargeThreshold::new::<N>()` outside 50–250 fails to compile (GW-06 fixture)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents presence and gateway commands, and `skill-check` passes

### Story 15.4: Prefix commands

As a bot author migrating from dbi,
I want to expose a slash Command as a prefix message command that runs the same handler,
So that users who type `!command` keep working during migration.

**Implements:** FR-14, FR-10 (same typed errors); AD-23; rules GW-05.

**Acceptance Criteria:**

**Given** the `prefix` feature off (default)
**When** a bot builds
**Then** no prefix parsing code is compiled and the App needs no message-content intent

**Given** the `prefix` feature on and a Command opted into prefix mode
**When** a message `<prefix><Command Path> <args>` arrives
**Then** `noctis-prefix` runs pipeline stages 3 to 10 and the same handler through the shared response abstraction, and argument parse failures produce the same typed errors as FR-10
**And** the message-content intent is required at App build when prefix Commands exist (GW-05)

**Given** the final feature matrix
**When** CI runs
**Then** `prefix` alone and with all features builds and tests

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents prefix commands as an opt-in P2 feature, and `skill-check` passes
