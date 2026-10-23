---
title: 'Story 10.4: Output formats and size budget'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_revision: '30171b9b5f23873b216f9125dc1b8e80ba519cc3'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-3-renderer-and-bounded-render-pool.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
warnings: ['oversized']
deferred:
  - summary: >-
      A derived or hand-built animation with a huge duration_ms x fps (up to u32::MAX ms at 90 fps) draws every full-size frame into memory before any budget check, which can exhaust memory on a pool thread.
    evidence: |-
      view_of only rejects more than u32::MAX frames; Encoder::draw collects all spans as full-size bitmaps (renderer.rs draw). 60 s at 90 fps on 800x400 is about 6.9 GB of RGBA. The intent-contract's Never list assigns per-job caps to Story 10.5, and the spec's Design Notes record it as a known residual.
    location: >-
      crates/noctis-image/src/renderer.rs (view_of, Encoder::draw)
    severity: high
  - summary: >-
      A GIF animation with fps above 50 gets sub-20 ms frame delays, which viewers clamp, so it plays slower than declared.
    evidence: |-
      GIF delays are centiseconds; the derive accepts fps up to 90 for every format and view_of has no GIF fps check, so fps 60 yields 1-2 cs delays that browsers clamp to about 10 cs. The intent-contract's Never list assigns the fps-cap check to Story 10.6.
    location: >-
      crates/noctis-image/src/renderer.rs (view_of); crates/noctis-macros/src/template.rs:349
    severity: medium
---

<intent-contract>

## Intent

**Problem:** Story 10.3's Renderer encodes only the four still formats at the Template's declared settings. Animated Templates are rejected ("not rendered yet"), and nothing keeps an output under the Upload Budget, so a large render fails at upload (ATT-01). GIF/APNG are not gated behind `image-gif`. `settings_used` never reflects any change (FR-33; AD-15, AD-22; ATT-01, ATT-07).

**Approach:**
- The takumi Renderer renders animated jobs: it calls the view once per frame and encodes animated WebP (the default), GIF or APNG (`image-gif` only).
- Every output is fitted to a size budget, `min(job.upload_budget, ImageConfig::size_target)`, by walking one documented, cumulative degradation ladder: fps, then size, then quality, then optionally a still frame. If nothing fits, the render returns `RenderError::OverBudget`.
- Core fills `RenderJob::upload_budget` from the interaction's Upload Budget.
- `RenderOutput.settings_used` reports the step that fitted.

## Boundaries & Constraints

**Always:**
- **Ports (`crates/noctis-ports/src/render.rs`).**
  - `RenderJob` gains `pub upload_budget: u64`, documented as "the largest output in bytes the upload accepts (ATT-01)". `RenderJob::of` sets it to the new `pub const DEFAULT_UPLOAD_BUDGET: u64 = 20_971_520` (an associated const on `RenderJob`).
  - Add `impl AnimatedFormat { pub const DEFAULT: Self = Self::Webp { quality: 80 }; }`.
  - Add the variant `RenderError::OverBudget { budget: u64, smallest: u64 }` with `#[error("the rendered image does not fit the size budget: the smallest encoding is {smallest} bytes, the budget {budget} bytes")]`.
  - `EncodeSettings` stays as it is.
  - Update every `RenderJob { .. }` literal in the workspace (tests).
- **Core.**
  - `responder::Shared` gets `pub(crate) fn upload_budget(&self) -> usize`, which is `self.file_limit().unwrap_or(self.limits.attachment_bytes)`. That is the same rule `Files::finish` applies.
  - In `crates/noctis-core/src/render.rs`, after `RenderJob::of`, set `job.upload_budget` to `inner.shared.as_ref().map_or(app.limits.attachment_bytes, |s| s.upload_budget())`, converted with `u64::try_from(..).unwrap_or(u64::MAX)`.
  - `fail` handles `OverBudget` like `Failed`: it is reported to the Render sink and adds no rejected-metric reason.
  - Fix the `BuildViolation::TemplateRejected` doc (`error.rs:372`): animated Templates are no longer rejected.
- **noctis-image features and deps.**
  - Add the feature `gif = ["render"]`.
  - Add the workspace dep `image = { version = "0.25.10", default-features = false }`. It is already in `Cargo.lock` through takumi-raster, MSRV 1.88, and adds no new crate. Use it in `noctis-image` as `image = { workspace = true, optional = true }`, added to `render`.
  - In the facade: `image-gif = ["image", "noctis-image/gif"]`.
