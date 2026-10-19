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
- four research reports in `../../research/`: tescord features
  (`tescord-features.md`), Discord Components V2 (`discord-components-v2.md`),
  twilight-rs (`twilight-rs.md`) and takumi (`takumi-image-generation.md`).

Features are grouped, and their FRs are numbered globally (FR-1 … FR-79) so later
documents can refer to them. FR-1 to FR-51 keep the ids of the first draft; FR-52
onwards were added in revision and sit inside the feature they belong to, so ids
are unique and gap-free but not monotonic in reading order. Terms in §3 are used
verbatim. Inline `[ASSUMPTION]` tags are collected in §11. Technical "how" (crate
names, libraries, wire formats, data structures, version pins) lives in
`addendum.md` and in the architecture document, not here.

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
- **Bot author coming from serenity/poise:** I want a concept map from what I know
  to Noctis, and a framework that handles Components V2 and scale for me.
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
  with `--publish-guild <test guild id>`, and `/setup start` appears in the test
  guild. **Edge case:** a user opens the wizard, leaves for two days and clicks
  "Next". The Ref has expired, so Noctis routes the click to the Expired Handler,
  which re-renders step 1 with a notice.
- **UJ-2. Deniz adds a server dashboard with tabs.** The Command
  `"dashboard"` sends a Container with a Navbar (Overview, Members, Logs). Members
  has 300 entries, so the View paginates them automatically under the component
  budget, and the Navbar stays on every page. The Overview tab embeds a generated
  activity-chart image in a media gallery.
- **UJ-3. Ece scales from one process to a cluster.** Ece's bot reaches 400k guilds.
  She changes configuration so each of four processes runs one Shard Range, points
  the App at a shared HTTP rate-limit proxy, and switches the RefStore,
  RateLimitStore, ModuleStateStore and IdentifyQueue to her Redis-backed
  implementations. Handler code is unchanged. Restarting a process resumes its
  Shards' sessions instead of re-identifying, using her SessionStore implementation.
- **UJ-4. Mert migrates a tescord module.** Mert ports a tescord pack that registers
  `"config (set|get) (prefix|welcome)"`. The same pattern string expands to the same
  four commands in Noctis. His handler gets a typed enum telling it which variant
  was invoked, and option values arrive as a typed struct. His pack's `en` locale
  file works unchanged for `en-US` and `en-GB` users.
- **UJ-5. A coding agent writes a leaderboard feature.** A Claude Code agent is given
  only the Project Template directory, with no access to Noctis source. It reads the
  Noctis Usage Skill and writes a Template, a View and a Command that compile on the
  first attempt.

## 3. Glossary

- **App** — The root object a bot builds. It holds configuration, Adapters and
  Modules, and runs Shards. One process may run several Apps (white-label bots).
- **Module** — A named, composable group of Commands, Component Handlers, Modal
  Handlers, Event Handlers and Middleware. Modules nest, and the App is the root
  Module. A Module can be enabled or disabled at runtime.
- **Pack** — tescord's name for a Module. Used only when discussing migration.
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
- **Route Guard** — A check declared on a Route (for example "invoking user only"
  or a required permission) that runs before its handler on every interaction.
- **Payload** — Typed data attached to a component's `custom_id` after its Route.
  It is encoded inline when it fits.
- **Stale Payload** — A Payload encoded by an incompatible earlier version of its
  type, typically on a message sent before a deploy.
- **Ref** — A short, unguessable key placed in a `custom_id` in place of a Payload
  that is too large or explicitly stored. The Payload itself lives in the RefStore
  and expires after a TTL.
- **Component Handler / Modal Handler** — The single function registered for a
  Route. It handles every component or modal using that Route, across all messages,
  Shards and restarts.
- **Event Handler** — A function registered for one gateway event type.
- **Lifecycle Hook** — A function registered for a framework event (App started,
  Shard ready, Publish completed, Module disabled, shutdown started, …) rather than
  a gateway event.
- **Interaction Lane** — The dedicated dispatch path for interactions. It is never
  dropped and never waits behind queued gateway events.
- **Middleware** — An ordered hook that runs before or after interactions or events.
  It can stop processing or attach typed Extensions that reach the handler.
- **Extension** — A typed value attached by Middleware and read by the handler by
  its type.
- **Error Sink** — A configurable destination for errors returned by handlers,
  selected by the error's origin (Command, Component/Modal, Autocomplete, Event,
  Render).
- **Adapter** — A trait with read/write methods for a storage-like concern, plus
  a default in-memory implementation. The bot author may supply their own struct
  instead. The set is open: every storage-like concern is an Adapter. Named Adapters
  in v1: RefStore, SessionStore, RateLimitStore, RenderCache, AssetStore,
  LocaleSource, CacheStore, ModuleStateStore, IdentifyQueue and DiscordCache.
- **ModuleStateStore** — The Adapter holding Module enable/disable state, global and
  per guild.
- **IdentifyQueue** — The Adapter that serialises Shard identifies per bucket, across
  every process of a Cluster.
- **DiscordCache** — The Adapter behind the cache of Discord resources (guilds,
  channels, roles, members, …).
- **Boot Flag** — A label on a Module or registration that decides at startup
  whether it exists at all in this run.
- **View** — A function from state to a Components V2 message. The same View
  renders the first response and every update.
- **Chrome** — The fixed parts of a View (header, Navbar, footer actions) that are
  kept on every page when the View paginates.
- **Overflow Policy** — What a View does when its content exceeds the Limits:
  paginate (default), error or truncate.
- **Navbar** — A row of tab buttons at the top of a View. Clicking a tab swaps the
  Container's content. The active tab is shown disabled.
- **Wizard** — A View made of ordered **Steps**, with a progress indicator and
  next/back navigation, used for setup-style flows.
- **Paginator** — A component that splits content into pages that each fit the
  Limits, with page navigation.
- **Limits** — The Discord Components V2 constraints that Noctis enforces, such as
  40 components per message and 4000 text characters. The Components V2 research
  report (§0) lists them all.
- **Upload Budget** — The maximum attachment size for a response: the interaction's
  `attachment_size_limit` when present, otherwise a configurable default.
- **Template** — A Tailwind-styled image layout written in Rust, rendered by the
  Renderer.
- **Theme** — App-wide Tailwind v4 design tokens and keyframes that Templates can use
  by class name.
- **Renderer** — The service that turns a Template plus data into PNG, JPEG, WebP,
  GIF or APNG bytes, on a bounded pool.
- **Render Job** — One request to the Renderer: a Template, its data, size, output
  format and settings.
- **Locale** — A language and the translated strings for Command metadata and
  content. The **User Locale** and **Guild Locale** are resolved per interaction.
- **Expired Handler** — The handler that runs when a component's Ref is missing or
  expired.
- **Project Template** — `examples/project-template/`, a ready-to-copy bot
  project. It includes the **Noctis Usage Skill** and an `AGENTS.md`.
- **UI Doctrine** — The rules in §4.0 for which surface and navigation pattern a
  bot uses.
- **Enforcement Point** — Where a Discord rule is checked: compile time (static
  input), App build (before connecting) or runtime (dynamic data).
- **Discord Rules Catalog** — `docs/discord-rules.md`, the maintained list of every
  Discord rule Noctis enforces, with its limit, source and Enforcement Point.
- **Priority Tier** — The MVP tier of an FR: **P0** core, **P1** required for v1,
  **P2** opt-in for v1 (§8).

## 4. Features

### 4.0 UI Doctrine

These rules are normative. Built-in widgets, the Project Template and the Usage
Skill follow them, and the Usage Skill teaches them (FR-50).

