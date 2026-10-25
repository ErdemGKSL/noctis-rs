---
title: 'Story 15.1: Renderer seam'
type: 'feature'
created: '2026-10-25'
status: 'in-progress'
baseline_revision: '8cf70210a142d6c44b1a653bc40a7451b749bc21'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-37 is half there. `AppBuilder::renderer(Arc<dyn Renderer>)` exists, but:
- the image plugin silently overwrites an author's Renderer at `build`;
- core trusts whatever a custom Renderer returns (empty or over-budget bytes reach the Responder);
- a custom Renderer has no Noctis-defined render key for its digest and cache;
- the renderer types are exported from stable `noctis::ports` and `noctis::image`, against AD-2 ("only through `noctis::image::unstable`").

**Approach:**
- `.renderer(MyRenderer)` takes any `R: Renderer`; a second Renderer on one App is a `BuildViolation`.
- `ctx.render` checks every Renderer's output against the job's Upload Budget, then names the attachment by `output.digest` (as now).
- `RenderJob::key(renderer)` gives custom Renderers a deterministic render key.
- The renderer-facing types move to `noctis::image::unstable` only.
- The Usage Skill, the template and the snapshot document the seam.

## Boundaries & Constraints

**Always:**
- **Setter.** `crates/noctis-core/src/app.rs` changes the signature to `pub fn renderer<R: Renderer>(&mut self, renderer: R) -> &mut Self`.
  - It stores `Arc::new(renderer)` as `Arc<dyn Renderer>` and records `std::any::type_name::<R>()`.
  - When a Renderer is already set, it keeps the first one and pushes `BuildViolation::DuplicateRenderer { first: &'static str, second: &'static str }`.
  - That violation's message: ``"a Renderer is set twice: `{first}`, then `{second}`: an App has one Renderer; keep `.renderer(..)` or the image plugin (`.theme(..)`/`.image(..)`), not both"``.
- **Arc impl.** `crates/noctis-ports/src/render.rs` adds `impl<R: Renderer + ?Sized> Renderer for Arc<R>`, forwarding `render`, `register` and `bind`. So `Arc<dyn Renderer>` and `Arc<X>` still go into `.renderer(..)`.
- **ImagePlugin.** `crates/noctis-image/src/plugin.rs:380` passes `TakumiRenderer::new(..)` without `Arc::new`.
- **Output check.** `crates/noctis-core/src/render.rs` `render_job` reads `let budget = job.upload_budget` before calling the Renderer. Then, in this order:
  1. When `output.bytes` is empty, it returns `RenderError::failed("the Renderer returned no bytes")`.
  2. When `output.bytes.len() as u64 > budget`, it returns `RenderError::OverBudget { budget, smallest: len }`.
  3. Otherwise it calls `Attachment::rendered(bytes, output.digest, output.format.extension())` (unchanged).

  Both errors go through the existing `fail` path: they are reported to the `Render` sink with no `noctis_render_rejected_total` reason. The duration histogram is recorded only for an accepted output (move the `histogram!` after the checks).
- **Key.** `RenderJob::key(&self, renderer: &[u8]) -> [u8; 32]` in noctis-ports, which gains the `blake3.workspace = true` dependency. It hashes with `blake3::Hasher::new()`.
  - Every variable-length field is a `u64` LE length plus its bytes; integers are fixed-width LE.
  - The fields, in order:
    1. `b"noctis.render.job.v1"`
    2. `env!("CARGO_PKG_VERSION")`
    3. `renderer`
    4. `template_id`
    5. `template_version` (u32)
    6. `data`
    7. width, then height (u32 each)
    8. the format tag (`Png 0`, `Jpeg 1`, `Webp 2`, `WebpLossless 3`), then a quality byte (0 when the format has none)
    9. the animation: `0u8`, or `1u8` + `duration.as_nanos()` (u128) + `fps` (u16) + a tag (`Webp 0`, `Gif 1`, `Apng 2`) + a quality byte
    10. `upload_budget` (u64)
  - Its doc says: the key covers every job field, the Noctis version and the renderer's own identity. A custom Renderer should pass its name, version and asset versions as `renderer`, return the key as `RenderOutput::digest`, and may use it as its `RenderCache` key (bound through `bind`). The takumi Renderer keeps its own AD-15 key.
