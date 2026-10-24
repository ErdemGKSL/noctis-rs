---
title: 'Story 10.10: Render cache and single-flight deduplication'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '4cab7fd8ec79cc64076d4940ed9bf1694ca8d6b4'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-9-emoji-rendering.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-36 is not implemented. Every `ctx.render` of the takumi Renderer draws and encodes again, even for identical requests. N concurrent identical requests take N queue slots and run N renders. The App's namespaced `RenderCache` port (`MemoryRenderCache`, 64 MiB, 1 day) is resolved but never used. `RenderOutput::digest` is `blake3(bytes)`, not a cache-key digest, so the attachment name does not come from the key (AD-22).

**Approach:**
- `TakumiRenderer` computes a blake3 render key (AD-15) for every admitted job. It joins a process-local single-flight entry for that key, or starts one.
- The flight looks the key up in the App's `RenderCache` (handed over through `RendererPorts`), renders on a miss, and stores the result.
- Every caller gets the output with `digest` = the key digest, so core's existing `Attachment::rendered(bytes, digest, ext)` names the attachment by the key.

## Boundaries & Constraints

**Always:**
- **Key (AD-15, AC 1).** `cache::RenderKey([u8; 32])` is `blake3::Hasher::new()` over these fields, in this order. Every variable-length field is framed as its `u64` LE byte length followed by its bytes. Integers are fixed-width LE.
  1. `KEY_SCHEMA` = `b"noctis.render.v1"`. Bump it whenever this layout changes.
  2. `NOCTIS_VERSION` = `env!("CARGO_PKG_VERSION")`. All crates share one version (AD-2).
  3. `TAKUMI_VERSION` = `"takumi=2.17.0;takumi-core=0.31.0"`.
  4. `template_id`.
  5. `template_version` (u32).
  6. `Theme::digest()` (32 bytes).
  7. `job.data` (the postcard bytes).
  8. The asset pairs: a `u64` count, then each pair as `(id framed, 32-byte version)`, sorted by id bytewise. The pairs are:
     - `"fonts"`: `FontRegistry::digest()`, a new blake3 over every `FontsConfig` font's digest, family and ranges in order, plus the `bundled` and `emoji` flags;
     - `"image:<id>"`: blake3 of the bytes, for every static image the App registered;
     - `"remote.cdn"`: blake3 of the `AssetFetcher` CDN base;
     - `"remote.fallback"`: blake3 of the configured fallback `StaticImage` bytes, or of `b"builtin"`.
     The pairs are computed once, in `TakumiRenderer::new`.
  9. `size`: width and height (u32 each).
  10. `format`: a tag (`Png 0`, `Jpeg 1`, `Webp 2`, `WebpLossless 3`) plus a quality byte (0 when there is none).
  11. `animation`: `0u8`; or `1u8` + `duration.as_nanos()` (u128) + `fps` (u16) + a format tag (`Webp 0`, `Gif 1`, `Apng 2`) + a quality byte.
  12. Encode settings: `min(job.upload_budget, size_target)` (u64), then `still_fallback` (u8).
- The key inputs pass through one `KeyInputs<'_>` struct. Its version strings are fields that default to the constants, so tests can vary every input.
- **Store key.** `render:v1:<64 lowercase hex>`. Core's namespace makes it `<app>:render_cache:render:v1:…`.
- **Entry format.** A 22-byte header, then the image bytes:
  - `b"NRC1"`;
  - the `OutputFormat` tag (`Png 0`, `Jpeg 1`, `Webp 2`, `Gif 3`, `Apng 4`);
  - width and height (u32 LE);
  - a quality presence byte plus the quality;
  - an fps presence byte plus the fps (u16 LE);
  - frames (u32 LE).
  An entry decodes when the magic is right, the tags are known, both sides and frames are ≥ 1, a presence byte is 0 or 1, and at least one image byte follows. Decoding returns `bytes.slice(22..)` (no copy).
