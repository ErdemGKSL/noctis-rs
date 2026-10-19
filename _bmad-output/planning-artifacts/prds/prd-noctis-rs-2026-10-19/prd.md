---
title: "PRD: Noctis"
status: draft
created: 2026-10-19
updated: 2026-10-19
---

# PRD: Noctis

## 0. Document Purpose

This PRD defines what Noctis v1 must do. Noctis is a Rust framework for building
large, UI-heavy Discord bots on twilight-rs. It is written for the architect and for
the epic and story authors who come next, and it builds on these inputs:
- the product brief (`../../briefs/brief-noctis-rs-2026-10-19/brief.md` and its
  addendum);
- four research reports in `../../research/`: tescord features, Discord Components
  V2, twilight-rs and takumi.

Features are grouped, and their FRs are numbered globally (FR-1 … FR-N) so later
documents can refer to them. Terms in §3 are used verbatim. Inline `[ASSUMPTION]` tags
are collected in §10. Technical "how" (crate names, libraries, data structures)
lives in `addendum.md` and in the architecture document, not here.

## 1. Vision

Noctis lets a Rust developer build a Discord bot that behaves like a small
application. Slash commands open rich, navigable Components V2 dashboards, generated
images show data at a glance, and the whole bot runs sharded across all CPU cores
for millions of users. Writing it should feel as direct as tescord, the TypeScript
framework the author uses today, and Rust's type system should catch the mistakes
that tescord only finds at runtime.

Raw twilight leaves every team to rebuild the same things: shard supervision,
command trees, publishing, `custom_id` parsing, interaction state and image
rendering. Noctis provides them as one coherent, opinionated layer. A single rule
runs through all of it: everything that stores or caches data is a trait with a good
default, so a bot can start in memory and move to its own database without
rewriting handlers.

## 2. Target User

### 2.1 Jobs To Be Done

- **Bot author:** When I add a feature, I declare a command and a View and get a
  working, paginated, limit-safe dashboard. I don't hand-assemble JSON or count
  components.
- **Bot author:** When my bot grows, I raise shard counts, split shards across
  processes, and swap in-memory stores for my database. I don't touch my handlers
  when I do.
- **Bot author:** When I need a status card, profile or leaderboard, I write a
  Tailwind-styled Template in Rust and get a PNG, WebP or GIF attached to my
  message. No separate service or browser is involved.
- **Bot author coming from tescord/dbi:** I want the same mental model (command
  names with spaces, auto-publish, locale files, modules) and stronger guarantees.
- **AI agent writing a bot:** I can learn the full public API from the Noctis
  Usage Skill in the Project Template, without reading Noctis source.

### 2.2 Non-Users (v1)

- Developers who want a minimal, unopinionated gateway wrapper. Raw twilight already
  serves them.
- Voice and music bot authors.
- Teams whose runtime is not tokio.

### 2.3 Key User Journeys

- **UJ-1. Deniz ships a setup wizard in an afternoon.** Deniz maintains a moderation
  bot. They create a project from the Project Template and register the Command
  `"setup start"`, which takes no options. The Command responds with a Wizard View:
  a Container with a progress header and three Steps (log channel, roles,
  confirmation). Each click edits the same message. Selected values travel in
  Payloads or Refs and are never kept in a per-message listener. Deniz runs the bot
  with the publish flag, and `/setup start` appears in the test guild. **Edge
  case:** a user opens the wizard, leaves for two days and clicks "Next". The Ref
  has expired, so Noctis routes the click to the Expired Handler, which re-renders
  step 1 with a notice.
- **UJ-2. Deniz adds a server dashboard with tabs.** The Command
  `"dashboard"` sends a Container with a Navbar (Overview, Members, Logs). Members
  has 300 entries, so the Paginator splits them into pages under the component
  budget, and the Navbar stays on every page. The Overview tab embeds a generated
  activity-chart image in a media gallery.
- **UJ-3. Ece scales from one process to a cluster.** Ece's bot reaches 400k guilds.
  She changes configuration so each of four processes runs one Shard Range, and
  switches the RefStore and RateLimitStore to her Redis-backed implementations.
  Handler code is unchanged. Restarting a process resumes its Shards' sessions
  instead of re-identifying, using her SessionStore implementation.