- **Unstable surface (AD-2).**
  - `crates/noctis/src/lib.rs` removes `Renderer`, `RendererPorts`, `RenderJob`, `RenderOutput`, `TemplateDescriptor`, `ErasedView` and `decode_template` from `noctis::ports`. It removes `RenderJob`, `RenderOutput` and `TemplateDescriptor` from `noctis::image`.
  - `noctis::image::unstable` re-exports all seven, next to `ViewFn`, `ClassCheck` and `ClassCheckMode`.
  - The stable modules keep `Template`, `Size`, `Format`, `AnimatedFormat`, `AnimationSpec`, `Frame`, `OutputFormat`, `EncodeSettings`, `RenderError`, `RenderCap`, `RenderCache` and `BoxFuture`.
  - Update the docs of `ports`, `image` and `unstable`: list the moved items; say semver does not cover them and that a replacement Renderer needs only them.
  - The macros already use `::noctis::__private::ports`, so their output is unchanged.
- **`NoRenderer` message.** It becomes ``"no Renderer is installed: call `.theme(..)` or `.image(..)` (feature `image`), or `.renderer(..)`, on the AppBuilder"``. Update the assertion at `render.rs:764`.
- **Same commit (AGENTS.md).**
  - Update the template code, its `AGENTS.md`, the Usage Skill, `xtask/public-api.txt` (re-blessed) and the `SKILL.md` `API hash`.
  - Update `docs/discord-rules.md` ATT-01: add "and `ctx.render` rejects any Renderer's output over `RenderJob::upload_budget` with `RenderError::OverBudget` before the attachment is built (Story 15.1)". The rule id and the limit are unchanged, so `noctis-rules` is not touched.

**Never:**
- No core-side render cache and no core dependency on `noctis-image`. Caching stays the Renderer's job (AD-15); core binds the App's namespaced `RenderCache` and `AssetStore` to any Renderer (already done).
- No change to `TakumiRenderer`'s key, cache, flights or golden hexes, and no change to Templates, `view!` or handler APIs.
- No remote render service, new cargo feature, new crate or `Cargo.lock` change (`blake3 1.8` is already locked).
- No re-export of a renderer type from any stable path. Do not run `cargo hack`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Custom renderer e2e | `.renderer(Fixed)`, the handler calls `ctx.render(Card)` | the uploaded file's bytes equal Fixed's bytes; name `<first 16 hex of digest>.png`; Fixed saw `template_id "card"`, version 1 and the interaction's budget | none |
| Same handler, takumi | the same Module with `.theme+.image` | real PNG, `(64, 32)` | none |
| Named by digest | two digests, equal bytes; equal digests, different bytes | different names; equal names | none |
| Extension | outputs `Jpeg`, `Webp`, `Gif`, `Apng` | `.jpg`, `.webp`, `.gif`, `.png` | none |
| Budget boundary | budget 1000; output length 999, 1000 | accepted | none |
| Budget max+1 | budget 1000; length 1001 | `OverBudget { budget: 1000, smallest: 1001 }`; 1 Render-sink report; no rejected metric; no duration sample | typed error |
| Detached budget | no interaction (`Limits::attachment_bytes` 20 MiB); length = 20 971 520, then +1 | accepted; `OverBudget` | typed error |
| Empty output | `bytes` empty | `Failed`, `Display` "render failed", source "the Renderer returned no bytes"; reported | typed error |
| Renderer error passthrough | Fixed returns `QueueFull` / `Failed` | unchanged from today (not reported / reported) | as today |
| Duplicate renderer | `.renderer(A).renderer(B)`; `.renderer(A)` + `.theme(..)` | `BuildError` with `DuplicateRenderer { first: type_name A, second: type_name B / TakumiRenderer }`; the first is kept | build fails |
| Arc forms | `.renderer(Arc::new(A))`, `.renderer(arc as Arc<dyn Renderer>)` | builds, `has_renderer()`; `register`/`bind` reach A exactly once | none |
| Custom register rejects | Fixed's `register` returns `Unsupported` | `TemplateRejected` (unchanged) | build fails |
| Key determinism | the same job and renderer twice | equal | none |
| Key sensitivity | change exactly one of: renderer, id, version, data (1 byte), width, height, each format tag, quality, animation None↔Some, duration +1 ns, fps, animated tag, animated quality, budget | every digest distinct from the base and from each other | none |
| Key framing | renderer `"ab"` + id `"c"` vs `"a"` + `"bc"`; empty data vs `[0]`; empty renderer | distinct | none |
| Key unicode | id `"kart.ş"`, data with `"😀é"`, renderer `"uzak=1"` | golden hex pinned in the test | none |
| Unstable paths | `noctis::image::unstable::{Renderer, RendererPorts, RenderJob, RenderOutput, TemplateDescriptor, ErasedView, decode_template}` | compile; `public-api.txt` lists none of them under `noctis::ports::` or directly under `noctis::image::` | none |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs`
  - `RenderJob` :88 (add `key`)
  - `RenderOutput` :347 (digest doc: "a custom Renderer uses `RenderJob::key`")
  - `RenderError::NoRenderer` :476
  - `Renderer` :578 (Arc impl after it)
  - tests :764 (NoRenderer message), :930 (`Plain` impl)
- `crates/noctis-ports/Cargo.toml` -- add `blake3.workspace = true`.
- `crates/noctis-core/src/app.rs`
  - `AppBuilder.renderer` field :1736 (add the type name); init :1866
  - setter :2765-2788 (its doc example passes `Fake` by value)
  - bind :3065
  - `register_templates` :3133
  - Debug :3218
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (:216, `#[non_exhaustive]`); add `DuplicateRenderer` with a doc example like `TemplateRejected` (:713).
- `crates/noctis-core/src/render.rs`
  - `render` :82 (sets `upload_budget` :98-103)
  - `render_job` (output check)
  - `fail` (unchanged mapping)
