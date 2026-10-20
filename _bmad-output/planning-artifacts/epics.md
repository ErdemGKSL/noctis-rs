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
  written (96 stories in 15 epics; Story 5.9 was added later from the warm-transport research) and [C] Continue was chosen (no Advanced
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
- **Correct course, 2026-10-20 (maintainer decision, binding):** Noctis drops its UI
  widget abstractions: no Navbar, Wizard or Paginator, no framework-managed widget
  state, no view envelope in `custom_id`, no automatic re-render and no automatic
  pagination (PRD FR-26 to FR-28 withdrawn, FR-25/FR-61/FR-62 revised; AD-20
  withdrawn, AD-5/AD-6/AD-11/AD-27 amended). Views are plain author-written render
  functions, as in the maintainer's TypeScript bot. Story 6.10 (appended as the last
  story of Epic 6) removes the already-shipped envelope code; Epic 8 was rewritten as
  "Views as plain functions, Responder ergonomics and the testing kit" (5 stories
  instead of 7), so the plan now has 95 stories. Later stories assume the simplified
  codec.

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
- **Dependencies:** no story depends on a later story in its own epic; the
  cross-epic mentions of later stories (1.7 → 2.2, 1.8 → 4.1, 2.5 → 7.5/7.7/7.8,
  2.6 → 7.1, 4.3 → 9.1, 5.2 → 12.4, 5.6 → 12.5/13.1, 5.8 → 13.5, 5.9 → 6.5/6.6/12.4, 6.1 → 11.4, 6.5 → 15.4,
  6.9 → 12.3/13.2, 12.6 → 14.3)
  are "later extended by" notes, not prerequisites. Each epic uses only
  earlier epics' outputs. The readiness fixes (`implementation-readiness.md`,
  Resolution) removed three hidden ones: basic Module nesting moved from Story 13.1
  into Story 5.6 (Story 7.6 needs it), Epic 4 tests implement `Handled` through the
  hidden `__private` path the handler macro uses (Story 4.1), and Story 5.1 no longer
  mounts Modules before Story 5.6 defines them. Gate 2 split the old Story 5.1 into
  Stories 5.1 to 5.3 (the old 5.2 to 5.6 became 5.4 to 5.8) and added the
  maintainer-gated Stories 14.8 and 14.9; no story depends on a later one.
- **File churn:** `noctis-core` is touched by Epics 5–9, 12 and 13. Consolidation was
  considered and rejected: each epic owns a distinct core subsystem (lanes and
  workers; command resolution and Responder; Route dispatch and Refs; Responder
  ergonomics and the testing kit; attachments; Cluster configuration; Module state), follows the crate
  layer order the architecture mandates, and is a feedback point (P0 → P1 → P2).

### Story conventions (apply to every story)

- **Implements** lines name the FR/NFR ids, architecture decisions (AD-n) and Discord
  rule ids (`docs/discord-rules.md`) a story delivers. Tests cite the same rule ids.
- **Release builds only:** every cargo command in a story, in CI and in local
  verification uses `--release` (`cargo build|test|clippy|run --release`); no story
  creates a debug build. trybuild never passes `--release`, so the repo-root
  `.cargo/config.toml` (Story 1.1, AD-28) makes the dev profile release-equivalent
  for every cargo invocation in the repo; no story weakens it.
- **Done means** `cargo fmt --check`, `cargo clippy --release --all-targets --all-features -- -D warnings`
  and `cargo test --release` pass locally (plus `cargo hack test --release
  --each-feature -p <crate>` for every crate whose feature-gated code the story
  touches), the commit is pushed to `main`, and the CI run on that pushed commit is
  green: every CI gate that exists at that point (AD-28; `ci.yml` runs on every push
  to `main` and on every pull request).
- **Compile-time rules** get compile-fail tests (run on the toolchain pinned in
  `rust-toolchain.toml`, locally and in CI). trybuild checks fixtures with
  `cargo check`, so every compile-time rule must fire under `cargo check` (AD-4:
  proc macros, trait bounds with `#[diagnostic::on_unimplemented]`, free `const _`
  items, or construction).
  - A rule Noctis diagnoses gets a trybuild fixture at
    `tests/ui/<area>/<RULE-ID>_<slug>.rs` whose `.stderr` contains `[<RULE-ID>]`.
  - A rule the catalog marks `Compile-time (by construction)` gets a `compile_fail`
    doctest or a trybuild fixture whose source carries `// [<RULE-ID>]` on the
    offending line; its `.stderr` is rustc's own error.
  - Fixtures that need a facade feature live in `tests/ui-<feature>/` behind
    `#[cfg(feature = "<feature>")]` and are blessed in the `cargo hack` run of
    exactly that feature; `tests/ui/` is blessed with default features.
  - Boundary passes are ordinary integration tests in `tests/boundary/`. App-build
    and runtime rules get unit tests that assert the typed error variant and name
    the rule id.
- **Maintainer-gated stories** need an action only the maintainer can take
  (crates.io token or publish, repository settings, merging the release PR). They
  carry `[Maintainer-gated]` in their title, sit last in Epic 14 (Stories 14.8 and
  14.9), and nothing else depends on them; an unattended run skips them, continues
  with Epic 15, and reports them as waiting for the maintainer.
- **API names without a story:** names the template or skill uses only as spellings
  (`IntoResponse`, `ModuleScope`, `ref_ttl`,
  `theme!()`, `.locales(..)`) are settled by the story that delivers the behavior
  behind them, which reconciles the template and skill. Behaviors have acceptance
  criteria (for example `#[handler(response = …)]` in Story 7.2).
- **Discord in tests:** tests that need Discord use the testkit fakes from Stories 5.2
  and 5.3 (`noctis_testkit::fake`, AD-24) on an App built on `Runtime::for_test()`;
  no story builds its own mock.
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
- FR-21 [P0]: Automatic acknowledgement — deferred response if the handler hasn't responded by a configurable deadline (default 2000 ms from gateway receipt); ephemerality declared up front per Command/Route; Commands defer as new deferred message, components/modal submits as in-place update; late responses (> 5 s) still delivered; V2 flag applied on the follow-up edit; modal-opening handlers never auto-deferred (missed deadline logs an error naming the handler), modal-in-response-to-modal-submit is a typed error; after 15 min public Views fall back to bot-token edit, ephemeral ones return `TokenExpired`; user-installed follow-up cap returns a typed error.
- FR-22 [P0]: Error routing — default: short localized ephemeral user error plus full structured trace with interaction context; Error Sinks selected by origin (Command, Component/Modal, Autocomplete, Event, Render).

**4.4 Components V2 UI Kit**

- FR-23 [P0]: Typed V2 builders — Container, Section, Text Display, Thumbnail, Media Gallery, File, Separator, Action Row, Button, all selects; illegal placement doesn't compile (FR-78); `IS_COMPONENTS_V2` on every V2 send/edit, `content`/`embeds`/`poll`/stickers/`shared_client_theme` impossible; editing legacy → View clears legacy fields in one request, V2 → legacy is a typed error; minimal legacy plain-text send path.
- FR-24 [P0]: Limit validation — 40 components counted at every depth; 4000-char Text Display budget; per-field limits (labels, placeholders, option counts, media items, `attachment://` refs, Section shape, gallery counts, link `url` length, premium button rules, accent color, select `default_values`); unique `custom_id` and numeric `id`; error names path, limit and actual value; lenient mode truncates with "…"; all numeric Limits in one configurable value; strict-mode missing alt text is a warning in the cost report.
- FR-25 [P0]: Views (revised 2026-10-20) — a View is a plain author-written render function (`render_x(state) -> V2Message`) used for the first response and every update; no View trait, registry, widget state or automatic re-render; page/tab/step/selection ids are ordinary typed Payload fields of the module's Routes; identical render for identical state (except per-render select ids); selects show the declared defaults after update; no parsed mentions by default (opt-in per message) plus escape helpers; message Limit cost available without sending; no public API mentions a view envelope, widget action or view state version.
- FR-61 [P0]: View responses and Responder ergonomics (revised 2026-10-20) — `ctx.respond(msg)` sends a new message (initial response, deferred-original edit or follow-up), `ctx.update(msg)` edits the component's message in place (FR-21 deferred variants when late), `ctx.reply_text(..)` sends a one-line notice (ephemeral by default); V2 flag and declared ephemerality applied without author code; an option-less Command calling `ctx.respond(render(..))` compiles, publishes with empty options and sends the message; the Project Template has no manual response-type selection.
- FR-62 [P0]: Limit budget helpers (revised 2026-10-20; automatic pagination withdrawn) — every component and message reports its cost (components at every depth, Text Display characters) without sending; a budget helper reports how many more items of a given shape fit beside a partly built message, so render functions size page slices themselves; an over-Limit message is the FR-24 error and is never paginated automatically.
- FR-26: Navbar — **Withdrawn (2026-10-20).** Tabs are an author-written recipe (a tab `PayloadEnum` field, a `match` in the render function, a Route button row with the active tab disabled) taught by the Usage Skill (Story 8.5).
- FR-27: Wizard — **Withdrawn (2026-10-20).** Step flows are an author-written recipe (a `step` Payload field, "Step n of m: Title" progress, back/next/cancel Route buttons, per-step validation in the handler) taught by the Usage Skill (Story 8.5).
- FR-28: Paginator — **Withdrawn (2026-10-20).** Pagination is an author-written recipe (a `page: u16` Payload field, a slice sized with the FR-62 budget helper, prev/next Route buttons disabled at the bounds) taught by the Usage Skill (Story 8.5).
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

- FR-41 [P1]: Command localization — names word by word, descriptions, option names/descriptions, choices; Publish sends localization maps; a file for one variant of a language (`en-US.yaml`) also covers its Discord sibling (`en-GB`) unless the sibling has its own file; invalid localized names are a typed error naming the Locale.
- FR-42 [P1]: Typed content strings — Locale strings called as typed functions with named/positional arguments; missing default-Locale key or wrong arguments is a compile error; key missing in another Locale falls back with a build warning; every placeholder occurrence replaced.
- FR-43 [P1]: Locale resolution — User and Guild Locale readable in every handler, Middleware can override; interactions: User → Guild → default, guild events: Guild → default; a code with no file of its own resolves to its same-language sibling's file (`en-GB` → `en-US`) first; Usage Skill documents User Locale for ephemeral single-user UI, Guild Locale for shared content.
- FR-72 [P1]: Module-scoped Locales — per-Module Locale files merged per language; a key defined by two Modules fails App build naming both; disabling a Module keeps its strings for Modules still referencing them.

**4.8 Modules, Feature Flags and Rate Limits**

- FR-44 [P1]: Module composition — Module tree mounted on the App, unique path-like ids; duplicate id is a typed error; registrations carry the Module id in logs, metrics and errors.
- FR-73 [P1]: Boot-time feature flags — Boot Flags on Modules/registrations selected at startup from config or CLI (`--features a,b` / `all`); unselected registrations are not registered, dispatched or published; dry-run reflects selected flags only.
- FR-45 [P1]: Runtime enable/disable — globally or per guild through ModuleStateStore; disabled handlers don't run; propagation to every process sharing the store within a documented delay; a disabled Module's global Command in a guild returns a localized ephemeral "disabled here" notice; only guild-scoped Commands drop out of a guild's Publish.
- FR-46 [P1]: Rate limits — per user, member, channel, guild or message on a Command or Route, stored in RateLimitStore; "N uses per window"; checked before the handler and any auto-defer, immediate localized ephemeral notice with time remaining; handlers can set/extend limits dynamically; rate-limited response overridable per App, Command or Route.
- FR-47 [P2]: Multiple Apps per process — several Apps with different tokens; same Module tree mountable on several Apps; Commands restrictable to a subset of Apps for Publish; guild allow/deny list per App resolved through an Adapter.

**4.9 Developer Experience and Project Template**

- FR-48 [P0]: Single facade crate with features — bot depends only on `noctis`; capabilities are cargo features; default features build without the image stack, image feature docs state build requirements; each documented feature combination builds in CI.
- FR-49 [P0]: Project Template — `examples/project-template/` builds and runs standalone; demonstrates an option-less entry Command opening a View (a hand-written render function), a hand-written step flow, a hand-written tabbed View with a paginated list, a Template in a Media Gallery, a custom Adapter and Locale files; CI builds it against the workspace on every change; follows the UI Doctrine (no embed/plain-content primary output, no collectors or inline listeners).
- FR-50 [P0]: Noctis Usage Skill — documents the whole public API (signatures, examples, one Limits entry per V2/modal component, UI Doctrine + UI quality rulebook, the tabs/pagination/step-flow recipes with complete code, schwi-style best practices, patterns/anti-patterns, common mistakes); every public item appears in it; every FR-23/FR-29 component type has an entry (missing entry fails CI); public-API change without skill update fails CI (snapshot check); no example uses a per-message listener.
- FR-51 [P1]: Testing kit — unit-test Commands, render functions and handlers without Discord via simulated interactions; invoke a Command or click by Route and assert the rendered message, response type and ephemerality offline; message JSON snapshots change only when JSON changes; Templates pixel-snapshot-tested with tolerance.
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
`resolver = "3"`, `exclude = ["examples/project-template"]`), `rust-toolchain.toml`
(stable 1.97.0), `.cargo/config.toml` (release-equivalent `[profile.dev]`),
`deny.toml`, `release-plz.toml`,
`crates/{noctis, noctis-rules, noctis-codec, noctis-ui, noctis-ports, noctis-gateway,
noctis-core, noctis-macros, noctis-i18n, noctis-image, noctis-prefix, noctis-testkit}`,
`xtask/`, `docs/guides/`, `.github/workflows/{ci.yml, main.yml, release.yml}`.

- **Toolchain and pins (AD-28, Stack):** edition 2024, MSRV 1.91, development
  toolchain stable 1.97.0 (`rust-toolchain.toml`); hyper 1.12 / hyper-util 0.1.21 /
  http-body-util 0.1.5 and tokio-websockets 0.13 (testkit fakes); yaml-rust2 0.13 and
  cssparser 0.38 (macros); tokio 1.53;
  twilight-* 0.17.1 (`twilight-http-ratelimiting >= 0.17.1`; gateway and http with
  `default-features = false` and the AD-21 TLS features); rustls 0.23.46 (`ring`
  provider, no `aws-lc-rs`); takumi `=2.17.0` and takumi-core `=0.31.0`;
  serde 1.0, postcard 1.1, thiserror 2.0, futures-util 0.3, tracing 0.1.44,
  metrics 0.24, arc-swap 1.9, moka 0.12, rayon 1.12, blake3 1.8, bytes 1.12,
  arrayvec 0.7, getrandom 0.4, regex 1.13, syn 3.0 / quote 1.0, trybuild 1.0,
  insta 1.49, proptest 1.12, criterion 0.8, dhat 0.3, proc-macro2 1.0, png 0.18,
  toml 1.1, pulldown-cmark 0.13, cargo_metadata 0.23, cargo-hack 0.6,
  cargo-public-api 0.52 on nightly-2026-10-15, cargo-semver-checks 0.51,
  cargo-deny 0.20, release-plz 0.3. `Cargo.lock` is committed.
- **Repo rule:** every cargo build/test/check/clippy/run uses `--release`; no debug
  builds (disk limits). CI follows the same rule. The repo `.cargo/config.toml` makes
  `[profile.dev]` release-equivalent (`opt-level = 3`, no debug info, assertions or
  incremental), which covers trybuild's own cargo runs; it must not be weakened.
- **CI gates (AD-28):** `ci.yml` runs on every push to `main` and on every pull
  request; fmt; clippy `-D warnings` on all targets; tests and doctests;
  `cargo hack test --release --workspace --each-feature` plus an all-features build;
  `missing_docs` denied for `noctis`;
  `cargo +1.91 check --release` (MSRV); trybuild UI tests on the pinned toolchain;
  `cargo-semver-checks` on `noctis`; `cargo-deny` (advisories, licenses, bans,
  sources); `xtask deps-check`, `rules-catalog`, `api-snapshot` (nightly),
  `skill-check`; `tw-crosscheck`; the allocation test; `insta` View snapshots;
  Project Template pixel snapshots; template build. `main.yml`: latency/memory
  benches reporting against `budgets.toml`, latest-takumi build. `release.yml`:
  release-plz publishing in layer order, crates.io token only in the protected
  release environment (set up by the maintainer, Story 14.8).