- **Flight (AC 2).** It lives in a new module, `crates/noctis-image/src/flight.rs`.
  - `Flights` holds `Mutex<HashMap<RenderKey, (u64 generation, WeakShared<FlightFuture>)>>` (std `Mutex`, never held across `.await`).
  - `FlightFuture` is `BoxFuture<'static, Result<RenderOutput, Arc<RenderError>>>` wrapped in `futures_util::future::Shared`. It owns `Arc<Shared>`, the `ViewFn`, the job, the key and the plan.
  - **Body:**
    1. With a bound cache, `get`. A decoded hit returns at once.
    2. On a miss, resolve the plan if there is one, then `submit` and await.
    3. Store the output with `put(key, entry, None)` when it succeeded and `!resolved.degraded()`.
    4. Return the output.
  - A guard in the body removes the map entry on drop (completion or abandonment) when its generation still matches. A later identical request after completion is a cache hit.
- **Admission order is unchanged where it can be.**
  - `render()` builds or joins the flight. It polls its own `Shared` clone once with `std::task::Waker::noop()` and returns `Box::pin(clone)`.
  - With no bound cache, or the in-memory one (whose `get` is ready at once), a job without remote images is still admitted at the call (10.3).
  - An adapter whose `get` pends admits after the lookup. Jobs with remote images admit after the fetch, as before.
- **Ordering in `render`.** `view_of` (version, animation, fps, caps) → `declared` (the plan; `OverCap` for remote images) → key → flight. These errors stay synchronous and never touch the cache or the map.
- **Cancellation.** The job is given up only when every caller has dropped its future: the last `Shared` clone drops the inner future, so a queued job is skipped and pending fetches are aborted. One caller dropping its future never cancels the others.
- **Errors.**
  - An error is shared with every waiter of that flight and never stored.
  - A waiter gets `Arc::try_unwrap(err)` when it holds the last reference. Otherwise it gets `share_error(&err)`, a private copy:
    - same variant and fields for every variant with owned data;
    - `Failed(e)` → `RenderError::failed(e.to_string())`;
    - unknown (non_exhaustive) → `failed(err.to_string())`.
- **Degraded renders are not stored.** `Resolved` gains `degraded: bool`. It is true when a non-required image fell back after a fetch failure, and when `fall_back` received any emoji over the cap.
- **Port failures never fail a render.**
  - A `get` error or an undecodable entry: one `tracing::warn!(target: "noctis::image", template, …)`, then render, then overwrite the entry.
  - A `put` error: one warning, and the output is still returned.
- **`RenderOutput::digest`** is the key digest on hit and on miss. Its doc becomes "the render's digest, which names the attachment; the takumi Renderer uses its render cache key (AD-15, AD-22)". Core keeps calling `Attachment::rendered` (AC 3; see Design Notes).
- **Ports (public API).**
  - `RendererPorts` gains `pub render_cache: Option<Arc<dyn RenderCache>>` (`None` from `new`) and `#[must_use] pub fn with_render_cache(self, cache: Arc<dyn RenderCache>) -> Self`.
  - `bind` keeps "first call wins" for both ports.
  - Core `app.rs:2708` binds `RendererPorts::new(asset_store).with_render_cache(ports.render_cache.clone())`.
- **Memory (NFR-1).** Render-cache memory is the port's own budget, which is configurable and reported: `MemoryRenderCache::with_limits`, `usage()`. The flight map holds only in-flight keys.
- **Same commit (AGENTS.md).** Update:
  - the Usage Skill: `images.md` §9, and `adapters.md` `:318-324`;
  - the template's `AGENTS.md` `:70` (one clause);
  - `xtask/public-api.txt` and the `SKILL.md` `API hash`;
  - the facade image docs.