- **UD-1 Components V2 for anything interactive.** Every interactive or dashboard
  surface is a Components V2 message built from a View. Plain `content` and embeds
  are secondary output; built-in widgets and examples never use them.
- **UD-2 Images for read-only visuals.** Generated images show dense, read-only data
  (cards, charts, leaderboards). They are embedded in a View (Thumbnail, Media
  Gallery) and never carry information the user must act on.
- **UD-3 Commands are entry points.** A chat-input Command is a parameter-light
  bootstrap that opens a View. Configuration and data entry happen inside the View
  (selects, modals, Wizard Steps), not through long option lists. Options remain for
  quick actions, autocomplete lookups and context-menu targets.
- **UD-4 Steps for setup flows.** Setup and configuration flows split content
  logically into Wizard Steps with a progress indicator, never into "page 2 of 3".
- **UD-5 Navbar for general views that don't fit.** A general-purpose View that
  doesn't fit one Container splits into Navbar tabs.
- **UD-6 Pagination on overflow.** Homogeneous lists paginate automatically when
  they exceed the Limits, inside a Navbar tab when there is one.
- **UD-7 No listener per message or component.** Interactive state travels in
  Payloads and Refs to one handler per Route. There is no collector, await-component
  loop or inline per-message closure (NFR-2).
- **UD-8 Allocation-conscious by design.** Every feature is designed for millions of
  users: no per-message state, bounded caches, and no avoidable heap allocation on
  the dispatch path (NFR-1, NFR-9).

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
- An invalid token, or intents Discord rejects (for example an unapproved privileged
  intent), fails startup with a typed error naming the cause.
- A Shard that closes fatally after startup is reported as a shard-status Lifecycle
  Hook and metric, never as a silent stop.
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
- A configured shard total that isn't a multiple of `max_concurrency` (when it is
  greater than 1) produces a startup warning.

#### FR-3: Shard Ranges for Clusters
A bot author can run a configured Shard Range (for example, shards 32–63 of 128)
in each process.
**Consequences (testable):**
- A process runs only Shards in its range, and Command Publish runs from exactly one
  designated process.
- All processes of a Cluster identify through the IdentifyQueue Adapter, so
  per-bucket identify concurrency holds Cluster-wide, not per process.
- A Shard Range whose bounds aren't aligned to `max_concurrency` is accepted and
  logged as a warning.

#### FR-4: Session resume
Shards can resume sessions after a process restart, using the SessionStore Adapter.
**Consequences (testable):**
- Restarting a process within Discord's resume window resumes its sessions instead
  of re-identifying, when the SessionStore persists across restarts.
- A stored session that Discord rejects falls back to a fresh identify, without
  operator action.

#### FR-5: Concurrent event dispatch with backpressure
Gateway events dispatch to handlers concurrently, under a configurable concurrency
bound. Interactions use the Interaction Lane.
**Consequences (testable):**
- Reading a Shard's socket and heartbeating never wait on handler queues. A handler
  that sleeps for 10 s doesn't delay heartbeats or other handlers.
- Interactions are never dropped and never queue behind gateway events. They are
  exempt from the event bound; with the event queue full, an interaction still
  reaches its handler within the FR-21 deadline.
- When the event bound is reached, the per-event-type policy applies: buffer up to a
  configured depth, then drop the oldest events of types marked droppable. Nothing
  is dropped silently: a metric counts drops per event type, and types not marked
  droppable are never dropped.
- A metric exposes queue depth per lane.
- A set of Event Handlers can opt into per-guild ordered dispatch.

#### FR-6: Typed Event Handlers and derived event filtering
A Module can register Event Handlers per gateway event type.
**Consequences (testable):**
- Noctis asks the gateway to deserialise only the event types that have a
  registered handler (or that the cache needs). Unhandled event types are not
  deserialised.
- Event Handlers run concurrently by default. A Module can mark one ordered, so it
  runs before the concurrent handlers for that event.
- Registering two Event Handlers for the same event keeps both; handler identity
  never depends on time or registration timing.

#### FR-52: Shared HTTP rate limiting
A bot author can point the App's HTTP client at an external rate-limit proxy,
which replaces the local limiter, because Discord's HTTP limits are per token, not
per process.
**Consequences (testable):**
- With a proxy configured, no request bypasses it and the local limiter is off.
- A Cluster (more than one process sharing a token) without a proxy logs a warning
  at startup naming the risk.
- The Cluster deployment guide and the Project Template configuration make proxy
  mode the documented setup for Clusters.
- The HTTP request timeout is configurable.

#### FR-53: Presence and gateway commands
A bot author can set an initial presence, update it at runtime for all Shards or one
Shard, and send gateway commands (such as request guild members) to the Shard that
owns a guild.
**Consequences (testable):**
- A gateway command for a guild outside this process's Shard Range returns a typed
  error.
- A gateway-command rate-limit response is a typed error, not a silent drop.

#### FR-54: Lifecycle Hooks
A Module can register Lifecycle Hooks for: App started; Shard ready, resumed,
disconnected or fatally closed; all Shards ready; Publish completed or failed (with
the diff); Module enabled or disabled (global or per guild); shutdown started.
**Consequences (testable):**
- Each listed event fires exactly once per occurrence, with typed data, and a test
  App can observe all of them in order on start, Publish and shutdown.
- A Lifecycle Hook that errors goes to an Error Sink and doesn't stop the App.

#### FR-55: Resumable shutdown
A graceful shutdown leaves every Shard resumable.
**Consequences (testable):**
- On graceful shutdown, each Shard closes with a resumable close and its session id,
  sequence and resume URL are written to the SessionStore after the final event,
  before the process exits.
- A non-resumable shutdown mode (for example `--no-resume`) exists and clears the
  stored sessions.

### 4.2 Commands and Publish

**Description:** Commands are declared in tescord style, as entry points to Views
(UD-3). A Command Path such as `"settings logs channel"` becomes command,
subcommand group and subcommand. Command Patterns expand to every combination, and
Commands from different Modules that share a base word merge into one Discord
command. Options are typed. Publish compares the local definition with Discord's and
updates only on change. Realizes UJ-1, UJ-4.

#### FR-7: Command Paths
A Module can register a chat-input Command by Command Path.
**Consequences (testable):**
- A Command Path with 1, 2 or 3 words publishes as a command, a subcommand, or a
  subcommand group plus subcommand.
- More than 3 words, an empty word, or a word that breaks Discord's name rules fails
  registration with a typed error.
- A Command exceeding Discord's total-size limit for one command fails registration
  with a typed error, not at Publish.

#### FR-8: Command Patterns
A Module can register one handler for a Command Pattern such as
`"config (set|get) (prefix|welcome)"`.
**Consequences (testable):**
- Expansion is defined here, not by reference: each `(a|b|…)` group yields one
  alternative per word, a group followed by `?` also yields the path without it, and
  combinations are produced left to right in declaration order. The example expands
  to `config set prefix`, `config set welcome`, `config get prefix` and
  `config get welcome`.
- Every expanded Command Path is validated against FR-7 and against Discord's
  structure. A word that would be both a subcommand and a subcommand group (as in
  `"config (set|get) (prefix|welcome)?"`, which yields both `config set` and
  `config set prefix`) fails to compile (FR-77); a dynamically built pattern fails
  App build.
- A pattern that expands beyond a configurable maximum (default 25) fails
  registration with a typed error.
- The handler receives a typed value telling it which combination was invoked.