- **UJ-4. Mert migrates a tescord module.** Mert ports a tescord pack that registers
  `"config (set|get) (prefix|welcome)"`. The same pattern string expands to the same
  four commands in Noctis. His handler gets a typed enum telling it which variant
  was invoked, and option values arrive as a typed struct.
- **UJ-5. An AI agent writes a leaderboard feature.** Given the Project Template, a
  coding agent reads the Noctis Usage Skill and writes a Template, a View and a
  Command that compile on the first attempt. It never opens Noctis source.

## 3. Glossary

- **App** — The root object a bot builds. It holds configuration, Adapters and
  Modules, and runs Shards. One process may run several Apps (white-label bots).
- **Module** — A named, composable group of Commands, Component Handlers, Modal
  Handlers, Event Handlers and Middleware. Modules nest, and the App is the root
  Module. A Module can be enabled or disabled at runtime.
- **Shard** — One gateway connection. A **Shard Range** is the subset of Shards one
  process runs; a **Cluster** is the set of processes running all Shard Ranges.
- **Command** — A chat-input (slash) command, user context-menu command or message
  context-menu command registered by a Module.
- **Command Path** — The space-separated name of a chat-input Command
  ("settings logs channel"). It maps to Discord's command, subcommand group and
  subcommand. A **Command Pattern** is a Command Path with `(a|b)` alternatives and
  `?` optional groups.
- **Publish** — Synchronising the registered Commands with Discord, globally or
  per guild, only when they differ.
- **Route** — The stable identifier of a Component Handler or Modal Handler. It
  forms the start of every `custom_id` the framework emits.
- **Payload** — Typed data attached to a component's `custom_id` after its Route.
  It is encoded inline when it fits.
- **Ref** — A short, unguessable key placed in a `custom_id` in place of a Payload
  that is too large or explicitly stored. The Payload itself lives in the RefStore
  and expires after a TTL.
- **Component Handler / Modal Handler** — The single function registered for a
  Route. It handles every component or modal using that Route, across all messages,
  Shards and restarts.
- **Event Handler** — A function registered for one gateway event type.
- **Middleware** — An ordered hook that runs before or after interactions or events.
  It can stop processing or attach typed Extensions that reach the handler.
- **Adapter** — A trait with read/write methods for a storage-like concern, plus
  a default in-memory implementation. The bot author may supply their own struct
  instead. Named Adapters: RefStore, SessionStore, RateLimitStore, RenderCache,
  AssetStore, LocaleSource and CacheStore.
- **View** — A function from state to a Components V2 message. The same View
  renders the first response and every update.
- **Navbar** — A row of tab buttons at the top of a View. Clicking a tab swaps the
  Container's content. The active tab is shown disabled.
- **Wizard** — A View made of ordered **Steps**, with a progress indicator and
  next/back navigation, used for setup-style flows.
- **Paginator** — A component that splits content into pages that each fit the
  Limits, with page navigation.
- **Limits** — The Discord Components V2 constraints that Noctis enforces, such as
  40 components per message and 4000 text characters. Research §0 lists them all.
- **Template** — A Tailwind-styled image layout written in Rust, rendered by the
  Renderer.
- **Renderer** — The service that turns a Template plus data into PNG, JPEG, WebP,
  GIF or APNG bytes, on a bounded pool.
- **Locale** — A language and the translated strings for Command metadata and
  content. The **User Locale** and **Guild Locale** are resolved per interaction.
- **Expired Handler** — The handler that runs when a component's Ref is missing or
  expired.
- **Project Template** — `examples/project-template/`, a ready-to-copy bot
  project. It includes the **Noctis Usage Skill** and an `AGENTS.md`.

## 4. Features

### 4.1 App, Gateway and Sharding

**Description:** A bot author builds an App with a token, intents, Modules and
Adapters, then runs it. Noctis follows twilight's recommended gateway model. It
fetches the recommended shard count and identify concurrency, starts Shards
concurrently on a multi-threaded tokio runtime, and dispatches events to handlers
without blocking the gateway. The same App runs either as a single process or as one
Shard Range of a Cluster. Realizes UJ-3.

#### FR-1: App builder and lifecycle
A bot author can build an App from a token, intents, Modules and Adapters, start
it, and shut it down gracefully.
**Consequences (testable):**
- Startup fails with a typed error, not a panic, when it runs outside a tokio runtime
  or without a TLS crypto provider. Noctis installs a default provider when none is
  set.
- On a shutdown signal, Shards close and in-flight handlers finish or time out
  within a configurable grace period.