- **Animated rendering (`noctis-image/src/renderer.rs`).**
  - `register` no longer rejects `is_animated()` views. The foreign-view rejection stays.
  - `view_of` (synchronous, before admission) returns `Unsupported` in two cases:
    - the animation has `fps == 0` or a duration under 1 ms: `why` is "an animation needs fps >= 1 and a duration of at least 1 ms";
    - `cfg(not(feature = "gif"))` with `AnimatedFormat::Gif | Apng`: `why` is "GIF and APNG output need the `image-gif` feature".
  - Frame timing reproduces takumi's `frame_spans`:
    - `total = duration.as_millis()`, saturating to `u64`;
    - `n = ceil(total * fps / 1000)`;
    - frame `i` starts at `i*1000/fps` and lasts `min((i+1)*1000/fps, total) - start`, and zero-length frames are dropped.
    - The view gets `Frame { index: i, count: spans.len() }`.
    - takumi renders each frame with `RenderOptions::time_ms(start)`, so CSS keyframes advance; Story 10.6 tests those.
    - When `spans.len()` exceeds `u32::MAX`, the render fails with `Unsupported`.
  - Encoders:
    - animated WebP: `takumi::write_animated_webp` with `AnimatedWebpOptions::builder().lossless(false).quality(q).build()`;
    - GIF and APNG: `write_animated_gif` / `write_animated_png` with default options (infinite loop), compiled only under `cfg(feature = "gif")`.
- **Budget ladder (new `noctis-image/src/budget.rs`, `pub(crate)`, pure).**
  - `struct Attempt { fps: Option<u16>, size: Size, quality: Option<u8>, still: bool }` and `fn ladder(job: &RenderJob, still_fallback: bool) -> Vec<Attempt>`.
  - The ladder is cumulative: each step keeps the earlier steps' most degraded values. Its steps, in order:
    1. **Requested.** Lossy formats carry their quality: `Jpeg{q}`, `Webp{q}` and animated `Webp{q}` give `Some(q)`. Every other format gives `None`. A still has `fps = None`.
    2. **fps (animated only).** `F/2`, then `F/4` (integer division). A step is kept only when it is at least `MIN_FPS = 5` and lower than the current fps.
    3. **size.** `(w*3/4, h*3/4)`, then `(w/2, h/2)` of the job size. Each side is floored and at least 1. A step equal to the current size is skipped.
    4. **quality (lossy only).** `75`, `50`, `25`. A step is kept only when it is lower than the current quality.
    5. **still (animated and `still_fallback` only).** Last size, `fps = None`, `still = true`. Quality is `min(q, 25)` for a lossy `job.format`, `None` otherwise.
  - `fn fit(attempts, budget, encode: impl FnMut(&Attempt) -> Result<Vec<u8>, RenderError>) -> Result<(Vec<u8>, Attempt), RenderError>` returns the first attempt whose `len <= budget`. An encode error propagates at once. If no attempt fits, it returns `OverBudget { budget, smallest }`, where `smallest` is the minimum length seen.
  - `budget = min(job.upload_budget, size_target)`.
- **Encoding per attempt (renderer).**
  - Frames are rendered at full job size once per distinct fps. An fps step re-renders by calling the views again. A size step downscales the full-size frames of the current fps with `image::imageops::resize(.., FilterType::Triangle)` through `Bitmap::{as_raw, from_raw}`; it does not re-render.
  - A still attempt of an animation encodes frame 0 of the current frames with `job.format` and the attempt's quality.
  - `RenderOutput`:
    - `format` is the animated format, or `job.format.into()` for a still or a still fallback;
    - `settings_used` is `{ size: attempt.size, quality: attempt.quality, fps: attempt.fps, frames: 1 for a still or a still fallback, else the span count }`;
    - `digest` is `blake3(bytes)`.
- **ImageConfig (`plugin.rs`).**
  - `#[must_use] size_target(u64)` defaults to `DEFAULT_SIZE_TARGET = 2_097_152` (2 MiB). Its range is `1024..=u64::MAX`; below that, the plugin violation is `"render size target {n} is out of range: use at least 1024 bytes"`.
  - `#[must_use] still_fallback(bool)` defaults to `true`.
  - Neither is part of `PoolConfig`, so Apps that differ only in these settings share one pool.
  - `TakumiRenderer::new` takes them.
- **Macro (`noctis-macros/src/template.rs`).**
  - `animation(..)`'s `format` becomes optional and defaults to `AnimatedFormat::Webp { quality: 80 }`. Drop it from the "missing" list.
  - For `Gif`/`Apng` the derive also emits `const _: () = ::noctis::__private::image::require_gif_output::<::noctis::__private::image::GifOutputFormat>();`.
  - In `noctis-image/src/template.rs`, add these `#[doc(hidden)]` items:
    - `pub enum GifOutputFormat {}`;
    - the trait `pub trait GifOutput {}` with `#[diagnostic::on_unimplemented(message = "GIF and APNG output need the `image-gif` feature of `noctis`", label = "animated format `Gif`/`Apng`", note = "enable it: noctis = { version = \"..\", features = [\"image-gif\"] }; or use `format = Webp { quality: 80 }` (the default)")]`;
    - `impl GifOutput for GifOutputFormat {}` under `cfg(feature = "gif")`;
    - `pub const fn require_gif_output<T: GifOutput>() {}`.
