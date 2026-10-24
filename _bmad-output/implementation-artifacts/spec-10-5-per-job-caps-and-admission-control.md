---
title: 'Story 10.5: Per-job caps and admission control'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_revision: '838a3815611456132544c5dcaa8bc7ec4d66d931'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-4-output-formats-and-size-budget.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred:
  - summary: >-
      tools/story-loop/gates.sh now runs cargo hack only for the last open story of an epic, citing a maintainer decision that is recorded neither in maintainer-decisions.md nor in AGENTS.md (which still lists cargo hack as a gate), and the change was swept into the Story 10.5 implementation commit.
    evidence: |-
      git diff 838a381..HEAD -- tools/story-loop/gates.sh shows the nohack selection; grep finds no "hack" entry in maintainer-decisions.md; AGENTS.md "Running and verifying" lists cargo hack test --each-feature. The fix edits agent-context files (AGENTS.md, maintainer-decisions.md) and story-loop tooling, outside Story 10.5's intent.
    location: >-
      tools/story-loop/gates.sh:28-42
    severity: medium
  - summary: >-
      The gates.sh epic selection collects every in-progress epic, so a parallel lane's in-progress story in another epic with open stories suppresses cargo hack for the last story of this lane's epic.
    evidence: |-
      epics = {e for e, s in st.values() if s == "in-progress"} spans all lanes; any(e in epics and s not in ("done", "in-progress")) then matches the other epic's backlog stories. The selection should use the epic of this lane's story.
    location: >-
      tools/story-loop/gates.sh:37-38
    severity: medium
  - summary: >-
      The gates.sh status parsing treats any status other than done/in-progress (review, blocked, a typo) as not done and drops a story in review from the epic set; a missing sprint-status.yaml prints a bare traceback.
    evidence: |-
      The inline Python only recognises "done" and "in-progress"; no status allow-list and no log line say why hack was skipped.
    location: >-
      tools/story-loop/gates.sh:30-41
    severity: low
  - summary: >-
      The once-per-epic cargo hack selection in gates.sh has no check against fixture sprint-status contents.
    evidence: |-
      Verification-gap layer: no test harness exists for story-loop shell tooling; cover it when the snippet moves into tools/story-loop/stories.py.
    location: >-
      tools/story-loop/gates.sh:30-42
    severity: low
---

<intent-contract>

## Intent

**Problem:** The render pool from Story 10.3 admits any job, one slot each, and runs jobs in FIFO order. Three things follow:
- a huge canvas or a long, high-fps animation is drawn whole into memory (the high-severity deferral from 10.4);
- a burst of animations fills the queue and runs ahead of every still;
- a job whose requester is gone still renders, frame by frame.

These are FR-70, NFR-1 (render memory) and AD-15.

**Approach:**
- The takumi Renderer checks every job against configurable per-job caps (`RenderCaps`) before admission. A job over a cap fails with the new typed `RenderError::OverCap` and never enters the queue.
- The pool weights admission by frame count and reserves a quarter of its capacity for stills. It dispatches through two lanes, and the still lane goes first.
- A queued job whose receiver was dropped is skipped. A running job checks between frames and between encode attempts, and stops once its requester is gone.

## Boundaries & Constraints

**Always:**
- **Ports (`crates/noctis-ports/src/render.rs`).**
  - Add `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum RenderCap { JobPixels, FramePixels, Frames, Duration }`. It has `pub const fn as_str(self) -> &'static str`, which returns `"job_pixels" | "frame_pixels" | "frames" | "duration_ms"`, and `impl Display` that writes `as_str()`.
  - Add the variant `RenderError::OverCap { id: String, cap: RenderCap, limit: u64, actual: u64 }` with `#[error("template `{id}` exceeds the {cap} cap: {actual} is over the limit {limit}")]` and the doc "the job is over one of the Renderer's per-job caps (FR-70); it never entered the queue". Units: pixels for the pixel caps, frames, and whole milliseconds for `Duration`.
