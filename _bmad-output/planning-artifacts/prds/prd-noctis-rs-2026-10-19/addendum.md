---
title: "PRD Addendum: Noctis"
created: 2026-10-19
updated: 2026-10-20
---

# Addendum: technical direction for architecture

These are technical decisions and constraints that came up while writing the PRD.
They belong to the architecture document, not to the PRD's capability statements.
Sources: the memlog and `../../research/`.

## Packaging

- **Repository:** a monorepo Cargo workspace.
- **Published crate:** `noctis` is the single facade crate. Users write
  `use noctis::...`.
- **Internal crates:** `noctis-*` sub-crates, enabled through `noctis` cargo
  features. Users never depend on them directly. All of these names were free on
  crates.io on 2026-10-19.
- **Repository name:** `ErdemGKSL/noctis-rs` (renamed from vesper-rs).

## Platform pins

- **twilight:** pin 0.17.1 for gateway, http, model, cache-inmemory and
  http-ratelimiting (`twilight-http-ratelimiting >= 0.17.1`; 0.17.0 can panic). Plan for 0.18,
  where `bucket()` replaces `create_recommended`.
- **takumi:** pin exactly `takumi = "=2.17.0"` and `takumi-core = "=0.31.0"` (MSRV
  1.91; architecture AD-15). It releases several times a week, so upgrade
  deliberately; bot authors get takumi fixes through Noctis releases.
- **Runtime:** tokio only, multi-threaded.
- **Toolchain (NFR-7):** edition 2024; MSRV 1.91 (the higher of twilight's and
  takumi's). twilight's MSRV may move in semver-compatible releases, so the pin is
  checked on every upgrade.
- **Observability (NFR-5):** spans and metrics hooks go through `tracing`.

## Startup safety

- **twilight constructors:** they panic outside a tokio runtime or without a rustls
  crypto provider. Noctis checks for both, installs a provider when none is set, and
  returns typed errors (FR-1).
- **Identify concurrency:** `InMemoryQueue::default()` allows only one identify at a
  time. Noctis passes `max_concurrency` from `GET /gateway/bot` (FR-2).
- **Multi-process HTTP (FR-52):** Discord's global HTTP rate limit (50 req/s by
  default) and per-route buckets are per token, so a Cluster needs a shared HTTP
  proxy (twilight-http-proxy or nirn-proxy) via
  `ClientBuilder::proxy(url, true).ratelimiter(None)`. In 0.17 `ResponseFuture`
  retries 429s immediately and indefinitely, so running a Cluster without a correct
  limiter is dangerous. Interaction and webhook endpoints bypass the global limit.
  Default request timeout 10-15 s.
- **Invalid token / intents (FR-1):** after a 401 the client fails every later
  request fast; `next_event` returning `None` means a fatal close (bad token,
  disallowed intents).
- **IdentifyQueue (FR-3):** the in-memory default wraps `InMemoryQueue` with the
  real `max_concurrency`; Cluster implementations serialize per bucket across
  processes (upstream HTTP-queue example, or Redis). Shard Ranges should be
  consecutive and aligned to multiples of `max_concurrency`.

## Gateway and dispatch

- **Interaction Lane (FR-5):** shard tasks only read the socket and forward; they
  never `send().await` into a bounded handler queue, which would stall the read loop,
  miss heartbeat ACKs and get the connection dropped. Interactions go to a separate
  unbounded-in-practice lane; other events go to a bounded queue with per-type drop
  policy. Optional per-guild ordering by guild-hashed workers.
- **Auto-defer deadline (FR-21):** default 2000 ms measured from gateway receipt.
  Commands defer with `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` (5), whose only allowed
  flag is `EPHEMERAL`; the V2 flag goes on Edit Original. Components and modal
  submits defer with `DEFERRED_UPDATE_MESSAGE` (6). `MODAL` (9) must be the first
  response and can't answer `MODAL_SUBMIT`. Tokens last 15 min; user-installed apps
  not in the guild get at most 5 follow-ups.