**Never:**
- No cross-process or distributed single-flight.
- No new crate, no `cargo update`, no `Cargo.lock` change. `futures-util` (already a `render` dependency) provides `Shared`.
- No core-side cache, and no core dependency on `noctis-image`.
- Never cache errors or degraded renders.
- Never key on `upload_budget` raw (only the effective budget), `ClassCheckMode`, caps or `RemoteConfig` limits. They change only errors or degraded (unstored) outputs.
- No new `ImageConfig` knob, no new metric, no new rule id, no `docs/discord-rules.md` change.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Key determinism | the same `KeyInputs` twice | equal digests | none |
| Key sensitivity | change exactly one of: schema, noctis version, takumi version, id, version, theme digest, data (1 byte), each asset pair version, an added/removed pair, width, height, format tag, quality, animation None↔Some, duration +1 ns, fps, animated format, effective budget, still_fallback | 20+ distinct digests, all ≠ base | none |
| Framing | id `"ab"` + data `"c"` vs id `"a"` + data `"bc"`; empty data vs one zero byte | different digests | none |
| Pair order | the same pairs inserted in two orders | equal digests (sorted) | none |
| Budget | `upload_budget` 3 MB vs 25 MB with `size_target` 2 MB; 1 MB vs 2 MB | equal; different | none |
| Unicode | `template_id` `"kart.ş"`, data holding `"😀é"` | stable digest (golden hex in test) | none |
| Store key | any key | `render:v1:` + 64 lowercase hex, 74 chars | none |
| Entry round trip | PNG still (no quality/fps, 1 frame); animated WebP q80 fps 30, 45 frames; GIF | decode == encode input; bytes slice shares the buffer | none |
| Entry invalid | empty; 21 bytes; 22 (no image byte); bad magic; tag 5; width 0; frames 0; presence byte 2 | `None` | warn, re-render, overwrite |
| Concurrent identical | 8 identical jobs joined, a counting view, unbound renderer | view called once, 8 equal outputs (bytes, digest), `pool.admitted()` peaks at 1 | none |
| Concurrent distinct | 2 jobs, data differs by one field | 2 renders, different digests | none |
| Sequential with cache | the same job twice, `MemoryRenderCache` bound | second: no view call, equal bytes, digest, format, settings_used | none |
| Sequential no cache | the same job twice, unbound | 2 renders, equal digest | none |
| Digest | any render | `output.digest == RenderKey` bytes ≠ `blake3(bytes)` | none |
| Admission | gated still jobs with distinct tags, memory cache bound, capacity 2 | the 3rd call is `QueueFull` without polling a, b | `QueueFull` |
| Leader dropped | A starts, B joins, A's future dropped | B still gets the output, 1 render | none |
| All dropped | A and B dropped before the job runs | job skipped (view never called), map entry gone | none |
| Shared error | 3 identical jobs with a panicking view | all 3 `Panicked { message: "boom" }`, view called once; a 4th later call renders again (not cached) | Render sink per caller (core) |
| Shared Failed | 2 identical waiters, `Failed(io error)` | the first-unwrapped one keeps the source, the other gets `Failed` with the same `Display` | none |
| Sync errors | unknown version; fps over cap; `OverCap` remote images | immediate error, no cache `get`, no map entry | typed error |
| Degraded | a non-required avatar 404s; 33 emoji with cap 32 | output returned, no `put`; next identical call renders again | warn (10.8/10.9) |
| Required failure | `#[asset(required)]` 404 | `RemoteImage` error, not stored | typed error |
| Cache hit skips fetch | an avatar Template rendered twice, cache bound | 1 HTTP request, second call no fetch | none |
| Cache get error | an adapter whose `get` returns `PortError::Backend` | renders; 1 warning; `put` attempted | warn |
| Cache put error | an adapter whose `put` errors | output returned; 1 warning | warn |
| Corrupt entry | the store holds `b"junk"` under the key | renders, overwrites with a valid entry | warn |
| Pending adapter | an adapter whose `get` yields once before `Ok(None)` | renders correctly (admitted after the lookup) | none |
| Theme/asset change | two renderers differing only in theme, a registered image, fonts or fallback, sharing one cache | the second misses | none |
| Takumi pin | `TAKUMI_VERSION` vs `Cargo.lock` `takumi`/`takumi-core` versions | test fails on mismatch | test failure |
| App surface | the same Template via an App's `renderer()` twice, a custom counting `.render_cache(..)` | 1 `put`, key starts `<app>:render_cache:render:v1:`, both attachments named `<first 16 hex of key>.png` | none |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs` `:347-357` (`RenderOutput::digest` doc), `:536-556` (`RendererPorts`: add the field and `with_render_cache`, keep `new`). The `store::RenderCache` import already exists at `:20`.
- `crates/noctis-core/src/app.rs` `:2706-2709`: the bind call. `crates/noctis-core/src/app/tests.rs` `:1250-1300` (`Binding`, `img_03_build_binds_the_namespaced_asset_store`): extend it to assert that the bound render cache writes `a:render_cache:<k>` into a backing `MemoryRenderCache` passed with `.render_cache(..)`.
- `crates/noctis-image/src/lib.rs` `:42-66`: add `#[cfg(feature = "render")] mod cache;` and `mod flight;`.
- `crates/noctis-image/src/cache.rs` (+ `cache/tests.rs`), new: `RenderKey`, `KeyInputs`, `KEY_SCHEMA`/`NOCTIS_VERSION`/`TAKUMI_VERSION`, `store_key`, `encode_entry`/`decode_entry`, plus the `Cargo.lock` pin test, which reads `concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")` with `std::fs`.
- `crates/noctis-image/src/flight.rs` (+ `flight/tests.rs`), new: `Flights`, the guard, `share_error`.
- `crates/noctis-image/src/renderer.rs`:
  - `Shared` `:46-67`: add `render_cache: OnceLock<Arc<dyn RenderCache>>`, `asset_pairs`, `flights`;
  - `new` `:150`: compute the pairs;
  - `render` `:279-310`: replace both arms with key → flight;
  - `submit` `:226`: unchanged;
  - `bind` `:336`: set both ports;
  - `draw` `:385-410`: drop the `blake3(bytes)` call; `draw` returns `digest: [0; 32]` and the flight body sets `output.digest = key.0` right after the receiver yields, before the `put`.