#### FR-9: Merge rules
Commands from different Modules that share a base word merge into one Discord
command.
**Consequences (testable):**
- Registration fails with a typed error when two registrations would publish
  conflicting `default_member_permissions`, `contexts`, `integration_types` or
  `nsfw` on the same base command. Noctis does not apply "first wins".
- Registration fails with a typed error when a base command would have both its own
  options and subcommands. Noctis does not silently drop either.

#### FR-56: Unique Command Paths
Each Command Path, and each context-menu name and type, has exactly one handler.
**Consequences (testable):**
- Registration fails with a typed error naming both Modules when two registrations,
  including expanded Command Pattern combinations, resolve to the same Command Path
  or to the same context-menu name and type.

#### FR-57: Explicit base and group metadata
A Module can declare the description, and its localizations, of a base command and
of a subcommand group, independently of any leaf Command.
**Consequences (testable):**
- If none is declared, Publish uses a documented deterministic rule (never
  registration order) and logs a warning.
- Two Modules declaring different values for the same base or group fail
  registration with a typed error.

#### FR-10: Typed options and autocomplete
A bot author can declare a Command's options as a typed struct, with descriptions,
choices, ranges, channel types, autocomplete and optional validators.
**Consequences (testable):**
- The handler receives the parsed struct. A missing required option or a wrong
  option type is a typed error, never a panic.
- A required option declared after an optional one, or choices and autocomplete on
  the same option, are compile errors for derived option structs and typed
  registration errors for dynamic declarations.
- An autocomplete handler returning more than 25 choices is truncated to 25, and a
  warning is logged. This is a deliberate exception to FR-24's strict default:
  autocomplete has no user-visible failure mode worth blocking on.
- If an autocomplete handler errors or misses the response deadline, Noctis responds
  with an empty choice list and routes the error to the Autocomplete Error Sink.
- An autocomplete handler for an option of a 3-word Command Path receives the focused
  value and the other filled options as a typed partial struct.
- A failed option validator returns a localized ephemeral message and the handler
  doesn't run.

#### FR-11: Command metadata
A bot author can set `default_member_permissions`, `contexts`, `integration_types`,
`nsfw` and a guild scope per Command.
**Consequences (testable):**
- Each field set on a Command appears in the Publish JSON (FR-13 dry run) exactly as
  declared; unset fields are omitted, not defaulted by Noctis.
- A guild-scoped Command is published only to its guilds and never globally.

#### FR-12: Context-menu Commands
A Module can register user and message context-menu Commands.
**Consequences (testable):**
- The handler receives the resolved target user (with member, when present) or
  target message as typed values.
- Context-menu names are localized as whole names (FR-41).

#### FR-13: Publish with diff
A bot author can Publish Commands globally or to specific guilds, and can run a
dry run that prints the difference.
**Consequences (testable):**
- Publish makes no write request when the remote state equals the local state.
- A dry run produces the full JSON and a human-readable diff without writing.
- Publish can run as a CLI mode of the bot binary (e.g. `--publish`,
  `--publish-guild <id>`), and can remove all of an App's Commands globally or from
  a guild (`--publish-clear`).
- With several Apps, Publish runs per App.
- Publish never runs on Shard ready; it runs only in the designated process (FR-3) or
  in CLI mode.

#### FR-14: Prefix commands (opt-in)
A Module can expose a chat-input Command as a prefix message command that reuses the
same handler through a shared response abstraction. It ships in v1 behind the
`prefix` cargo feature, which is off by default (§8, Priority Tier P2).
**Consequences (testable):**
- With the feature off, no prefix parsing code is compiled and the App needs no
  message-content intent.
- With it on, a message `<prefix><Command Path> <args>` runs the same handler as the
  slash form; argument parse failures produce the same typed errors as FR-10.

### 4.3 Interaction Routing, Payloads and Refs

**Description:** Noctis never registers a listener per message or per component
instance (UD-7). Every component and modal belongs to a Route, and each Route has
exactly one handler. A `custom_id` is the Route, a separator and a compact, typed
Payload. When the Payload doesn't fit in 100 characters, or is marked to be stored,
Noctis puts it in the RefStore and writes a Ref instead. Realizes UJ-1, UJ-2, UJ-3.

#### FR-15: Route registration
A Module can register exactly one Component Handler or Modal Handler per Route.
**Consequences (testable):**
- Registering the same Route twice is a typed error.
- Dispatch from `custom_id` to handler takes constant time, independent of the number
  of registered Routes.

#### FR-58: Route-bound component construction
A component is built from its Route's typed handle, so a component can't point at a
Route that has no handler.
**Consequences (testable):**
- Emitting a component whose Route has no registered handler is a compile error, or,
  for dynamically composed Modules, a typed error at App startup.
- No code-generation or watch step is needed for this check.

#### FR-59: Route component defaults
A Route can declare default presentation for its component (style, label, emoji,
placeholder, min and max values). Each render can override any of them.
**Consequences (testable):**
- A component rendered with no overrides uses the Route defaults; one with overrides
  (for example `disabled` or a localized label) uses them for that render only.
- The result is validated against the Limits (FR-24) after overrides apply.

#### FR-16: Typed Payloads
A bot author can attach a typed Payload to a component, and the handler receives the
decoded value.
**Consequences (testable):**
- Encoding and then decoding a Payload gives back an equal value (property-tested).
- Strings in a Payload may contain the separator character; the encoding escapes it.
- A `custom_id` built by Noctis never exceeds 100 characters. The maximum encoded
  length of each Route is proven at compile time (FR-76); only uniqueness within a
  message, which depends on runtime data, is checked at run time.
- A bot author can make their own type usable as a Payload field by implementing a
  Noctis trait; the round-trip property test and the 100-character check apply to
  it.