#### FR-2: Automatic sharding
The App can start the recommended number of Shards, or an explicit total, honouring
Discord's identify concurrency.
**Consequences (testable):**
- With `max_concurrency = 16`, Shards in the same bucket identify in parallel, not
  one at a time.
- The shard total can be fixed by configuration, or taken from Discord when it is
  not set.

#### FR-3: Shard Ranges for Clusters
A bot author can run a configured Shard Range (for example, shards 32–63 of 128)
in each process.
**Consequences (testable):**
- A process runs only Shards in its range, and Command Publish runs from exactly one
  designated process.

#### FR-4: Session resume
Shards can resume sessions after a process restart, using the SessionStore Adapter.
**Consequences (testable):**
- Restarting a process within Discord's resume window resumes its sessions instead
  of re-identifying, when the SessionStore persists across restarts.

#### FR-5: Concurrent event dispatch with backpressure
Gateway events dispatch to handlers concurrently, under a configurable concurrency
bound.
**Consequences (testable):**
- A handler that sleeps for 10 s does not delay heartbeats or other handlers.
- When the bound is reached, new work waits; nothing is silently dropped. A metric
  exposes the queue depth.

#### FR-6: Typed Event Handlers and derived event filtering
A Module can register Event Handlers per gateway event type.
**Consequences (testable):**
- Noctis asks the gateway to deserialise only the event types that have a
  registered handler (or that the cache needs). Unhandled event types are not
  deserialised.
- Event Handlers run concurrently by default. A Module can mark one ordered, so it
  runs before the concurrent handlers for that event.

### 4.2 Commands and Publish

**Description:** Commands are declared in tescord style. A Command Path such as
`"settings logs channel"` becomes command, subcommand group and subcommand. Command
Patterns expand to every combination, and Commands from different Modules that
share a base word merge into one Discord command. Options are typed. Publish
compares the local definition with Discord's and updates only on change. Realizes
UJ-1, UJ-4.

#### FR-7: Command Paths
A Module can register a chat-input Command by Command Path.
**Consequences (testable):**
- A Command Path with 1, 2 or 3 words publishes as a command, a subcommand, or a
  subcommand group plus subcommand.
- More than 3 words, an empty word, or a word that breaks Discord's name rules fails
  registration with a typed error.

#### FR-8: Command Patterns
A Module can register one handler for a Command Pattern such as
`"config (set|get) (prefix|welcome)?"`.
**Consequences (testable):**
- The pattern expands to every combination, using the same semantics as tescord.
- The handler receives which combination was invoked.

#### FR-9: Merge rules
Commands from different Modules that share a base word merge into one Discord
command.
**Consequences (testable):**
- Registration fails with a typed error when two registrations would publish
  conflicting `default_member_permissions`, `contexts`, `integration_types` or
  `nsfw` on the same base command. Noctis does not apply "first wins".
- Registration fails with a typed error when a base command would have both its own
  options and subcommands. Noctis does not silently drop either.

#### FR-10: Typed options and autocomplete
A bot author can declare a Command's options as a typed struct, with descriptions,
choices, ranges, channel types and autocomplete.
**Consequences (testable):**
- The handler receives the parsed struct. A missing required option or a wrong
  option type is a typed error, never a panic.
- An autocomplete handler returning more than 25 choices is truncated to 25, and a
  warning is logged.

#### FR-11: Command metadata
A bot author can set `default_member_permissions`, `contexts`, `integration_types`,
`nsfw` and a guild scope per Command.

#### FR-12: Context-menu Commands
A Module can register user and message context-menu Commands.

#### FR-13: Publish with diff
A bot author can Publish Commands globally or to specific guilds, and can run a
dry run that prints the difference.
**Consequences (testable):**
- Publish makes no write request when the remote state equals the local state.
- A dry run produces the full JSON and a human-readable diff without writing.
- Publish can run as a CLI mode of the bot binary (e.g. `--publish`,
  `--publish-guild <id>`).

#### FR-14: Prefix commands (opt-in) `[ASSUMPTION: v1 includes an opt-in module]`
A Module can expose a chat-input Command as a prefix message command that reuses the
same handler through a shared response abstraction.

### 4.3 Interaction Routing, Payloads and Refs

