# Rubric Walker Review: Noctis Architecture Spine

- **Spine:** `../ARCHITECTURE-SPINE.md` (status: draft, altitude: initiative)
- **Judged against:** Good-spine checklist (`.claude/skills/bmad-architecture/references/reviewer-gate.md`), spine philosophy (`SKILL.md`)
- **Cross-checked:** `prds/prd-noctis-rs-2026-10-19/prd.md` (FR-1..FR-79, NFR-1..NFR-10, UD-1..UD-8), `docs/discord-rules.md`
- **Date:** 2026-10-19

## Verdict

**Not ready to hand off; fix before story breakdown.** The spine is well shaped: it names a paradigm, its 17 ADs mostly carry enforceable Rules, it is terse, and every Stack version is current on crates.io. But one Rule (AD-10) contradicts the requirement it binds and would stall heartbeats. Three Rules (AD-5/6, AD-14, AD-15) leave or create exactly the divergence they claim to prevent. The dependency graph contradicts its own rule. The FR-52..FR-74 band, most of the UI Doctrine and the benchmark NFRs are mapped only loosely or not at all. The release/versioning and supply-chain part of the operational envelope is never mentioned.

Counts: 1 critical, 5 high, 7 medium, 6 low.

---

## Critical

### C1. AD-10 back-pressure on the shard task contradicts FR-5 and AD-10's own Prevents
- **Where:** AD-10 Rule, bullet 3: "When [the event lane] is full, the shard task applies back-pressure."
- **Problem:** FR-5 says "Reading a Shard's socket and heartbeating never wait on handler queues." It also requires a per-event-type policy: buffer to a configured depth, then drop the oldest *droppable* events, counting drops per type. Back-pressure in the shard task means the task stops polling the twilight `Shard` stream. In twilight 0.17 heartbeats are driven by polling that stream, so a full event lane stalls heartbeats. That is the "slow handler stalling heartbeats" divergence AD-10 says it prevents. Two implementers will also split here: one blocks on `send().await`, another uses `try_send` and drops.
- **Fix (autofix):** Replace bullet 3 with: "The shard task never awaits lane capacity. On a full event lane it applies the per-event-type policy: buffer to the configured depth, then drop the oldest events of droppable types. It never drops a non-droppable type, and it counts drops per type (`noctis_events_dropped_total{type}`)." Then state what happens to a full lane of non-droppable types, either a bounded overflow buffer or a disconnect-and-resume. Choosing between those is a discussion item.
- **Also missing from AD-10:** per-guild ordered dispatch (FR-5 last bullet) and ordered-before-concurrent Event Handlers (FR-6). The worker set needs a fixed mechanism, such as a keyed sub-queue per guild hash. Otherwise core and gateway will each invent one.

---

## High

### H1. `custom_id` versioning, integrity tag and route-id alphabet are ambiguous; spine and catalog disagree (AD-5, AD-6)
- `<ver>` "lets the format evolve", but FR-60 requires a **per-Payload schema version**, so that Stale Payloads go to the Expired Handler and compatible changes decode. The spine doesn't say whether `<ver>` is the codec format version or the Payload schema version, or where a schema version lives otherwise. The codec and `derive(Payload)` can choose incompatibly. FR-60 is also missing from the capability map.
- FR-19's optional **integrity tag** has no place in the wire format and no width in AD-6's length formula. A tagged Route can therefore pass the compile-time assert and still exceed 100 characters.
- `docs/discord-rules.md` CID-01 computes `MAX_LEN = route.len() + 1 + Payload::MAX_ENCODED_LEN`, with no `<ver>` character, and gives the by-ref worst case as `route.len()+1+1+11`. AD-6 uses `ver + route + 1 + …`. The catalog's example route `shop.buy` contains `.`, which the spine's `[a-z0-9-]{1,24}` forbids. The catalog writes `overflow = spill` where the spine writes `overflow = "spill"`. The two authoritative sources give the compile-time assert different formulas.
- **Fix:** In AD-5, define `<ver>` precisely, either as a format version or as format plus schema with a fixed width. Add the schema-version field and the optional tag (with its width) to the grammar, and add both to AD-6's formula. Then reconcile CID-01 and the route alphabet in the catalog (discuss which side wins).