- **Docs.**
  - `docs/discord-rules.md`:
    - the ATT-01 How cell adds: "The takumi Renderer also fits each rendered image under the job's Upload Budget (`RenderJob::upload_budget`, filled by `ctx.render`) and `ImageConfig::size_target` (FR-33, Story 10.4)";
    - the ATT-07 How cell adds: `animation(format = ..)` defaults to `Webp { quality: 80 }`, and `Gif`/`Apng` need `image-gif` (compile error otherwise).

**Never:**
- No per-job caps, admission weighting or skipping a gone requester (10.5). No fps-cap check, `render_with`/`RenderOptions`/`render_report`, keyframe classes or root-size-mismatch error (10.6). No fonts (10.7), assets (10.8) or cache (10.10).
- No new metric or metric label. No takumi or `image` type in a stable public item.
- No re-render for a size or quality step. No change to the order of the ladder or to the documented constants without a skill update.
- No edit to `.cargo/config.toml` or `_bmad/`. No debug builds.

## I/O & Edge-Case Matrix

R is a `TakumiRenderer` from `renderer/tests.rs` (`threads 2`, Deny). Unless a row says otherwise, size_target is the default (2 MiB) and `still_fallback` is on. "Noise" is a 64×64 Template whose root `flex flex-wrap w-full h-full` holds 64×64 cells of 1 px (`Style` width/height px) with `Style::bg_rgb` colors from a fixed-seed xorshift; it gives incompressible output (its 32×32 PNG is well over 1024 bytes). "Anim(F, ms, fmt)" is a noise Template whose colors depend on `frame.index`, with `animation(duration_ms = ms, fps = F, format = fmt)`.