- **Crate layering (Invariants graph):** only the drawn edges are allowed
  (`xtask deps-check`); every edge points to a lower layer; `noctis-image` is a
  layer-5 extension whose edges to core, ports and ui exist only with its `render`
  feature (off by default, so `noctis-macros` uses the takumi-free `tw` module); no
  `testkit → image` edge; no crate depends on `noctis` except via dev-dependencies;
  testkit reachable only via the facade's `testkit` feature in `[dev-dependencies]`.
- **AD-1 / AD-14 ports:** every storage concern is a `noctis-ports` trait,
  `Send + Sync + 'static`, dyn-compatible, `BoxFuture<'_, Result<T, PortError>>`,
  `Arc<dyn Port>`; op vocabulary `get`, `put(ttl)`, `put_if_absent`, `take`,
  `delete`, `touch`, `incr_with_ttl`, `version()`; `IdentifyQueue::acquire(bucket) -> Lease`;
  v1 ports: RefStore, SessionStore, IdentifyQueue, ModuleStateStore, RateLimitStore,
  CacheStore (general key-value store, `ctx.kv()`), DiscordCache (typed Discord
  resource cache, memory default wraps twilight-cache-inmemory, `ctx.cache()`),
  RenderCache, AssetStore, LocaleSource, GuildAccessList, plus the `Renderer` driven
  port with the `Template`/`RenderJob` contract (AD-22); bounded memory defaults in
  `noctis_ports::memory` (moka, byte weighers); `Namespaced<P>` (`<app>:<port>:`) and
  `Shared<P>` (`_shared:<port>:`) wrappers only in core; postcard + schema byte for
  typed values.
- **AD-2 facade:** bot code depends only on `noctis`; `noctis::twilight` re-export;
  `noctis::image::unstable` outside semver; macro output uses
  `::noctis::__private::<crate>` paths; sub-crate docs say "internal; use `noctis`".
- **AD-3 / AD-4 rules:** one sync, I/O-free `noctis-rules` crate with stable rule ids,
  `Limits` (Strict/Lenient), `discord_len`; macros call it directly; compile-time →
  App-build → runtime ladder; every test names its rule id; Noctis-diagnosed
  compile errors carry `[<RULE-ID>]` in `.stderr`, by-construction rules carry
  `// [<RULE-ID>]` in the fixture or `compile_fail` doctest source; `components!`
  checks literal V2 trees (CV2-03, CV2-04, CID-02, …) at expansion; `xtask
  rules-catalog` fails CI when a row has no test of the kind its tier requires.
- **AD-5 / AD-6 / AD-18 / AD-19 / AD-20 codec:** normative `custom_id` layout
  (`F route ":" [S] [N] body [T]`; the `E` view envelope was withdrawn on
  2026-10-20 and Story 6.10 removes it), `pub const` layout constants checked against
  the catalog, total decoding, `Field::MAX_LEN` widths, `#[derive(Route)]` with
  `assert_fits`, `EncodedId = Inline | RefSlot(PendingRef)`, two-phase Ref minting by
  the Responder, `R: Handled` component constructors (AD-20, widget state on the View
  Route, is withdrawn), integrity tag keyed by `NOCTIS_PAYLOAD_KEY` (32 bytes, rotation with one
  previous key, never logged).
- **AD-7 Registry:** Modules produce descriptors; Boot Flags and per-App filters apply
  before the fold; `App::build` folds into one immutable Registry with every App-build
  check (returns all violations); O(1) dispatch maps; Publish JSON only from the
  Registry; Module enablement is an `arc-swap` bitset snapshot.
- **AD-9 stateless handlers**, `State<T>`, `Ext<T>`, no globals.
- **AD-10 lanes:** `Inbound { shard, received_at, kind }`, shard tasks only `try_send`,
  interaction lane priority, bounded event lane with droppable/overflow policy,
  semaphore-bounded worker set in core, guild-keyed ordered sub-queues.
- **AD-11 Responder:** per-kind ctx types, `Responder<K, S = Pending>` taken as a
  handler parameter (`CommandResponder<M>`, `ComponentResponder<M>`, `ModalResponder`,
  `M = NoModal | OpensModal`), `Responder<Pending>` → `Responder<Acked>`
  typestate, `opens_modal` declared on `#[command]` or `#[route]`, atomic runtime state machine shared with auto-defer, follow-up counting,
  `TokenExpired`, typed flag sets, webhook serializer with `with_components=true`.
- **AD-12 V2 model:** Noctis's own component types serialized directly (raw
  twilight-http body); twilight-model only for ids, interactions, events, commands;
  modal submissions decoded from the raw interaction JSON bytes (`&[u8]`; core passes
  the gateway's `RawJson` bytes); unknown data kept as `serde_json::Value`.
- **AD-13 errors:** per-crate `thiserror` enums, `noctis::Error` `#[non_exhaustive]`,
  `HandlerError` the only type-erased public error, `catch_unwind` around every
  handler/hook/event future and Render Job, no `unwrap`/`expect` on external input.
- **AD-15 rendering:** takumi `=2.17.0` with takumi-core `=0.31.0` on a
  process-scoped rayon pool, bounded queue, frame-weighted admission, per-job caps;
  `noctis_image::tw` grammar generated by `xtask tw-grammar` (syn-parsing takumi-core's
  `src/style/tw/map.rs`), takumi-free; `tw-crosscheck` CI against takumi-core's
  public `TailwindValues` parser; blake3 cache key; deterministic Template data (no
  `HashMap`); code-driven animation through `TemplateView::view(&self, Frame)`;
  `AssetFetcher` the only non-Discord HTTP.
- **AD-16 i18n:** `i18n!(default = "en-US", dir = "locales")` (both optional), YAML
  via `yaml-rust2`, files named by exact Discord locale code (`en-US.yaml`,
  `tr.yaml`), `Locale` (32 codes, `Locale::sibling()` for `en-US`↔`en-GB` and
  `es-ES`↔`es-419`) / `MessageKey` / `Translate` in `noctis-rules`, reserved
  `noctis.*` keys, fixed `cmd.<path>.description`-style keys with `<path>` as
  dot-joined nested words, `cmdword.*` / `ctxcmd.<key>.name` keys (no typed accessors),
  descriptions literal without the `i18n` feature and from the default-locale file
  with it (a conflicting literal is a compile error), overlays content-only.
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
  Upload Budget and edit diff only in the Responder, `ctx.render(...).await`;
  `Template: Serialize + DeserializeOwned` trait, `RenderJob { template_id,
  template_version, data (postcard), size, format, animation: Option<AnimationSpec> }`
  and `Renderer` in `noctis-ports`, implemented by `noctis-image`;
  `Attachment::rendered(bytes, digest, ext)` (primitives only) marks rendered images.
- **AD-23 pipeline:** fixed 10-stage interaction pipeline; Extensions type map with
  App-build check of `Ext<T>` providers.
- **AD-24 resource scoping and plugins:** one `Arc<twilight_http::Client>` per App
  (proxy mode, timeout 10 s default); `Runtime` defined in `noctis-core` and
  re-exported as `noctis::Runtime` (`global()`, `for_test()`, type-erased process
  resources such as the render pool, takumi caches, fonts, emoji font, metrics
  recorder, frozen by the first App); `trait Plugin { fn install(&self, &mut
  AppBuilder) }` with `ImagePlugin` + `AppBuilderImageExt` (`.image()`, `.theme()`)
  in `noctis-image` and `PrefixPlugin` in `noctis-prefix`; public `Dispatcher` for
  stages 3 to 10; testkit Apps on `Runtime::for_test()` with the fakes.
- **AD-25 allocation:** zero-allocation Route lookup and inline decode (dhat test),
  bounded caches, fixed-size per-guild state, benches in `noctis-core/benches` with
  `budgets.toml`, testkit `assert_memory_flat`.
- **AD-26 authoring surface:** `#[command]`, `#[user_command]`, `#[message_command]`,
  `Module::new("a/b")`, Route-value component construction, tescord naming.
- **AD-27 UI Doctrine in `noctis-ui`** (see UX-DRs) and `xtask skill-check` doctrine
  lint of the Project Template.
- **AD-28 release:** semver, release-plz, changelog with migration notes,
  `#[non_exhaustive]` upstream wrappers, MSRV policy, takumi bumps (both pins) as their own commit.
- **AD-29 observability:** span names `noctis.shard|interaction|event|render|publish`,
  fields, `noctis_*` metric names with `app` label, no ids/free text in labels,
  redacted token and key newtypes, Error Sinks receive every error/panic/drop.
- **Conventions:** crate/feature naming (`image`, `prefix`, `i18n`, `testkit`,
  `image-gif`, `image-emoji`, `default-font`), Route/Module/App id formats, compile
  error format `[<RULE-ID>] …` with note/help lines, `BuildError` lists all violations,
  `App::from_env()` with `NOCTIS_*`, the built-in command-line parser
  (`App::run_with_args`, `noctis::cli::NoctisArgs::split`), additive features, trybuild
  `compile_fail` fixtures at `tests/ui/<area>/<RULE-ID>_<slug>.rs`, boundary passes in
  `tests/boundary/`, `insta` View snapshots.
- **Deferred decisions to settle in stories:** HTTP client crate for `AssetFetcher`
  (image epic); PRD open questions Q1–Q6 keep their conservative defaults
  (configurable Limits, WebP default, benches report until confirmed, legacy → V2 on
  modal submit is a typed error, ModuleStateStore poll 5 s).
- **Project Template (FR-49, AD-17):** `examples/project-template/` already exists as
  a scaffold written against the planned API (modules `setup`, `dashboard`,
  `profile`, `adapters/bounded_cache.rs`, `strings.rs`, tests `flows.rs`,
  `adapters.rs`, `templates.rs`, `noctis-theme.css`, `locales/{en-US,tr}.yaml`, the
  Noctis Usage Skill at `.claude/skills/noctis/` with `references/`). Stories
  reconcile it with the real API as each API lands, and it must build in CI once the
  features it uses exist.

### UX Design Requirements

No UX design contract exists. The PRD's UI Doctrine (§4.0) is normative UX guidance
for the Project Template, the examples and the Usage Skill's recipes, and is treated
as the UX design requirements. Since the 2026-10-20 correct course, Noctis ships no
widgets: the author implements UD-4 to UD-6 by hand from the recipes.

- UX-DR1 (UD-1): Components V2 for anything interactive — recipes, the template and examples send only V2 messages; plain `content` and embeds exist only behind `LegacyMessage`.
- UX-DR2 (UD-2): Images for read-only visuals — rendered images enter messages only as Thumbnail or Media Gallery attachments with alt text and never carry actionable information.
- UX-DR3 (UD-3): Commands are entry points — an option-less Command answers with `ctx.respond(render_x(..))` and no response-type code; configuration happens inside the View.
- UX-DR4 (UD-4): Steps for setup flows — the step-flow recipe's progress reads "Step n of m: Title", never page numbers.
- UX-DR5 (UD-5): Tabs for general views that don't fit — the tabs recipe: a Route button row at the top, active tab disabled, at most 5 tabs per row (CV2-06).
- UX-DR6 (UD-6): Pagination for long lists — the pagination recipe: a `page` Payload field, a slice sized with the budget helper, prev/next disabled at the bounds, inside the active tab when there is one; Noctis never paginates automatically.
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
| FR-14 | P2 | E15 | Prefix commands behind the `prefix` feature (`PrefixPlugin` over the public `Dispatcher` from E6) |
| FR-15 | P0 | E7 | One handler per Route, O(1) dispatch |
| FR-16 | P0 | E2 | Field encodings, escaping, round-trip property tests (+E7 runtime use) |
| FR-17 | P0 | E2 + E7 | Ref format, `EncodedId`, `spill`, and the "error" policy as a compile-time rejection (E2); RefStore minting, TTL, one-shot (E7) |
| FR-18 | P0 | E7 | Expired Handler, defaults and overrides |
| FR-19 | P0 | E2 + E7 | Total decoding and integrity tag (E2); pipeline handling and Route Guards (E7) |
| FR-20 | P0 | E7 | Middleware and typed Extensions |
| FR-21 | P0 | E6 + E7 (+E5) | Responder typestate and auto-defer for Commands (E6); components and modals (E7); warm, hedged, deadline-bounded callback transport (E5, Story 5.9) |
| FR-22 | P0 | E5 + E6 | Error Sinks by origin (E5); user-facing localized errors and panics as responses (E6) |
| FR-23 | P0 | E4 | Typed V2 builders, flags, legacy path |
| FR-24 | P0 | E4 | Runtime Limit validator, strict/lenient, configurable `Limits` |
| FR-25 | P0 | E4 + E6 + E8 | Mention default (E4); removal of the shipped view envelope and `view` Route shape (E6, Story 6.10); deterministic renders, select freshness, escape helpers (E8) |
| FR-26 | — | — | Withdrawn (2026-10-20): tabs recipe in the Usage Skill and example (E8, Story 8.5) |
| FR-27 | — | — | Withdrawn (2026-10-20): step-flow recipe in the Usage Skill and example (E8, Story 8.5) |
| FR-28 | — | — | Withdrawn (2026-10-20): pagination recipe in the Usage Skill and example (E8, Story 8.5) |
| FR-29 | P0 | E4 + E7 | Modal model and `derive(Modal)` (E4); modal Routes and typed submission decoding (E7) |
| FR-30 | P1 | E9 | Attachment tracking, edit diff, Upload Budget, alt text |
| FR-31 | P1 | E9 | Raw component JSON, unknown received components, raw REST |
| FR-32 | P1 | E10 | Templates, compile-time class validation, Theme |
| FR-33 | P1 | E10 | Output formats, size budget and degradation |
| FR-34 | P1 | E10 | Bounded render pool |
| FR-35 | P1 | E10 | Fonts, fallback fonts, static assets |
| FR-36 | P1 | E10 | RenderCache key and single-flight deduplication |
| FR-37 | P2 | E15 | Renderer seam proven with a test Renderer (port and `Template`/`RenderJob` contract defined in E3) |
| FR-38 | P0 | E3 | Port traits with bounded in-memory defaults, incl. `DiscordCache` over twilight-cache-inmemory (+E7 typed `ctx.kv()` over CacheStore; +E12 DiscordCache wiring and `ctx.cache()`) |
| FR-39 | P1 | E14 | Conformance suites in the testkit |
| FR-40 | P1 | E12 | DiscordCache wiring, `ctx.cache()`, configuration, backend swap and minimal intents (port from E3) |
| FR-41 | P1 | E11 | Command localization in Publish |
| FR-42 | P1 | E11 | `i18n!` typed content strings |
| FR-43 | P1 | E11 | User/Guild Locale resolution |
| FR-44 | P1 | E13 | Module tree, unique ids, ids in logs/metrics/errors (basic `Module` and nesting from E5) |
| FR-45 | P1 | E13 | Runtime enable/disable through ModuleStateStore |
| FR-46 | P1 | E13 | Rate limits through RateLimitStore |
| FR-47 | P2 | E15 | Multiple Apps per process, GuildAccessList |
| FR-48 | P0 | E1 | Facade crate, features, feature-matrix CI with `cargo hack` (+E14 final matrix, release pipeline, maintainer-gated publish) |
| FR-49 | P0 | E1 + all + E14 | CI template job (E1); each API story reconciles the template; template green and gate required (E14) |
| FR-50 | P0 | E1 + all + E14 | `api-snapshot`/`skill-check` gates (E1); each API story updates the Usage Skill; completeness audit (E14) |
| FR-51 | P1 | E5 + E8 + E10 | Discord HTTP and gateway fakes and `Runtime::for_test()` (E5, Stories 5.1 to 5.3); testkit simulated interactions and View snapshots (E8, per roadmap M2); pixel snapshots (E10) |
| FR-52 | P1 | E12 (+E5) | HTTP proxy mode, Cluster warning, timeout (E12); Discord edge forwarding to the configured proxy (E5, Story 5.9) |
| FR-53 | P2 | E15 | Presence and gateway commands |
| FR-54 | P1 | E13 | Lifecycle Hooks API (shard status events produced in E5) |
| FR-55 | P1 | E12 | Resumable shutdown, `--no-resume` |
| FR-56 | P0 | E6 | Unique Command Paths and context-menu names |
| FR-57 | P0 | E6 | Explicit base/group metadata, deterministic fallback |
| FR-58 | P0 | E4 + E7 | `R: Handled` component constructors (E4); handler macro and App-build check (E7) |
| FR-59 | P1 | E9 | Route component defaults and per-render overrides |
| FR-60 | P0 | E2 + E7 | Schema version and Stale decoding (E2); stale routing (E7) |
| FR-61 | P0 | E6 + E8 | Returned message as the Command response (E6, Story 6.5); `ctx.respond`/`ctx.update`/`ctx.reply_text` (E8, Story 8.1) |
| FR-62 | P0 | E4 + E8 | `cost` (E4, Story 4.3); budget helpers "how many items fit" (E8, Story 8.3) |
| FR-63 | P1 | E9 | Modal pre-fill |
| FR-64 | P1 | E7 | Radio/Checkbox decoding from raw JSON (built into the modal decoder per AD-12) |
| FR-65 | P1 | E9 | Views over webhooks |
| FR-66 | P1 | E10 | Tailwind v4 grammar, v3 suggestions, runtime class warnings |
| FR-67 | P1 | E10 | AssetFetcher: allow-list, caps, avatar helpers |
| FR-68 | P1 | E10 | Emoji font and custom emoji images |
| FR-69 | P1 | E10 | Animated Templates |
| FR-70 | P1 | E10 | Per-job caps and admission weights |
| FR-71 | P0 | E7 | `Namespaced<P>` and `Shared<P>` wrappers and the typed `ctx.kv()` handle in core (port keys opaque since E3) |
| FR-72 | P1 | E11 | Module-scoped Locale catalogs |
| FR-73 | P1 | E13 | Boot Flags |
| FR-74 | P1 | E14 | Launch documentation |
| FR-75 | P0 | E1 + all | trybuild harness and catalog check (E1); each rule story adds its fixtures |
| FR-76 | P0 | E2 | `derive(Route)` compile-time length proof |
| FR-77 | P0 | E6 | `#[command]` compile-time validation |
| FR-78 | P0 | E4 | Static component tree (`components!`) and literal checks |
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
| UX-DR1..9 | — | E4 + E8 + E14 | V2-only message model (E4); recipes, rulebook and example (E8, Story 8.5); doctrine lint (E1 gate, E8, E14) |

## Epic List

Fifteen epics, ordered by priority tier (P0 → P1 → P2) and, inside a tier, by the
crate layers (rules → codec → ports/ui → gateway → core → macro-heavy features →
image → i18n → scale). Each epic builds only on earlier epics. The README roadmap
milestone is given per epic.

**Epic order decision (autonomous):** the Components V2 model (E4, roadmap M2) is
pulled ahead of the gateway and core epics because `noctis-core` depends on
`noctis-ui` (layer 2) and the Responder must send V2 messages; Views (plain render
functions), Responder ergonomics and budget helpers stay in M2 (E8). The P1 UI completions (E9) sit at the head of the P1 block because they
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
**FRs covered:** FR-1, FR-2, FR-5, FR-6, FR-21 (callback transport), FR-22 (Error Sinks), FR-51 (Discord fakes), FR-52 (edge to proxy); NFR-4, NFR-5.

### Epic 6: Slash commands and Publish (P0, roadmap M1)
A bot author declares tescord-style Commands, Patterns, typed options, autocomplete and
context menus, gets compile-time and App-build rule checks, responds safely through the
Responder, and publishes only when something changed. The epic ends by removing the
withdrawn widget envelope from the shipped crates (Story 6.10).
**FRs covered:** FR-7, FR-8, FR-9, FR-10, FR-11, FR-12, FR-13, FR-21 (Commands), FR-22 (user errors), FR-25 (envelope removal), FR-56, FR-57, FR-77.

### Epic 7: Routed components, Refs and modals (P0, roadmap M1)
A bot author handles every button, select and modal through one stateless handler per
Route, with Refs for large state, expiry handling, guards, middleware and auto-defer.
**FRs covered:** FR-15, FR-17 (store), FR-18, FR-19 (pipeline, guards), FR-20, FR-21 (components, modals), FR-29 (routing, submissions), FR-58 (handler side), FR-60 (routing), FR-64, FR-71; NFR-2, NFR-9.

### Epic 8: Views as plain functions, Responder ergonomics and the testing kit (P0 + FR-51, roadmap M2)
A bot author writes each screen as a plain render function, answers with
`ctx.respond`/`ctx.update`/`ctx.reply_text`, sizes lists with budget helpers, tests
flows without Discord, and learns tabs, pagination and step flows as recipes.
**FRs covered:** FR-25, FR-51 (simulated interactions, message snapshots), FR-61, FR-62; NFR-2; UX-DR1..UX-DR9. (FR-26 to FR-28 withdrawn 2026-10-20.)

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
docs exist, the release pipeline is ready, and `noctis` 0.1.0 is published (Stories 14.8 and 14.9 are maintainer-gated).
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

**Implements:** FR-48, FR-49 (CI job), NFR-7, NFR-8; AD-2, AD-28 (toolchain and builds), Stack, Structural Seed.

**Acceptance Criteria:**

**Given** an empty repository apart from docs, planning artifacts and `examples/project-template/`
**When** the story is done
**Then** the root `Cargo.toml` declares a virtual `[workspace]` with `resolver = "3"`, members `crates/{noctis, noctis-rules, noctis-codec, noctis-ui, noctis-ports, noctis-gateway, noctis-core, noctis-macros, noctis-i18n, noctis-image, noctis-prefix, noctis-testkit}` and `xtask`, `exclude = ["examples/project-template"]`, `version.workspace = true` in every crate, `edition = "2024"`, `rust-version = "1.91"`, `license = "MIT OR Apache-2.0"` in `[workspace.package]` (inherited by every crate), `[workspace.dependencies]` pinned to the Stack table and shared `[workspace.lints]`
**And** `Cargo.lock` is committed (AD-15, AD-28), and `noctis-image` declares a `render` feature with empty default features, so its dependency-free `tw` module is all that builds by default
**And** each sub-crate is a compiling `lib` whose crate docs say "internal; use `noctis`", `noctis-macros` is a `proc-macro` crate, `xtask` has `publish = false`, and `deny.toml`, `release-plz.toml` and `docs/guides/` (with a placeholder file so it exists in git) exist
**And** `cargo metadata` shows `examples/project-template` is not a workspace member (it keeps its own empty `[workspace]`)

**Given** the toolchain and build profiles (AD-28)
**When** the repository root is inspected
**Then** `rust-toolchain.toml` pins `channel = "1.97.0"` with `rustfmt` and `clippy`, and `/.cargo/config.toml` sets `[profile.dev] opt-level = 3, debug = false, debug-assertions = false, overflow-checks = false, incremental = false`, so any cargo invocation in the repo that isn't `--release` (trybuild's own runs under `target/tests/trybuild/`) still builds release-grade; `[profile.release]` in the root `Cargo.toml` stays the real release profile
**And** a CI step asserts both files exist with those values, so the dev profile can't be weakened silently
**And** `deny.toml` allows exactly `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Unicode-3.0`, `Zlib`, `MPL-2.0` (cssparser and its dependencies; maintainer-approved) and `CC0-1.0`; adds the crate-scoped `exceptions` `xxhash-rust` (`BSL-1.0`) and `jpeg-encoder` (`IJG`), both reached through takumi (found by resolving the Stack with `cargo metadata` on 2026-10-19); sets `[graph] targets` to the non-wasm tier-1 targets (which keeps the wasm-only `webpki-root-certs`, `CDLA-Permissive-2.0`, out of the graph); and bans `aws-lc-rs` and `aws-lc-sys` (AD-21); any further license needs the maintainer's approval
**And** the story's dev notes record the `[workspace.lints]` set and the release-plz skip for `xtask`