#### FR-60: Payload evolution
Messages live for months, so components on them can carry Stale Payloads after a
deploy.
**Consequences (testable):**
- Every encoded Payload carries a schema version. A Stale Payload is routed to the
  Expired Handler (or a Route's own stale handler), not to a generic decode error.
- Changes documented as compatible (for example adding an optional field) decode
  successfully from the earlier version.

#### FR-17: Refs with overflow policy
When a Payload would exceed the `custom_id` limit, or when a Route is marked
by-ref, Noctis stores the Payload in the RefStore and encodes a Ref.
**Consequences (testable):**
- A Ref carries at least 64 bits of entropy from a CSPRNG, encoded in exactly 11
  base62 characters. Refs are never sequential and never derived from interaction
  data.
- Refs are namespaced by App. A Ref created by one App is never resolved by another
  App sharing the same RefStore; it is treated as missing (Expired Handler).
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
- A Route can declare Route Guards (for example "invoking user only" or "requires
  MANAGE_GUILD") that run before the handler on every interaction; a failed guard
  returns a localized ephemeral notice and the handler doesn't run.
- Optionally, inline Payloads carry a short integrity tag; a mismatch is treated as a
  forged Payload and handled like an unknown Route.

#### FR-20: Middleware with typed Extensions
A Module can register Middleware before and after interactions and events.
**Consequences (testable):**
- A Middleware can stop processing.
- A Middleware can attach a typed Extension, which the handler can read by type.
- Middleware ordering is deterministic: registration order, parent Module before
  child.

#### FR-21: Automatic acknowledgement
If a handler has not responded before a configurable deadline (default 2500 ms,
measured from gateway receipt so queueing time counts), Noctis sends a deferred
response. Later responses edit the original message.
**Consequences (testable):**
- Each Command and Route declares up front whether its reply is ephemeral (default:
  public for Commands, update-in-place for components). The auto-defer uses that
  declaration, because ephemerality can't change after the first response.
- Commands defer as a new deferred message; component and modal-submit interactions
  defer as an in-place update.
- A handler that responds after 5 s still delivers its message, with no
  "interaction failed" shown to the user.
- On a deferred chat-input response, the Components V2 flag is applied on the
  follow-up edit, as Discord requires.
- A handler declared as modal-opening is never auto-deferred, because a modal must
  be the first response. If it misses the deadline, Noctis logs an error naming the
  handler. Responding to a modal submit with another modal is a typed error.
- After the 15-minute interaction token lifetime, updates to a public View fall back
  to a bot-token message edit; updates to an ephemeral one return a typed
  `TokenExpired` error.
- When the follow-up cap for user-installed contexts would be exceeded, Noctis
  returns a typed error instead of sending.

#### FR-22: Error routing
Errors returned by handlers go to configurable Error Sinks.
**Consequences (testable):**
- By default, the user sees a short, localized, ephemeral error, and the full error
  is logged through structured tracing with interaction context.
- Error Sinks are selected by origin (Command, Component/Modal, Autocomplete, Event,
  Render), so an author can handle each differently.

### 4.4 Components V2 UI Kit

**Description:** Components V2 is Noctis's primary interactive surface (UD-1), and
Views are how authors build it. The builder makes illegal structures impossible to
express. Before anything is sent, it checks every Limit. Views paginate on overflow
(UD-6), and Navbar, Wizard and Paginator are built-in widgets that report their own
component and character cost. Realizes UJ-1, UJ-2.

#### FR-23: Typed V2 builders
A bot author can build every Components V2 component through typed builders:
Container, Section, Text Display, Thumbnail, Media Gallery, File, Separator,
Action Row, Button and all select menus.
**Consequences (testable):**
- An illegal placement doesn't compile. Examples: a Container inside a Container, two
  selects in one Action Row, or a Section without an accessory (FR-78).
- Noctis sets the `IS_COMPONENTS_V2` flag on every send and edit of a V2 message.
  `content`, `embeds`, `poll`, stickers and `shared_client_theme` can't be set on
  one.
- Editing a legacy message into a View clears its `content`, `embeds`, `poll` and
  stickers in the same request. A V2 message can't be edited back to legacy (typed
  error).
- A minimal legacy (non-V2) send path for plain text exists for FR-14 and for
  authors who need it.

#### FR-24: Limit validation
Noctis validates a message against all Limits before sending it.
**Consequences (testable):**
- The validator counts every component at every nesting level against the
  40-component limit, not just the top level.
- Text across all Text Displays is counted against the 4000-character budget.
- Per-field limits are enforced: labels, placeholders, option counts, media items,
  `attachment://` references, Section shape (1–3 Text Displays and one accessory),
  Media Gallery item counts, link button `url` length, premium button rules, accent
  colour range and select `default_values` counts.
- Both `custom_id` and numeric component `id` are unique within the message.
- A violation returns an error that names the path, the limit and the actual value.
  A lenient mode can truncate text with "…".
- All numeric Limits live in one configurable value, so a Discord limit change needs
  no Noctis release.
- In strict mode, a Thumbnail or gallery item without alt text is a warning and
  appears in the Limit cost report.

#### FR-25: Views
A bot author can define a View that renders state into a message. The same View is
used for the first response and for every update after a component interaction.
**Consequences (testable):**
- Rendering the same state twice yields identical components, except for
  per-render select identifiers.
- After an update, every select shows the View's declared defaults, not the user's
  previous local selection.
- Views send no parsed mentions by default: a View listing `<@id>` pings nobody
  unless the author opts in per View or per message. A helper escapes Markdown and
  mentions in user-supplied text.
- A View can opt into a state version. An interaction carrying an older version
  re-renders from current state instead of applying the stale action, and the user
  sees a short notice.
- A View's Limit cost (components, characters) is available without sending it.

#### FR-61: View responses
A Command, Component Handler or Modal Handler can return a View, optionally with
attachments, and Noctis sends it with the right response type.
**Consequences (testable):**
- A Command declared with no options and a View-returning handler compiles,
  publishes with an empty option list, and sends the View as its initial response
  with no response code from the author (UD-3).
- A Component Handler returning a View updates the message in place; FR-21 deferred
  variants are applied when the deadline passes.
- The Project Template contains no manual response-type selection.

#### FR-62: View overflow
A View paginates automatically when its content exceeds the Limits. This is its
default Overflow Policy; `Error` and `Truncate` are opt-in.
**Consequences (testable):**
- A View whose list grows from 10 to 300 items produces a paginated message with no
  code change and no Limit error.
- The View's Chrome (header, Navbar, footer actions) appears on every page, and its
  cost is reserved before content is packed.
- Only when a single item can't fit on one page alongside the Chrome does the View
  return a typed error naming the item, so the author can render it as an image or a
  detail view.
- With `Error`, any overflow yields the FR-24 error naming the path and Limit.

#### FR-26: Navbar
A bot author can add a Navbar to a View. Selecting a tab updates the same message
with that tab's content, and the active tab is shown disabled (UD-5).
**Consequences (testable):**
- Clicking a tab edits the same message in place; no new message is sent.
- The active tab's button is disabled and the others are enabled, on every page.
- The selected tab travels in the Payload; switching tabs stores nothing.
- A tab whose content overflows paginates inside the tab (FR-62) with the Navbar on
  every page.
- A Navbar with more tabs than fit one Action Row is a typed error.

#### FR-27: Wizard
A bot author can define a Wizard with Steps, progress display, back/next/cancel and
per-Step validation. Step state travels in Payloads or Refs (UD-4).
**Consequences (testable):**
- The progress indicator shows the current Step's position and title out of the
  total ("Step 2 of 3: Roles"), never page numbers.
- A Step whose validation fails stays on that Step and shows the validation message
  inline; Next doesn't advance.
- Back returns to the previous Step with its earlier values kept.
- Cancel edits the message to a cancelled state, removes its interactive components
  and deletes the Wizard's Refs.
- When a Step's Ref has expired, the click goes to the Expired Handler, which by
  default re-renders Step 1 with a notice (UJ-1).
- A Step that collects input through a modal returns to a message Step before any
  next modal; the Wizard handles this without author code.

#### FR-28: Paginator
A bot author can give a Paginator a list of items and a per-item renderer. The
Paginator packs as many items as fit the Limits onto each page, reserving room for
surrounding content such as a Navbar.
**Consequences (testable):**
- With 300 items, no page exceeds any Limit, and navigation reaches every item.
- When a single item can't fit on one page, the Paginator returns a typed error, so
  the author can render that item as an image or a detail view.
- The current page travels in the Payload; page navigation stores nothing.
- With more than 5 pages, a jump control reaches any page in a bounded number of
  clicks.
- The Paginator can render items as chunked String Selects (25 options each)
  instead of one Section per item.

#### FR-29: Modals with V2 components
A bot author can build modals whose top level is Label or Text Display, and whose
Labels wrap Text Input, any select menu, File Upload, Radio Group, Checkbox Group or
Checkbox. They receive the submission as a typed struct, with selects joined to
resolved entities and File Upload values joined to their attachments.
**Consequences (testable):**
- The builder rejects `disabled` anywhere in a modal, more than 5 top-level
  components, title or Label length violations, Radio Group options outside 2–10,
  Checkbox Group options outside 1–10, `required` on a Checkbox, and
  `min_values = 0` combined with `required = true`.
- Noctis never emits a deprecated Action Row-wrapped Text Input or a Text Input
  `label`.
- File Upload values expose attachment metadata and a download helper; the docs
  state that the URL is ephemeral.

#### FR-63: Modal pre-fill
A modal can be opened with pre-filled values.
**Consequences (testable):**
- Text Inputs accept an initial value, and selects, Radio Groups and Checkbox Groups
  accept default selections, for example from the entity being edited.
- A pre-filled value that breaks a field's limits is a typed error before sending.

#### FR-64: Radio Group and Checkbox Group with raw fallback
Modals using Radio Group, Checkbox Group and Checkbox work end to end even where the
supported twilight version doesn't model those types.
**Consequences (testable):**
- A modal submit containing Radio Group, Checkbox Group or Checkbox values decodes
  into the typed submission struct, including when twilight drops their data.
- Upgrading to a twilight version that models them requires no change to author
  code.

#### FR-65: Views over webhooks
A bot author can send and edit a View through a webhook (log channels, white-label
personas, cross-posting).
**Consequences (testable):**
- Noctis always requests component support on webhook execute and edit, so
  components are never silently dropped.
- Sending a View with interactive components through a webhook the application
  doesn't own is a typed error.
- Only flags valid for the target webhook endpoint are sent.

#### FR-30: Attachments
Images and files attached to a View are tracked automatically: their
`attachment://` names are generated, and attachments the message no longer uses
are dropped on edit.
**Consequences (testable):**
- Editing a View that no longer references an attachment removes it from the
  message; one that still references it keeps it without re-uploading.
- Generated attachment names are unique within the message.
- An attachment larger than the Upload Budget is rejected with a typed error before
  sending.
- In strict mode, a rendered image placed in a Thumbnail or Media Gallery without alt
  text is a validation error; in lenient mode it is a warning.

#### FR-31: Raw escape hatch
A bot author can include raw component JSON for component types that twilight
does not yet model, and send raw REST requests for endpoints it doesn't model.
**Consequences (testable):**
- Raw component JSON is counted by the validator.
- A received message or modal submit containing an unknown component type still
  dispatches; the unknown part is available to the handler as raw JSON.
- Editing a View never re-sends a component Noctis couldn't parse; the edit uses the
  View's own rendered tree.
- A raw REST request goes through the App's HTTP client and rate limiting (or the
  FR-52 proxy).