- `crates/noctis-image/src/fonts.rs` `:244-284`: `FontRegistry` gains `digest: [u8; 32]`, computed in `init`; `Font::digest` `:100` is reused.
- `crates/noctis-image/src/assets.rs` `:146-190`: `StaticImages::versions()` → `(id, blake3(bytes))`.
- `crates/noctis-image/src/remote/fetch.rs`:
  - `AssetFetcher` `:55-160`: `key_pairs()` for `remote.cdn` and `remote.fallback`; the fallback image bytes come from `config.fallback_image()` at `:125`;
  - `Resolved` `:455-490` and `resolve` `:246-290`: `degraded`.
- Tests that render identical jobs on purpose and must vary their data, so the dedup does not merge them:
  - `renderer/tests.rs` `:114` (`Gated` → `Gated { tag: u8 }`) and `:487`;
  - `crates/noctis/tests/render.rs` `:127`, `:687`;
  - any App-level test in `crates/noctis/tests/{render,emoji,remote*}.rs` that renders one job twice and expects a second fetch or warning.
  Keep each test's intent.
- `crates/noctis/src/lib.rs` `:2085-2160` (image module docs): add a "Cache (FR-36)" bullet.
- Skill:
  - `examples/project-template/.claude/skills/noctis/references/images.md` `:749-754` (§9);
  - `references/adapters.md` `:318-324`;
  - `SKILL.md` `:23`;
  - `examples/project-template/AGENTS.md` `:70`.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ports/src/render.rs`: `RendererPorts::render_cache` + `with_render_cache`, and the `digest` doc. The port carries the cache.
- `crates/noctis-core/src/app.rs` and `app/tests.rs`: bind the namespaced render cache, and test the namespace.
- `crates/noctis-image/src/cache.rs` + `cache/tests.rs`: the key, the store key and the entry codec. Tests `fr_36_*` cover every Key, Framing, Pair order, Budget, Unicode, Store key, Entry and Takumi pin row.
- `crates/noctis-image/src/fonts.rs`, `assets.rs`, `remote/fetch.rs` (+ their tests): the digests, the pairs and `degraded`.
- `crates/noctis-image/src/flight.rs` + `flight/tests.rs`: single-flight, the removal guard and `share_error`, tested on plain futures (leader dropped, all dropped, error sharing).
- `crates/noctis-image/src/renderer.rs` + `renderer/tests/cache.rs` (new, registered in `renderer/tests.rs`): wiring. Tests use a counting view (a static `AtomicUsize`, as `DROP_LOG` does) and a test `RenderCache` adapter (errors, pending, corrupt), and cover the Concurrent, Sequential, Digest, Admission, Errors, Degraded, Hit skips fetch, Port failure and Theme/asset rows. Fix the identical-job tests named in the Code Map.
- `crates/noctis/tests/render.rs`: the App surface row, with `#![cfg(feature = "image")]` as already present.
- `crates/noctis/src/lib.rs`, the skill files, `AGENTS.md`, `xtask/public-api.txt`: the docs.
  - `images.md` §9 states: the key's inputs; that a hit skips remote fetches and admission; process-local dedup; that errors and degraded renders are not cached; that you bump `version` when the look changes (a view change alone is not in the key); that registering another static image or font invalidates the App's cache; the attachment name; and `MemoryRenderCache::with_limits`.
  - Re-bless with `cargo run --release -p xtask -- api-snapshot --bless` and copy the hash into `SKILL.md`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: the planner already logged the decisions.