### H2. AD-15 render cache key violates FR-36
- The key is `blake3(template_id, template_version, postcard(data), size, format)`. FR-36 also requires output settings, referenced asset versions, and the Noctis and renderer versions, so that "upgrading Noctis or the renderer never serves a stale cached image". The Rule as written requires the stale-cache bug.
- **Fix (autofix):** Extend the key with `settings`, `asset_versions`, `NOCTIS_VERSION` and `takumi` version. Also state that the attachment filename is derived from the key (FR-36).
- **Related gap:** "Remote images are fetched only through `AssetStore`" names a cache port, not a fetcher. Nothing assigns ownership of the HTTP fetch, the host allow-list, the byte cap, the timeout or the decode concurrency bound (FR-67). `noctis-image` and `noctis-core` could each implement it. Fix this with a sentence naming the owning crate and client.

### H3. AD-14 port shape can't express the semantics several ports need
- "Store ports operate on keys plus `Bytes` values with TTL." Some ports need more than that:
  - **RateLimitStore** needs atomic "N uses per window" increments across processes (FR-46). Get-then-set on KV races.
  - **RefStore** one-shot Refs (FR-17) need an atomic take (get-and-delete).
  - **IdentifyQueue** is a queue or lease, not KV.
  - **ModuleStateStore** needs change propagation within a documented delay (FR-45).
  Adapter authors will each pick their own atomicity, which is the "adapters with incompatible semantics" divergence AD-14 claims to prevent.
- Namespacing `<app>:<port>:<key>` doesn't say **who applies it**: the core wrapper or the adapter. If both do, keys are double-prefixed; if neither does, Apps collide. It also doesn't say what `<app>` is (App id source), or how FR-71's opt-in cross-App sharing (for example CacheStore) bypasses the prefix.
- **Fix:** Separate "store ports (KV+TTL)" from ports with their own primitive operations (`incr_window`, `take`, `acquire`, `watch`/poll). Make the conformance suites assert atomicity. Say that the core wrapper applies the namespace and adapters see opaque keys.

### H4. The crate dependency graph contradicts its own rule and the declared paradigm
- The rule says "no crate may depend on `noctis` or `noctis-testkit` except examples and tests", but the diagram has `noctis --> testkit`. If that edge is an optional `testkit` feature, the rule has to say so.
- `core --> gateway`: the Application layer depends directly on an edge adapter. The layer table also says the application does I/O "through ports and twilight-http only". The paradigm, though, says Discord I/O "is a driving or driven adapter at the edge". Either ratify the pragmatic layering (core owns gateway and HTTP) and drop the claim of a pure hexagonal edge, or invert the dependency.
- "No edge may point upward" is not enforceable as written, because the layer table doesn't define an order (for example, is `noctis-gateway` above or below `noctis-core`?). No mechanism checks it.
- Smaller inconsistencies: the layer table says `noctis-macros` does "no (reads locale files)" I/O. `tw!` validates with takumi's parser, so `macros` depends on takumi behind a feature, and that edge is absent from the diagram.
- **Fix:** Give an explicit allowed-edges list or numbered layers, plus an `xtask deps-check` (or `cargo-deny` `[bans]` wrappers) step in CI. Correct the testkit edge and the paradigm sentence.