### 4.5 Image and GIF Rendering

**Description:** Read-only visuals such as cards, charts and leaderboards are
generated by the Renderer from Templates written in Rust with Tailwind classes
(UD-2). All Tailwind support is native: Noctis uses only takumi's built-in parser,
with no Tailwind CLI, Node.js or CSS build step anywhere. Rendering happens off the
async runtime on a bounded pool. The image feature is off by default. Realizes
UJ-2, UJ-5.

#### FR-32: Templates
A bot author can write a Template in Rust with Tailwind classes and dynamic data.
**Consequences (testable):**
- Unknown Tailwind classes in literal class strings are compile errors. Validation
  uses the same parser as rendering, run inside the proc macro.
- User-supplied text can enter a Template only as text content, never as a class
  string or markup.
- A bot author can register a Theme once on the App; Templates use its class names,
  and compile-time validation accepts tokens declared in the Theme.

#### FR-66: Tailwind v4 class names and unknown classes
Template classes follow Tailwind v4 naming, and an unknown class is never dropped
silently.
**Consequences (testable):**
- The compile error for a known Tailwind v3 name suggests its v4 replacement (for
  example `bg-gradient-to-r` → `bg-linear-to-r`).
- Dynamic values (widths, colours, percentages) are set through typed style setters,
  not by formatting class strings.
- A class string built at runtime that contains an unknown class logs a warning once
  per distinct string, and fails the test in the testing kit (FR-51).

#### FR-33: Output formats
The Renderer produces PNG, JPEG and WebP stills, and animated WebP, GIF and APNG.
**Consequences (testable):**
- The animated format is configurable. The default is animated WebP, with GIF as a
  fallback. `[ASSUMPTION: animated WebP plays in Thumbnail and Media Gallery on all
  clients; verify on mobile.]`
- The size budget is the smaller of the Upload Budget and a configurable target
  (default about 2 MB). When output exceeds it, Noctis lowers settings in a
  documented order (frame rate, size, quality, then optionally a still frame) and
  returns a typed error if the output still doesn't fit.
- The settings actually used are reported with the image.

#### FR-69: Animated Templates
A bot author can produce an animation either from Tailwind/CSS animations in a
Template, with custom keyframes supplied through the Theme, or by rendering one
Template state per frame.
**Consequences (testable):**
- Requesting a frame rate above the output format's cap is a typed error before
  rendering.
- Redefining a built-in animation name is a registration error.
- All scenes of one animation share one size; a mismatch is a typed error.

#### FR-34: Bounded render pool
Rendering runs on a dedicated, bounded pool and never blocks the gateway or the
async runtime.
**Consequences (testable):**
- Under saturation, render requests wait in a bounded queue. When the queue is full,
  requests fail fast with a typed error.
- A Command that triggers a render automatically uses FR-21, so the interaction
  never times out.
- Rendering parallelism stays inside the render pool; it never runs on tokio worker
  threads or a thread pool shared with the author.

#### FR-70: Per-job render caps
Each Render Job is checked against configurable caps (pixels, frames, duration)
before it is queued.
**Consequences (testable):**
- A Render Job over any cap fails with a typed error and never enters the queue.
- Animated jobs weigh more than stills in admission control, so a burst of
  animations can't starve stills.
- When the requester gives up (for example the interaction expired), a queued job is
  dropped, and a per-frame animation stops between frames.

#### FR-35: Fonts and assets
A bot author can register fonts and static images. Remote images such as avatars
are fetched and cached through the AssetStore Adapter (FR-67).
**Consequences (testable):**
- If no font is registered, startup fails with a clear error unless the default-font
  feature is enabled. Text is never rendered invisibly.
- A bot author can register fallback fonts with a priority order and Unicode ranges
  (for example CJK) without changing Templates.

#### FR-68: Emoji rendering
Emoji in Template text render as glyphs or images, never as tofu boxes or raw
markup.
**Consequences (testable):**
- A colour emoji font is opt-in (Twemoji). `[ASSUMPTION: emoji font stays opt-in]`
  Without one, Noctis logs a startup warning when any Template is registered, and the
  Usage Skill documents that emoji then render as boxes.
- Discord custom emoji (`<:name:id>`) in Template text render as images fetched under
  FR-67, never as raw text.

#### FR-67: Remote image fetching
A Template declares the remote images it uses, and Noctis resolves all of them
before the render starts.
**Consequences (testable):**
- Each fetch has a timeout and a byte cap. A failed, oversized or undecodable image
  is replaced by a configurable fallback image and logged; it fails the render only
  when the Template marks that image required.
- Fetches are limited to an allow-list of hosts (default: the Discord CDN), so
  Template data can't make the bot fetch arbitrary URLs.
- Avatar and icon helpers key the cache by user or guild id, asset hash and size, so
  a changed avatar invalidates naturally. Animated avatars animate in animated
  output.
- Fetching and decoding run off the async runtime under a concurrency bound.

#### FR-36: Render cache and deduplication
Identical render requests are served from the RenderCache Adapter. Concurrent
identical requests run one render and share the result.
**Consequences (testable):**
- Requests are identical only when Template and Template version, data, size, output
  format and settings, referenced asset versions, and Noctis and renderer versions
  all match. Upgrading Noctis or the renderer never serves a stale cached image.
- N concurrent identical requests cause exactly one render.
- The attachment filename is derived from the cache key.

#### FR-37: Renderer seam
The Renderer is a trait, so a bot author can replace local rendering with a remote
render service. v1 ships only the local implementation.
**Consequences (testable):**
- A test Renderer implementing the trait can replace the local one with no change to
  Templates, Views or handlers.