- `.github/workflows-disabled/ci.yml`: confirm only. There is no new gate.

**Acceptance Criteria:**
- Given a render request, when its key is computed, then it is the blake3 of the AD-15 fields in the order above, and a change to any single field changes the digest (the Key sensitivity test enumerates every field).
- Given N concurrent identical `Renderer::render` calls, when they run, then the view runs once, one queue slot is used, and all N receive equal `RenderOutput`s. A later identical call with a bound cache renders nothing.
- Given a rendered or cached result, when `ctx.render` attaches it, then the attachment name is the first 16 hex of the render-key digest: a `noctis-image` test asserts `output.digest == RenderKey` bytes (and ≠ `blake3(bytes)`), and the App-level test asserts the attachment name is `<first 16 hex of output.digest>.png` on both the miss and the hit.
- Given Noctis or takumi is upgraded, when an old cache entry exists, then the key differs (versions are key fields; the pin test keeps `TAKUMI_VERSION` honest), so no stale image is served.
- Given the public API change, when the story is done, then `api-snapshot`, `skill-check`, `deps-check` and `rules-catalog` pass, and `Cargo.lock` is unchanged.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 22 findings — high 0, medium 5, low 13, false 4, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) A degraded render and a later complete render of one key share the attachment name `<key16>.png`, so `AttachmentSet::add` keeps the first one in a message — verified: `Flight::run` sets `output.digest = key.0` also when `degraded`, and `AttachmentSet::add` keeps the first entry under a name. Grouped with intent D4 (same root cause). Patch: a degraded (uncached) output is named by its content (`blake3(bytes)`); the epics AC only requires the key name for cached results.
  - `[low]` `[reject]` (blind) The cache `put` is awaited before waiters get the output and is lost if every caller drops during it — real, but the in-memory default is ready at once and the spec settled the order; a detached put adds spawning and lifetime handling.
  - `[medium]` `[patch]` (blind) Entries are written with `ttl = None` — verified: the store contract says `None` means no expiry; only `MemoryRenderCache` caps it at 1 day, so a swapped adapter (the skill suggests Redis) keeps every render forever, and §9's "until the entry expires" never ends. Patch: document in `images.md` §9 (and the `adapters.md` render_cache row) that the Renderer writes with no TTL and a swapped adapter must bound lifetime itself.
  - `[medium]` `[patch]` (blind) `RemoteImage::url(..)` content that changes under the same URL is served stale — verified: remote images are keyed by identity only (Design Notes), which holds for CDN hashes but not for a mutable allow-listed URL. Grouped with intent D1. Patch: §9 states that a URL image must change its URL (e.g. a version query) when its content changes.
  - `[low]` `[reject]` (blind) The NRC1 entry has no payload length or hash, so a truncated backend value decodes — needs a corrupting backend; the fix is a new format version plus checks.
  - `[low]` `[reject]` (blind) The eager noop poll runs the cache get and `resolve` (tokio timers) inside `render()`, panicking outside a runtime — real only for a call outside tokio; such a future would hit the same panic on its first poll before this change, and `ctx.render` always runs in the runtime. Grouped with edge E1.
  - `[low]` `[reject]` (blind) `share_error` silently maps a future `RenderError` variant to `Failed`, and which waiter keeps the source depends on drop order — the wildcard is forced by `#[non_exhaustive]`, drop-order behaviour is the spec's design; harm only on a future variant, and the fix is a new test harness or a ports API change.
  - `[low]` `[reject]` (blind) `draw`'s placeholder `digest: [0; 32]` would leak through any future path that skips the flight — the flight is the only caller today; a hypothetical future path, and the fix is a signature refactor.
  - `[low]` `[reject]` (blind) The `raced` branch of `Flights::join` has no multi-threaded test — read and verified correct: the losing body is dropped unpolled outside the lock and its guard's generation does not match the winner's, so it never removes the winner; adding a threaded harness is more than a direct correction.
  - `[low]` `[reject]` (blind) The TAKUMI_VERSION pin test reads `../../Cargo.lock` and panics in a packaged crate — tests are not run from the tarball in this project (publish verification only builds); the fix adds a skip branch.
  - `[low]` `[reject]` (blind) `ClassCheckMode` is not keyed, so a hit skips a Deny-mode class error — the spec excludes the mode on purpose (it only changes errors); the image served is identical and only a cache shared across differently configured Apps reaches it.
  - `[low]` `[reject]` (blind) `RenderOutput::digest` changed meaning without a release note — the repo has no changelog before launch (Epic 14); the ports doc, `adapters.md` and the skill were updated; the "same digest, same bytes" guarantee is restored by the degraded-naming patch above.
  - `[low]` `[reject]` (edge E1) Eager poll outside a tokio runtime panics in `resolve` — same root cause and reasoning as the blind eager-poll row.
  - `[low]` `[reject]` (edge E2) A panicking user `RenderCache` adapter poisons the `Shared` flight for every joiner until all clones drop — needs a panicking adapter (a user bug that panicked the caller before this change too); the fix wraps the body in `catch_unwind`.
  - `[low]` `[patch]` (vgap) `KeyInputs::of`'s mapping of template version, format, animation and still_fallback is pinned by no test (the digest tests are circular) — pre-verified gap; the mapping reads correctly today. Patch: add `fr_36_format_animation_and_version_changes_miss_the_cache` to `renderer/tests/cache.rs`.
  - `[medium]` `[patch]` (intent D1) Asset pairs are App-wide, not per job; remote content changing behind one identity is served stale — the App-wide invalidation is documented and only costs re-renders after a deploy; the stale-URL part is real and grouped with the blind stale-URL row (same patch).
  - `[false]` `[reject]` (intent D2) Concurrent dedup is tested only at the Renderer level — `ctx.render` (`noctis-core/src/render.rs:130`) calls `renderer.render(job)` directly, so the Renderer-level tests cover the bot-visible path.
  - `[low]` `[reject]` (intent D3) Dedup is per Renderer, not per process — two Apps in one process also have separately namespaced caches, so cross-App dedup is not a cache-consistent goal; the cost is one extra render per App, and moving `Flights` into a Runtime resource is more than a direct correction.
  - `[medium]` `[patch]` (intent D4) Editing a message holding a degraded card into the complete card keeps the old fallback file, because both are `<key16>.png` and the edit diff keeps a known name — verified in `noctis-core/src/responder/files.rs` (`KnownFiles`); grouped with the blind degraded-naming row (same patch).
  - `[false]` `[reject]` (intent D5) Output `EncodeSettings` are not keyed — they are a deterministic function of the keyed pre-render inputs (format, quality, animation, effective budget, still_fallback), so no request pair differs only in them.
  - `[false]` `[reject]` (intent D6) `Attachment::rendered` instead of `with_digest` — AD-22 makes `rendered` the call for rendered images; it names by digest exactly like `with_digest`.
  - `[false]` `[reject]` (intent D7) `RenderOutput::digest` changes meaning in a public contract — FR-36 requires the key-derived name, and the doc, `adapters.md` and skill were updated in the same change.