| Scenario | Input / State | Expected | Error handling |
|---|---|---|---|
| ladder still png | `Png`, 64×32, budget irrelevant | `[{None,64×32,None}, {None,48×24,None}, {None,32×16,None}]` | — |
| ladder jpeg | `Jpeg{90}` | requested q90, 48×24 q90, 32×16 q90, then 32×16 at q75, q50 and q25 | — |
| ladder low q | `Webp{30}` | quality steps: only `25` | — |
| ladder q25/q1 | `Jpeg{25}`, `Jpeg{1}` | no quality step | — |
| ladder lossless | `WebpLossless` | no quality step | — |
| ladder 1×1 | `Png`, 1×1 | one attempt (the size steps equal 1×1 and are skipped) | — |
| ladder 3×1 | 3×1 | 2×1 (3/4 floors to 2), then 1×1 (1/2 floors to 1, min 1) | — |
| ladder anim fps | Anim(20, 1000, Webp{80}) | fps 20, 10, 5, then sizes at 5, then q75, 50, 25, then still | — |
| ladder fps floor | F = 9 / F = 5 / F = 1 | `[9]` (4 < 5) / `[5]` / `[1]` (no fps steps) | — |
| ladder fps 90 | F = 90 | 90, 45, 22 (two steps only) | — |
| ladder gif | Anim(10, .., Gif), still `Png` | quality always `None`; the still attempt has `None` | — |
| ladder no fallback | animated, `still_fallback = false` | no `still` attempt | — |
| ladder still lossy | animated, `job.format = Jpeg{90}` | the still attempt has quality 25 | — |
| fit first | fake encoder lengths `[10, 5]`, budget 10 | attempt 0 (`len == budget` fits) | — |
| fit second | `[11, 10]`, budget 10 | attempt 1 | — |
| fit none | `[30, 20, 25]`, budget 10 | `OverBudget{budget:10, smallest:20}` | typed error |
| fit encode err | attempt 1's encoder errors | that error; attempt 2 never encoded | propagated |
| fit budget 0 | any | `OverBudget{budget:0, ..}` | typed error |
| spans | 1000 ms at 3 fps / 1 ms at 90 fps / 2500 ms at 2 fps | durations `[333,333,334]` / `[1]` (one frame) / `[500×5]` | — |
| still fits | Card PNG, default budget | `settings_used == {64×32, None, None, 1}` (unchanged 10.3 row) | — |
| still size step | Noise PNG; budget = (full length − 1) | `len <= budget`; `settings_used.size` is 48×48 or 32×32; PNG decodes at that size | — |
| still quality | Noise `Jpeg{90}`; budget = the 32×32 q90 length − 1 (measured with the renderer's internal per-attempt encoder in `renderer/tests.rs`) | `size 32×32`, `quality ∈ {75, 50, 25}` | — |
| still over | Noise PNG, budget 10 | `Err(OverBudget{budget:10, smallest > 10})` | typed error |
| target wins | facade App with `.image(ImageConfig::new().size_target(1024))`; Noise PNG (its 32×32 step is > 1024 bytes), `upload_budget` 20 MiB | `Err(OverBudget{budget:1024, smallest > 1024})` | typed error |
| upload wins | size_target 2 MiB, Noise PNG job with `upload_budget` 1024 | `Err(OverBudget{budget:1024, ..})` | typed error |
| anim webp | Anim(10, 500, Webp{80}) | bytes `RIFF....WEBP` containing an `ANIM` chunk; `format == Webp`; ext `webp`; `settings_used {64×64, Some(80), Some(10), 5}` | — |
| anim frames | Anim that records each `Frame` | the view saw `index 0..5`, `count 5`, in a pool thread | — |
| anim gif | `gif` on: Anim(10, 500, Gif) | starts `GIF89a`; `format == Gif`; ext `gif`; quality `None` | — |
| anim apng | `gif` on: Anim(10, 500, Apng) | PNG signature and an `acTL` chunk; `format == Apng`; ext `png` | — |
| gif off | `gif` off: a job with `AnimatedFormat::Gif` / `Apng` | `Err(Unsupported{why ∋ "image-gif"})` before admission (the queue counter is unchanged) | — |
| anim zero | animation `fps 0` / `Duration::ZERO` / 500 µs | `Err(Unsupported)` | — |
| anim fps degrade | Anim(20, 1000, Webp{80}); budget = full length − 1 | `fps == Some(10)` or lower; `frames == spans(1000, fps)` | — |
| anim still | Anim(20, 1000, Webp{80}), still Png; budget = between the still 32×32 PNG length and the smallest animated attempt (both measured with the internal per-attempt encoder) | `frames 1`, `fps None`, `format == Png`, ext `png` | — |
| anim no fallback | same, `still_fallback(false)` | `Err(OverBudget)` | typed error |
| register animated | derived animated Template | build ok (the 10.3 rejection is gone) | — |
| config bounds | `size_target(1023)`, `1024`, `u64::MAX` | violation (names `1023`, `at least 1024`), ok, ok | `BuildError` |
| config defaults | `ImageConfig::new()` | `size_target 2_097_152`, `still_fallback true`, equal to `default()` | — |
| shared pool | two Apps on one Runtime differing only in `size_target` | both build, and they share one pool | — |
| macro default | `animation(duration_ms = 1000, fps = 10)` | `ANIMATION.format == AnimatedFormat::DEFAULT` | — |
| macro gif off | trybuild `ui_image` (image only): `format = Gif` | compile error "GIF and APNG output need the `image-gif` feature" | compile error |
| macro gif on | `--features image-gif`: `format = Gif` compiles and renders | GIF bytes | — |
| core default budget | core unit test, fake Renderer records the job, detached ctx (`shared: None`) | `upload_budget == app.limits.attachment_bytes` (20 MiB default; 1000 with `AppBuilder::limits(attachment_bytes 1000)`) | — |
| core interaction budget | core pipeline test (pattern `att_01_interaction_attachment_size_limit`): command interaction with `attachment_size_limit 10485760` calls `ctx.render` | the fake sees `upload_budget 10_485_760` | — |
| core over budget | fake returns `OverBudget` | handler gets it; one Render sink `Error` report; no rejected-metric sample | — |
| ports | `RenderJob::of(&card)` | `upload_budget == RenderJob::DEFAULT_UPLOAD_BUDGET == 20_971_520`; `OverBudget` Display text | — |
| default build | `cargo tree -p noctis -e normal` | no `noctis-image`, `takumi`, `rayon` or `image` | — |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs:84-121` -- `RenderJob`/`of`; `:190-207` `AnimatedFormat`; `:282-293` `EncodeSettings`; `:309-349` `RenderError`. Tests are at the bottom; `crates/noctis-ports/tests/ports.rs:429` has a `RenderJob` literal.
- `crates/noctis-image/src/renderer.rs` -- `view_of` (:96, which drops the animated rejection and adds the fps/duration/gif checks), `register` (:151), `Shared::draw` (:163, split into a frame render, `encode(attempt)` and `fit`), `encoding` (:207). The existing tests are in `renderer/tests.rs` (`:120` animated Template, `:239` and `:348` assert the old rejections and must flip, `:195` `card`, `:295/:350-387` literals).
- takumi (read only): `~/.cargo/registry/src/*/takumi-raster-0.5.8/src/write.rs`:
  - `:131` `Bitmap::from_raw`, `:120` `as_raw`;
  - `:152` `AnimationFrame::new(bitmap, ms)`;
  - `:171` `AnimatedWebpOptions` (a TypedBuilder; `lossless` defaults to true, so set it false);
  - `:492` `write_animated_gif`, `:601` `write_animated_png`;
  - `webp/libwebp.rs:344` `write_animated_webp(Cow<[AnimationFrame]>, &mut W, opts)`;
  - `render.rs:46` `time_ms` builder field, `:230` `frame_spans` (the formula to copy).
  - All of these are re-exported at `takumi::` root and `takumi::prelude`.
- `~/.cargo/registry/src/*/image-0.25.10/src/imageops/sample.rs:964` -- `resize`, `FilterType::Triangle`; `image::RgbaImage::from_raw`.
- `crates/noctis-image/src/plugin.rs:14-160` -- `ImageConfig` (setters and doctest), the range checks in `install`, `TakumiRenderer::new(..)` call.
- `crates/noctis-image/src/template.rs:41-80` -- `ViewFn`; add the hidden gif-gate items.
- `crates/noctis-image/{Cargo.toml,src/lib.rs}` -- the `gif` feature, the `image` dep, `mod budget`.
- `crates/noctis-macros/src/template.rs:162-190` (emission), `:304-390` (`parse_animation`: make `format` optional), tests `template/tests.rs:74-91,233-250`.
- `crates/noctis-core/src/render.rs:27-55` -- set `upload_budget`; `:95` `fail`. `responder.rs:1286,1603-1610` -- `Shared`, `file_limit()`; `handler.rs:215` `AppShared.limits`.
- `crates/noctis-core/src/render/tests.rs:60-140` -- the `Fake` renderer and `parts(app)`; record the received job. `crates/noctis-core/src/pipeline/tests.rs:2928-2970` -- the raw-interaction `attachment_size_limit` pattern.
- `crates/noctis-core/src/error.rs:372` -- the `TemplateRejected` doc.
- `crates/noctis/Cargo.toml:16-22` (features), `crates/noctis/src/lib.rs:561-566` (feature docs), `:1952-2050` (`noctis::image` re-exports: add `EncodeSettings`, `OutputFormat`, `RenderOutput`, `RenderJob`).
- `crates/noctis/tests/{render.rs,ui.rs}`, `tests/types-image/` -- the facade render tests (`render.rs:416` literal) and the `ui_image` suite (image only).
- Skill: `examples/project-template/.claude/skills/noctis/references/images.md`: §2 table `:78`, §8 `:246-330` (Unsupported bullet `:272`, TemplateRejected `:290`, Planned `:304-308`, Animation `:310-322`, Size budget `:324-328`). `SKILL.md:23` (API hash). `examples/project-template/AGENTS.md` (Templates bullet).

## Tasks & Acceptance

**Execution:**
- `Cargo.toml` (workspace) -- add `image = { version = "0.25.10", default-features = false }` to `[workspace.dependencies]`.
- `crates/noctis-ports/src/render.rs` -- add `upload_budget`, `DEFAULT_UPLOAD_BUDGET`, `AnimatedFormat::DEFAULT` and `OverBudget`, with Display and `of` tests. Fix the literals in `tests/ports.rs`.
- `crates/noctis-image/Cargo.toml` -- add the `gif` feature and the `image` dep (in `render`).
- `crates/noctis-image/src/budget.rs` (+ `budget/tests.rs`) -- `Attempt`, `ladder`, `fit`, `spans`, `MIN_FPS`. Unit-test every ladder, fit and spans row.
- `crates/noctis-image/src/renderer.rs` (+ tests) -- animated rendering, the gif gate, per-attempt encoding with downscale, `fit`, `settings_used`. Flip the 10.3 animated-rejection tests. Add the still and animated rows that need no App.
- `crates/noctis-image/src/template.rs` -- the hidden `GifOutput` gate.
- `crates/noctis-image/src/plugin.rs` -- `size_target`, `still_fallback`, the range check and its doctest.
- `crates/noctis-image/src/lib.rs` -- the module, the docs (formats, budget) and the hidden re-exports.
- `crates/noctis-macros/src/template.rs` (+ tests) -- make `format` optional and emit the gif gate.
- `crates/noctis-core/src/{responder.rs,render.rs,error.rs}` (+ `render/tests.rs`, `pipeline/tests.rs`) -- `upload_budget()`, fill the job, the doc fix, and the three core rows.
- `crates/noctis/Cargo.toml`, `src/lib.rs` -- `image-gif` wiring, the feature docs and the re-exports.
- `crates/noctis/tests/render.rs` -- the facade rows (config bounds/defaults, shared pool, register animated, macro default, anim webp, still size step, target wins, OverBudget); the `cfg(feature = "image-gif")` rows anim gif, anim apng and macro gif on.
- `crates/noctis/tests/types-image/template_gif_needs_feature.rs` (+ `.stderr`, blessed with `TRYBUILD=overwrite cargo test --release -p noctis --test ui --features image`) -- the macro gif-off row.
- `docs/discord-rules.md` -- the ATT-01 and ATT-07 How cells.
- `examples/project-template/.claude/skills/noctis/references/images.md`:
  - §2: the `animation(..)` row says `format` defaults to `Webp { quality: 80 }` and `Gif`/`Apng` need `image-gif`.
  - §8: `Unsupported` covers fps or duration 0 and gif off; add `OverBudget { budget, smallest }`; animated Templates now render through `ctx.render` (ext `webp`/`gif`/`png`); drop "an animated one" from `TemplateRejected`; add `ImageConfig::size_target` and `still_fallback`; remove "the size budget [planned, Story 10.4]".
  - Size budget: drop `[planned]`. Document the exact ladder and its constants, `RenderOutput.settings_used` (through `app.renderer()`; `render_report()` stays planned for 10.6) and that the Upload Budget is the interaction's `attachment_size_limit`.
  - Animation: code-driven Templates render with `ctx.render` now; `render_with` stays [planned].
- `examples/project-template/.claude/skills/noctis/SKILL.md` -- mention `OverBudget`, `size_target`, `still_fallback`, `RenderJob::upload_budget`, `AnimatedFormat::DEFAULT` and the new re-exports as `skill-check` requires; update the API hash.
- `examples/project-template/AGENTS.md` -- in the Templates bullet, say that renders fit the upload budget automatically and that `RenderError::OverBudget` means the image cannot shrink enough.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; put the hash in `SKILL.md`.
- `.github/workflows-disabled/ci.yml` -- no new gate (`--each-feature` and `--all-features` cover `gif`). Confirm only.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- append implementation-time decisions only.

**Acceptance Criteria:**
- Given a bot with `features = ["image"]` and a derived Template with `animation(duration_ms = 500, fps = 10)`, when a handler calls `ctx.render(t).await?`, then it gets an Attachment named `<16 hex>.webp` holding an animated WebP of 5 frames; with `format = Png | Jpeg{..} | Webp{..} | WebpLossless` and no animation, the matching still.
- Given `features = ["image"]` without `image-gif`, when a Template declares `animation(.., format = Gif)` or `Apng`, then `cargo check` fails naming the `image-gif` feature; with `image-gif`, it compiles and renders GIF / APNG bytes (ext `gif` / `png`).
- Given a render whose requested encoding exceeds `min(Upload Budget, size_target)`, when the encoder finishes, then the Renderer returns the first attempt of the documented ladder (fps → size → quality → optional still) that fits, and the output's length is ≤ the budget; when none fits, `ctx.render` returns `RenderError::OverBudget { budget, smallest }` and the Render sink gets one Error report.
- Given an interaction whose `attachment_size_limit` is N, when its handler renders, then the Renderer receives `upload_budget == N`; without one, it receives the App's `Limits::attachment_bytes`.
- Given a finished render through `app.renderer()`, when it returns, then `RenderOutput.settings_used` equals the size, quality, fps and frame count actually encoded (decoding the bytes confirms the size).
- Given the finished story, when `skill-check`, `api-snapshot`, `rules-catalog` and `deps-check` run, then all pass, and `cargo tree -p noctis -e normal` shows no image crate.

## Spec Change Log

## Review Triage Log

### 2026-10-22 — Review pass
- verdicts: 28 findings — high 2, medium 4, low 12, false 10, maybe-false 0
- findings:
  - `low` `patch` (blind) Animated quality steps resample the full-size frames again at every quality step — real: resized_frames runs per encode with no cache; patch: cache the frames resampled at the last size next to `Frames`.
  - `low` `reject` (blind) `resized` copies the pixel buffer with `to_vec()` — real, but the premultiplied-alpha patch below needs that working copy (it premultiplies into it), so there is nothing left to remove.
  - `medium` `patch` (blind) Resampling assumes straight RGBA and can bleed transparent-pixel colour into edges — verified: takumi's canvas demultiplies on output (takumi-raster canvas.rs:383), so `Bitmap` is straight alpha and clear pixels are (0,0,0,0); Triangle on straight alpha darkens translucent edges. Grouped with the edge-hunter premultiply finding; patch: premultiply before `imageops::resize`, demultiply after, plus a test.
  - `low` `reject` (blind) A lossless PNG still fallback rarely beats the last lossy animated attempt — real cost in some cases, but it does help long animations (the 5 s rows), and the fix changes the documented ladder (new sub-steps or a different format), more than a direct correction.
  - `high` `defer` (blind) No interim frame cap; huge duration_ms x fps can OOM a pool thread — real; the intent-contract's Never list assigns per-job caps to Story 10.5. Deferred (grouped with the edge-hunter OOM finding).
  - `low` `patch` (blind) `settings_used.frames` is recomputed with `span_count` instead of taken from the encoded frames — two sources of truth held equal only by a test; patch: take the count from the encoder's cached `Frames` and drop the second `too_many_frames` path in `draw`.
  - `low` `patch` (blind) Docs say output "never fails ATT-01 at upload" without naming ATT-02 (25 MiB per request) — authors may read it as "uploads never fail on size"; patch: one sentence next to that claim and in the `size_target` docs.
  - `low` `reject` (blind) Public `RenderJob::upload_budget` without `#[non_exhaustive]` breaks struct literals — the spec deliberately adds a public field and `RenderJob::of` is the constructor; the proposed fix adds public surface (setter) for a pre-1.0 crate, more than a direct correction.
  - `low` `patch` (blind) Test gaps: animated quality steps, lossy still fallback quality, GIF/APNG over budget, non-command ctx budgets — real for the animated steps and the lossy still fallback (grouped with the verification-gap animated-step finding); the ctx kinds share one `CtxInner` code path in `noctis-core/src/render.rs`, and the lossless-animated skip is ladder-tested.
  - `false` `reject` (blind) The gate diagnostic's help line exposes `__private` paths and the ui.rs comment is misleading — the `help` line is rustc's own trait-not-implemented text, which `on_unimplemented` cannot suppress; ui.rs already gates `ui_image` to exclude `image-gif` (ui.rs:89-92), so the fixture cannot run with it.
  - `false` `reject` (blind) fps steps come before size steps with no recorded rationale — the order fps, size, quality is the story's acceptance criterion in epics.md, not an implementation choice.
  - `low` `reject` (blind) The spec's Matrix and tasks do not record the 5 s renderer-row deviation — the fix is to edit this build's spec; the deviation is logged in the memlog.
  - `high` `defer` (edge) Huge duration x fps holds every full-size frame in memory — same root cause as the blind OOM finding; deferred to Story 10.5.
  - `medium` `patch` (edge) Triangle resize on straight alpha bleeds hidden RGB into edges — same root cause as the blind premultiply finding; same patch.
  - `medium` `defer` (edge) GIF with fps above 50 gets sub-20 ms delays that viewers clamp — real; the intent-contract's Never list assigns the fps-cap check to Story 10.6. Deferred.
  - `low` `reject` (edge) `settings_used.frames` overstates frames when takumi merges identical consecutive WebP frames — true (webp/mod.rs `UniqueFrame::push` lengthens duplicates), but playback timing and the rendered frames are unchanged, and the fix would have to mirror takumi's dedupe, more than a direct correction.
  - `low` `patch` (vgap) `ImageConfig::still_fallback(false)` is never observed through `ImagePlugin` — pre-verified regression gap; patch: facade test asserting `OverBudget` with the fallback off and a 1-frame PNG with the default.
  - `medium` `patch` (vgap) Animated size and quality steps are never checked against the encoded output — pre-verified regression gap; patch: renderer rows asserting the WebP canvas equals `settings_used.size` and a quality-stepped output below q80 (grouped with the blind test-gap finding, plus a lossy still-fallback quality check).
  - `low` `patch` (vgap) The pre-admission u32::MAX-frames rejection has no renderer test — pre-verified regression gap; patch: a renderer row with `Duration::MAX` at 1000 fps asserting `Unsupported` and an unchanged admitted counter.
  - `false` `reject` (intent D1) The animated format is configurable per Template only — the epics AC1 asks for GIF/APNG configurable as the animated format, which `animation(format = ..)` satisfies; per-call overrides (`render_with`) are Story 10.6 in epics.md.
  - `false` `reject` (intent D2) `image-gif` does not gate the GIF/PNG encoder dependencies — takumi-raster links them unconditionally and offers no feature to drop them; the spec deliberately settled a code-and-API gate.
  - `false` `reject` (intent D3) Core does not re-check a custom Renderer's output length — the AC binds the Renderer ("the Renderer returns the first attempt that fits"); the port documents `upload_budget` as the limit, and an oversize custom output still fails with a typed ATT-01 error at the Responder.
  - `false` `reject` (intent D4) The budget is per file, not per request (ATT-02) — the AC defines the budget as min(Upload Budget, target) per render; the documentation side is handled by the ATT-02 doc patch above.
  - `low` `reject` (intent D5) No end-to-end test of ctx.render with an interaction budget through the takumi Renderer — both halves are tested, and the join is one field write (`job.upload_budget`) that the core test asserts reaches the Renderer; a full interaction-plus-takumi harness is more than a direct correction.
  - `false` `reject` (intent D6) `settings_used` is only on `RenderOutput` — AC3 names `RenderOutput.settings_used` literally; `render_report()` is Story 10.6.
  - `false` `reject` (intent D7) Size steps resample instead of re-laying out — the spec settled this with a recorded reason (fixed-px layouts would crop) and the skill documents it.
  - `false` `reject` (intent D8) The still fallback is an App-wide switch — "optionally" names no location; the spec settled `ImageConfig::still_fallback`.
  - `false` `reject` (intent D9) Fixed ladder granularity ends in `OverBudget` after at most 9 encodes — the AC requires a typed error when nothing fits; the steps are documented in the skill and the spec.

## Design Notes

- **Animated rendering moves into 10.4.** The animated encoders and the fps step both need real frames, so the per-frame view loop ships here, which makes animated Templates renderable through `ctx.render`. Story 10.6 keeps keyframe and `animate-*` validation, the root-size-mismatch error, fps caps at job creation, `render_with` and `render_report`.
- **The budget travels in the job.** The Renderer port knows no App or interaction. `ctx.render` knows the Upload Budget (the same rule as the Responder's ATT-01 check), so it writes it into `RenderJob`. `size_target` is per App (ImageConfig), not per pool. A hand-built job gets 20 MiB from `RenderJob::of`.
- **Size steps downscale instead of re-laying out.** Rendering at a smaller viewport would crop fixed-px layouts, so the full-size frames are resampled (Triangle) instead. An fps step must re-render, because the frame count changes what the view draws.
- **Ladder shape.** It has at most 9 encodes (1 + 2 fps + 2 size + 3 quality + 1 still), and the pure `ladder`/`fit` pair is unit-tested without takumi. Lossless formats (PNG, WebP lossless, GIF, APNG) have no quality step. The still fallback is on by default, because the story's goal is that images never fail to upload; it is reported as `frames 1` and the still `format`.
- **GIF gate.** It is checked twice: at compile time, through an `on_unimplemented` trait whose impl exists only with `noctis-image/gif` (enabled by `image-gif`); and at render time in `view_of`, for hand-built jobs. takumi-raster always links its GIF/PNG encoders, so the feature gates Noctis code only, not dependencies.
- **Known residual (10.5).** A derived Template with a huge `duration_ms × fps` can still allocate many frames until the per-job caps land in Story 10.5.

## Verification

**Commands:**
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` and `cargo test --release --workspace --all-features` -- pass
- `cargo test --release -p noctis --test ui --features image` -- the `ui_image` suite passes with the new fixture
- `cargo hack test --release --workspace --each-feature` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `cargo tree -p noctis -e normal | grep -E 'noctis-image|takumi|rayon| image v'` -- no output
- `bash tools/story-loop/gates.sh` -- ALL-OK

## Auto Run Result

- **Summary:** Output formats (PNG/WebP/JPEG stills; WebP/GIF/APNG animated, GIF gated by `image-gif` at compile time and render time) and a size-budget ladder (fps -> size -> quality -> optional still fallback, max 9 encodes) fitting each render within min(Upload Budget, `size_target`); `RenderOutput.settings_used` reports the settings that fit; typed `OverBudget` when nothing fits. Animated per-frame rendering moved into this story.
- **Files changed:** `noctis-ports/src/render.rs` (job `upload_budget`, `settings_used`, `OverBudget`); `noctis-image/src/budget.rs` (pure ladder/fit); `noctis-image/src/renderer.rs` (encoders, frame loop, premultiplied resampling, frame cache); `noctis-image/src/plugin.rs`, `template.rs` (`still_fallback`, GIF gate); `noctis-core/src/render.rs`, `responder*.rs` (budget written into job); `noctis-macros/src/template.rs` (animation format, gate diagnostic); tests and trybuild fixture; `docs/discord-rules.md`; project-template skill, `images.md`, `AGENTS.md`; `xtask/public-api.txt`.
- **Review findings:** 28 total (high 2, medium 4, low 12, false 10). Patched (by verdict): medium 3 (premultiplied-alpha resize, x2 grouped; animated size/quality output checks), low 6 (frame cache, single frames source, ATT-02 doc note, test gaps, still_fallback facade test, u32::MAX-frames test). Deferred: high 2 (frame-count OOM -> Story 10.5), medium 1 (GIF fps>50 -> Story 10.6). Rejected: 16 with recorded reasons in the triage log (`to_vec` copy needed; PNG fallback ladder change; non_exhaustive; 5 s row deviation logged in memlog; WebP dedupe frame count; e2e harness; 10 intent/false items).
- **Follow-up review recommended: true** — three medium patches; unverified risk: premultiply/demultiply rounding on near-transparent edge pixels and the new per-size frame cache interacting with fps re-renders could yield wrong canvas or stale frames.
- **Verification:** `tools/story-loop/gates.sh` ALL-OK (fmt, clippy -D warnings, workspace tests incl. all-features and ui_image, cargo hack each-feature, api-snapshot, deps-check, rules-catalog, skill-check, tw-grammar, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** huge `duration_ms x fps` can still allocate many frames until Story 10.5 caps; GIF delays below 20 ms clamped by viewers until Story 10.6 fps caps; budget is per file, ATT-02 per-request 25 MiB is documented, not enforced here.