- `crates/noctis-core/src/render/tests.rs` -- the `Fake`/`Answer` harness (:60-200), `over_budget_is_reported_without_a_rejected_reason` :432. Extend `Answer` with sized and empty outputs.
- `crates/noctis-core/src/app/tests.rs:1160-1360` -- renderer and plugin tests. Callers that do `Arc::clone(..) as Arc<dyn Renderer>` still compile.
- `crates/noctis-core/src/pipeline/tests.rs:3142` -- `b.renderer(renderer)`.
- `crates/noctis-image/src/plugin.rs:380` -- `app.renderer(Arc::new(TakumiRenderer::new(..)))`.
- `crates/noctis/src/lib.rs`
  - `ports` docs and re-exports :2010-2060
  - `image` docs :2144-2260, re-exports :2300-2314, `unstable` :2316
- Facade tests that import the moved names; switch them to `noctis::image::unstable`:
  - `crates/noctis/tests/{render,fonts,emoji,remote_images,template}.rs`
  - `crates/noctis/tests/types-image/template_missing_deserialize.rs` (check its `.stderr`)
- `crates/noctis/tests/render_e2e.rs` -- the pattern for the new e2e test: `TestApp::build`, `uploaded(..)`, `show(..)`.
- `crates/noctis-testkit` -- uses `noctis_ports` directly; unaffected.
- `examples/project-template/src/adapters/{mod.rs,bounded_cache.rs}` and `tests/adapters.rs` -- the pattern for a template Adapter and its test. `tests/templates.rs` builds a `ProfileCard`.
- Skill files:
  - `references/adapters.md:24, 395-445` (§2b)
  - `references/images.md:737` (`use noctis::image::RenderJob`), §8, §9, §12 :826
  - `references/app-and-sharding.md:100, 128`
  - `references/testing.md:287`
  - `SKILL.md:23` (API hash)