**Description:** Noctis never registers a listener per message or per component
instance. Every component and modal belongs to a Route, and each Route has exactly
one handler. A `custom_id` is the Route, a separator and a compact, typed Payload.
When the Payload doesn't fit in 100 characters, or is marked to be stored, Noctis
puts it in the RefStore and writes a Ref instead. Realizes UJ-1, UJ-2, UJ-3.

#### FR-15: Route registration
A Module can register exactly one Component Handler or Modal Handler per Route.
**Consequences (testable):**
- Registering the same Route twice is a typed error.
- Dispatch from `custom_id` to handler takes constant time, independent of the number
  of registered Routes.

#### FR-16: Typed Payloads
A bot author can attach a typed Payload to a component, and the handler receives the
decoded value.
**Consequences (testable):**
- Encoding and then decoding a Payload gives back an equal value (property-tested).
- Strings in a Payload may contain the separator character; the encoding escapes it.
- A `custom_id` built by Noctis never exceeds 100 characters. Where field widths
  are bounded, the bound is checked at compile time; otherwise it is checked at run
  time, as are uniqueness rules within a message.

#### FR-17: Refs with overflow policy
When a Payload would exceed the `custom_id` limit, or when a Route is marked
by-ref, Noctis stores the Payload in the RefStore and encodes a Ref.
**Consequences (testable):**
- A Ref is a random key of at most 12 characters, so it can't be guessed or
  enumerated.
- Each Route can set a TTL; otherwise the App default applies. A Route can also mark
  its Refs one-shot (deleted on use).
- The overflow policy is configurable per Route: spill to the RefStore, or return an
  error.

#### FR-18: Expired Handler
When a Ref is missing or expired, the interaction goes to the Expired Handler.
**Consequences (testable):**
- The default Expired Handler replies with a short ephemeral localized notice. A bot
  author can override it per App or per Route, for example to re-render the
  starting View.

#### FR-19: Untrusted input
Noctis treats every decoded Payload and Ref as untrusted client input.
**Consequences (testable):**
- A malformed `custom_id`, an unknown Route or an undecodable Payload produces a
  typed error and a logged warning. It never causes a panic.

#### FR-20: Middleware with typed Extensions
A Module can register Middleware before and after interactions and events.
**Consequences (testable):**
- A Middleware can stop processing.
- A Middleware can attach a typed Extension, which the handler can read by type.
- Middleware ordering is deterministic: registration order, parent Module before
  child.

#### FR-21: Automatic acknowledgement
If a handler has not responded before a configurable deadline (default about
2.5 s), Noctis sends the right deferred response. Later responses edit the
original message.
**Consequences (testable):**
- A handler that responds after 5 s still delivers its message, with no
  "interaction failed" shown to the user.
- On a deferred chat-input response, the Components V2 flag is applied on the
  follow-up edit, as Discord requires.

#### FR-22: Error routing
Errors returned by handlers go to configurable error sinks.
**Consequences (testable):**
- By default, the user sees a short, localized, ephemeral error, and the full error
  is logged through `tracing` with interaction context.

### 4.4 Components V2 UI Kit

**Description:** Components V2 is Noctis's primary interactive surface, and Views
are how authors build it. The builder makes illegal structures impossible to
express. Before anything is sent, it checks every Limit. Navbar, Wizard and Paginator
are built-in widgets that report their own component and character cost. Realizes
UJ-1, UJ-2.

#### FR-23: Typed V2 builders
A bot author can build every Components V2 component through typed builders:
Container, Section, Text Display, Thumbnail, Media Gallery, File, Separator,
Action Row, Button and all select menus.
**Consequences (testable):**
- An illegal placement doesn't compile. Examples: a Container inside a Container, two
  selects in one Action Row, or a Section without an accessory.
- Noctis sets the `IS_COMPONENTS_V2` flag on every send and edit of a V2 message.
  `content` and `embeds` can't be set on one.

#### FR-24: Limit validation
Noctis validates a message against all Limits before sending it.
**Consequences (testable):**
- The validator counts every component at every nesting level against the
  40-component limit, not just the top level.
- Text across all Text Displays is counted against the 4000-character budget.
- Per-field limits are enforced: labels, placeholders, option counts, media items,
  `attachment://` references.
- A violation returns an error that names the path, the limit and the actual value.
  A lenient mode can truncate text with "…".

#### FR-25: Views
A bot author can define a View that renders state into a message. The same View is
used for the first response and for every update after a component interaction.