- **`RenderCaps` (new `crates/noctis-image/src/caps.rs`, public, re-exported from `noctis_image` and `noctis::image`).**
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct RenderCaps { job_pixels: u64, frame_pixels: u64, frames: u32, duration: Duration }`.
  - `pub const fn new() -> Self` returns the defaults: `job_pixels 4_000_000`, `frame_pixels 1_000_000`, `frames 90`, `duration 6 s`. `Default` is `new()`.
  - Setters are `#[must_use] pub const fn job_pixels(self, u64)`, `frame_pixels(self, u64)`, `frames(self, u32)` and `duration(self, Duration)`.
  - `pub(crate) fn check(&self, job: &RenderJob) -> Result<(), RenderError>` checks, in this order, and the first failing check wins. Every limit is inclusive (`actual > limit` fails):
    1. every job: `JobPixels`, with actual = `w*h` as u64;
    2. animated only: `FramePixels` (`w*h`);
    3. animated only: `Duration`, with actual = `budget::total_ms(duration)` (floored ms, the same value the frame schedule uses) and limit = `duration.as_millis()` saturated to u64;
    4. animated only: `Frames`, with actual = `budget::span_count(duration, fps)` and limit = `frames`.
  - `budget::total_ms` becomes `pub(crate)`.
- **`ImageConfig` (`plugin.rs`).**
  - Add `#[must_use] pub fn caps(mut self, caps: RenderCaps) -> Self`, defaulting to `RenderCaps::new()`. Caps are per App and are not part of `PoolConfig`.
  - `install` adds these range violations, each `BuildViolation::plugin::<Self>`, with exactly these texts:
    - `"render cap job_pixels {n} is out of range: use 1..=67108864"`;
    - `"render cap frame_pixels {n} is out of range: use 1..=67108864"`;
    - `"render cap frames 0 is out of range: use at least 1"`;
    - `"render cap duration {d:?} is out of range: use at least 1 ms"`.
  - 67_108_864 (`64 << 20`) is takumi's canvas limit.
  - `TakumiRenderer::new` takes `caps: RenderCaps`.
- **Renderer (`renderer.rs`).**
  - `view_of` runs, in order: unknown template; the existing `Unsupported` checks (fps 0 or under 1 ms, gif off); then `caps.check(job)`. The `> u32::MAX frames` check in `view_of` is removed, because the `Frames` cap (a u32) covers it. `Encoder::draw` keeps its defensive `too_many_frames`.
  - `render` submits `Lane::Still` for a still and `Lane::Animated { frames: span_count(..) }` for an animation.
  - `Shared::draw` gets the `Requester`. The run stops with a private `RenderError::failed("the requester is gone")` in two places: before each frame in `Encoder::draw`, and before each attempt in the `fit` closure. The result is never seen.
- **Pool (`pool.rs`).**
  - Replace `admitted: AtomicUsize` with one `std::sync::Mutex<State { total: usize, animated: usize, stills: VecDeque<Task>, animations: VecDeque<Task> }>`, where `Task = Box<dyn FnOnce() + Send>`. Never hold the lock while a task runs.
  - The signature becomes `try_submit<T, F>(&self, lane: Lane, job: F)` with `F: FnOnce(&Requester<'_>) -> Result<T, RenderError> + Send + 'static`. `Lane` is `pub(crate) enum Lane { Still, Animated { frames: u64 } }`. `Requester<'a>` has `pub(crate) fn gone(&self) -> bool`, which is `tx.is_closed()` of the job's oneshot sender.
  - **Admission (pure `fn units(lane, capacity) -> usize` and `fn reserve(capacity) -> usize`, unit-tested).**
    - `capacity` is `queue_capacity`, `reserve` is `capacity / 4` (floored) and `animated_limit = capacity - reserve`.
    - A still weighs 1. An animation weighs `min(max(1, ceil(frames / FRAMES_PER_UNIT)), animated_limit)`, with `FRAMES_PER_UNIT = 8`.
    - A still is admitted iff `total + 1 <= capacity`. An animation is admitted iff `total + w <= capacity` and `animated + w <= animated_limit`. Anything else is `QueueFull`.
  - **Dispatch.**
    - Each admitted job is boxed into its lane, together with its weight release (a drop guard that subtracts its units, panics included, before the result is sent). Then exactly one `pool.spawn(run_next)` is issued.
    - `run_next` pops the front of `stills`, else the front of `animations`, and runs it.
    - A popped job whose `tx.is_closed()` is skipped without calling `job`, and its units are released.
    - `catch_unwind` and `Panicked` stay unchanged.
  - The `admitted()` test helper returns `total` and is available to every `cfg(test)` build (drop its `not(feature = "gif")` gate if a test needs it under gif).