## Design Notes

- **Why the Renderer owns the cache.** The key needs the takumi version, the Theme digest, fonts and static images, which only `noctis-image` knows. Core stays image-free (AD-22), and a remote Renderer (FR-37) may cache its own way. `RendererPorts` was left `#[non_exhaustive]` in 10.8 for this purpose.
- **`Attachment::with_digest` vs `rendered`.** AD-22 makes `rendered(bytes, digest, ext)` the call for rendered images. It is `with_digest` plus the `Rendered` origin that the validator needs (FR-30). Feeding it the key digest satisfies AC 3.
- **Remote images by identity, not by bytes.** Their identity (ids, avatar hashes, emoji ids, URLs) is already in `postcard(data)`. The App-level config that changes their bytes (CDN, fallback) is in the pairs. So a hit needs no fetch. Renders that drew a fallback because of a failure are not stored, so a transient 404 is never pinned for a day.
- **The poll-once trick.** Polling the leader's `Shared` clone with a noop waker runs the body until its first real pending point. The in-memory `get` is ready, so `try_submit` runs inside `render()` and the "admitted at the call" contract of 10.3/10.5 holds without a second code path.
- **The key is not a bot-author secret.** It is derived from data the bot already holds. The 16-hex attachment prefix (64 bits) is the existing naming scheme.