### H5. PRD coverage: the FR-52..FR-79 band, the UI Doctrine and the benchmark NFRs are mapped loosely or not at all
- The capability map uses "V2 additions", "render additions" and "port additions" instead of FR ids. **AD-12 binds "FR-52 and up (V2 and modal FRs)", but FR-52 is shared HTTP rate limiting.** The following FRs have no explicit home or governing AD:
  - FR-52 (HTTP proxy; the deployment diagram shows it, but no AD covers it)
  - FR-53 (presence, gateway commands)
  - FR-54 (Lifecycle Hooks)
  - FR-55 (resumable shutdown)
  - FR-57
  - FR-58 (route-bound component construction, a real cross-crate divergence point: where the typed Route handle lives (codec/ui/core) and how "no handler" is detected for dynamic Modules)
  - FR-59
  - FR-60
  - FR-61, FR-62 (overflow policy)
  - FR-63 to FR-65 (webhooks: AD-11's "all replies" doesn't cover webhook sends or their flag rules)
  - FR-66 to FR-70 (per-job caps, admission weighting)
  - FR-71 to FR-74 (FR-73 Boot Flags: who filters, macros or Registry; FR-72 Module-scoped Locales: how separately invoked `i18n!` accessors merge and how key clashes are detected at App build)
- **FR-21's deadline is "measured from gateway receipt"**. The receipt timestamp must be stamped by the shard task and carried through the lane to the Responder. Neither AD-10 nor AD-11 fixes this, so gateway and core can diverge.
- **UD-1..UD-8** are listed in `binds`, but only UD-7 appears (AD-9). The following aren't mapped to any rule, convention or crate:
  - UD-1 (V2 only in built-in widgets and examples)
  - UD-2 (images only inside Views)
  - UD-3 (option-less entry Command returning a View, FR-61)
  - UD-4 to UD-6 (Wizard/Navbar/Paginator defaults, paginate-on-overflow default)
  - UD-8
  At minimum, the map needs a row binding them to `noctis-ui` and AD-17, or to the template lint.
- **NFR-1, NFR-3, NFR-9:** no benchmark harness, location or CI job. NFR-3 explicitly delegates the benchmark's event mix "to architecture", and the spine doesn't define it. NFR-9's allocation-free Route lookup and fixed-width decode has no Rule (AD-7 says O(1) but says nothing about allocation). NFR-5's required metrics (Ref hits and misses, render times, shard status) aren't named.
- **Fix:** Rewrite the map with explicit FR ids, correct AD-12's binds, and add a UD row and an NFR row. Add Rules for FR-58 and the FR-21 receipt timestamp.

---

## Medium

### M1. Operational envelope: release/versioning, supply chain and secrets are missing
- **Release/versioning:** AD-2 fixes one shared version and lockstep release. Missing:
  - the semver policy (PRD §6: breaking changes bump the minor version before 1.0, with changelog migration notes)
  - release tooling (release-plz or cargo-release)
  - crates.io publish order for the sub-crates
  - who or what publishes, from which branch or tag
  - a semver check (`cargo-semver-checks`)
  - the MSRV bump policy
  - the "unstable renderer module" outside semver (PRD §6)
- **Security/secrets:** the crates.io token in CI, supply-chain checks (`cargo-deny`/`cargo-audit`), the CSPRNG source for Refs, and the key source for FR-19's optional integrity tag (an HMAC key: where it comes from, rotation, sharing across a Cluster). None of these is decided, deferred or listed as an open question.
- **Deployment and observability** are adequately covered: the Deployment envelope plus the Logging and metrics convention.

### M2. The CI seed omits gates the PRD requires
- `ci.yml` lists fmt, clippy, test, trybuild, doc, public-api, skill-check and template build. Missing:
  - the feature-combination matrix (FR-48)
  - an MSRV job (NFR-7; also, the catalog says trybuild `.stderr` depends on a pinned rustc, which must be fixed somewhere)
  - NFR-1/3/9 benchmarks (§8.3 launch criterion)
  - pixel snapshots that gate renderer upgrades (NFR-8)
  - the rules-catalog check (FR-79), which is listed under `xtask` but not in CI
  - `missing_docs` denial (FR-74)

### M3. AD-11's runtime state machine conflicts with the catalog's typestate
- AD-11 says "A transition that isn't legal is an error". `docs/discord-rules.md` INT-05 specifies a compile-time typestate (`modal()` exists only on `Responder<Pending>`). Per AD-4 (earliest stage), the typestate should win, and the runtime error only covers the auto-defer race. Fix this in AD-11.