#### FR-26: Navbar
A bot author can add a Navbar to a View. Selecting a tab updates the same message
with that tab's content, and the active tab is shown disabled.

#### FR-27: Wizard
A bot author can define a Wizard with Steps, progress display, back/next/cancel and
per-Step validation. Step state travels in Payloads or Refs.

#### FR-28: Paginator
A bot author can give a Paginator a list of items and a per-item renderer. The
Paginator packs as many items as fit the Limits onto each page, reserving room for
surrounding content such as a Navbar.
**Consequences (testable):**
- With 300 items, no page exceeds any Limit, and navigation reaches every item.
- When a single item can't fit on one page, the Paginator returns a typed error, so
  the author can render that item as an image or a detail view.

#### FR-29: Modals with V2 components
A bot author can build modals from Label, Text Input, select menus, File Upload,
Checkbox and Text Display. They receive the submission as a typed struct.
**Consequences (testable):**
- The builder rejects `disabled` inside modals, more than 5 top-level components,
  and title or label length violations.

#### FR-30: Attachments
Images and files attached to a View are tracked automatically: their
`attachment://` names are generated, and attachments the message no longer uses
are dropped on edit.

#### FR-31: Raw escape hatch
A bot author can include raw component JSON for component types that twilight
does not yet model. The raw JSON is still counted by the validator.

### 4.5 Image and GIF Rendering

**Description:** Read-only visuals such as cards, charts and leaderboards are
generated by the Renderer from Templates written in Rust with Tailwind classes. All
Tailwind support is native: Noctis uses only takumi's built-in parser, with no
Tailwind CLI, Node.js or CSS build step anywhere. Rendering happens off the async
runtime on a bounded pool. Realizes UJ-2, UJ-5.

#### FR-32: Templates
A bot author can write a Template in Rust with Tailwind classes and dynamic data.
**Consequences (testable):**
- Unknown Tailwind classes in literal class strings are compile errors. Validation
  uses the same parser as rendering.

#### FR-33: Output formats
The Renderer produces PNG, JPEG and WebP stills, and animated WebP, GIF and APNG.
**Consequences (testable):**
- The animated format is configurable. The default is animated WebP, with GIF as a
  fallback. `[ASSUMPTION: animated WebP plays in Thumbnail and Media Gallery on all
  clients; verify on mobile.]`
- When output exceeds the upload size budget, Noctis lowers settings (frame rate,
  size, quality) and returns an error if the output still doesn't fit.

#### FR-34: Bounded render pool
Rendering runs on a dedicated, bounded pool and never blocks the gateway or the
async runtime.
**Consequences (testable):**
- Under saturation, render requests wait in a bounded queue. When the queue is full,
  requests fail fast with a typed error.
- A Command that triggers a render automatically uses FR-21, so the interaction
  never times out.

#### FR-35: Fonts and assets
A bot author can register fonts and static images. Remote images such as avatars
are fetched and cached through the AssetStore Adapter.
**Consequences (testable):**
- If no font is registered, startup fails with a clear error unless the default-font
  feature is enabled. Text is never rendered invisibly.
- An emoji font is opt-in (Twemoji). `[ASSUMPTION]`

#### FR-36: Render cache and deduplication
Identical render requests (same Template, data and size) are served from the
RenderCache Adapter. Concurrent identical requests run one render and share the
result.

#### FR-37: Renderer seam
The Renderer is a trait, so a bot author can replace local rendering with a remote
render service. v1 ships only the local implementation.

### 4.6 Adapters

**Description:** Adapters are the project-wide rule for anything that stores or
caches data. Each is a trait with read/write methods, plus an in-memory default.
A bot author can implement the trait on their own struct and hand it to the App.
The framework then uses it everywhere that concern appears. Realizes UJ-3.

#### FR-38: Adapter traits with defaults
Noctis defines these Adapters:
- RefStore
- SessionStore
- RateLimitStore
- RenderCache
- AssetStore
- LocaleSource
- CacheStore, a shared key-value store with TTL for bots and Modules

Each has an in-memory default.
**Consequences (testable):**
- Replacing an Adapter needs no change to handler code.
- In-memory defaults are bounded by configurable capacity and TTL, so they never
  grow without limit.

#### FR-39: Adapter conformance tests
Noctis ships a reusable conformance test suite per Adapter trait. Third-party
implementations (e.g. Redis, SQL) run it to prove correct semantics: TTL, overwrite,
delete, missing key.