### 4.6 Adapters

**Description:** Adapters are the project-wide rule for anything that stores or
caches data. The rule is open: every framework feature that stores, caches or
shares data across Shards or processes does so through an Adapter with an in-memory
default, and a new feature that introduces such state must add or reuse one rather
than hold process-local state that breaks under a Cluster. A bot author can
implement the trait on their own struct and hand it to the App. The framework then
uses it everywhere that concern appears. Wizard, Navbar and Paginator state are
Payloads or Refs and need no Adapter of their own. Realizes UJ-3.

#### FR-38: Adapter traits with defaults
Noctis defines these Adapters in v1:
- RefStore
- SessionStore
- RateLimitStore
- RenderCache
- AssetStore
- LocaleSource
- CacheStore, a shared key-value store with TTL for bots and Modules
- ModuleStateStore, holding Module enable/disable state (FR-45)
- IdentifyQueue, serialising identifies per bucket across a Cluster (FR-3)
- DiscordCache, the backend of the Discord resource cache (FR-40)

Each has an in-memory default.
**Consequences (testable):**
- Replacing an Adapter needs no change to handler code.
- In-memory defaults are bounded by configurable capacity and TTL, so they never
  grow without limit.

#### FR-71: Adapter namespacing per App
Adapter keys are namespaced by App by default.
**Consequences (testable):**
- Two Apps sharing one Adapter instance never read or overwrite each other's Refs,
  sessions, rate-limit counters or Module state.
- An author can opt a specific Adapter (for example CacheStore) into sharing keys
  across Apps.

#### FR-39: Adapter conformance tests
Noctis ships a reusable conformance test suite per Adapter trait. Third-party
implementations (e.g. Redis, SQL) run it to prove correct semantics: TTL, overwrite,
delete, missing key.
**Consequences (testable):**
- Each in-memory default passes its own suite in CI.
- The suite covers concurrent writers and App namespacing (FR-71), and a deliberately
  broken implementation (for example one ignoring TTL) fails it.

#### FR-40: Discord cache configuration
A bot author can choose which Discord resources are cached, replace the cache's
storage through the DiscordCache Adapter, or disable the cache. The default caches
only what enabled framework features need.
**Consequences (testable):**
- The default intents are the minimum required by registered Event Handlers and
  enabled cache resources (only guild events for an interaction-only bot).
- Startup fails with a typed error when a registered Event Handler or cache resource
  needs an intent that isn't enabled.
- Cache reads return owned or shared values; no cache guard is exposed that could be
  held across an `.await`.
- A DiscordCache implementation outside the process serves every process of a Cluster
  the same data.

### 4.7 Localization

**Description:** Command metadata and content strings are localized through Locale
files, keeping the tescord mental model (§2.1). A Command Path can be translated word
by word. Every handler receives the User Locale and Guild Locale, with fallback to
the default Locale.

#### FR-41: Command localization
A bot author can localize Command names word by word, plus descriptions, option
names, option descriptions and choices. Publish sends Discord's localization maps.
**Consequences (testable):**
- A Locale file for a base language (`en`) covers all Discord variants of it
  (`en-US`, `en-GB`) at Publish unless a variant file exists; variant files override
  base files.
- A localized name that breaks Discord's name rules fails registration with a typed
  error naming the Locale.

#### FR-42: Typed content strings
A bot author can define content strings in Locale files and call them from code as
typed functions with named or positional arguments.
**Consequences (testable):**
- A missing key in the default Locale, or a call with the wrong arguments, is a
  compile error (Locale files are static input).
- A key missing in another Locale falls back to the default and produces a
  build-time warning.
- Every occurrence of a placeholder is replaced, not just the first.

#### FR-43: Locale resolution
Every handler can read the User Locale and Guild Locale. Middleware can override
them, for example with a per-guild setting stored by the bot.
**Consequences (testable):**
- For interactions, resolution is User Locale → Guild Locale → default Locale; for
  Event Handlers on guild events it is Guild Locale → default.
- A regional runtime locale with no file of its own (for example `pt-BR`) resolves to
  its base language before the default.
- The Usage Skill documents the rule: User Locale for ephemeral single-user UI, Guild
  Locale for shared content.

#### FR-72: Module-scoped Locales
Each Module can ship its own Locale files, merged per language with the App's.
**Consequences (testable):**
- A key defined by two Modules is a build-time error naming both.
- Disabling a Module doesn't remove its strings from Modules that still reference
  them.

### 4.8 Modules, Feature Flags and Rate Limits

**Description:** Modules group related features, as Packs do in tescord. Rust
cannot hot-swap code, so Modules are selected at boot through Boot Flags and enabled
or disabled at runtime through the ModuleStateStore instead.

#### FR-44: Module composition
A bot author can compose Modules into a tree and mount it on the App. Each Module
has a unique, path-like id.
**Consequences (testable):**
- Mounting two Modules with the same id is a typed error.
- A Module's registrations carry its id in logs, metrics and errors.

#### FR-73: Boot-time feature flags
A Module or an individual registration can carry Boot Flags. The App selects the
active flags at startup from configuration or the CLI (for example
`--features a,b` or `all`).
**Consequences (testable):**
- Registrations whose flags aren't selected are not registered, not dispatched and
  not published.
- Publish's dry run (FR-13) reflects only the selected flags.

#### FR-45: Runtime enable/disable
A bot author can enable or disable a Module at runtime, globally or per guild,
through the ModuleStateStore. A disabled Module's handlers don't run.
**Consequences (testable):**
- Disabling a Module in one process takes effect in every process sharing the
  ModuleStateStore within a documented propagation delay.
- Invoking a disabled Module's global Command in a guild returns a localized
  ephemeral "disabled here" notice; the handler doesn't run.
- Only guild-scoped Commands are left out of a guild's Publish; global Commands stay
  published.

#### FR-46: Rate limits
A bot author can declare rate limits on a Command or Route per user, member,
channel, guild or message. The limits are stored in the RateLimitStore.
**Consequences (testable):**
- A limit is "N uses per window", not only one use per cooldown.
- The check runs before the handler and before any auto-defer; a limited interaction
  gets an immediate ephemeral localized notice that includes the time remaining.
- A handler can set or extend a limit dynamically, for example only after a
  successful action.
- The rate-limited response can be overridden per App, Command or Route.

#### FR-47: Multiple Apps per process
A process can run several Apps with different tokens. This supports white-label
bots, which share Adapters where the author chooses (FR-71).
**Consequences (testable):**
- The same Module tree can be mounted on several Apps.
- A Command can be restricted to a subset of Apps for Publish.
- An App can be given a guild allow-list or deny-list, resolved through an Adapter,
  so a white-label App serves only its customer's guild and the main App skips it.

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
**Consequences (testable):**
- A bot built with default features compiles without the image stack; the image
  feature is off by default, and its docs state its extra build requirements.
- Each documented feature combination builds in CI.

#### FR-49: Project Template
`examples/project-template/` builds and runs as a standalone bot. It demonstrates:
- an option-less entry Command opening a View;
- a Wizard;
- a Navbar View with an overflowing list;
- a Template rendered into a media gallery;
- a custom Adapter implementation;
- Locale files.

**Consequences (testable):**
- CI builds the Project Template against the current workspace on every change.
- It follows the UI Doctrine (§4.0): no handler responds with an embed or a plain
  `content` message as its primary output, and no code uses a per-message collector,
  await-component loop or inline listener.