### M4. AD-3's runtime `Limits` override and compile-time defaults can diverge
- Macros check static input against default Limits, and static input is never rechecked at run time (AD-4, FR-75). An operator who *tightens* a Limit at run time (FR-24: "a Discord limit change needs no Noctis release") therefore gets no enforcement on static trees. Decide whether runtime revalidates static trees at App build when Limits differ from the defaults, or that overrides may only loosen.

### M5. Postcard port values versus long-lived Refs
- AD-14 serializes typed port values with postcard, which isn't self-describing. Refs live for months (FR-60), so a Payload struct change silently misdecodes stored Refs unless the stored bytes carry the schema version from H1. Tie the Ref value encoding to the same versioning rule.

### M6. Deferred omits the PRD's open questions that affect the architecture
- PRD Q6 (the FR-45 ModuleStateStore propagation delay; AD-7 mentions "a local cache" with no TTL or invalidation rule), Q2 (the animated WebP default), Q5 (legacy→V2 via modal submit) and Q3 (benchmark numbers). Carry these as open items with revisit conditions.

### M7. Error convention versus HandlerError
- The convention says "never `Box<dyn Error>` in public APIs", but AD-13's `HandlerError` "carries a source", which is necessarily type-erased for arbitrary bot errors. State the allowed erased form (for example `HandlerError::new(impl Error + Send + Sync + 'static)`) so crates don't diverge. AD-13 also doesn't say how a panic in a Render Job (rayon) is contained (NFR-4).

---

## Low

- **L1.** The AD-9 Rule "the framework never stores a closure, task or timer per message" can only be checked in review. Consider a testkit assertion (for example a memory-flat test across N messages) so it can be enforced.
- **L2.** AD-16: a runtime `LocaleSource` overlay can change a value's placeholders, so a mismatch reappears at run time. State that overlays are validated against the compiled placeholder set.
- **L3.** `takumi 2.17.0` was published today (2026-10-19) and is pinned exact. That is current, but reconfirm before the first build. All other Stack versions match crates.io's latest stable (syn 3.0.9, tokio 1.53.3, twilight 0.17.1, etc.). Pairing `quote 1.0` with `syn 3.0` is fine.
- **L4.** The catalog's Command Pattern expansion is "breadth-first over the first group", while FR-8 says "left to right in declaration order". This is a PRD/catalog mismatch that AD-8 should pin, because the handler's typed combination value depends on the order.
- **L5.** FR-25/FR-16 require custom_id uniqueness within a message at run time, for example Navbar tabs and Paginator controls sharing a Route. The disambiguation scheme (per-render identifiers) isn't fixed in AD-5.
- **L6.** Terseness: the spine is mostly decisions-only. A few Prevents lines lean toward rationale (AD-12's twilight-model gap list), which is acceptable. No action needed.

---

## Checklist summary

| Checklist item | Result |
| --- | --- |
| Fixes the real divergence points, misses none | Partial: misses FR-58 handle, FR-21 receipt stamp, per-guild ordering, Boot Flags, Module-scoped Locale merge, namespace ownership, fetcher ownership |
| Every AD Rule enforceable and actually prevents its divergence | Fail on AD-10 (contradicts), AD-15 (incomplete key), AD-14 (no atomics), AD-5/6 (ambiguous ver, catalog mismatch); dependency rule unenforced |
| Nothing under Deferred lets units diverge | Pass: Deferred items are safe. But PRD open questions are missing from it (M6) |
| Named tech verified-current | Pass (checked crates.io 2026-10-19) |
| Brownfield ratification | N/A (greenfield); `docs/discord-rules.md` acts as an existing reference, and the spine contradicts it in places (H1, M3) |
| Spec/PRD capability coverage | Partial (H5) |
| Operational envelope decided/deferred/open | Partial: deployment and observability yes; release/versioning, supply chain and secrets silent (M1); CI incomplete (M2) |
| Terse (decisions, not rationale) | Pass |