#### FR-40: Twilight cache configuration
A bot author can choose which Discord resources are cached in memory, or disable the
in-memory cache. The default caches only what enabled framework features need.

### 4.7 Localization

**Description:** Command metadata and content strings are localized through Locale
files. A Command Path can be translated word by word. Every handler receives the
User Locale and Guild Locale, with fallback to the default Locale. Realizes UJ-4.

#### FR-41: Command localization
A bot author can localize Command names word by word, plus descriptions, option
names, option descriptions and choices. Publish sends Discord's localization maps.

#### FR-42: Typed content strings
A bot author can define content strings in Locale files and call them from code as
typed functions with named or positional arguments.
**Consequences (testable):**
- A missing key in the default Locale is a compile error.
- A key missing in another Locale falls back to the default and produces a
  build-time warning.
- Every occurrence of a placeholder is replaced, not just the first.

#### FR-43: Locale resolution
Every handler can read the User Locale and Guild Locale. Middleware can override
them, for example with a per-guild setting stored by the bot.

### 4.8 Modules, Feature Flags and Rate Limits

**Description:** Modules group related features, as Packs do in tescord. Rust
cannot hot-swap code, so Modules are enabled and disabled at runtime through flags
instead.

#### FR-44: Module composition
A bot author can compose Modules into a tree and mount it on the App. Each Module
has a unique, path-like id.

#### FR-45: Runtime enable/disable
A bot author can enable or disable a Module at runtime, globally or per guild. A
disabled Module's handlers don't run, and its Commands can be left out of a guild's
Publish.

#### FR-46: Rate limits
A bot author can declare rate limits on a Command or Route per user, member,
channel or guild. The limits are stored in the RateLimitStore. A rate-limited
interaction gets a localized notice that includes the time remaining.

#### FR-47: Multiple Apps per process
A process can run several Apps with different tokens. This supports
white-label bots, which share Adapters where the author chooses.

### 4.9 Developer Experience and Project Template

**Description:** Noctis is published as one crate, `noctis`. Each capability is a
cargo feature of that crate, so authors never depend on internal sub-crates
directly. The Project Template is a complete starter bot that is kept in sync with
the public API. It ships with an `AGENTS.md`, the Noctis Usage Skill and the
rust-best-practices skill, so AI agents can build bots without reading Noctis
source. Realizes UJ-5.

#### FR-48: Single facade crate with features
A bot author depends only on `noctis`. They enable capabilities through cargo
features, for example images, GIF output, prefix commands and the default font.

#### FR-49: Project Template
`examples/project-template/` builds and runs as a standalone bot. It demonstrates:
- a Command;
- a Wizard;
- a Navbar View with a Paginator;
- a Template rendered into a media gallery;
- a custom Adapter implementation;
- Locale files.

**Consequences (testable):**
- CI builds the Project Template against the current workspace on every change.

#### FR-50: Noctis Usage Skill
The Project Template contains a detailed skill that documents the whole public API.
It covers:
- signatures and examples;
- Limits;
- patterns and anti-patterns;
- common mistakes.

**Consequences (testable):**
- Every public item exported by `noctis` appears in the skill.
- A change to the public API that doesn't update the skill fails CI.
  `[ASSUMPTION: enforced by a public-API snapshot check]`

#### FR-51: Testing kit
A bot author can unit-test Commands, Views and handlers without Discord, using a
simulated interaction context. The kit snapshot-tests rendered Views as JSON and
reports their Limit cost.

## 5. Cross-Cutting NFRs

- **NFR-1 Memory.** The framework's own memory per idle guild stays within a
  documented budget, measured by a CI benchmark. `[ASSUMPTION: target set during
  architecture; initial goal ≤ 1 KiB per guild excluding the twilight cache]`
- **NFR-2 No per-message state.** No framework feature holds a listener, timer or
  closure per message or per component instance.
- **NFR-3 Latency.**
  - Framework overhead from event arrival to handler start is p99 ≤ 1 ms at 10k
    events/s on 4 cores, excluding handler time.
  - A standard card (≈934×282) renders and encodes in p50 ≤ 10 ms and p99 ≤ 50 ms
    under load.
- **NFR-4 Reliability.** No panics on any input from Discord or users. Every panic
  in a handler is caught, logged and turned into an error response.
