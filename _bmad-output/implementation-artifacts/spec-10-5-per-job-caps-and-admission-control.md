---
title: 'Story 10.5: Per-job caps and admission control'
type: 'feature'
created: '2026-10-23'
status: 'ready-for-dev'
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
deferred: []
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