- **Session resume (FR-4, FR-55):** `CloseFrame::NORMAL` (1000/1001) invalidates the
  session; only `CloseFrame::RESUME` (4000) keeps it. Store session id, sequence
  **and** `resume_url`; after `close`, poll the shard until `GatewayClose` so the
  final sequence is saved.
- **Intents / event filters / cache (FR-6, FR-40):** intents decide what Discord
  sends, `EventTypeFlags` what twilight deserializes, cache `ResourceType`s need
  matching intents. The upstream intents→EventTypeFlags helper was reverted, so
  Noctis owns the mapping. Interaction-only default: `Intents::GUILDS`.
- **Gateway commands (FR-53):** sent per shard through `MessageSender`, limited to
  120 per 60 s per shard; 0.17.1 adds `Event::RateLimited` for request guild members.
- **Resharding (deferred):** large bots must use a shard count that is a multiple of
  `max_concurrency` (16); zero-downtime resharding starts a new shard set alongside
  the old one.

## Interaction routing

- **Dispatch index:** a flattened O(1) map, swapped atomically when a Module is
  enabled or disabled (ArcSwap-style). No heap allocation on lookup (NFR-9).
- **Compile-time enforcement (FR-75 to FR-79):** proc macros expand Command Patterns
  and validate paths, names, options and static component trees; the Payload derive
  sums the worst-case encoded width of the version tag, route id, separators and
  fields and emits `compile_error!` above 100. Tests use trybuild. Cross-Module
  checks run in the App builder before shards start. The catalog lives at
  `docs/discord-rules.md`; a CI script cross-checks catalog rows against trybuild
  and unit tests.
- **Refs (FR-17):** 11 base62 characters from a CSPRNG (62^11 ≈ 2^65.5), key
  prefixed by App id in the RefStore (FR-71).
- **Payload schema version (FR-60):** the codec's version tag is per Payload type;
  a mismatch routes to the Expired Handler or the Route's stale handler.
- **Select freshness (FR-25):** a per-render nonce in select `custom_id`s (within the
  100-char budget) makes clients show the server's `default_values`.
- **Optional integrity tag (FR-19):** truncated HMAC over inline Payloads.
- **`custom_id` codec:** a version tag, a short route id, a separator, then fields
  with escaping. Integers and snowflakes are base-62 packed. Field widths that are
  provable are length-checked at compile time; a runtime check plus the overflow
  policy covers the rest (FR-16, FR-17).
- **RefStore sketch (from research §4.3):**
  - methods `put(key, bytes, ttl)`, `get`, `delete` and an optional `touch`;
  - values serialized with serde;
  - the in-memory default is bounded, with TTL and LRU eviction.

## Components V2

- **Placement types:** placement rules are encoded in types (MessageTopLevel,
  ContainerChild, SectionAccessory, `ActionRow` as an enum, separate message and modal
  select types).
- **`Limits` struct:** configurable, so changes to Discord limits don't require a
  release.
- **No widget layer (2026-10-20):** Noctis ships no Navbar, Wizard or Paginator,
  no widget state, no view envelope or `view` route argument in `custom_id`, and no
  automatic re-render or pagination. A View is a plain author-written function
  `render_x(state) -> Message`; tab, page and step ids are ordinary typed Payload
  fields of the module's Routes, and handlers call the same render function and
  pass the result to `ctx.update(..)`. This mirrors the maintainer's TypeScript bot
  (schwi-v2). The UX patterns (tabs, pagination, step flows) are Usage Skill
  recipes. Budget helpers (FR-62) expose a message's cost and how many items of a
  given shape still fit.