- `docs/discord-rules.md:262` (ATT-01).

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-ports/Cargo.toml`, `crates/noctis-ports/src/render.rs` -- add `RenderJob::key`, the `Arc<R>` impl and the `NoRenderer` message. Unit-test every Key row of the matrix, including the golden hex. `Arc` forwarding: a counting renderer behind `Arc<dyn Renderer>`.
2. `crates/noctis-core/src/error.rs`, `crates/noctis-core/src/app.rs` -- the generic setter and `DuplicateRenderer`. Test the Duplicate and Arc rows in `app/tests.rs`.
3. `crates/noctis-core/src/render.rs` (+ `render/tests.rs`) -- the output check. Test the Budget, Detached, Empty, Extension, Named-by-digest and passthrough rows.
4. `crates/noctis-image/src/plugin.rs` -- pass the renderer by value.
5. `crates/noctis/src/lib.rs` -- move the re-exports into `unstable` and update the module docs. Fix the facade test imports.
6. `crates/noctis/tests/renderer_seam.rs` (new, `#![cfg(all(feature = "image", feature = "testkit"))]`) -- define `Fixed` from `noctis::image::unstable` only. Test the e2e rows (custom and takumi, from one Module-building fn), the Duplicate row through `.theme(..)`, and the Unstable-paths row.
7. `examples/project-template/src/adapters/fixed_renderer.rs` (+ `mod.rs` export) -- `FixedRenderer`:
   - `new(bytes: Bytes, format: OutputFormat)`, `const ID: &[u8] = b"noctis-template.fixed=1"`;
   - `render` returns the bytes with `digest = job.key(Self::ID)` and `EncodeSettings { size: job.size, quality: None, fps: None, frames: 1 }`;
   - `register` returns `Ok(())`.
   
   It is documented as a handler-test double and the skeleton of a remote Renderer.
   
   In `examples/project-template/tests/adapters.rs`, test that `FixedRenderer` renders a `ProfileCard` job with the key as digest, and that `App::builder().module(modules::profile::module()).renderer(FixedRenderer::new(..))` builds with no font. Add one clause to the template `AGENTS.md` image bullet: "replace the takumi Renderer only through `.renderer(..)` with `noctis::image::unstable` types (outside semver; never together with `.theme`/`.image`)".
8. The Usage Skill:
   - `adapters.md` §2b: the seam, its instability and the `unstable` paths, the setter, `DuplicateRenderer`, the output check, `RenderJob::key`, and a `FixedRenderer` example;
   - `images.md` §8 import and §12;
   - `app-and-sharding.md` setter row;
   - `testing.md`.
   
   Then `docs/discord-rules.md` ATT-01, then `cargo run --release -p xtask -- api-snapshot --bless` and the new `API hash`.

**Acceptance Criteria:**
- Given a Module with a derived Template and a handler calling `ctx.render`, when the App uses `.renderer(Fixed)` instead of `.theme(..)`/`.image(..)`, then the same Module and handler upload Fixed's bytes named by its digest, with no source change.
- Given any Renderer, when its output exceeds the job's Upload Budget or is empty, then `ctx.render` returns the typed error and the Render sink gets one report; no file is sent.
- Given the public API, when `api-snapshot` and `skill-check` run, then the renderer types appear only under `noctis::image::unstable`, every new item is mentioned in the skill, and the `API hash` matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Reading of "cache key still applies"** (decision): AD-15 makes caching the Renderer's responsibility, and 10.10 ruled out a core cache. Core therefore applies the rest:
  - the digest names the attachment, so identical digests upload once and edits keep them by name;
  - the App's namespaced `RenderCache` and `AssetStore` are bound to any Renderer;
  - the Upload Budget is checked at the render boundary, and again by the Responder (ATT-01).
  
  `RenderJob::key` gives a custom Renderer FR-36's key semantics: it covers the job, the Noctis version and the renderer's version.
- **Why the duplicate check** (decision): plugins install inside `build`, after the author's setters, so without it `.theme(..)` would silently replace `.renderer(Fixed)` and break AC 1.
- **Moved items** (decision): the renderer-facing types are those only a Renderer implementor needs. A hand-written `impl Template` now imports `ErasedView` from `unstable`; the derive is the stable path.
- Golden example:
  ```rust
  use noctis::image::unstable::{Renderer, RenderJob, RenderOutput, TemplateDescriptor};
  impl Renderer for Fixed {
      fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
          let out = RenderOutput { bytes: self.0.clone(), digest: job.key(b"fixed=1"),
              format: OutputFormat::Png, settings_used: EncodeSettings { size: job.size, quality: None, fps: None, frames: 1 } };
          Box::pin(async move { Ok(out) })
      }
      fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> { Ok(()) }
  }
  ```

## Verification

**Commands** (repository root, release only):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-ports` and `cargo test --release -p noctis-core` -- pass
- `cargo test --release -p noctis --features image,testkit --test renderer_seam --test render_e2e --test render` -- pass
- `cargo test --release` in `examples/project-template` -- pass
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot`, `skill-check`, `deps-check`, `rules-catalog`, `tw-grammar --check` -- ok
- `grep -nE '^pub (struct|trait|fn|enum) noctis::(ports|image)::(Renderer|RendererPorts|RenderJob|RenderOutput|TemplateDescriptor|ErasedView|decode_template)\b' xtask/public-api.txt` -- no match
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is not run here (the gate script runs it in the epic's last story).
