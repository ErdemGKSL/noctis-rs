---
title: "PRD Addendum: Noctis"
created: 2026-10-19
updated: 2026-10-19
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
  http-ratelimiting. Avoid http-ratelimiting 0.17.0 (it can panic). Plan for 0.18,
  where `bucket()` replaces `create_recommended`.
- **takumi:** pin 2.17.x (MSRV 1.91). It releases several times a week, so upgrade
  deliberately.
- **Runtime:** tokio only, multi-threaded.

## Startup safety

- **twilight constructors:** they panic outside a tokio runtime or without a rustls
  crypto provider. Noctis checks for both, installs a provider when none is set, and
  returns typed errors (FR-1).
- **Identify concurrency:** `InMemoryQueue::default()` allows only one identify at a
  time. Noctis passes `max_concurrency` from `GET /gateway/bot` (FR-2).
- **Multi-process HTTP:** Discord's global HTTP rate limit is per token, so a
  Cluster needs a shared HTTP proxy (twilight-http-proxy or nirn). Document this, and
  make the proxy URL configurable.

## Interaction routing

- **Dispatch index:** a flattened O(1) map, swapped atomically when a Module is
  enabled or disabled (ArcSwap-style).
- **`custom_id` codec:** a version tag, a short route id, a separator, then fields
  with escaping. Integers and snowflakes are base-62 packed. Field widths that are
  provable are length-checked at compile time; a run-time check plus the overflow
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
- **Gaps in twilight 0.17.1:**
  - Checkbox and Checkbox Group are missing; Radio Group is missing everywhere.
  - Unknown component types lose their data.
  - Media items send `"content_type": null`.

  Noctis needs a raw JSON path (FR-31) and possibly its own component model that
  serializes directly.

## Rendering

- **Parser:** native Tailwind only, through takumi's built-in `tw` parser. Literal
  classes are validated at compile time by a proc macro that uses the same parser.
  Takumi uses Tailwind v4 class names (`bg-linear-to-r`, not
  `bg-gradient-to-r`).
- **Render pool:** a dedicated rayon pool behind a semaphore.
- **Render cache:** keyed by a hash of template, data and size, with deduplication
  of identical concurrent requests (single-flight).
- **Formats and budget:** animated WebP is the default animated format and GIF the
  fallback. A degrade loop keeps output under the upload budget.
- **Inputs:** takumi does not fetch URLs. The AssetStore downloads and caches remote
  images.

## Project Template and skill

- **Location:** `examples/project-template/` has its own `Cargo.toml`. In the
  workspace it uses a path dependency on `noctis`, swapped to a version dependency
  when the template is copied out.
- **Contents:** the Noctis Usage Skill, `AGENTS.md`, and a copy of the
  rust-best-practices skill.
- **Sync rule:** every public API change updates the template in the same commit
  (repo policy in the root `AGENTS.md`).