#### FR-50: Noctis Usage Skill
The Project Template contains a detailed skill that documents the whole public API.
It covers:
- signatures and examples;
- Limits, with one entry per Components V2 and modal component (where it may
  appear, its Limits and its builder);
- the UI Doctrine and a UI quality rulebook (one primary action per View, visible
  active state, confirmation for destructive actions, ephemeral for single-user UI
  and public for shared content, a way back or cancel on every interactive View,
  designed empty and error states);
- when to use Wizard, Navbar or Paginator (UD-4 to UD-6);
- patterns and anti-patterns;
- common mistakes.

**Consequences (testable):**
- Every public item exported by `noctis` appears in the skill.
- Every component type in FR-23 and FR-29 has an entry; a builder without one fails
  CI.
- A change to the public API that doesn't update the skill fails CI.
  `[ASSUMPTION: enforced by a public-API snapshot check]`
- No example in the skill or in the docs uses a per-message listener (UD-7).

#### FR-51: Testing kit
A bot author can unit-test Commands, Views and handlers without Discord, using a
simulated interaction context. The kit snapshot-tests rendered Views as JSON and
reports their Limit cost.
**Consequences (testable):**
- A test can invoke a Command or click a component by Route and assert on the
  resulting View, response type and ephemerality without network access.
- A View snapshot changes only when its rendered JSON changes.
- Templates can be pixel-snapshot-tested with a tolerance.

#### FR-74: Launch documentation
The v1 release ships these documents:
- docs.rs documentation for every public item, with examples;
- a tescord/dbi → Noctis concept map (Pack → Module, `buildComponent` → typed
  Payload, `locale.user` → User Locale, …) and a short serenity/poise → Noctis
  section;
- a Cluster deployment guide (Shard Ranges, IdentifyQueue, HTTP proxy, shared
  Adapters);
- a changelog with migration notes (§6).

**Consequences (testable):**
- `missing_docs` is denied for the `noctis` crate in CI.
- Each listed guide exists in the repository at release and is linked from the crate
  README.

### 4.10 Compile-time Rule Enforcement

**Description:** Noctis enforces Discord's rules at compile time wherever the input
is static, and falls back to runtime checks only for genuinely dynamic data.
Conflicts that exist only once Modules are combined are caught when the App is
built, before any gateway connection. The runtime typed errors named in other FRs
apply to dynamic input only. Every rule and its Enforcement Point is recorded in the
Discord Rules Catalog. Realizes UJ-4, UJ-5.

#### FR-75: Enforcement points
Each Discord rule is enforced at the earliest point its input is known: compile time
for static input, App build for cross-Module combinations, runtime for dynamic data.
**Consequences (testable):**
- Every compile-time rule has a compile-fail test (trybuild) whose expected error
  message names the rule, the limit and the offending value, pointing at the
  offending source span.
- Cross-Module conflicts (duplicate Command Paths, conflicting base metadata,
  options mixed with subcommands across Modules, duplicate Routes, Locale key
  clashes) fail App build with a typed error before any Shard connects.
- A rule that could be checked at compile time is never checked only at runtime; a
  runtime check for static input fails the NFR-10 audit.

#### FR-76: Provable `custom_id` length
The Payload derive computes the maximum encoded length of each Route's `custom_id`.
**Consequences (testable):**
- Refs have a fixed length (11 characters) and every Payload field is fixed-width or
  declares a bound, so a Route whose maximum encoded length can exceed 100
  characters is a `compile_error!`.
- An unbounded field (for example a `String` without a declared bound) is a compile
  error unless the Route is marked by-ref.
- A trybuild test shows a 101-character worst case failing to compile and a
  100-character one compiling.

#### FR-77: Command declarations checked at compile time
Command Paths and Command Patterns are expanded and validated in the proc macro.
**Consequences (testable):**
- Word count, name format, a word used as both subcommand and subcommand group, a
  base command mixing options and subcommands, and duplicate paths within one macro
  invocation are compile errors. `"config (set|get) (prefix|welcome)?"` fails to
  compile; `"config (set|get) (prefix|welcome)"` compiles.
- Literal descriptions, option names, choice counts and option ordering (required
  before optional) are checked at compile time.

#### FR-78: Static component trees and text checked at compile time
Literal labels, descriptions, placeholders, option and choice counts, and static
component trees are validated at compile time.
**Consequences (testable):**
- Illegal nesting, more than 5 buttons in one Action Row, a Section with fewer than
  1 or more than 3 Text Displays, an over-length literal label or placeholder, and
  modal rules on static input (FR-29) are compile errors, each with a trybuild test.
- The same rules applied to runtime-built trees produce the FR-24 typed errors.

#### FR-79: Discord Rules Catalog
The project maintains `docs/discord-rules.md`, listing every Discord rule Noctis
enforces with its limit, source (Discord docs link or research finding) and
Enforcement Point (compile-time, build-time or runtime).
**Consequences (testable):**
- Every Limit in the configurable Limits value (FR-24) and every rule in FR-7 to
  FR-10, FR-16, FR-23, FR-24 and FR-29 has a catalog row.
- CI fails when a compile-time row has no matching trybuild test, or a runtime row
  has no matching unit test.
- The Usage Skill links the catalog for its Limits entries.

## 5. Cross-Cutting NFRs

- **NFR-1 Memory.** The framework's own memory per idle guild stays within a
  documented budget, measured by a CI benchmark with a stated default cache
  configuration (no members, no messages). Discord cache memory and Renderer cache
  memory (each with a documented, configurable byte budget) are reported separately.
  `[ASSUMPTION: target confirmed by the first benchmark; initial goal ≤ 1 KiB per
  guild excluding the Discord cache]`
- **NFR-2 No per-message state.** No framework feature holds a listener, timer or
  closure per message or per component instance (UD-7).
- **NFR-3 Latency.** These are targets, confirmed or adjusted by the first benchmark.
  `[ASSUMPTION]`
  - Framework overhead from event arrival to handler start is p99 ≤ 1 ms at 10k
    events/s on 4 cores, excluding handler time, under the benchmark's event mix
    (defined with the benchmark in architecture).
  - The reference card (the Project Template's standard card Template, ≈934×282,
    PNG output, warm caches) renders and encodes in p50 ≤ 10 ms and p99 ≤ 50 ms with
    the render pool saturated.
- **NFR-4 Reliability.** No panics on any input from Discord or users. Under the
  default `panic = "unwind"` profile, a panic in a handler or a Render Job becomes an
  error response and a logged event, and the Shard keeps running.
- **NFR-5 Observability.**
  - Noctis emits structured tracing spans for every interaction, event and render.
  - It exposes metrics hooks for queue depths per lane, dropped events, render
    times, Ref hits and misses, and shard status.
- **NFR-6 Security.**
  - Payloads and Refs are untrusted.
  - Refs carry at least 64 bits of CSPRNG entropy and are namespaced per App.
  - Views ping nobody unless the author opts in (FR-25).
  - Remote image fetches are host-allow-listed (FR-67).
  - Tokens are never logged.
  - Adapters receive only serialized bytes and keys, never secrets.
- **NFR-7 Toolchain.** Noctis targets the current Rust edition and an MSRV no lower
  than its dependencies require, with dependency versions pinned in the workspace
  (current values in the addendum). Public Noctis types don't force exhaustive
  matches on upstream enums, so new upstream variants aren't breaking for authors.
- **NFR-8 Quality gates.** Every workspace crate and the Project Template pass
  formatting, lints with warnings denied, tests and doc tests in CI. Renderer
  upgrades merge only after the Project Template's pixel snapshots pass.