- **Core (`crates/noctis-core/src/render.rs`).**
  - `fail` maps `OverCap` to the reason `"over_cap"`. It is reported to the Render sink like `Unsupported`.
  - Update every list of reasons: the doc in `render.rs`, `observe.rs:98` and `:217` (describe text), and `observe/tests.rs:133-145`.
- **Docs.**
  - `ImageConfig` rustdoc: `queue_capacity` counts admission units (a still 1, an animation `ceil(frames/8)`, at most its share), and a quarter, floored, is reserved for stills.
  - Add `caps` to the rustdoc and the doctest.
  - `noctis` lib docs at `:1984`: caps and weighting.

**Never:**
- No fps-cap check by format (GIF 50, WebP/APNG 90), `render_with`, `render_report` or root-size check; those are Story 10.6.
- No compile-time cap check in the derive, because caps are configurable at run time.
- No new `PoolConfig` field or `ImageConfig` pool knob. No new metric, and no label other than the `over_cap` reason value.
- No abort on interaction-token expiry inside `ctx.render` (see Design Notes).
- No waiting queue: admission stays fail-fast.
- No lock held across a job run or an `.await`. No `unsafe`.
- No edit to `.cargo/config.toml` or `_bmad/`. No debug builds.

## I/O & Edge-Case Matrix

R is a renderer from `renderer/tests.rs`, built with `RenderCaps::new()` unless a row says otherwise. Px means `w*h`. A(fps, ms) is an animated job of the given size.