## Verification

**Commands** (run in `/home/claude/lanes/a/noctis-rs`):
- `cargo fmt --all --check`: clean.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --release -p noctis-image --features render` and `--features render,emoji,gif`: pass.
- `cargo test --release -p noctis-ports` and `cargo test --release -p noctis-core`: pass.
- `cargo test --release --workspace --all-features`: pass. Run it via `tools/story-loop/bg.sh`.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check` and `-- tw-grammar --check`: pass.
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`: pass.
- `cargo deny check` and `cargo +1.91 check --release --workspace`: pass.
- `git diff --exit-code Cargo.lock`: no change.
- `bash tools/story-loop/gates.sh`: ALL-OK.

## Auto Run Result

- **Summary:** `noctis-image`'s Renderer now owns a render cache (FR-36) keyed by blake3 over template id/version, postcard(data), format, animation, still_fallback, takumi version, Theme digest and App-wide font/asset pairs, behind the `RenderCache` port (`MemoryRenderCache` default, 1-day cap), plus per-Renderer single-flight deduplication (`Flights`, shared future polled once eagerly so admission still happens at the call). Cached outputs are named `<key16>.png` via `Attachment::rendered`; degraded (fallback) renders are not stored.
- **Files changed:** `noctis-image/src/cache.rs` (+tests: key derivation, NRC1 entry encoding); `flight.rs` (+tests: single-flight join/race/cancel); `renderer.rs` and `renderer/tests/cache.rs` (cache + flight wiring, FR-36 tests); `fonts.rs`, `assets.rs`, `remote/fetch.rs` (digest pairs for the key); `noctis-ports/src/render.rs` (`RenderOutput::digest` = key digest, docs); `noctis-core/src/app.rs` (+tests), `noctis/src/lib.rs`, `noctis/tests/render*.rs` (wiring, re-exports); project-template skill (`SKILL.md`, `images.md` section 9, `adapters.md`), `AGENTS.md`, `xtask/public-api.txt` (re-blessed).
- **Review findings:** 22 findings (high 0, medium 5, low 13, false 4). Patched: 5 medium (grouped into 3 patches: degraded outputs named by content blake3 instead of key; no-TTL writes documented for swapped adapters; mutable remote URLs must change URL on content change) and 1 low (test `fr_36_format_animation_and_version_changes_miss_the_cache`). Deferred: none. Rejected: 12 low (detached put, NRC1 integrity, eager poll outside runtime x2, share_error wildcard, draw placeholder digest, raced-branch threaded test, Cargo.lock pin test in package, ClassCheckMode not keyed, no changelog, panicking adapter poisons flight, per-Renderer dedup) and 4 false (Renderer-level dedup tests cover ctx.render; EncodeSettings derived from keyed inputs; `rendered` is AD-22's call; digest meaning change required by FR-36), each with its reason in the triage log.
- **Follow-up review recommended: true** (first pass, 5 medium patched; high 0). Named risk: the degraded-naming patch changes attachment naming for fallback renders and its interaction with the edit-diff `KnownFiles` path in `noctis-core/src/responder/files.rs` is verified by reasoning, not by an end-to-end edit test.
- **Verification:** fmt, clippy -D warnings, workspace and per-crate tests, xtask api-snapshot/deps-check/rules-catalog/skill-check/tw-grammar, tw_crosscheck, cargo deny, MSRV 1.91 check; `tools/story-loop/gates.sh` all green. CI paused by the maintainer.
- **Residual risks:** swapped RenderCache adapters without own TTL grow unbounded; mutable URL images served stale until the URL changes; cache put lost if all callers drop mid-put; a panicking user adapter poisons a shared flight.