**Given** the facade crate `noctis`
**When** its `Cargo.toml` is inspected
**Then** it exposes the additive features `image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `prefix` and `testkit` (initially empty), default features build without any image dependency, and `#![deny(missing_docs)]` is set

**Given** a push to `main` or a pull request
**When** `.github/workflows/ci.yml` runs (its triggers are `push: branches: [main]` and `pull_request`, because the maintainer pushes to `main` directly)
**Then** it executes, on the toolchain from `rust-toolchain.toml`, `cargo fmt --check`, `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`, `cargo test --release --workspace` (tests and doctests, default features), `cargo hack test --release --workspace --each-feature` (cargo-hack 0.6: each crate's tests with each feature alone, which also builds each facade feature alone), `cargo build --release --workspace --all-features`, `cargo deny check` (advisories, licenses, bans, sources), plus a separate MSRV job `cargo +1.91 check --release --workspace`; every cargo invocation uses `--release`
**And** every job is a gate: a failing job fails the run, and a story is done only when the run on its pushed commit is green
**And** a `template-build` job runs `cargo build --release` in `examples/project-template/` in tracking mode (non-blocking, result reported) as decided in Story conventions
**And** `main.yml` and `release.yml` exist as stubs referencing the jobs later epics fill in

**Given** the root `AGENTS.md`
**When** the workspace exists
**Then** its TODO line about missing `Cargo.toml` is replaced by the real verification commands (all `--release`, including the `cargo hack` run), and its line on the repo `.cargo/config.toml` dev profile is kept

### Story 1.2: Crate layer enforcement with `xtask deps-check`

As a Noctis contributor,
I want CI to reject any workspace dependency edge the architecture does not allow,
So that the hexagonal layering cannot erode as crates grow.

**Implements:** AD-1, AD-2, Invariants graph, AD-28.

**Acceptance Criteria:**

**Given** the allowed-edge list from the spine's Invariants graph encoded in `xtask`
**When** `cargo run --release -p xtask -- deps-check` runs on the workspace
**Then** it passes when only drawn edges exist (the facade may depend on any crate), treating a labelled edge as allowed only behind its feature (`noctis-image → noctis-core`/`ports`/`ui` only as optional dependencies enabled by `render`)

**Given** a crate adds a forbidden edge (for example `noctis-ui → noctis-core`, or any crate → `noctis` outside `[dev-dependencies]`)
**When** `deps-check` runs
**Then** it fails naming the offending edge and the rule it breaks
**And** `noctis-testkit` reachable from the facade other than through the `testkit` feature also fails, as do `noctis-testkit → noctis-image` and a non-optional `noctis-image → noctis-core` edge

**Given** CI
**When** `ci.yml` runs on a push to `main` or a pull request
**Then** `deps-check` runs as a gate

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
**Then** it is a closed enum with exactly those codes (LOC-01), parses from and prints to the Discord code (a bare language such as `en` doesn't parse), offers `Locale::sibling() -> Option<Locale>` with exactly `en-US` ↔ `en-GB` and `es-ES` ↔ `es-419` (AD-16; no base-language type exists), and `MessageKey` and `trait Translate` live in this crate

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

**Implements:** FR-23, FR-24, FR-29 (rule functions), FR-79; AD-3; rules CV2-03..CV2-16, MOD-01..MOD-07, MSG-01, MSG-03..MSG-06, ATT-01..ATT-06, WH-04, INT-07.

**Acceptance Criteria:**

**Given** the rule functions in `noctis_rules::component`, `::modal`, `::message` and `::attachment`
**When** they are called with boundary values
**Then** each accepts the limit exactly and rejects limit + 1 for: 40 components (CV2-03), 4000 text characters via `discord_len` (CV2-04), 1–5 buttons or exactly one select per row (CV2-06), 1–3 Text Displays plus one accessory per Section (CV2-07), button label 80 and link `url` 512 and premium rules (CV2-08), select placeholder 150 / options 25 / min-max / `default_values` (CV2-09), gallery 1–10 items and descriptions 1024 (CV2-10, CV2-11), separator spacing and accent color range (CV2-13), legacy 5 rows (CV2-15)
**And** modal rules MOD-01..MOD-07, message rules MSG-01 and MSG-03..MSG-06 (MSG-02 is documentation-only: `LegacyMessage` has no embeds in v1), attachment rules ATT-01..ATT-06 (20 MiB default budget, 25 MiB request, 10 files configurable), webhook name WH-04 and autocomplete choice cap INT-07 are covered the same way

**Given** a rule function that later proc macros call at expansion (`label!`, `rgb!`, `components!`, attribute literals; AD-3, AD-4)
**When** it is called from a proc macro
**Then** it is a plain function of `noctis-rules` that needs no runtime state, so the macro and the runtime validator call the same code; no rule relies on a `const { … }` assertion in generic code (AD-4)

**Given** a lowered runtime `Limits` override
**When** a rule function receives `&Limits`
**Then** it checks against the override, and no function reads `Limits::default()` internally (AD-3)

### Story 1.7: trybuild harness and rules-catalog cross-check

As a Noctis maintainer,
I want CI to prove every catalog rule has a test at its enforcement point,
So that the catalog and the code can never silently diverge.

**Implements:** FR-75, FR-79, NFR-10; AD-3, AD-4, AD-5 (constants quote), AD-28 (UI tests), Conventions (Tests).

**Acceptance Criteria:**

**Given** the workspace
**When** the trybuild harness is added
**Then** the facade's test suite has one `compile_fail("tests/ui/**/*.rs")` test for featureless fixtures, skipped when any optional facade feature is on (`#[cfg(not(any(feature = "image", feature = "i18n", feature = "prefix", feature = "testkit")))]`), and one `compile_fail("tests/ui-<feature>/**/*.rs")` test per feature behind `#[cfg(feature = "<feature>")]` (added by the first story that needs that feature's fixtures), with no trybuild `pass` cases (so trybuild runs only `cargo check`); boundary passes live in `tests/boundary/` as ordinary integration tests; the suites run inside `cargo test --release` and `cargo hack test --release --each-feature` on the toolchain pinned by `rust-toolchain.toml`, locally and in CI
**And** `.stderr` files under `tests/ui/` are blessed with default features and those under `tests/ui-<feature>/` in the run of exactly that feature
**And** a fixture for a rule Noctis diagnoses must have the rule id of its file name in its `.stderr`; a fixture for a `Compile-time (by construction)` rule must instead carry `// [<RULE-ID>]` on the offending line of its source
**And** a test proves the repo `.cargo/config.toml` dev profile applies to trybuild's runs (for example by asserting `target/tests/trybuild/` holds no debug info), and a toolchain bump re-blesses `.stderr` files in the same commit

**Given** `docs/discord-rules.md`
**When** `cargo run --release -p xtask -- rules-catalog` runs
**Then** it parses every rule row and its Enforcement column (with `pulldown-cmark` 0.13; `toml` 1.1 for `rules-pending.toml`), and fails when a Noctis-diagnosed Compile-time row has no fixture whose `.stderr` contains its id, a `Compile-time (by construction)` row has neither a fixture nor a `compile_fail` doctest whose source carries `// [<ID>]`, or an App-build or Runtime row has no unit test citing its id
**And** rows whose Enforcement is only a parenthesis (permitted, out of scope, no API in v1) are documentation-only, and `see <ID>`/`as <ID>` rows are covered by that rule's tests
**And** rows whose enforcement lands in a later epic are listed in `xtask/rules-pending.toml` with the story id that will add the test; a pending entry that already has a test also fails (the list only shrinks)

**Given** the `custom_id` layout constants table in the catalog
**When** `rules-catalog` runs after Epic 2 adds the constants
**Then** it asserts the catalog values equal the `pub const`s in `noctis-codec` (until then the check is listed as pending for Story 2.2)

**Given** CI
**When** `ci.yml` runs on a push to `main` or a pull request
**Then** `rules-catalog` runs as a gate

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
**When** `ci.yml` runs on a push to `main` or a pull request
**Then** `api-snapshot` and `skill-check` run as gates, and the skill's `API hash` line is updated from `pending` to the current hash in this story; its wording says the hash is updated with every public API change (not "once `noctis` is published")

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
**Then** a Route whose worst case is 101 characters fails to compile and one of exactly 100 compiles (`tests/boundary/`), an unbounded `String` field without `by_ref`/`overflow = spill` fails with CID-01, `by_ref` together with `signed` fails, a generic Route fails, and a route id violating `[a-z0-9._-]{1,24}` or starting with `_` fails with CID-03
**And** every `.stderr` follows `[<RULE-ID>] <what is wrong>` with a `help:` line

**Given** the codec arguments of AD-6: `by_ref`, `overflow = spill`, `schema = n` (1..=61), `view`, `select`, `signed`
**When** they are combined legally
**Then** the derived `SHAPE` and the length bound include `S`, `E`, `N`, `T` only when declared (`select` sets the `N` nonce section), and `spill` Routes encode inline when the value fits and by Ref otherwise
**And** the derive emits an empty `RouteRuntime` const that later stories fill (runtime arguments `ttl`/`one_shot` in Story 7.5, `owner`/`permissions`/`guard` in 7.7, `opens_modal` in 7.8), and any argument not yet in AD-6's grammar is a compile error naming it

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

**Implements:** FR-38, NFR-6; AD-1, AD-14, AD-22 (render contract).

**Acceptance Criteria:**

**Given** `noctis-ports`
**When** it is built
**Then** it defines `RefStore`, `SessionStore`, `IdentifyQueue`, `ModuleStateStore`, `RateLimitStore`, `CacheStore`, `DiscordCache`, `RenderCache`, `AssetStore`, `LocaleSource` and `GuildAccessList`, each `Send + Sync + 'static` and dyn-compatible, with async methods returning `BoxFuture<'_, Result<T, PortError>>`
**And** each store port exposes only its subset of `get`, `put(ttl)`, `put_if_absent(ttl) -> bool`, `take`, `delete`, `touch`, `incr_with_ttl(key, by, ttl) -> u64` as listed in AD-14 (`CacheStore`: `get`, `put`, `put_if_absent`, `delete`, `incr_with_ttl`), `ModuleStateStore` adds `version() -> u64`, and `IdentifyQueue` is `acquire(bucket) -> Lease`
**And** `DiscordCache` is the typed port of AD-14: `update(&Event)` and owned getters (`guild`, `channel`, `role`, `member`, `user`, `message`, per-guild lists) returning twilight-model values, with no reference or guard in any signature

**Given** the render contract of AD-22
**When** it is declared in `noctis-ports`
**Then** it defines `trait Template: Serialize + DeserializeOwned + Send + Sync + 'static` (`ID`, `VERSION`, `SIZE`, `FORMAT: Format`, `ANIMATION: Option<AnimationSpec>`, `view() -> ErasedView`), `RenderJob { template_id, template_version, data: Bytes, size, format: Format, animation: Option<AnimationSpec> }` (data is postcard), the still `Format` with its encoder settings, `AnimationSpec { duration, fps, format: AnimatedFormat }`, `Frame { index, count }` (`Frame::STILL`), `TemplateDescriptor`, `RenderOutput { bytes, digest, format: OutputFormat, settings_used }` with `OutputFormat::extension()`, and `trait Renderer` (`render`, `register`), without depending on takumi or on `noctis-image`; Stories 10.2, 10.3 and 10.6 build on exactly these types
**And** a unit test round-trips a sample Template value through postcard (`Serialize` then `DeserializeOwned`), as the Renderer will

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

**Given** `noctis_ports::memory::MemoryDiscordCache`
**When** it is built for a set of resource types
**Then** it wraps `twilight-cache-inmemory` limited to those types (and twilight's per-channel message cap), `update(&Event)` applies events, and every getter returns an owned clone (unit tests over recorded events)

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
**Then** the only constructor is from an `R` value (`BuyButton { .. }.button()`, `.string_select(..)` and the other selects, which also require a Route declared `select`), it requires `R: Handled`, and there is no string `custom_id` constructor (AD-19); a fixture shows a Route without `Handled` failing to compile (FR-58 compile-time half)
**And** Epic 4's own tests make a Route `Handled` through the `#[doc(hidden)]` path the handler macro will emit (`impl ::noctis::__private::ui::Handled for R {}`), which is documented as not public API; `Handled` carries `#[diagnostic::on_unimplemented]` pointing to `#[handler]`
**And** `noctis-ui` defines `Attachment { name, bytes: Bytes, content_type, alt }` named `<first 16 hex of blake3(bytes)>.<ext>` (or `Attachment::with_digest`), `Attachment::rendered(bytes, digest: [u8; 32], ext: &'static str)` (primitives only, named by digest, origin `Rendered`; AD-22), an `AttachmentSet` keyed by name, and `AttachmentRef`, obtainable only by adding an attachment; `File` accepts only an `AttachmentRef` (CV2-12), and numeric component ids are assigned deterministically from the tree (CV2-14)

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
**And** these by-construction rules (CV2-01, CV2-02, MSG-03, MSG-05, MSG-07, INT-09) each have a `compile_fail` doctest or trybuild fixture whose source carries `// [<ID>]` on the line that tries the missing field, method or flag

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
**When** a Thumbnail or gallery item that isn't marked `Rendered` has no alt text
**Then** a warning is emitted and appears in the cost report (rendered images are an error from Story 9.1)

**Given** any tree
**When** `cost(&tree)` is called
**Then** it returns component count, text characters and warnings without sending (used by FR-25)

### Story 4.4: Compile-time checks for literals, arity and the `components!` tree macro

As a bot author,
I want literal labels, placeholders, arities and whole literal component trees checked by `cargo check` (AD-4 mechanisms only),
So that static mistakes never reach runtime.

**Implements:** FR-75, FR-78, NFR-10; AD-3, AD-4, AD-12; rules CV2-03, CV2-04, CV2-05, CV2-06, CV2-07, CV2-08, CV2-09, CV2-10, CV2-11, CV2-13, CV2-15, CID-02.

**Acceptance Criteria:**

**Given** builders that take `Label`, `Placeholder` and similar newtypes
**When** a literal is passed through `label!("…")`, `placeholder!("…")` or a builder macro attribute
**Then** the proc macro measures it with `noctis-rules`, so an over-length literal (81-char button label, 151-char placeholder, 1025-char description) fails under `cargo check` with `[CV2-08]`, `[CV2-09]` or `[CV2-11]`, and the runtime `try_from` path enforces the same limit via the same function

**Given** const-generic arity
**When** `ActionRow::buttons([b; N])`, `Section::new([t; N], accessory)` or `MediaGallery::new([i; N])` is built
**Then** `N` outside 1–5, 1–3 or 1–10 fails under `cargo check` because the arity trait bounding `N` is implemented only for legal lengths, and the error text starts with `[CV2-06]`, `[CV2-07]` or `[CV2-10]` through `#[diagnostic::on_unimplemented]`; `rgb!(0x…)` above 0xFFFFFF fails in the macro (`[CV2-13]`), and `Rgb::try_from(u32)` is the runtime path

**Given** the `components!` macro in `noctis-macros`, re-exported as `noctis::components!` (AD-12), with this grammar:

```text
components! { node,* }                       // a message body (top-level nodes)
node      := container | section | text | gallery | file | separator | row | hole | spread
container := "container" [ "(" "accent" "=" expr ")" ] "[" node,* "]"
section   := "section" "[" text,+ ";" "accessory" "=" (thumbnail | button) "]"
text      := "text" "(" (string-literal | expr) ")"
thumbnail := "thumbnail" "(" expr [ "," "alt" "=" (string-literal | expr) ] ")"
gallery   := "gallery" "[" item,+ "]"
item      := "item" "(" expr [ "," "alt" "=" (string-literal | expr) ] ")"
file      := "file" "(" expr ")"                                // an AttachmentRef
separator := "separator" [ "(" "spacing" "=" ("small" | "large") [ "," "divider" "=" bool ] ")" ]
row       := "row" "[" (button,+ | select) "]"
button    := expr "." ("button" | "button_with") "(" args ")"    // a Route value's constructor
           | "link" "(" "url" "=" string-literal "," "label" "=" string-literal ")"
           | "premium" "(" expr ")"
select    := expr "." ("string_select" | "user_select" | "role_select" | "mentionable_select" | "channel_select") "(" args ")"
hole      := "{" expr "}"                     // one already-built node of the right placement type
spread    := ".." expr                         // IntoIterator of nodes of the right placement type
```

**When** a literal tree is expanded
**Then** it expands into the typed builder calls of Story 4.1 under `::noctis::__private::ui` (so the placement traits still apply to holes and spreads) and evaluates to a `Components` value that `V2Message::new(..)` and `View::render` accept
**And** at expansion it calls `noctis-rules` and emits `compile_error!`s that name the rule id, with spans on the offending node: a node kind illegal at its position (container in container, thumbnail outside an accessory, button outside a row or accessory: `[CV2-05]`); more than 40 components counted at every depth, accessories included, where a hole counts 1 and a spread 0 (`[CV2-03]`, with the count per subtree as a `note:`); literal `text("…")` totalling more than 4000 by `discord_len` (`[CV2-04]`); a row with 0 or more than 5 buttons, or a select next to anything else (`[CV2-06]`); a section with 0 or more than 3 texts (`[CV2-07]`); a gallery with 0 or more than 10 literal items (`[CV2-10]`); literal labels, placeholders and alt texts over their limits (`[CV2-08]`, `[CV2-09]`, `[CV2-11]`); and two Route components whose Route type and Payload expression are token-identical with only literal field values (`[CID-02]`, both spans shown)
**And** holes, spreads and non-literal text are the static lower bound only; the runtime validator of Story 4.3 checks the actual tree before sending, and no rule here relies on a `const` assertion in generic code

**Given** `tests/ui/cv2/`, `tests/ui/cid/` and `tests/boundary/`
**When** the UI tests run
**Then** each rule above has a failing fixture whose `.stderr` contains its rule id, including a `components!` fixture with 41 components, one with 4001 literal text characters and one with a duplicate literal `(Route, Payload)` pair (CID-02), plus boundary-pass integration tests (exactly 5 buttons, exactly 40 components, 4000 text characters, 80-char label), and the matching `rules-pending.toml` entries are removed

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's `references/components-v2.md` documents `components!` (grammar, what it checks, holes and spreads), `label!`, `placeholder!` and `rgb!`, and `skill-check` passes

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

**Given** a modal submission as raw interaction JSON bytes (`&[u8]`; `noctis-ui` never names the gateway's `RawJson`, whose bytes core passes from Story 7.8 on, AD-12) containing Text Input, selects, File Upload, Radio Group, Checkbox Group and Checkbox values
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
interactions. Tests use the testkit's Discord fakes, built in Stories 5.2 and 5.3 (AD-24), so no
network is needed.

### Story 5.1: App builder, Runtime, Plugin seam and startup safety

As a bot author,
I want to build an App from a token, state and Adapters and get typed errors for every startup mistake,
So that misconfiguration fails clearly instead of panicking, and optional features attach through one seam.

**Implements:** FR-1, FR-38 (Adapter injection), NFR-4, NFR-6; AD-9, AD-13, AD-21, AD-24, AD-29; Conventions (Config).

**Acceptance Criteria:**

**Given** `App::builder()` and `App::from_env()` (reading `NOCTIS_*` variables)
**When** a bot author sets the token, `State<T>` values and any port implementation (`.ref_store(..)`, `.cache_store(..)`, …) (Modules are mounted from Story 5.6)
**Then** `.build()` returns an `App` or a `BuildError` listing every violation at once, and unset ports default to the bounded memory implementations
**And** the token is a newtype whose `Debug`/`Display` are redacted and that is never logged (NFR-6)

**Given** `App::run` is called outside a multi-thread tokio runtime
**When** it starts
**Then** it returns a typed error instead of panicking; when no process-default rustls crypto provider is installed, Noctis installs rustls's `ring` provider, and a failure to do so is a typed error (AD-21)
**And** `[workspace.dependencies]` pins `rustls` 0.23.46 with `default-features = false, features = ["ring", "std", "tls12", "logging"]`, `twilight-gateway` with `default-features = false, features = ["rustls-platform-verifier", "twilight-http", "zstd"]` and `twilight-http` with `default-features = false, features = ["rustls-platform-verifier", "decompression"]`, and `cargo deny` bans `aws-lc-rs` and `aws-lc-sys`

**Given** the App
**When** it is built
**Then** core constructs exactly one `Arc<twilight_http::Client>` per App with a configurable request timeout (default 10 s), and no other crate constructs a Discord HTTP client (AD-24)

**Given** `noctis_core::Runtime`, re-exported as `noctis::Runtime` (AD-24)
**When** Apps are built
**Then** `App::builder()`/`App::from_env()` attach to `Runtime::global()`, `AppBuilder::runtime(&Runtime)` attaches to another one, and `Runtime::resource_or_init::<T>(config)` creates a type-erased process resource on first use; the first App's configuration freezes it, and a later conflicting configuration is a `BuildError` (unit test with a dummy resource)
**And** `Runtime::for_test()` returns an isolated Runtime that is never the global one and reports `is_test() == true`, so two tests can configure conflicting resources without interfering (unit test)

**Given** the `Plugin` trait (`fn install(&self, app: &mut AppBuilder)`)
**When** a plugin is registered with `AppBuilder::plugin(p)` (one per type, reachable through `plugin_mut::<P>()`)
**Then** `build()` calls every `install` exactly once, in registration order, before the App-build checks; `install` uses only public `AppBuilder` methods (`renderer(Arc<dyn Renderer>)`, port setters, `intents(..)`, `runtime()`, and `module(..)` once Story 5.6 adds it), and violations it reports join the one `BuildError` (unit test with a test plugin that installs a fake Renderer)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's `references/app-and-sharding.md` documents the builder, `from_env` variables, `Runtime`, plugins and errors, the template's `main.rs`/`lib.rs` `configure()` are reconciled with the real builder, and `skill-check` passes

### Story 5.2: Discord HTTP fake and the test Runtime

As a Noctis contributor,
I want a local fake of Discord's HTTP API that a real App talks to through its normal client,
So that every later story can test REST behavior end to end without network access.

**Implements:** FR-51 (Discord HTTP fake); AD-24, AD-14; Conventions (Tests).

**Acceptance Criteria:**

**Given** `noctis-testkit` (AD-24)
**When** the story is done
**Then** `noctis_testkit::fake::FakeDiscordHttp` is a local `hyper` server (hyper 1.12, hyper-util 0.1.21, http-body-util 0.1.5, pinned in `[workspace.dependencies]` as the Stack lists) with per-test routes, a canned `GET /gateway/bot` (configurable shard count and `max_concurrency`, `session_start_limit`) and a request log for assertions

**Given** an App built on `Runtime::for_test()` with `.http_proxy(fake.url())` (this story adds the `http_proxy` setter, which configures `proxy(url, true)` and `ratelimiter(None)`; Story 12.4 adds the Cluster semantics around it)
**When** it sends a request
**Then** the request reaches the fake only through twilight's HTTP `proxy(url, true)` override, with no test-only branch in core, and a smoke test in `crates/noctis-testkit/tests/` asserts it in the request log
**And** end-to-end tests of Epics 5 to 7 live in `crates/noctis-testkit/tests/`

### Story 5.3: Discord gateway fake

As a Noctis contributor,
I want a local fake of Discord's gateway that a real App's shards connect to,
So that sharding, lanes, resume and shutdown can be tested end to end without network access.

**Implements:** FR-51 (Discord gateway fake); AD-21, AD-24; Conventions (Tests).

**Acceptance Criteria:**

**Given** `noctis_testkit::fake::FakeGateway`
**When** a test starts it
**Then** it is a local `tokio-websockets` 0.13 server speaking HELLO, IDENTIFY/READY, RESUME/RESUMED, heartbeat ACKs (with a controllable interval and a hook to observe heartbeat timing), scripted dispatches and close codes (including 4004, 4014 and a resumable close), and records every frame it receives

**Given** an App built on `Runtime::for_test()` with `.gateway_url(fake.url())` and `.http_proxy(http_fake.url())`
**When** it connects
**Then** it reaches the fake only through twilight's gateway `proxy_url` (exposed as `.gateway_url(..)`, also used for real gateway proxies), with no test-only branch in core, and a smoke test in `crates/noctis-testkit/tests/` connects one shard to both fakes, receives READY and one dispatch, and closes cleanly

### Story 5.4: Automatic sharding with bucketed identify

As a bot author,
I want Noctis to start the recommended number of shards and identify them as fast as Discord allows,
So that a large bot boots quickly without identify storms.

**Implements:** FR-1 (token/intent errors), FR-2; AD-10, AD-21; rules GW-01, GW-02, GW-03, GW-05.

**Acceptance Criteria:**

**Given** `FakeDiscordHttp` answering `GET /gateway/bot` with a shard count and `max_concurrency = 16`
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

### Story 5.5: Interaction Lane and bounded event lane

As a bot author,
I want interactions on a priority lane that never drops and events on a bounded lane with explicit drop policy,
So that slow handlers never cost heartbeats or user-visible interactions.

**Implements:** FR-5, NFR-3 (structure), NFR-5, UX-DR8; AD-10, AD-25, AD-29.

**Acceptance Criteria:**

**Given** shard tasks
**When** they enqueue
**Then** they only `try_send`; they never await lane capacity or a handler, so a handler sleeping 10 s doesn't delay heartbeats (test with `FakeGateway`'s heartbeat timing)

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

### Story 5.6: Modules and typed Event Handlers with derived filters and intents

As a bot author,
I want to register typed Event Handlers in a Module and have Noctis derive the event filter and required intents,
So that unhandled events are never deserialized and missing intents fail before connecting.

**Implements:** FR-5 (worker set, per-guild ordering), FR-6, FR-44 (basic Module and nesting), NFR-4; AD-7, AD-9, AD-10, AD-13, AD-21, AD-23 (ancestor order); rules GW-05.

**Acceptance Criteria:**

**Given** `Module::new("a/b")` with `#[event]` handlers added, mounted with `AppBuilder::module(..)`
**When** the App is built
**Then** the Registry is folded once from descriptors, and Noctis asks the gateway to deserialize only event types with a handler, so an unhandled type is never deserialized (test with counting deserialization); Story 12.5 later adds the types the Discord cache needs

**Given** a parent Module with a child mounted through `Module::module(child)`
**When** the App is built
**Then** the child's registrations are folded with the parent's, in parent-before-child order, and each registration records its ancestor path for AD-23 ordering (Story 13.1 later adds unique-id checks and Module ids in logs, metrics and errors)

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
**Then** the Usage Skill documents `Module`, `.module(..)` nesting, `#[event]`, ordering and intent derivation, the template's event handler (profile XP on message) is reconciled, and `skill-check` passes

### Story 5.7: Observability contract and Error Sinks

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

### Story 5.8: Graceful shutdown and fatal-close reporting

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

### Story 5.9: Warm, deadline-safe Discord HTTP edge

As a bot operator,
I want Discord HTTP connections kept warm and interaction callbacks sent outside the shared rate-limit queue,
So that the first request after a quiet period never hangs on a dead connection and no interaction misses its 3 s deadline, without polling Discord to stay warm.

**Implements:** FR-21 (transport side of the ack deadline), FR-51 (HTTP fake: h2c, connection log, black-hole), FR-52 (proxy composition, request timeout), NFR-4, NFR-5, NFR-6; AD-13, AD-21, AD-24, AD-29; rules INT-01, INT-13, RL-01.

**Acceptance Criteria:**

**Given** an App (research: `research/http-warm-transport.md` §7)
**When** it is built
**Then** core binds a loopback-only listener (`127.0.0.1:0`, a std socket so `build()` stays sync) for its crate-internal Discord edge and builds the App's one `Arc<twilight_http::Client>` with twilight's public `proxy("127.0.0.1:<port>", true)`; no other crate constructs a Discord HTTP client (AD-24)
**And** the edge forwards only paths under `/api/v10/` (anything else is answered 403 and never forwarded), rewrites only the authority/`Host`, forwards method, headers and body byte-for-byte, streams the response back unchanged (so twilight's RateLimiter keeps learning buckets, RL-01), and never adds or logs `Authorization` or any header carrying personal data (NFR-6)

**Given** no `.http_proxy(..)` configured
**When** the edge forwards
**Then** its upstream is `https://discord.com` over one tuned hyper client: ALPN `h2, http/1.1`, `TokioTimer` timers, h2 PING keep-alive every 20 s with a 5 s timeout while idle, `TCP_NODELAY`, TCP keepalive (30 s / 10 s / 3 probes, plus `TCP_USER_TIMEOUT` on Linux), a 1.5 s connect timeout per address, a 3 s TLS timeout, async DNS with an in-memory cache honoring TTL (floor 30 s, cap 5 min, background refresh at 75 % of TTL, last good answer served on resolver failure for up to 1 h), and the process-default rustls `ring` provider (AD-21)

**Given** `.http_proxy(url)` configured (the Story 5.2 setter, also used for the fakes and for a Cluster's shared proxy)
**When** the edge forwards
**Then** its upstream is that URL instead of discord.com, twilight keeps `ratelimiter(None)` as Story 5.2 set it, the proxy's own behavior (including its rate limiting) is preserved, and a byte-equality test against `FakeDiscordHttp` shows the same method, path, headers (except `Host`) and body as twilight produced, including a multipart upload and `X-Audit-Log-Reason`
**And** the Story 5.2 and 5.3 smoke tests pass unchanged, so tests run the same edge code path as production with no test-only branch in core

**Given** `FakeDiscordHttp` (this story adds h2c prior-knowledge serving on the same port, a per-connection log with h2 PING frames, a stall control per route, and a black-hole mode that keeps the TCP connection open but silently discards bytes)
**When** `App::run` starts
**Then** the edge's keeper opens one upstream connection and completes TCP, TLS (for https) and the h2 handshake before `GET /gateway/bot` and before the first shard identifies, and redials with jittered back-off (0, 0.5, 1, 2, 5 s cap) whenever the connection closes (GOAWAY, PING timeout, error) and when a shard reconnects after a network loss; warm-up makes no API request (connection log shows zero HTTP requests from it)
**And** an `http://` upstream uses h2c only when `HttpConfig::upstream_h2c` is set (the testkit helper sets it), otherwise HTTP/1.1 with the same TCP settings

**Given** an idle App with no requests for 10 minutes against the fake (paused tokio clock, or the same test with intervals scaled down through `HttpConfig`)
**When** time advances
**Then** the fake observes h2 PING frames at the configured interval and zero HTTP requests, and the next request reuses the warm connection (`noctis_http_conn_reused_total{reused="true"}` increments); no code path polls an API endpoint to keep connections warm

**Given** the fake black-holes the upstream connection
**When** one PING interval plus the PING timeout passes
**Then** the keeper detects the dead connection, redials, and a callback sent afterwards succeeds on the new connection; **Given** a callback sent onto the black-holed connection before detection **Then** its hedge on a fresh connection succeeds before the interaction's deadline, and no request waits for twilight's 10 s timeout

**Given** a crate-internal interaction-callback path (Story 6.5's Responder later sends every initial callback through it; AD-11's single Responder path is unchanged)
**When** core sends `POST /api/v10/interactions/{id}/{token}/callback` (with `?with_response=true` when asked) with a body built from twilight-model types
**Then** it goes directly over the edge's upstream client, bypassing twilight's RateLimiter (INT-13: interaction endpoints are outside the global limit), while follow-ups and edits stay on twilight
**And** four concurrent callbacks against a fake route answering after 1 s all complete within 1.2 s (no serialization; twilight's limiter baseline is 1/2/3/4 s)

**Given** a fake route that stalls the first callback attempt indefinitely
**When** no response headers arrive within `hedge_after` (default `min(1000 ms, deadline − now − 1200 ms)`, never below 300 ms)
**Then** an identical second attempt goes out on a new connection that bypasses the pool, the first 2xx wins and the loser is cancelled, at most 2 attempts are made, the ack succeeds before the deadline, and `noctis_interaction_ack_hedges_total{outcome="won"}` increments
**And** an HTTP 400 with Discord code 40060 ("already acknowledged") on either attempt counts as success, increments `{outcome="40060"}` and logs no error

**Given** a callback's deadline (`Inbound::received_at` + 3 s − 100 ms, INT-01)
**When** it passes, or a 429's `retry_after` doesn't fit before it
**Then** the call returns a typed `AckDeadlineMissed` error (AD-13) and makes no further attempts

**Given** a request that failed before any byte was written (connect refused, DNS or TLS error, h2 `REFUSED_STREAM`, GOAWAY with `last_stream_id` below ours)
**When** the edge handles it
**Then** exactly one retry goes out; a non-callback request that failed after bytes were written is not retried and the edge answers twilight with 502 within the per-phase timeouts instead of hanging until twilight's timeout (default 10 s, still applied to every request)

**Given** a DNS answer with TTL 60 s (unit test with a scripted resolver behind a crate-internal seam)
**When** 50 s pass
**Then** the cache refreshes in the background and no request in the window waits on DNS; **Given** the resolver then fails **Then** the cached address is still used

**Given** any upstream request (Story 5.7 observability contract: `metrics` facade, `noctis_` prefix, `app` label)
**When** it completes
**Then** `noctis_http_phase_seconds{phase="dns|connect|tls|ttfb|total"}` has samples (dns/connect/tls on new connections only), and `noctis_http_connections_total{reason="warmup|redial|hedge"}`, `noctis_http_conn_reused_total{reused}`, `noctis_http_h2_ping_rtt_seconds`, `noctis_http_ping_timeouts_total` and `noctis_interaction_ack_hedges_total{outcome}` are recorded, with matching fields on a `noctis.http` span
**And** the Story 5.7 label test covers them: no label, span field or header the edge adds carries a token, id or personal data

**Given** shutdown (Story 5.8)
**When** core follows the AD-21 order
**Then** the edge and its keeper stay live while lanes drain ("HTTP still live") and stop only after `gateway.closed()`

**Given** the new public API (`HttpConfig { ping_interval, ping_timeout, hedge_after, connect_timeout, dns_ttl_floor, dns_ttl_cap, upstream_h2c }` with the defaults above, set through `AppBuilder::http(HttpConfig)`, and the `AckDeadlineMissed` error)
**When** the story is done
**Then** `examples/project-template/` (code, `AGENTS.md`, Usage Skill: `references/app-and-sharding.md` documents the edge, `HttpConfig` and the metrics), `xtask/public-api.txt` and the Usage Skill's `API hash` line are updated in the same commit, and `skill-check` and every local gate in AGENTS.md pass

## Epic 6: Slash commands and Publish

A bot author declares tescord-style Commands, Command Patterns, typed options,
autocomplete and context menus; gets compile-time and App-build rule checks; responds
safely through the Responder with automatic deferral; and publishes only when
something changed.

### Story 6.1: `#[command]` with compile-time Command Path and Pattern validation

As a bot author,
I want to declare a Command as an `async fn` with a Command Path or Pattern that is validated at compile time,
So that illegal command structures fail `cargo check` and my handler knows which variant ran.

**Implements:** FR-7, FR-8, FR-75, FR-77, UX-DR3; AD-4, AD-8, AD-16 (description source without `i18n`), AD-26; rules CMD-01, CMD-02, CMD-03, CMD-04, CMD-05, CMD-10, CMD-11, CMD-12, CMD-13, CMD-15, OPT-14.

**Acceptance Criteria:**

**Given** `#[command("settings logs channel", description = "…")]` on an `async fn`
**When** it compiles
**Then** it produces a command descriptor whose word count maps to command, subcommand, or group plus subcommand (FR-7), with subcommand `required` not settable (OPT-14)
**And** the literal `description = "…"` is the description (AD-16); doc comments are rustdoc only, and Epic 6 reads no locale file (the `i18n` source and the conflict rule arrive in Story 11.4)

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
**Then** the Usage Skill's `references/commands.md` documents `#[command]`, `description = "…"`, patterns and the variant enum, the template's commands are reconciled, and `api-snapshot`/`skill-check` pass

### Story 6.2: Typed options with `derive(Options)` and `derive(Choices)`

As a bot author,
I want to declare a Command's options as a typed struct and receive it parsed,
So that missing or wrong option values are typed errors, never panics.

**Implements:** FR-10 (options, validators), FR-77 (option rules); AD-4, AD-26; rules OPT-01..OPT-11, OPT-13, CMD-06.

**Acceptance Criteria:**

**Given** `#[derive(Options)]` with field types implementing the option trait (`i64`, `f64`, `bool`, `String`, `Id<UserMarker>`, channel, role, mentionable, attachment)
**When** the Command is invoked
**Then** the handler receives the parsed struct; a missing required option or a wrong option type is a typed error (OPT-02, OPT-03 runtime decode)
**And** each option's description is the literal `#[option(description = "…")]` (AD-16; a missing or 101-char one fails with CMD-04), with the locale-file source added in Story 11.4

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

**Implements:** FR-21 (callback rules), FR-61 (Command half, with `V2Message`); AD-11, AD-12, AD-23, AD-24 (`Dispatcher`); rules INT-03, INT-04, INT-06, INT-08, INT-09, INT-10, INT-12.

**Acceptance Criteria:**

**Given** a Command interaction from the interaction lane
**When** core dispatches it
**Then** it resolves the Command from the Registry's O(1) map by default (unlocalized) names (LOC-06) and runs the AD-23 stages that apply to Commands at this point (resolve, locale default, auto-defer arm point, handler, error routing)

**Given** a Command handler taking a `CommandResponder` parameter (AD-11: `Responder<Command<NoModal>, Pending>`)
**When** the handler responds
**Then** `Responder<Pending>` offers `reply` and `defer`, each consuming `self` and returning `Responder<Acked>`, which offers only `edit_original`, `followup(msg, Visibility) -> FollowupHandle` and `delete_original` (INT-03), and `done()` yields the `Responded` the handler returns
**And** `CommandResponder` has no `update` and `CommandResponder<NoModal>` has no `modal` (INT-04, by construction: trybuild fixtures carrying `// [INT-04]`), a second `reply` after `Responder<Acked>` doesn't compile (INT-03, `// [INT-03]`), `defer` takes only an `Ephemerality` value (INT-08), and every V2 send or edit carries `IS_COMPONENTS_V2`

**Given** a handler that returns a `V2Message`
**When** it completes
**Then** core sends it as the initial response with the Command's declared ephemerality (default public, INT-10), validated by the runtime validator before the HTTP request, and after a defer it uses `edit_original` explicitly (INT-12)

**Given** the pipeline (AD-23, AD-24)
**When** the story is done
**Then** core exposes it as the public `noctis::Dispatcher`, returned by `EventCtx::dispatcher()`, whose `run_command(invocation)` runs an already-resolved Command Path with caller-decoded options through the pipeline stages from 3 on (each later stage joins it as it lands) and replies through a message-reply Responder; a unit test dispatches a Command from an Event Handler through it (Story 15.4's `PrefixPlugin` is its first real user)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `CommandCtx`, `Responder<K, S>`, its aliases and states, ephemerality and `Dispatcher`, matching AD-11 and AD-24, and `skill-check` passes

### Story 6.6: Automatic deferral, token lifetime and follow-up cap

As a bot author,
I want slow handlers deferred automatically and expired tokens handled,
So that users never see "interaction failed" and late updates still land.

**Implements:** FR-21; AD-11, AD-23, AD-29; rules INT-01, INT-02, INT-05, INT-08, INT-11.

**Acceptance Criteria:**

**Given** a Command handler that hasn't responded by the deadline (default 2000 ms, configurable, measured from `Inbound::received_at` so queueing time counts)
**When** the deadline passes
**Then** core sends a type-5 deferred response with only the declared `EPHEMERAL` flag (INT-08) over the direct, hedged callback path (Story 5.9), and the handler's later `reply` becomes `edit_original` carrying the V2 flag (runtime state machine shared by the handler and the auto-defer task)
**And** a handler responding after 5 s still delivers its message (test with paused clock), and `noctis_autodefer_total` and `noctis_ack_latency_seconds` are recorded

**Given** `#[command(.., opens_modal)]` whose handler takes `CommandResponder<OpensModal>`
**When** it is dispatched
**Then** auto-defer is never armed and `modal(..)` is available (INT-05); if it misses the deadline, an error naming the handler is logged; declaring `opens_modal` without the `OpensModal` responder, or the reverse, fails to compile (INT-05 fixture)

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

**Given** `#[option(autocomplete = handler)]` on a supported field, where `handler` is an `#[autocomplete]` fn
**When** an autocomplete interaction arrives for an option of a 3-word Command Path
**Then** the handler receives an `AutocompleteCtx` (which offers only `respond(choices) -> Responded`, no defer: INT-06) with the focused value and a `Partial<Options>` whose fields are all `Option<T>` (OPT-12), and returns the `Responded` from `respond`

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

**Implements:** FR-11 (JSON), FR-13, FR-61 (empty option list); AD-7, AD-26, AD-29, Conventions (Command line); rules META-04, REG-05.

**Acceptance Criteria:**

**Given** the Registry
**When** Publish builds the command JSON
**Then** it is generated only from the Registry; each metadata field set on a Command appears exactly as declared and unset fields are omitted; an option-less Command publishes an empty option list; guild-scoped Commands are published only to their guilds

**Given** remote commands equal to the local definition (`FakeDiscordHttp`)
**When** Publish runs
**Then** it makes no write request; when they differ it writes once and logs the diff

**Given** `--publish --dry-run`
**When** the binary runs
**Then** it prints the full JSON and a human-readable diff, the number of creates a write would cost (REG-05), runs the same App-build checks, and writes nothing

**Given** the CLI modes `--publish`, `--publish-guild <id>` and `--publish-clear`
**When** `App::run` sees them
**Then** it performs that Publish mode and returns without connecting shards; Publish never runs on shard ready, and in normal runs it runs only when the App is configured as the publisher

**Given** the built-in command-line parser (Conventions, no clap)
**When** `App::run()` reads `std::env::args()`, a test calls `App::run_with_args([..])`, or a bot calls `noctis::cli::NoctisArgs::split(args)` before its own parser
**Then** Noctis's flags are recognized in all three, `split` returns the remaining arguments untouched, and an unknown flag reaching `App::run` is a typed error naming it (later flags `--features` and `--no-resume` join the same parser in Stories 13.2 and 12.3)
**And** before writing, Publish reads the application object and reports a typed error when a declared `integration_types` value isn't enabled for the app (META-04), and emits a `noctis.publish` span

**Given** the new public API
**When** the story is done
**Then** the Usage Skill and the template README document the publish workflow (`cargo run --release -- --publish-guild <id>`), and `skill-check` passes

### Story 6.10: Simplification: remove the widget envelope and view-state support from shipped crates

As a bot author,
I want the shipped crates to carry no trace of the withdrawn widget layer (the `view` Route shape, the `E` view envelope, widget actions and view state versions),
So that the public API, the `custom_id` layout and the docs match the plain-render-function model and nothing is built on a removed concept.

**Implements:** correct course 2026-10-20 (FR-26 to FR-28 withdrawn, FR-25 revised); AD-5, AD-6 (amended), AD-17, AD-20 (withdrawn), AD-27; rules CID-01, CID-02, CID-03, CID-07.

This story only removes code that Epics 1 to 6 already shipped; it depends on no later story, and Epics 7 onwards assume the simplified codec.

**Acceptance Criteria:**

**Given** `noctis-codec`
**When** the story is done
**Then** `ViewEnvelope`, `EnvelopeError`, `VIEW_ENV_LEN`, `Flags::E`, `RouteShape::view()` and `has_view()`, `Parts::with_envelope`, the envelope accessors of `PendingRef` and the decoded id, `Section::Envelope` and `EncodeError::{MissingEnvelope, UnexpectedEnvelope}` no longer exist, and the layout is `F route ":" [S] [N] body [T]`
**And** flag value 2 is reserved: an id with it set decodes as a `CodecError` (malformed), never as `Stale` and never a panic (unit test plus the existing decode property tests, whose generators no longer produce envelopes), and the zero-allocation decode guard (Story 2.6) still passes

**Given** `route_width` and `assert_fits`
**When** a Route's width is computed
**Then** `MAX_LEN = FMT_LEN + route.len() + SEP_LEN + S + N + max(inline, by_ref)`, the by-ref worst case is 41 characters, and the width breakdown printed by the derive's test has no `E` term (the template's `noctis_route_width` doc lines are updated to match)

**Given** `#[derive(Route)]`
**When** `#[route("a", view)]` is written
**Then** it fails with `[CID-03]` as an unknown argument whose help lists `by_ref`, `overflow = spill`, `schema = n`, `select` and `signed` (trybuild fixture `tests/ui/cid/CID-03_view_withdrawn.rs`; the existing CID-03 `.stderr` files are re-blessed), and the macro's own unit tests drop `view`

**Given** `noctis-ui`, `noctis-rules` and the facade
**When** a Route component is built
**Then** `CustomId::new` resolves at once for every Route without a select nonce or integrity tag (no `has_view` branch), and no rustdoc, comment or test in `crates/` mentions a View envelope, widgets or view state versions (the `noctis` facade docs, `noctis-ui` `lib.rs`/`route.rs`, `noctis-codec` `parts.rs`/`layout.rs`/`error.rs` and the `noctis-rules` `Translate` comment included)

**Given** `docs/discord-rules.md`
**When** `xtask rules-catalog` runs
**Then** the withdrawn `E` line of the quoted layout, the `VIEW_ENV_LEN` constants row, the withdrawal note and every `E` term are gone, the by-ref worst case reads 41, the CID-01 and CID-03 notes no longer mention `view`, and the check passes

**Given** the public API
**When** the story is done
**Then** `xtask/public-api.txt` is re-blessed; the Usage Skill (`routes-payloads-refs.md`, `limits-cheatsheet.md`, `components-v2.md`, `modals.md`, `troubleshooting.md`) drops the withdrawn items and their "withdrawn" notes and its `API hash` line is updated; a search of `crates/`, `docs/`, `README.md` and `examples/` for `ViewEnvelope`, `VIEW_ENV_LEN`, `has_view`, `view envelope` and `#[route(.., view` finds nothing; and every local gate (AGENTS.md) passes

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

**Given** a component handler taking `ComponentResponder` (AD-11)
**When** the handler responds
**Then** `Responder<Pending>` offers `update` (type 7), `defer` (type 6 in-place) and `reply`; the declared default for components is update-in-place; the auto-defer uses type 6 at the deadline and later updates become `edit_original` with the V2 flag

**Given** `#[handler(response = ephemeral)]` or `#[handler(response = public)]` (AD-6 handler arguments)
**When** the handler sends its rendered message (by returning it, or through the typestate Responder)
**Then** it is sent as a new ephemeral or public message (type 4, auto-defer type 5 with only the declared `EPHEMERAL` flag) instead of updating in place; `response = update` is the default, and the skill's sub-panel pattern uses this
**And** spans `noctis.interaction` cover every stage, and `noctis_handler_duration_seconds{kind}` and `noctis_handler_errors_total{kind}` are recorded

### Story 7.3: Expired Handler and stale Payload routing

As a bot author,
I want clicks on expired or pre-deploy components to reach an Expired Handler instead of failing,
So that months-old messages degrade gracefully.

**Implements:** FR-18, FR-60; AD-5, AD-23.

**Acceptance Criteria:**

**Given** a `custom_id` that decodes as `Stale` (schema or shape mismatch)
**When** it is dispatched
**Then** it goes to the Route's stale handler when declared (`#[handler(on_stale = fn)]`), else to the Expired Handler, never to a generic decode error

**Given** the default Expired Handler
**When** it runs
**Then** it replies with a short ephemeral notice from `noctis.expired`

**Given** an App-level override or a Route-level `#[handler(on_expired = fn)]`
**When** a Route's interaction expires
**Then** the override runs instead, and can return a View-like response such as re-rendering a starting message (test with a `V2Message`)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents stale and expired handling and Payload evolution rules (append `Option` fields; otherwise bump `schema`), and `skill-check` passes

### Story 7.4: App-namespaced ports

As a bot author running several bots on shared storage,
I want every port key namespaced by App unless I opt into sharing,
So that two Apps never read or overwrite each other's Refs, sessions, counters or Module state.

**Implements:** FR-71, FR-38 (typed CacheStore handle), NFR-6; AD-1, AD-14; Conventions (App ids).

**Acceptance Criteria:**

**Given** an `AppId` (`[a-z0-9-]{1,32}`, default the application id in base62)
**When** core wraps a port
**Then** `Namespaced<P>` prefixes keys with `<app>:<port>:`, escaping `:` in key segments, and adapters receive keys verbatim (they never prefix)

**Given** two Apps sharing one `RefStore` instance
**When** each writes and reads the same logical key
**Then** neither sees the other's value (unit test for RefStore, SessionStore, RateLimitStore and ModuleStateStore)

**Given** an Adapter opted into sharing (for example `CacheStore`)
**When** it is passed to the normal port setter wrapped in `Shared::new(..)` (`.cache_store(Shared::new(store))`; there is no separate `.share(..)` method)
**Then** core keeps it as `Shared<P>` with the prefix `_shared:<port>:` and both Apps see the same keys

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `app_id`, namespacing and sharing, the template's `app_id("noctis-template")` is reconciled, and `skill-check` passes

**Given** a handler or Middleware calling `ctx.kv()` (AD-1)
**When** it uses the typed handle over the App's `Namespaced<CacheStore>`
**Then** `get::<T>` and `put(&T, ttl)` use postcard plus a schema byte (a schema mismatch reads as a miss), `put_if_absent` returns whether it won, `incr(key, by, ttl)` and `counter(key)` (0 when missing) use `incr_with_ttl`/`get` with 8-byte big-endian values, and `delete` removes; each maps to one `CacheStore` call (unit tests with a counting store)
**And** the Usage Skill's adapters reference documents `ctx.kv()`, and the template's profile and setup code that stores data uses it

### Story 7.5: Ref minting and resolution

As a bot author,
I want large or by-ref Payloads stored and resolved for me with TTLs and one-shot semantics,
So that I can carry rich state in components without hitting the 100-character limit.

**Implements:** FR-17, FR-18 (missing Ref), NFR-5, NFR-6; AD-6 (runtime arguments), AD-18, AD-23; rules CID-05.

**Acceptance Criteria:**

**Given** a response containing `EncodedId::RefSlot`s
**When** the Responder finalizes it before validation and sending
**Then** it stores each slot with `RefStore::put_if_absent` (retrying on collision), calls `codec::finish` for the final `custom_id`, and identical slots in one message share one Ref; no crate below `noctis-core` holds a `RefStore` handle

**Given** the runtime arguments `ttl = "…"` and `one_shot`, which this story adds to `#[derive(Route)]` (AD-6), and the App default TTL
**When** Refs are stored
**Then** the Route's TTL applies when declared, else the App default; a Route marked `one_shot` resolves with `take` so the Ref is deleted on use; `ttl` or `one_shot` on a plain inline Route fails to compile

**Given** a plain inline Route (no `by_ref`, no `spill`)
**When** its Payload could exceed 100 characters
**Then** it doesn't compile (Story 2.5, CID-01): this is FR-17's "error" overflow policy, and no runtime `overflow = error` exists (AD-6)

**Given** a Ref that is missing, expired, or created by another App
**When** stage 2 resolves it
**Then** the interaction goes to the Expired Handler; a record whose route differs from the id's route is treated as forged; a schema mismatch is Stale
**And** `noctis_ref_lookups_total{result}` counts hit, miss, stale and forged

**Given** a response carrying a `RefOps` deletion list (from pure code such as a step flow's cancel)
**When** core sends it
**Then** core executes the deletions after a successful send

### Story 7.6: Middleware and typed Extensions

As a bot author,
I want Middleware that runs before and after handlers and can attach typed Extensions,
So that cross-cutting concerns (auth, settings, metrics) live in one place.

**Implements:** FR-20; AD-23; Conventions (BuildError).

**Acceptance Criteria:**

**Given** before- and after-Middleware registered on a parent Module and a child Module (nesting from Story 5.6)
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

**Given** the runtime arguments this story adds to `#[derive(Route)]` (AD-6): `#[route("dash.mod", permissions = MANAGE_GUILD)]`, `owner = user` (Payload-carried invoking user) or `guard = path::to::fn`
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

**Given** a Route declared `#[route(.., opens_modal)]` (the runtime argument this story adds to the derive; Commands declare it on `#[command]` since Story 6.6)
**When** its handler runs
**Then** it takes `ComponentResponder<OpensModal>`, whose `Responder<Pending>` alone offers `modal(..)`, auto-defer is disabled for it, and a handler taking `ComponentResponder<NoModal>` calling `modal` fails to compile, as does a mismatch between the declaration and the alias (INT-05 trybuild fixtures)

**Given** a modal-submit interaction
**When** it is dispatched to the modal Route's handler
**Then** the handler receives `ModalSubmitCtx` and the typed `derive(Modal)` struct, which core decodes by passing the bytes of the interaction's `RawJson` to the Story 4.6 decoder, including Radio Group, Checkbox Group and Checkbox values that twilight drops (FR-64 end-to-end test through `FakeGateway`)
**And** `ModalResponder` has no `modal` method (INT-04), so responding to a submit with another modal can't be expressed; a dynamic attempt is a typed error

**Given** a modal submit
**When** the handler is slow
**Then** the auto-defer uses type 6 (in-place update), as for components

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's modal recipe shows the full open/submit flow, the template's profile bio modal compiles against it in tracking mode, and `skill-check` passes

## Epic 8: Views as plain functions, Responder ergonomics and the testing kit

A bot author writes each screen as a plain render function (`render_x(..) ->
V2Message`) and calls it from the Command and from every component and modal handler,
answers with `ctx.respond`, `ctx.update` or `ctx.reply_text`, sizes lists with budget
helpers, tests complete flows without Discord, and learns tabs, pagination and step
flows as recipes. Noctis ships no widgets and keeps no UI state (correct course,
2026-10-20); tab, page and step ids are typed Payload fields of the author's Routes.

### Story 8.1: Responder ergonomics: `ctx.respond`, `ctx.update` and `ctx.reply_text`

As a bot author,
I want to send a rendered message with one call that picks the callback type, deferral, flags and ephemerality for me,
So that every handler ends with `ctx.respond(render_x(..))` or `ctx.update(render_x(..))` and contains no response plumbing.

**Implements:** FR-61, FR-21 (deferred variants), UX-DR3; AD-11, AD-23, AD-27; rules INT-01, INT-03, INT-04, INT-08, INT-10, INT-12, CV2-01, MSG-05.

**Acceptance Criteria:**

**Given** a Command, Component or Modal Submit handler
**When** it calls `ctx.respond(msg).await?`
**Then** a new message is sent: the initial response (type 4) with the declared ephemerality while the interaction is unanswered, an edit of the deferred original after a Command's auto-defer, and a follow-up with the declared visibility once the original was answered; it returns the `Responded` the handler returns

**Given** a Component or Modal Submit handler
**When** it calls `ctx.update(msg).await?`
**Then** the message the component lives on is edited in place (type 7 while unanswered, `edit_original` after the type-6 auto-defer) with `IS_COMPONENTS_V2` and the runtime validator applied
**And** `CommandCtx` has no `update` (E0599; a `compile_fail` doctest carrying `// [INT-04]`)

**Given** any interactive handler
**When** it calls `ctx.reply_text(text)` with a `String`, a `&'static str` or a localized `MessageKey`
**Then** a one-Text-Display V2 notice is sent, ephemeral by default (`.public()` opts out), as the initial response or, once the original was answered or deferred, as an ephemeral follow-up; it pings nobody (MSG-05)

**Given** a handler that uses both the ctx methods and the typestate `Responder`
**When** a second initial response is attempted through either
**Then** it returns `ResponderError::AlreadyResponded` without a request (INT-03 runtime half, unit test); the typestate Responder stays the tool for modals and multi-message flows

**Given** an option-less `#[command]` whose handler is `ctx.respond(render_x(..)).await`
**When** it is published and invoked through the fakes
**Then** it publishes with an empty option list and sends the message as its initial response (FR-61, UD-3)

**Given** `xtask skill-check`
**When** it lints the template
**Then** the doctrine lint accepts `ctx.respond(`, `ctx.update(` and `ctx.reply_text(` (the lint learned this in the 2026-10-20 correct course) and still rejects `.reply(`, `.update(`, `.defer(` and `.edit_original(` on any other receiver in an interactive handler

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's `responder.md` and `views-and-recipes.md` present the three methods as the default way to answer, the template's handlers are reconciled with them, and `api-snapshot`/`skill-check` pass

### Story 8.2: Deterministic renders, select freshness and safe text

As a bot author,
I want the same render function to produce the same message for the same state, selects that always show my declared defaults, and helpers that neutralize user text,
So that re-rendering after every click is safe and predictable.

**Implements:** FR-25, NFR-6, UX-DR1; AD-5 (`N` nonce), AD-12, AD-27; rules CID-02, MSG-05.

**Acceptance Criteria:**

**Given** a render function called twice with the same state
**When** both messages are serialized
**Then** the JSON is identical except for per-render select nonces (snapshot test)

**Given** a component built from a `select` Route
**When** the message is sent or updated through the Responder
**Then** a fresh `N` nonce is drawn for every send, so after `ctx.update(render_x(..))` the client shows the defaults the render function declared, not the user's previous local selection

**Given** user-supplied text (names, reasons, bios)
**When** it is passed through `escape_markdown` or `escape_mentions`
**Then** Markdown and mentions are neutralized (unit tests over a table of inputs), and a message listing `<@id>` still pings nobody unless the author opts in per message (MSG-05, from Story 4.2)

**Given** two Route components with the same Route and Payload in one message
**When** the message is validated
**Then** the CID-02 error names both paths and its help suggests a distinguishing Payload field (for example the target page of a prev/next pair)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents the escape helpers and select freshness, and `skill-check` passes

### Story 8.3: Budget helpers: cost and "how many items fit"

As a bot author,
I want to know how many more rows of a given shape fit beside the fixed parts of my message,
So that my render function can size a page slice itself and never hits a Limit at runtime.

**Implements:** FR-62, FR-24, UX-DR6; AD-27; rules CV2-03, CV2-04, CV2-06.

**Acceptance Criteria:**

**Given** a partly built message (its fixed parts, for example the header, tab row and prev/next row) and the App's `Limits`
**When** `noctis::ui::Budget::of(&msg, &limits)` is called
**Then** it reports the components left (40 minus the cost at every depth) and the Text Display characters left (4000 minus the counted text)

**Given** a budget and one sample item (any Container child, such as a Section with an accessory or a Text Display)
**When** `budget.fits(&item)` is called
**Then** it returns the largest `n` such that `n` copies fit both budgets (property test over random items: `n` copies validate and, when a Limit binds, `n + 1` fail)
**And** `budget.fits_prefix(items)` returns how many items from the front of an iterator of differently sized items fit in order

**Given** a list of 300 items, a page slice sized with `fits` and the fixed parts above
**When** every page is rendered and validated
**Then** no page exceeds any Limit, and stepping prev/next reaches every item (test)

**Given** a message over a Limit
**When** it is sent
**Then** it is the FR-24 error naming path, Limit and actual value; Noctis never paginates or splits it, and Lenient mode still only truncates text (FR-24)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `Budget` in `views-and-recipes.md` (pagination recipe) and `limits-cheatsheet.md`, the template's dashboard Roles tab uses it, and `skill-check` passes

### Story 8.4: Testing kit — simulated interactions and message snapshots

As a bot author,
I want to invoke Commands and click components in tests without Discord,
So that I can verify flows, response types and Limit costs in CI.

**Implements:** FR-51 (simulated interactions, snapshots), NFR-2; AD-10, AD-25; Conventions (Tests).

**Acceptance Criteria:**

**Given** `noctis::testkit` enabled through the facade's `testkit` feature in `[dev-dependencies]`
**When** a test builds `TestApp` from an `AppBuilder` with test guilds, channels, roles and users
**Then** the App is built on `Runtime::for_test()` with the Story 5.2 and 5.3 fakes, the bot's own builder function installs its plugins exactly as in production (AD-24), and it injects simulated `Inbound` interactions into the same lanes the gateway uses, answers Discord HTTP with `FakeDiscordHttp` from Story 5.2, and needs no network access

**Given** a test
**When** it calls `app.command("setup start").by(user).in_guild(guild).run()` or clicks a component by Route or label
**Then** it can assert the resulting message, response kind (reply, update, deferred, modal), ephemerality, disabled state of a button and Limit cost

**Given** a render function's output
**When** it is snapshot-tested with `insta`
**Then** the snapshot is its rendered JSON and changes only when that JSON changes

**Given** `assert_memory_flat`
**When** it runs N interactions against a handler
**Then** it fails if framework memory grows with N (NFR-2)

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's testing reference documents `TestApp` and assertions, the template's `tests/flows.rs` is reconciled with the real testkit API, and `skill-check` passes

### Story 8.5: UI recipes and example: tabs, pagination, step flows and settings screens

As a coding agent learning Noctis from the Usage Skill,
I want compiling, hand-written examples of the UI Doctrine patterns and a rulebook that tells me which pattern to use,
So that my first bot follows the UI Doctrine without any framework widget and compiles on the first attempt.

**Implements:** FR-49 (P0 demonstrations), FR-50 (recipes, rulebook, best practices), UX-DR1..UX-DR9, SM-3 groundwork; AD-17, AD-27.

**Acceptance Criteria:**

**Given** `examples/ui-recipes/` (a small focused example, added to the root `members` list with `publish = false`, so the workspace commands build and test it)
**When** it builds and its testkit tests run (`cargo test --release`)
**Then** it demonstrates, by hand and using only Payloads and Refs: an option-less entry Command calling `ctx.respond(render_x(..))`; tabs (a `#[derive(PayloadEnum)]` tab field, a `match` in the render function, a Route button row with the active tab disabled); pagination (a `page: u16` field, a slice sized with the Story 8.3 budget helper, prev/next disabled at the bounds); a 3-step flow (a `step` field, "Step n of m: Title" progress, back/next/cancel, per-step validation in the handler with an inline notice); and a settings screen (Sections with accessory buttons, an enable/disable toggle, an entity switcher select, add and delete rows, a button → modal → re-render chain that carries the same Payload)
**And** its tests assert that the active tab is disabled, prev is disabled on the first page and next on the last, no page of a 300-item list exceeds a Limit, and cancel leaves a message with no components

**Given** the Usage Skill
**When** the story is done
**Then** `references/views-and-recipes.md` contains the same recipes with complete code kept in sync with the example, the UI quality rulebook (one primary action per View, visible active state, confirmation for destructive actions via a one-shot Route, ephemeral for single-user UI and public for shared content, a way back or cancel on every interactive View, designed empty and error states), the decision table (step flow vs tabs vs pagination vs settings screen), the schwi-style best practices and pitfalls, and patterns/anti-patterns, with no example using a per-message listener (UD-7)

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

**Given** a rendered attachment (`Attachment::rendered(bytes, digest, ext)`, origin `Rendered`, AD-22) placed in a Thumbnail or Media Gallery without alt text
**When** it is validated
**Then** Strict mode returns a validation error and Lenient mode a warning (FR-30); any other image without alt text keeps Story 4.3's warning; alt text over 1024 is rejected (ATT-06)

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
**Then** it accepts only a non-interactive `StaticMessage` (a V2 message type that cannot hold buttons or selects), so interactive components through a non-owned webhook fail to compile (WH-02 trybuild fixture), and a dynamic attempt is a typed error

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

**Given** `[workspace.dependencies]`
**When** the image stack is added
**Then** it pins `takumi = "=2.17.0"` and `takumi-core = "=0.31.0"` (takumi 2.17.0 depends on `takumi-core ^0.31.0`, so both are exact), `noctis-image` gains the optional takumi dependencies behind its `render` feature, and `Cargo.lock` stays committed

**Given** `xtask tw-grammar` (the generation approach of AD-15; no spike needed)
**When** it runs
**Then** it locates the pinned `takumi-core` source through `cargo_metadata` (`manifest_path` of `takumi-core 0.31.0`), `syn`-parses `src/style/tw/map.rs`, and extracts the keys of the `FIXED_PROPERTIES` `phf_map!` (exact utilities), the `PREFIX_PARSERS` `phf_map!` (utility prefix → parser variants), the `property_parsers!` invocation (parser variant → value parser type) and `VAR_TARGETS` (theme namespaces); variant prefixes (`sm:`, `!`) follow `split_variant` in `src/style/tw/mod.rs`
**And** every value parser type maps to a suffix grammar in a closed table kept in `xtask` (spacing scale, fractions, named colors with `/opacity`, keyword sets, arbitrary `[…]` values); a parser type missing from the table, or a source file whose shape no longer matches, fails generation with an error naming it
**And** it writes the committed `crates/noctis-image/src/tw/generated.rs` (takumi-free, builds with the `render` feature off), together with theme-token hooks and a v3 → v4 rename table (for example `bg-gradient-to-r` → `bg-linear-to-r`); running it again on an unchanged pin produces no diff (CI checks this)

**Given** the generated grammar
**When** the CI job `tw-crosscheck` runs
**Then** for every grammar entry's sample classes and a committed corpus of near-misses (v3 names, typos, unknown prefixes), it compares the grammar's verdict with `takumi_core`'s public `TailwindValues::from_str`, where a class is recognized when parsing it alone yields a non-empty value list; any disagreement fails the job and names the class, and a smoke render of every sample checks that rendering doesn't panic

**Given** a runtime class string
**When** it is checked by `tw::validate`
**Then** unknown classes are reported with a v4 suggestion when the rename table knows one

### Story 10.2: Templates with compile-time class validation and a Theme

As a bot author,
I want to write image Templates in Rust with Tailwind classes that are checked by `cargo check`,
So that a typo in a class fails the build instead of rendering nothing.

**Implements:** FR-32, FR-66, FR-75, UX-DR2; AD-2, AD-15, AD-22 (Template contract); rules ATT-07.

**Acceptance Criteria:**

**Given** `#[derive(Template)]` with `#[template(id, version, size, format)]` (plus an optional `animation(duration_ms, fps, format)`) and a `view!`/`tw!` body
**When** it compiles
**Then** literal class strings are validated against `noctis_image::tw` and the tokens declared in the crate's `noctis-theme.css` (parsed with `cssparser`, tracked with `include_bytes!` so edits rebuild); an unknown class is a compile error, and a known v3 name's error suggests its v4 replacement (trybuild fixtures under `tests/ui-image/tw/`, run in the `image` feature run)
**And** the derive implements `noctis_ports::Template` (the Story 3.1 contract: `ID`, `VERSION`, `SIZE`, `FORMAT`, `ANIMATION`, `view()` returning the `ErasedView` that holds the decoder `fn(&[u8], Frame) -> Result<Node, RenderError>`, which postcard-decodes the value and calls `TemplateView::view(&self, frame)`), requires the struct to be `Serialize + Deserialize` (a missing `Deserialize` is a compile error naming the derive), and `Module::template::<T>()` records its `TemplateDescriptor`; `App::build` rejects two Templates with one id

**Given** user-supplied data
**When** it enters a Template
**Then** it can only be text content, never a class string or markup, and dynamic values (widths, colors, percentages) go through typed style setters

**Given** a class string built at runtime with an unknown class
**When** it renders
**Then** a warning is logged once per distinct string, and on an App whose `Runtime` is `Runtime::for_test()` (every testkit App) the same string is an error, so the test fails

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

**Given** the `image` feature and an App builder that calls `.image(ImageConfig)` or `.theme(Theme)` from `noctis::image::AppBuilderImageExt` (in the prelude with `image`; AD-24)
**When** the App is built
**Then** the registered `ImagePlugin`'s `install` gets or creates the render pool from the App's `Runtime` (`resource_or_init`) and installs a per-App `Renderer` bound to the App's Theme through `AppBuilder::renderer(..)`; renders run with takumi `=2.17.0` on that process-scoped rayon pool, never on tokio workers or a pool shared with the author; `Renderer::register` receives every `TemplateDescriptor` at App build and fails the build for one it can't render
**And** `deps-check` shows the `noctis-image → noctis-core` edge exists only behind `render`, and core still has no edge to `noctis-image`

**Given** a handler calling `ctx.render(template).await`
**When** the render takes longer than the ack deadline
**Then** core postcard-encodes the Template value into a `RenderJob { template_id, template_version, data, size, format, animation }`, calls the App's `Arc<dyn Renderer>` (core has no dependency on `noctis-image`; `deps-check` proves it), the FR-21 auto-defer covers the wait, and the result is `Attachment::rendered(output.bytes, output.digest, output.format.extension())` placed in a Thumbnail or Media Gallery
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
**Then** the Usage Skill documents `ctx.render`, `.image(..)`/`.theme(..)`, attachments of renders and the feature flags, and `skill-check` passes

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

**Given** a `RenderJob` whose `animation` is `Some(AnimationSpec { duration, fps, format })` (from the Template's `ANIMATION` or `ctx.render_with(t, RenderOptions::new().animation(..))`)
**When** it renders
**Then** the Renderer calls the Template's view once per frame with `Frame { index, count }` (count = duration × fps), so a code-driven animation computes one Template state per frame from one data value, and a CSS-driven one ignores the frame and lets takumi advance its keyframes

**Given** a code-driven animation
**When** a frame's root node declares an explicit size different from the job's size
**Then** a typed error is returned (FR-69: every scene shares one size)

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
**Then** startup fails with a clear typed error; with `default-font` on, the bundled default font is used, and its license is recorded in the crate's `NOTICE` and accepted in `deny.toml`

**Given** fallback fonts registered with a priority order and Unicode ranges (for example CJK)
**When** a Template renders mixed-script text
**Then** glyphs come from the matching fallback without any Template change (pixel test)

**Given** fonts registered through `.image(ImageConfig::new().font(..))`
**When** `ImagePlugin::install` runs
**Then** the font registry is a process resource of the App's `Runtime` (`resource_or_init`, AD-24); a second App on the same Runtime configuring a conflicting registry is a `BuildError`, while an App on `Runtime::for_test()` gets its own

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
**Then** they render as color glyphs (pixel test), and the Twemoji graphics' CC-BY-4.0 attribution is recorded in the crate's `NOTICE` and the feature docs

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

**Given** a `TestApp` built from the bot's builder function with the `image` and `testkit` facade features (so its `ImagePlugin` installed a Renderer; AD-24)
**When** a test calls `assert_render_snapshot!(app.renderer(), "name", template, tolerance = ..)`
**Then** the testkit renders through the App's `Renderer` port (no `testkit → image` edge), decodes the output with `png` 0.18, compares against a stored PNG with the tolerance and fails beyond it; the first run writes the snapshot

**Given** the reference card Template (a copy of the Project Template's profile card, ≈934×282, PNG, warm caches, kept with the benches)
**When** the reference card bench in `noctis-image/benches` (AD-25; it needs no core, so no forbidden edge) runs on `main`
**Then** it reports p50 and p99 against `budgets.toml` (targets p50 ≤ 10 ms, p99 ≤ 50 ms with the pool saturated), reporting only until PRD Q3 confirms the numbers

**Given** a commit that bumps takumi (both pins together)
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

**Given** `noctis::i18n!(default = "en-US", dir = "locales")` (both arguments optional with these defaults, AD-16) and YAML files whose stems are exact Discord locale codes (`en-US.yaml`, `tr.yaml`)
**When** the crate compiles
**Then** the macro parses them with `yaml-rust2`, emits `include_bytes!` for every file it read so edits rebuild the crate, and generates `Strings::of(locale)` with nested accessors mirroring the YAML tree (one method per leaf, arguments in placeholder order, named or positional), a `MessageKey` const per key in `keys::*` (dots → `_`, uppercased), and a `catalog()` descriptor; keys under `cmd.`, `cmdword.` and `ctxcmd.` get no accessors and no `keys::` consts (AD-16)

**Given** trybuild fixtures under `tests/ui-i18n/loc/` (run in the `i18n` feature run)
**When** the UI tests run
**Then** a key missing from the default locale, a call with the wrong number or type of arguments, and a locale file whose stem isn't an exact Discord code (LOC-01; `en.yaml` fails with a help naming `en-US.yaml` and `en-GB.yaml`) fail to compile

**Given** a key missing in a non-default locale
**When** the crate compiles
**Then** a build warning is emitted and the default value is used at runtime (LOC-04)

**Given** a string with the same placeholder twice
**When** it is formatted
**Then** every occurrence is replaced

**Given** the new public API
**When** the story is done
**Then** the Usage Skill's i18n reference documents `i18n!` and the file naming, the template's `strings.rs` and `locales/{en-US,tr}.yaml` are reconciled, and `skill-check` passes

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

**Given** an interaction with `locale = "en-GB"` and only an `en-US` file, one with `locale = "pt-BR"` and no `pt-BR` file, or a `tr` user in an `en-US` guild
**When** stage 4 resolves locales
**Then** interactions resolve User → Guild → default, where a code with no file falls back to its `Locale::sibling()`'s file before the next step (`en-GB` → `en-US`; `pt-BR` has no sibling and moves on), and handlers read both via `ctx.user_locale()` and `ctx.guild_locale()`

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

**Implements:** FR-41, FR-12 (whole-name context menus), FR-13, FR-77 (descriptions from locale files); AD-16; rules LOC-02, LOC-03, LOC-05, LOC-06, CMD-01..CMD-05, CTX-01, OPT-09.

**Acceptance Criteria:**

**Given** keys `cmd.<path>.name`, `cmd.<path>.description`, `cmd.<path>.opt.<opt>.name|description|choice.<value>`, per-word `cmdword.<word>` and `ctxcmd.<key>.name` (from `#[user_command(.., key = "…")]`/`#[message_command]`), where `<path>` is the Command Path's words joined by `.` and written as nested YAML maps (`cmd: { setup: { description: …, start: { description: … } } }`; AD-16)
**When** Publish builds JSON from the merged catalog
**Then** it emits Discord's `name_localizations`/`description_localizations`, a full-path key overrides the per-word translation, and context-menu names localize as whole names

**Given** the `i18n` feature and a default-locale file (AD-16)
**When** `#[command]` compiles
**Then** it reads `cmd.<path>.description` (and option descriptions) from that file, using the same `dir`/`default` as `i18n!` or `#[command(.., locales = "<dir>")]`; the literal `description = "…"` may be omitted, a literal that differs from the file fails to compile, and a missing key with no literal fails with CMD-04, and a Command word equal to `name`, `description` or `opt` fails because its keys would be ambiguous (trybuild fixtures under `tests/ui-i18n/`); without the feature, or without a default-locale file, Story 6.1's literal rule applies unchanged

**Given** an `en-US` file and no `en-GB` file
**When** Publish runs
**Then** `en-US` values are emitted for both `en-US` and `en-GB` (`Locale::sibling()`); an `en-GB` file, when present, is used for `en-GB` instead

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
**When** the process restarts within the resume window (`FakeGateway`)
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

**Given** `--no-resume` (added to the Story 6.9 command-line parser)
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
**Then** twilight keeps routing through core's Discord edge (AD-24, Story 5.9), which forwards to the proxy, with `ratelimiter(None)`, and a test with a mock proxy shows no request bypasses it (including raw REST, webhook requests and interaction callbacks)
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

### Story 12.5: Discord cache wiring, `ctx.cache()`, configuration and minimal intents

As a bot author,
I want to read cached Discord resources through `ctx.cache()`, choose which resources are cached, back the cache with my own storage, or disable it,
So that memory stays proportional to what my features actually use.

**Implements:** FR-40, FR-38 (DiscordCache port), FR-6 (intents); AD-1, AD-14, AD-21, AD-25; rules GW-05.

**Acceptance Criteria:**

**Given** the `DiscordCache` port and `MemoryDiscordCache` from Stories 3.1 and 3.2
**When** the App runs
**Then** core feeds the cache every gateway event its configured resources need (these event types join the Story 5.6 filter), before the event reaches handlers, and every context (`CommandCtx`, `ComponentCtx`, `ModalSubmitCtx`, `AutocompleteCtx`, `EventCtx`) exposes `ctx.cache()`, a typed read handle with owned getters (`guild`, `channel`, `role`, `member`, `user`, `message`, per-guild lists)
**And** `.discord_cache(DiscordCacheConfig::…)` chooses resources or `none()`, and `.discord_cache_backend(impl DiscordCache)` replaces the storage; with the cache disabled, getters return a typed `CacheDisabled` error

**Given** default configuration
**When** the App is built
**Then** only resources needed by enabled framework features are cached, and the default intents are the minimum required by registered Event Handlers and cache resources (`Intents::GUILDS` for an interaction-only bot)

**Given** a cache resource or handler needing an intent that isn't enabled
**When** `App::build` runs
**Then** it fails with a typed error naming the resource and intent (GW-05)

**Given** the cache API
**When** a handler reads it
**Then** it returns owned or shared values; no guard that could be held across `.await` is exposed (clippy lint test or compile-fail fixture)

**Given** a Discord cache backed by an out-of-process `DiscordCache` implementation
**When** two in-process Apps standing in for Cluster processes read it
**Then** they see the same data

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents `ctx.cache()`, cache configuration and the backend swap, the template's `ctx.cache()` reads (dashboard roles, profile user) are reconciled, and `skill-check` passes

### Story 12.6: Memory-per-guild benchmark

As a bot operator,
I want framework memory per idle guild measured in CI with the Discord and render caches reported separately,
So that I can size deployments and regressions are visible.

**Implements:** NFR-1, UX-DR8; AD-25, AD-28.

**Acceptance Criteria:**

**Given** the memory bench in `noctis-core/benches` with a stated default cache configuration (no members, no messages)
**When** it loads 10k idle guilds through `FakeGateway`
**Then** it reports framework bytes per guild, Discord cache bytes and Renderer cache bytes separately against `budgets.toml` (initial goal ≤ 1 KiB per idle guild excluding the Discord cache)
**And** because twilight-cache-inmemory exposes no size, Discord cache bytes are the difference in live heap bytes (dhat counting allocator) between two otherwise identical runs, one with the default cache configuration and one with `DiscordCacheConfig::none()`; framework bytes come from the `none()` run minus the empty-App baseline, and Renderer cache bytes from the memory `RenderCache`'s byte weigher

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

**Given** a tree three or more levels deep built with `Module::module(..)` (basic nesting exists since Story 5.6)
**When** the tree is mounted on the App
**Then** registrations of every nested Module are folded into the Registry, each Module's id is its full path, and parent-before-child ordering applies to Middleware at every level

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
**When** the App starts with `--features a,b` (or `all`, or the equivalent config; the flag joins the Story 6.9 parser)
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

**Given** `#[command("profile", rate_limit(user, 3, per = "10s"))]` or `#[handler(rate_limit(..))]` on a Route's handler (AD-6: never on `#[route]`)
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
catalog has no pending rows, the launch documents exist, the release pipeline is
ready, and `noctis` 0.1.0 is published to crates.io (PRD §8.3 launch criteria). The
last two stories (14.8, 14.9) are maintainer-gated (Story conventions).

### Story 14.1: Adapter conformance suites

As an author of a Redis or SQL adapter,
I want a reusable conformance suite per port,
So that I can prove my implementation has the semantics Noctis relies on.

**Implements:** FR-39, FR-71; AD-14.

**Acceptance Criteria:**

**Given** `noctis::testkit::conformance::<port>` suites for every store port, `IdentifyQueue` and `DiscordCache` (owned reads, event updates, disabled resources)
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
**Then** all pass, covering: an option-less entry Command opening a View (`/dashboard`), a hand-written step flow (`/setup start`), a hand-written tabbed View with a paginated list, a Template rendered into a Media Gallery (`/profile`), a custom Adapter (`BoundedCacheStore`) and `en-US`/`tr` Locale files
**And** the `template-build` job switches from tracking mode to a required gate, the template's Template pixel snapshots and Adapter conformance test join the required gates (NFR-8), and the "written against the planned API" notes are removed from the template sources

**Given** the template
**When** `xtask skill-check` lints it
**Then** no handler responds with an embed or plain `content` as primary output, there is no manual response-type selection, and no per-message collector, await-component loop or inline listener exists

**Given** the template's user code
**When** its lines are counted (excluding tests, locales and the theme)
**Then** the count for sharding, auto-published Commands, the 3-step setup flow and the rendered card is reported against the SM-1 goal of ≤ 150 lines

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
**When** the story records them
**Then** `budgets.toml` keeps each PRD target the measurement meets and, for a target it misses, records the measured value plus 10% as the gate with a note naming the PRD target (the maintainer delegated this decision); the latency, memory (Story 12.6) and allocation jobs switch from reporting to gating on `main`, and the changelog notes which NFR-1/3/9 assumptions were confirmed and which were re-set

### Story 14.4: Rules catalog completeness audit

As a Noctis maintainer,
I want every catalog rule enforced and tested at its declared point before release,
So that the NFR-10 promise "static input is always checked at compile time" holds for v1.

**Implements:** FR-75, FR-79, NFR-10; AD-3, AD-4; rules CMD-07, CMD-14, LOC-04, MSG-02, REG-06 (documentation-only confirmation).

**Acceptance Criteria:**

**Given** `xtask/rules-pending.toml`
**When** this story is done
**Then** it is empty (P2-only rows such as prefix-command rules may remain only if tagged `p2`), and `xtask rules-catalog` passes with every Noctis-diagnosed Compile-time row covered by a trybuild fixture whose `.stderr` contains its id, every `Compile-time (by construction)` row covered by a `compile_fail` doctest or fixture whose source carries `// [<ID>]`, and every App-build or Runtime row by a unit test citing its id
**And** the by-construction rows are exactly those the catalog marks so (at Gate 2: CV2-01, CV2-02, CV2-12, CV2-14, CID-05, CID-07, INT-03, INT-04, INT-06, INT-08, INT-09, INT-10, MSG-03, MSG-05, MSG-07, WH-01, WH-03, META-05, OPT-12, OPT-14, CTX-02, CTX-03, ATT-05, ATT-07, GW-07); each has its comment-tagged evidence

**Given** the audit
**When** each rule with static input is reviewed
**Then** none is checked only at runtime; any found is moved to compile time or recorded in the catalog with the reason its input is dynamic
**And** documentation-only rows are confirmed as such in the catalog: the discovery restriction of CMD-07, REG-06 (out of scope for v1), CMD-14/LOC-04 (permitted, no check) and MSG-02 (no embed API in v1); LOC-05 and MOD-08 are covered through CMD-05 and CID-01

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

### Story 14.7: Release pipeline ready for 0.1.0

As a Noctis maintainer,
I want the release pipeline, final feature matrix and changelog finished and dry-run green,
So that publishing 0.1.0 needs only my repository setup and one merge.

**Implements:** FR-48, NFR-7, NFR-8; AD-2, AD-28.

**Acceptance Criteria:**

**Given** the facade features
**When** CI runs the final feature matrix
**Then** each documented feature alone (`cargo hack --each-feature`) and all features together build and test (`image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `testkit`; `prefix` once Epic 15 lands), default features build without the image stack, and `cargo-semver-checks` runs on `noctis`

**Given** `release-plz.toml` and `.github/workflows/release.yml`
**When** the story is done
**Then** release-plz is configured to open the release PR from `main` and, on merge, publish every workspace crate except `xtask` at the shared version in layer order from a job bound to the protected `release` environment that holds `CARGO_REGISTRY_TOKEN`; a `cargo publish --dry-run` of every crate in layer order passes in CI without any token
**And** `CHANGELOG.md` has the 0.1.0 entry with migration notes, and the runbook `docs/guides/release.md` lists the maintainer steps of Stories 14.8 and 14.9

### Story 14.8: [Maintainer-gated] Repository settings and release secrets

As the Noctis maintainer,
I want the repository configured for releases,
So that release-plz can open its PR and the release job can publish.

**Implements:** AD-28, NFR-8.

**Acceptance Criteria:**

**Given** the runbook from Story 14.7
**When** the maintainer applies it
**Then** GitHub Actions may create pull requests (Settings → Actions → General), a protected `release` environment restricted to `main` holds `CARGO_REGISTRY_TOKEN` (a crates.io token scoped to publishing the `noctis` crates), and, if the maintainer wants it, branch protection lists the `ci.yml` gates as required for pull requests without blocking the maintainer's direct pushes to `main`
**And** the next push to `main` makes release-plz open the 0.1.0 release PR

**Given** an unattended run
**When** it reaches this story
**Then** it skips it, reports it as waiting for the maintainer, and continues with Epic 15

### Story 14.9: [Maintainer-gated] Publish `noctis` 0.1.0

As a bot author,
I want `noctis` 0.1.0 on crates.io with all sub-crates published consistently,
So that I can depend on it from my own bot.

**Implements:** FR-48, FR-49 (template on the published crate), NFR-7; AD-2, AD-28.

**Acceptance Criteria:**

**Given** the release PR opened after Story 14.8
**When** the maintainer merges it
**Then** all workspace crates are published at the shared version in layer order from the protected release environment, and the changelog entry is included

**Given** the published crate (the agent's follow-up once it is on crates.io)
**When** the Project Template's `noctis` dependency is switched to `version = "0.1"`
**Then** the copied-out template builds with `cargo build --release`, and the README roadmap marks Milestones 1–4 done

**Given** an unattended run
**When** it reaches this story before the publish has happened
**Then** it skips it and reports it as waiting for the maintainer

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

**Given** two Apps attached to one `noctis::Runtime` (`Runtime::global()` or one passed with `AppBuilder::runtime(&rt)`)
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

**Implements:** FR-14, FR-10 (same typed errors); AD-23, AD-24; rules GW-05.

**Acceptance Criteria:**

**Given** the `prefix` feature off (default)
**When** a bot builds
**Then** no prefix parsing code is compiled and the App needs no message-content intent

**Given** the `prefix` feature on, `.plugin(noctis::prefix::PrefixPlugin::new("!"))` on the builder, and a Command opted into prefix mode
**When** a message `<prefix><Command Path> <args>` arrives
**Then** the `MESSAGE_CREATE` Event Handler that `PrefixPlugin::install` mounted parses the prefix, Command Path and arguments and calls `Dispatcher::run_command` (Story 6.5, AD-24), which runs pipeline stages 3 to 10 and the same handler through the message-reply Responder; argument parse failures produce the same typed errors as FR-10, and core contains no prefix code
**And** `PrefixPlugin::install` requires the message-content intent, so App build fails without it when prefix Commands exist (GW-05)

**Given** the final feature matrix
**When** CI runs
**Then** `prefix` alone and with all features builds and tests

**Given** the new public API
**When** the story is done
**Then** the Usage Skill documents prefix commands as an opt-in P2 feature, and `skill-check` passes