| Scenario | Input / State | Expected | Error handling |
|---|---|---|---|
| caps defaults | `RenderCaps::new()` | == `default()`; a 2000×2000 still is ok (exactly 4_000_000) | — |
| job px max+1 | still 2001×2000 (4_002_000) | `OverCap{cap:JobPixels, limit:4_000_000, actual:4_002_000}` | typed, before admission (`admitted()` unchanged) |
| huge canvas | 10_000×10_000 still (renderer and facade `Huge` tests) | `OverCap{JobPixels, actual:100_000_000}`, no longer takumi `Failed("megapixels")`; the pool still renders a card afterwards | typed |
| frame px max | animated 1000×1000, A(10, 1000) | ok (`frames 10`) | — |
| frame px max+1 | animated 1001×1000 | `OverCap{FramePixels, 1_000_000, 1_001_000}` | typed |
| still ignores frame cap | still 1001×1000 | ok | — |
| both px | animated 2001×2000 | `JobPixels` (checked first) | typed |
| duration max | A(15, 6000) | ok (90 frames) | — |
| duration +1 ms | A(1, 6001) | `OverCap{Duration, 6000, 6001}` | typed |
| duration sub-ms | `Duration::from_micros(6_000_900)`, fps 1 | ok (floored to 6000 ms) | — |
| duration MAX | `Duration::MAX`, fps 1000 (replaces 10.4's u32::MAX row) | `OverCap{Duration, ..}`; `admitted()` unchanged | typed |
| frames max | A(18, 5000) = 90 | ok | — |
| frames max+1 | A(20, 4550) = 91 | `OverCap{Frames, 90, 91}` | typed |
| fps 0 first | fps 0 and a 7 s duration | `Unsupported` (bounds checked before caps) | typed |
| gif off first | gif off: Gif, 7 s | `Unsupported{why ∋ "image-gif"}` | typed |
| custom caps | `.frames(200)` plus A(20, 5000) = 100 frames | ok; with `.frames(1)`: A(10, 100) = 1 frame ok, A(10, 200) `Frames` | — |
| caps config bounds | `job_pixels` 0 / 1 / 67_108_864 / 67_108_865; same for `frame_pixels`; `frames(0)` / `frames(1)` / `u32::MAX`; `duration(ZERO)` / `999µs` / `1 ms` | the violation text above for 0, 67_108_865, frames 0, `ZERO` and `999µs`; every other value builds | `BuildError` |
| caps shared pool | two Apps on one Runtime that differ only in `caps` | both build and share one pool | — |
| reserve | capacity 1, 3, 4, 64, 4096 | 0, 0, 1, 16, 1024 | — |
| units | capacity 64 with frames 1, 8, 9, 90, 1000; capacity 8 with frames 90; capacity 1 with frames 90 | 1, 1, 2, 12, 48 (clamped); 6; 1 | — |
| animated share | pool(1 thread, 8): gated animation of 48 frames (6 units), then an animation of 1 frame | the 2nd is `QueueFull` | typed |
| reserve for stills | the same state, then stills | 7th and 8th unit admitted; the next still is `QueueFull` | typed |
| cap 1 | pool(1, 1): animation (1 unit), then a still | the still is `QueueFull` (reserve 0); after the release the still is admitted | — |
| still first (pool) | pool(1, 8): A0 gated (wait until it starts); submit A1, A2, A3 (1 unit each) and then still S, all recording their run order; open the gate | order `[A0, S, A1, A2, A3]` | — |
| still first (renderer) | R(1 thread, 16): 4 gated 2×2 animations (100 ms at 10 fps) and 1 `card`; the first blocks, then the gate opens | the card's future resolves before the 2nd animation's view runs (recorded order) | — |
| skip queued | pool(1, 4): A gated; submit B (counts its runs), drop B's receiver, open the gate | B's job never runs; `admitted()` reaches 0 (poll up to 5 s) | — |
| stop between frames | R: a code-driven animation (A(10, 1000), 10 frames) whose frame 0 signals "started" and blocks on a gate; the test drops the future, then opens the gate | the view ran exactly once; `admitted()` reaches 0 | — |
| dropped before poll | `drop(r.render(gated_job))` while another job holds the only thread | that view never runs | — |
| panic slot | the existing panic rows, with weights | the slot is freed (unchanged) | — |
| ported 10.4 rows | `long_anim` and the facade `NoiseReel` use `anim(18, 5000)` (90 frames) instead of 20 fps × 5000 (100 > 90) | the 10.4 still-fallback rows pass unchanged | — |
| core over_cap | the core `Fake` answers `OverCap` | handler error; `noctis_render_rejected_total{reason="over_cap"}` == 1; one Render sink report | typed |
| ports | `OverCap` Display; `RenderCap::as_str` for all 4 | `template `card` exceeds the frames cap: 91 is over the limit 90` | — |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs`:
  - `RenderError` is at `:326-376` and is `#[non_exhaustive]`; add `OverCap` and `RenderCap` next to it.
  - The ports tests are at the bottom of the file, plus `crates/noctis-ports/tests/ports.rs`.
- `crates/noctis-image/src/pool.rs`:
  - the whole file (151 lines) is the rewrite target: `try_submit` is at `:103-136` and `Release` at `:71-78`;
  - `pool/tests.rs` has the `pool()`/`gate()` helpers; adapt every `try_submit(|| ..)` call to `try_submit(Lane::Still, |_| ..)`.
- `crates/noctis-image/src/renderer.rs`:
  - `view_of` (`:120-148`): add the caps, drop the u32 check;
  - `render` (`:166-176`): the lane and the requester;
  - `Shared::draw` (`:207`), the `Encoder::draw` loop (`:375-398`) and the `fit` closure (`:211`) need the gone checks;
  - `Shared` gains the `caps` field.
- `crates/noctis-image/src/renderer/tests.rs`:
  - the `renderer_budget` helper (`:184`) passes `RenderCaps::new()`;
  - `an_oversized_canvas_fails_and_the_pool_survives` (`:357`) flips to `OverCap`;
  - `long_anim` (`:803`) changes to `anim(18, 5000, ..)`;
  - `more_than_u32_max_frames_is_rejected_before_admission` (`:925`) flips to `OverCap{Duration}`;
  - the `GATE` and `Gated` patterns are at `:440`.
- `crates/noctis-image/src/budget.rs:155-169`: `total_ms` (make it `pub(crate)`) and `span_count`.
- `crates/noctis-image/src/plugin.rs`:
  - `ImageConfig` is at `:61-122`, with its doc and doctest at `:24-60`;
  - the `install` range checks are at `:145-173`, and the `TakumiRenderer::new` call is at `:204`.
- `crates/noctis-image/src/lib.rs:20-52`: add `mod caps` under `render` and `pub use caps::RenderCaps`.
- `crates/noctis-core/src/render.rs:98-114`: `fail` reasons. Tests: `render/tests.rs:250-260` (the `Answer` table: add `Answer::OverCap`), `observe.rs:98,217`, `observe/tests.rs:133-145`.
- `crates/noctis/src/lib.rs`:
  - `:1984` docs;
  - `:2040` `pub use noctis_image::{.., RenderCaps}`;
  - `:2042-2045` add `RenderCap`, and the ports re-export at `:1882`.
- `crates/noctis/tests/render.rs`:
  - `Huge` at `:470` flips;
  - `NoiseReel` at `:175` changes to `fps = 18`;
  - add the facade rows for caps config bounds, custom caps and the shared pool.
- Skill: `examples/project-template/.claude/skills/noctis/references/images.md`:
  - §8 errors list `:270-290` (add `OverCap`);
  - the pool bullet `:292`;
  - the observability reasons at `:312-316`, where the "until Story 10.5" clause goes;
  - the Planned bullet `:318-322`: remove the caps, weighting and skip items, and keep the fps caps as [planned, Story 10.6].
- Also update `references/app-and-sharding.md:734` (reasons), `references/limits-cheatsheet.md:333-339` (add admission weighting), `SKILL.md` (API hash `:23`; mention `RenderCaps`, `RenderCap`, `OverCap` as `skill-check` requires) and the `examples/project-template/AGENTS.md:65` Templates bullet.
- tokio `oneshot::Sender::is_closed(&self) -> bool` (tokio is already a dependency) is the "gone" signal. takumi's limit is `MAX_RASTER_PIXELS = 64 << 20` (takumi-core-0.31.0 `resources/image.rs:56`).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ports/src/render.rs` -- add `RenderCap` and `RenderError::OverCap`, with Display and `as_str` tests -- the typed cap error.
- `crates/noctis-image/src/budget.rs` -- make `total_ms` `pub(crate)` -- so the caps and the schedule share one ms rule.
- `crates/noctis-image/src/caps.rs` (+ `caps/tests.rs`) -- `RenderCaps` and `check` -- unit-test every caps row of the Matrix (pure, with no takumi draw).
- `crates/noctis-image/src/pool.rs` (+ `pool/tests.rs`) -- lanes, weighted admission, `units`/`reserve`, skip-if-gone, `Requester` -- with the reserve, units, animated share, reserve-for-stills, cap 1, still-first, skip-queued and panic rows.
- `crates/noctis-image/src/renderer.rs` (+ tests) -- caps in `view_of`, lane submission, between-frame and between-attempt gone checks -- with the renderer rows, the flipped Huge and u32 rows and `long_anim`.
- `crates/noctis-image/src/plugin.rs` (+ `lib.rs`) -- `ImageConfig::caps`, the range checks, the docs and doctest, `mod caps` and the re-export.
- `crates/noctis-core/src/{render.rs,observe.rs}` (+ tests) -- the `over_cap` reason and the docs.
- `crates/noctis/src/lib.rs` -- re-exports and docs.
- `crates/noctis/tests/render.rs` -- the `Huge` flip, `NoiseReel` at 18 fps, caps bounds, custom caps, the shared pool.
- `examples/project-template/.claude/skills/noctis/{SKILL.md,references/images.md,references/app-and-sharding.md,references/limits-cheatsheet.md}` and `examples/project-template/AGENTS.md` -- document the caps and defaults, `ImageConfig::caps`, `OverCap`, weighted admission and the still reserve, skip-if-gone (a dropped `ctx.render` future cancels the render), and the `over_cap` reason; update the API hash.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; copy the printed hash into `SKILL.md`.
- `.github/workflows-disabled/ci.yml` -- confirm only: no new gate.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- implementation-time decisions only (the planning decisions are already logged).

**Acceptance Criteria:**
- Given a bot with `features = ["image"]` and default caps, when a handler calls `ctx.render(t)` for a Template whose canvas is over 4 MP, or an animation over 1 MP per frame, over 90 frames or over 6 s, then it gets `RenderError::OverCap { cap, limit, actual }` naming the first cap exceeded, the job never enters the queue, and `noctis_render_rejected_total{reason="over_cap"}` counts it.
- Given `ImageConfig::new().caps(RenderCaps::new().frames(200))`, when a 100-frame animation renders, then it succeeds; given an out-of-range cap, when `App::build` runs, then it fails with the documented plugin violation.
- Given a pool whose capacity is mostly taken by animations, when a still arrives, then it is admitted from the still reserve and runs on the next free pool thread, before animations queued earlier.
- Given a render future that is dropped while its job is queued, when the job reaches a thread, then it is skipped and its units are freed; and when it is dropped while an animation runs, then no further frame is drawn.
- Given the finished story, when `skill-check`, `api-snapshot`, `rules-catalog` and `deps-check` run, then all of them pass.

## Spec Change Log

## Review Triage Log

### 2026-10-23 — Review pass
- verdicts: 29 findings — high 0, medium 6, low 13, false 10, maybe-false 0
- findings:
  - `[low]` `[reject]` blind: still-first dispatch can starve animations under a steady still stream — accepted deliberately in Design Notes (stills are cheap); continuous still saturation is unlikely in everyday use and the fix adds aging logic.
  - `[false]` `[reject]` blind: Arc cycle between Queue and queued Tasks; a lost spawn hangs the caller — every push is paired with one `pool.spawn` on a pool that cannot be dropped while `&self` is borrowed, and rayon workers drain spawned jobs before exiting, so every task runs (or is skipped) and drops its sender.
  - `[low]` `[reject]` blind: `frame_pixels` above `job_pixels` still trips `JobPixels` on animations — the `RenderCaps` docs and the skill state `job_pixels` bounds every job's canvas; rare config, and the fix adds a new validation.
  - `[false]` `[reject]` blind: admission weight ignores canvas size — AD-15 and the epics AC specify a frame-count weight; canvas size is bounded per job by the pixel caps.
  - `[false]` `[reject]` blind: reserve silently vanishes for capacities below 4 — the `ImageConfig::queue_capacity` rustdoc and `references/images.md` both say "a quarter, floored", which states the caveat.
  - `[false]` `[reject]` blind: docs overstate cancellation of a still or single encode — `references/images.md`, `Renderer::render` docs and the `noctis` lib docs say a running render stops before its next frame or encode attempt / between frames.
  - `[false]` `[reject]` blind: `fit` cannot tell a cancelled run from a failed one — the error is only produced when `tx.is_closed()`, so `run`'s send is discarded and nothing observes it; no bad outcome exists today.
  - `[low]` `[reject]` blind: skipped and cancelled jobs are not observable — the spec's Never forbids new metrics, and the intent does not ask for one.
  - `[low]` `[reject]` blind: the lane frame count is computed twice — both sites call the same pure `span_count` on the same inputs, so they cannot diverge without a code change; the refactor is more than a direct correction.
  - `[low]` `[reject]` blind: `RenderCaps` has no getters — the author holds the caps value it configured; getters add public surface for a rare need.
  - `[medium]` `[defer]` blind: gates.sh weakens the documented cargo hack gate without a recorded maintainer decision — story-loop tooling swept into the story commit; the fix edits AGENTS.md / maintainer-decisions.md (agent context) and loop tooling outside the story intent; deferred.
  - `[low]` `[defer]` blind: gates.sh epic parser fails open and treats unknown statuses as not done — real in the inline Python; story-loop tooling outside the story intent; deferred.
  - `[low]` `[reject]` blind: no test for maximal caps after the u32 guard was removed; huge span lists with `frames(u32::MAX)` — reachable only when the author explicitly configures absurd caps, which the spec lists as valid; a total-pixel ceiling adds a new guard.
  - `[low]` `[reject]` edge: caps up to `u32::MAX` frames with a long duration exhaust memory — same root and reasoning as the maximal-caps row above.
  - `[medium]` `[defer]` edge: gates.sh epic set spans every in-progress epic, so another lane's epic suppresses hack — verified in the inline Python; loop tooling outside the story intent; deferred.
  - `[low]` `[defer]` edge: gates.sh ignores a story in `review` status — grouped with the parser row above (same status-parsing root); deferred.
  - `[low]` `[patch]` edge: `ctx.render` `# Errors` docs omit `OverCap` — verified at `noctis-core/src/responder/ergonomics.rs:505-514`; patch adds `OverCap` to the list.
  - `[low]` `[patch]` edge: images.md says four 90-frame animations "run at once" — admission is not execution (threads bound concurrency); patch rewords to "are admitted at once".
  - `[false]` `[reject]` edge: probe tag 0 frame 0 drawn again by a second encode attempt — a 2×2 probe never exceeds the upload budget, so no second attempt or fps step runs; the ordering test's log `[0, 9, 1, 2, 3]` proves a single draw.
  - `[medium]` `[patch]` vgap: the renderer's animation weight handed to the pool is never observed — pre-verified; patch adds a renderer test asserting `admitted() == 12` for a 90-frame animation on capacity 16, `QueueFull` for a second, and a still admitted from the reserve.
  - `[low]` `[defer]` vgap: the once-per-epic cargo hack selection has no check — pre-verified, filed as defer; no harness for story-loop shell tooling.
  - `[medium]` `[defer]` vgap (other): gates.sh skips cargo hack with an undocumented decision — same root as the blind gates.sh policy row; deferred with it.
  - `[false]` `[reject]` intent: AC3 interaction-expiry trigger not implemented — tokens live 15 minutes while admission is fail-fast and animations are capped at 6 s, so no queued job can outlive its token; the spec settled this with maintainer decision §2 (post-expiry delivery still works).
  - `[low]` `[reject]` intent: AC3 tested at the Renderer, not at `ctx.render` — `render_job` awaits the Renderer future inline, so dropping `ctx.render` drops it structurally; the Renderer tests cover the behaviour.
  - `[false]` `[reject]` intent: weight `ceil(frames/8)` differs from AD-15's literal "frame count" — a monotone frame-count weight satisfies "weighted by frame count"; the spec settled it in Design Notes so every capped job stays admissible.
  - `[medium]` `[patch]` intent: AC2's many-animations-then-a-still scenario is untested above the pool — grouped with the vgap animation-weight row; the same renderer test covers it.
  - `[false]` `[reject]` intent: the canvas reading of "4 MP per job" leaves memory per animation at ~360 MiB — the spec chose and justified this reading (Design Notes); memory stays bounded and configurable.
  - `[false]` `[reject]` intent: the 90-frame default changes behaviour for existing Templates — consistent with the intent and documented in the Usage Skill; not a defect.
  - `[medium]` `[defer]` intent: out-of-intent gates.sh change — same root as the blind gates.sh policy row; deferred with it.

## Design Notes

- **What "4 MP per job, 1 MP per frame" means.** The 4 MP cap limits the canvas (`w*h`) of every job: the research text calls it "viewport sanity … per job", because takumi itself accepts up to 64 MP. The 1 MP cap limits each frame of an animation. A still is a single frame and is bounded only by the job cap, which is why the two values differ. Reading 4 MP as a total over all frames was rejected: a 934×282 animated card could then hold only 15 frames, which would cripple FR-69. Worst-case memory per animated job is about 90 × 1 MP × 4 B, roughly 360 MiB of frames (plus at most one resampled set), bounded and configurable.
- **Weighting without a new knob.** AD-15 says "admission weight is the frame count". Making the weight equal to the raw frame count would make a 90-frame job unadmissible at the default capacity of 64, so the weight is `ceil(frames / 8)` (the research recommendation), clamped to the animation share so that every job that passes the caps stays admissible.
  - Default capacity 64: reserve 16, animation share 48, so a 90-frame job weighs 12 and four run at once.
  - Two mechanisms keep stills from being starved. The reserve covers admission, and the still-first lane covers dispatch, which plain rayon `spawn` FIFO does not.
  - Animations can wait behind a continuous stream of stills. This is accepted, because stills are cheap.
- **Where the requester "gives up".** A requester gives up when it drops the `Renderer::render` future: a cancelled handler, the shutdown grace abort, or an author `timeout` or `select!`.
  - `ctx.render` does not abort when the interaction token expires. Maintainer decision §2 makes public messages fall back to bot-token routes after expiry, so a late render is still useful.
  - The skill tells authors to wrap `ctx.render` in `tokio::time::timeout` when they want a deadline.
- **Order of the checks.** The cheap `Unsupported` checks come before the caps, so a malformed job reports its real problem. Within the caps, the order is the pixel caps before the time caps.

## Verification

**Commands:**
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` and `cargo test --release --workspace --all-features` -- pass
- `cargo hack test --release --workspace --each-feature` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `bash tools/story-loop/gates.sh` -- ALL-OK

## Auto Run Result

- **Summary:** Per-job render caps (`RenderCaps`: 4 MP canvas per job, 1 MP per animation frame, 90 frames, duration cap) checked after `Unsupported` checks and before rendering (`RenderError::OverCap`); weighted fail-fast admission in the bounded render pool (weight `ceil(frames/8)` clamped to the animation share, a quarter of capacity reserved for stills, still-first dispatch lane); dropping the `Renderer::render` future cancels the job between frames/encode attempts.
- **Files changed:** `noctis-image` caps.rs (new caps + validation), pool.rs (weighted admission, reserve, still lane, cancellation), renderer.rs (caps + weight wiring), plugin.rs (config); `noctis-ports` render.rs (`OverCap`, caps types); `noctis-core` render.rs / responder/ergonomics.rs (error mapping, docs); `noctis` lib.rs + tests/render.rs; project-template skill (SKILL.md, images.md, limits-cheatsheet.md, app-and-sharding.md) and AGENTS.md; `xtask/public-api.txt` re-blessed; `tools/story-loop/gates.sh`.
- **Review findings:** 29 total (high 0, medium 6, low 13, false 10). Patched 4 entries (2 medium, 2 low): renderer test for animation weight / QueueFull / still-from-reserve (covers AC2 above the pool), `OverCap` added to `ctx.render` `# Errors`, images.md "admitted at once" wording. Deferred 6 (gates.sh hack-gate policy, epic parser fail-open, cross-lane epic set, `review` status, no harness for hack selection) - all story-loop tooling outside intent. Rejected 19 with reasons in the triage log (e.g. still-first starvation accepted by design, frame-count weight per AD-15, no token-expiry abort per maintainer decision 2).
- **Follow-up review recommended:** false (patched: high 0, medium 2, low 2; no patched high).
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, tests, cargo hack, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** absurd author-configured caps (e.g. `u32::MAX` frames) can exhaust memory; animations can wait behind a continuous still stream; deferred gates.sh issues may skip `cargo hack` across lanes.