- **Gaps in twilight 0.17.1:**
  - Checkbox and Checkbox Group are missing; Radio Group is missing everywhere.
  - Unknown component types lose their data.
  - Media items send `"content_type": null` (fixed on `main`, #2592).

  Noctis needs a raw JSON path (FR-31) and possibly its own component model that
  serializes directly, omitting unset optional media fields.
- **Receiving types 21/22/23 (FR-64):** twilight drops their payload, so Noctis
  consumes raw `Message`s from the shard stream, calls `twilight_gateway::parse`
  itself and keeps a raw-JSON fallback for modal submits.
- **Webhooks (FR-65):** always `?with_components=true`. Execute allows flags
  `SUPPRESS_EMBEDS`, `SUPPRESS_NOTIFICATIONS`, `IS_COMPONENTS_V2`; Edit allows
  `SUPPRESS_EMBEDS`, `IS_COMPONENTS_V2`. Non-owned webhooks may send only
  non-interactive components.
- **Upload Budget:** default 20 MiB (since 2026-09-03), overridden by the
  interaction's `attachment_size_limit`. Documented `attachment://` image types:
  jpg, jpeg, png, webp, gif.

## Rendering

- **Parser:** native Tailwind only, through takumi's built-in `tw` parser. Literal
  classes are validated at compile time by a proc macro that uses the same parser.
  takumi uses Tailwind v4 class names (`bg-linear-to-r`, not
  `bg-gradient-to-r`).
- **Render pool:** a dedicated rayon pool behind a semaphore.
- **Formats and budget (FR-33):** animated WebP is the default animated format
  (about 2.8x faster to encode than GIF and slightly smaller) and GIF the fallback.
  Degrade order: fps, viewport, lossy quality, still frame. Target about 2 MB.
- **Animation (FR-69):** CSS-driven (`animate-*`, arbitrary `animate-[...]` with
  `@keyframes` from the Theme stylesheet) or code-driven (one scene per frame).
  FPS caps: GIF 50, WebP/APNG 90. Redefining a built-in name such as `spin` yields a static result, hence the
  FR-69 registration error.
- **Per-job caps (FR-70):** defaults ≤ 4 MP per job, ≤ 90 frames, ≤ 1 MP per frame,
  ≤ 6 s duration. takumi accepts 1-64 MP and can't be interrupted mid-frame.
- **Render cache (FR-36):** single-flight deduplication of identical concurrent
  requests. Key: hash of Template id + version, data, size, format and settings,
  referenced asset versions (avatar hash), Noctis and takumi versions.
- **Inputs (FR-67):** takumi does not fetch URLs; it resolves `src` only against a
  pre-decoded image map or `data:` URIs. The AssetStore downloads, decodes (off the
  runtime) and caches remote images.
- **Fonts and emoji (FR-35, FR-68):** no bundled font in Rust takumi. Emoji need a
  COLR/bitmap font (Twemoji COLR ~1 MB, Noto Color Emoji COLRv1 ~4 MB). There is no
  CDN emoji hook, so custom emoji are fetched as images and laid out inline.
- **Theme (FR-32):** stylesheet with `@theme` tokens and `@keyframes` passed through
  render options (`from_html` drops `<style>`).
- **Global caches (NFR-1):** glyph cache 8 MiB (set before first render, unstable
  API), Tailwind parse cache 1 MiB, resource cache 64 MiB per handle.
- **Build (FR-48):** native WebP encoder (`libwebp-sys`) needs a C compiler; the
  image feature adds ~100 crates, so it is off by default. takumi types stay behind
  an unstable module (§6).
- **NFR-3 card benchmark:** measured ~2 ms render + ~1.5 ms PNG warm on 4 cores;
  WebP still encode at q85 is 6-10 ms, so the benchmark fixes PNG output.

## Project Template and skill

- **Location:** `examples/project-template/` has its own `Cargo.toml`. In the
  workspace it uses a path dependency on `noctis`, swapped to a version dependency
  when the Project Template is copied out.
- **Contents:** the Noctis Usage Skill, `AGENTS.md`, and a copy of the
  rust-best-practices skill.
- **Sync rule:** every public API change updates the template in the same commit
  (repo policy in the root `AGENTS.md`).