- **NFR-9 Allocation.** The dispatch path (event arrival → handler start, and
  `custom_id` → Route → decoded inline Payload) has a documented allocation budget,
  measured by a CI benchmark. `[ASSUMPTION: target confirmed by the first benchmark;
  goal is no heap allocation for Route lookup and fixed-width Payload decode]`
- **NFR-10 Rule enforcement coverage.** Each Discord rule Noctis enforces has a
  documented Enforcement Point in the Discord Rules Catalog (FR-79), and static
  input is always checked at compile time.

## 6. API Surface and Versioning

- The public API is what `noctis` re-exports. Sub-crates are implementation details
  and can change without notice.
- Noctis follows semver. Before 1.0, breaking changes bump the minor version and are
  listed in a changelog with migration notes.
- Twilight types appear in the public API where they are the natural vocabulary,
  such as ids and models. Noctis re-exports the twilight version it supports, so
  authors never need to match versions themselves.
- Renderer types are Noctis's own. Types of the underlying renderer are reachable
  only through a clearly unstable module that semver doesn't cover.

## 7. Non-Goals (Explicit)

- Noctis does not implement its own gateway or HTTP client. It builds on twilight.
- There is no voice or music support, and no Lavalink integration.
- There is no web dashboard.
- Runtimes other than tokio are not supported.
- Code is not hot-reloaded. Boot Flags and runtime Module enable/disable take its
  place.
- There is no Tailwind CLI, Node.js or CSS build tooling.
- There is no remote render service in v1; only the Renderer seam (FR-37) is
  provided.
- There is no collector or inline-listener API (per-message closures). One-off
  confirmations use a Route with a one-shot Ref (FR-17).

## 8. MVP Scope

The MVP kind is a platform: a crate other teams build on. Every FR below is in v1;
the Priority Tier decides what slips first.

### 8.1 In Scope

- **P0 core** (a single-process bot with Commands and routed Views; nothing ships
  without it): FR-1, FR-2, FR-5, FR-6, FR-7, FR-8, FR-9, FR-10, FR-11, FR-12, FR-13,
  FR-15, FR-16, FR-17, FR-18, FR-19, FR-20, FR-21, FR-22, FR-23, FR-24, FR-25,
  FR-26, FR-27, FR-28, FR-29, FR-38, FR-48, FR-49, FR-50, FR-56, FR-57, FR-58,
  FR-60, FR-61, FR-62, FR-71, FR-75, FR-76, FR-77, FR-78, FR-79.
- **P1 required for v1** (launch-blocking, built after P0): FR-3, FR-4, FR-30,
  FR-31, FR-32, FR-33, FR-34, FR-35, FR-36, FR-39, FR-40, FR-41, FR-42, FR-43,
  FR-44, FR-45, FR-46, FR-51, FR-52, FR-54, FR-55, FR-59, FR-63, FR-64, FR-65,
  FR-66, FR-67, FR-68, FR-69, FR-70, FR-72, FR-73, FR-74.
- **P2 opt-in for v1** (shipped if ready, behind features or as extensions; slips
  first): FR-14 (prefix commands, `prefix` feature, off by default), FR-37
  (Renderer seam), FR-47 (multiple Apps), FR-53 (presence and gateway commands).
- In-memory defaults for every Adapter.

### 8.2 Out of Scope for MVP
- Official Redis and SQL Adapter crates. They are deferred to v1.x; the
  conformance suite makes third-party versions safe.
- A cross-process request bus (a `broadcastEval` equivalent). This is deferred.
- A Locale scaffolding CLI, which generates translation templates from registered
  Commands. This is deferred to v1.x.
- The Primary Entry Point command type and the LaunchActivity response type. twilight
  does not model them yet; the raw REST path (FR-31) covers urgent needs.
- Resharding without downtime when Discord's recommended shard count grows. Deferred
  to v1.x; v1 reshards by restart.
- Templates loaded at runtime from HTML or JSON (designer-editable cards). Deferred.
- Fallback handlers for unmatched Routes and Route prefix handlers. Deferred; the
  FR-19 typed error is the v1 behaviour.
- A named-emoji registry with per-environment ids. Deferred.
- Component Embeds (link previews) and a legacy text fallback for DMs. Deferred.
- A library of reusable Views and Templates (cards, leaderboards, charts) beyond the
  Project Template examples. Deferred to v1.x.

### 8.3 Launch Criteria
- All P0 and P1 FRs are done, with their consequences covered by tests.
- The NFR-1, NFR-3 and NFR-9 benchmarks run in CI and their targets are confirmed or
  re-set from the first results.
- The FR-74 documents exist, and `noctis` is published to crates.io with a changelog.

## 9. Success Metrics

**Primary**
- **SM-1:** The Project Template bot has sharding, auto-published Commands, a
  3-Step Wizard and a rendered card, all in ≤ 150 lines of user code. Validates
  FR-1, FR-7, FR-27, FR-32 and FR-49.
- **SM-2:** The author's next production bot (the schwi successor) runs on Noctis.
  `[ASSUMPTION: it is the first production consumer]`
- **SM-3:** A Claude Code agent given only the Project Template completes the UJ-5
  task, and its code compiles without reading Noctis source, in ≥ 4 of 5 trials.
  Validates FR-50.

**Secondary**
- **SM-4:** NFR-1, NFR-3 and NFR-9 benchmarks pass in CI against their confirmed
  targets.
- **SM-5:** The SM-2 bot gets zero Limit-related 400 responses in its first 30 days
  in production. A Limit-related 400 is an HTTP 400 with Discord error code 50035
  whose error path points into `components`. Validates FR-24 and FR-62.
- **SM-6:** Within 90 days of the first crates.io release, at least 5 bots by
  authors other than the maintainer are built from the Project Template.
  `[ASSUMPTION: threshold to revisit at launch]`

**Counter-metrics (do not optimize)**
- **SM-C1:** Number of features or builder methods. More API is not better; the
  Usage Skill must stay learnable. Counterbalances SM-1.
- **SM-C2:** Benchmark memory reached by disabling caches users rely on.
  Counterbalances SM-4.
- **SM-C3:** Adoption reached by shipping breaking changes without migration notes.
  Counterbalances SM-6.

## 10. Open Questions

1. How is the 4000-character text budget counted (UTF-16 code units or code
   points), and does a single Text Display have its own maximum? Verify
   empirically. The answer affects FR-24.
2. Does animated WebP play in Thumbnail and Media Gallery on every client? The
   answer affects FR-33's default.
3. What are the confirmed NFR-1, NFR-3 and NFR-9 numbers? NFR-3's values are working
   targets; all three are confirmed or re-set by the first benchmark (§8.3).
4. What is the maximum media `url` length in messages (2048 is documented only for
   Component Embeds)? Affects FR-24.
5. Can an update in response to a modal submit switch a legacy message to V2?
   Affects FR-23.
6. What propagation delay should FR-45 document for ModuleStateStore changes across a
   Cluster?

## 11. Assumptions Index

- FR-33: animated WebP is a safe default animated format.
- FR-50: public-API drift from the skill is enforced by a snapshot check in CI.
- FR-68: the emoji font is opt-in (Twemoji).
- NFR-1: initial memory target ≤ 1 KiB per idle guild, excluding the Discord cache.
- NFR-3: latency numbers are targets until the first benchmark confirms them.
- NFR-9: allocation goal of none for Route lookup and fixed-width Payload decode.
- SM-2: the schwi successor is the first production consumer.
- SM-6: 5 external bots in 90 days is the right adoption threshold.

FR-14 is no longer an assumption: prefix commands ship as an opt-in P2 module by
decision (dbi parity), overriding the brief's default of leaving them out.