- **NFR-5 Observability.**
  - Noctis uses `tracing` spans for every interaction, event and render.
  - It exposes metrics hooks for queue depths, render times, Ref hits and misses,
    and shard status.
- **NFR-6 Security.**
  - Payloads and Refs are untrusted.
  - Refs are unguessable.
  - Tokens are never logged.
  - Adapters receive only serialized bytes and keys, never secrets.
- **NFR-7 Toolchain.** Noctis targets edition 2024. Its MSRV is the higher of
  twilight's and takumi's MSRV (currently 1.91), and the workspace pins dependency
  versions.
- **NFR-8 Quality gates.** Every workspace crate and the Project Template pass
  `cargo fmt`, `clippy -D warnings`, tests and doc tests in CI.

## 6. API Surface and Versioning

- The public API is what `noctis` re-exports. Sub-crates are implementation details
  and can change without notice.
- Noctis follows semver. Before 1.0, breaking changes bump the minor version and are
  listed in a changelog with migration notes.
- Twilight types appear in the public API where they are the natural vocabulary,
  such as ids and models. Noctis re-exports the twilight version it supports, so
  authors never need to match versions themselves.

## 7. Non-Goals (Explicit)

- Noctis does not implement its own gateway or HTTP client. It builds on twilight.
- There is no voice or music support, and no Lavalink integration.
- There is no web dashboard.
- Runtimes other than tokio are not supported.
- Code is not hot-reloaded. Runtime Module enable/disable takes its place.
- There is no Tailwind CLI, Node.js or CSS build tooling.
- There is no remote render service in v1; only the Renderer seam (FR-37) is
  provided.

## 8. MVP Scope

### 8.1 In Scope
- FR-1 to FR-13 and FR-15 to FR-51, with the FR-14 prefix module opt-in.
- In-memory defaults for every Adapter.
- Documentation and the Project Template.

### 8.2 Out of Scope for MVP
- Official Redis and SQL Adapter crates. They are deferred to v1.x; the
  conformance suite makes third-party versions safe.
- A cross-process request bus (a `broadcastEval` equivalent). This is deferred.
- A Locale scaffolding CLI, which generates translation templates from registered
  Commands. This is deferred to v1.x.
- The Primary Entry Point command type and the LaunchActivity response type. twilight
  does not model them yet.

## 9. Success Metrics

**Primary**
- **SM-1:** The Project Template bot has sharding, auto-published Commands, a
  3-Step Wizard and a rendered card, all in ≤ 150 lines of user code. Validates
  FR-1, FR-7, FR-27, FR-32 and FR-49.
- **SM-2:** The author's next production bot (the schwi successor) runs on Noctis.
  `[ASSUMPTION]`
- **SM-3:** An AI agent given only the Project Template completes the UJ-5 task, and
  its code compiles without reading Noctis source, in ≥ 4 of 5 trials. Validates
  FR-50.

**Secondary**
- **SM-4:** NFR-1 and NFR-3 benchmarks pass in CI.
- **SM-5:** Rendered Views produce zero Limit-related 400 responses from Discord in
  production logs. Validates FR-24 and FR-28.

**Counter-metrics (do not optimize)**
- **SM-C1:** Number of features or builder methods. More API is not better; the
  Usage Skill must stay learnable. Counterbalances SM-1.
- **SM-C2:** Benchmark memory reached by disabling caches users rely on.
  Counterbalances SM-4.

## 10. Open Questions

1. How is the 4000-character text budget counted (UTF-16 code units or code
   points), and does a single Text Display have its own maximum? Verify
   empirically. The answer affects FR-24.
2. Does animated WebP play in Thumbnail and Media Gallery on every client? The
   answer affects FR-33's default.
3. Should Noctis target twilight 0.17.1 now and migrate to 0.18 when it ships, or
   wait for 0.18? It affects FR-2 (`bucket()` API).
4. What are the precise NFR-1 memory and NFR-3 latency targets? They are set in
   architecture and confirmed by the first benchmark.

## 11. Assumptions Index

- FR-14: an opt-in prefix-command module is in v1.
- FR-33: animated WebP is a safe default animated format.
- FR-35: the emoji font is opt-in (Twemoji).
- FR-50: public-API drift from the skill is enforced by a snapshot check in CI.
- NFR-1: initial memory target ≤ 1 KiB per idle guild, excluding the twilight
  cache.
- SM-2: the schwi successor is the first production consumer.
