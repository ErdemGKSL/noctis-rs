---
title: 'Story 15.1: Renderer seam'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_revision: '8cf70210a142d6c44b1a653bc40a7451b749bc21'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred:
  - summary: >-
      The project template's tests/adapters.rs (FixedRenderer render and the profile Module building on FixedRenderer alone) run in no gate or CI step.
    evidence: |-
      examples/project-template is excluded from the workspace; tools/story-loop/gates.sh never enters it; ci.yml only runs cargo metadata and a continue-on-error build of the template (tracking mode until Story 14.2). FixedRenderer itself is covered through crates/noctis/tests/renderer_seam.rs.
    location: >-
      examples/project-template/tests/adapters.rs
    severity: low
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

### 2026-10-24 — Review pass
- verdicts: 29 findings — high 0, medium 3, low 17, false 9, maybe-false 0
- findings:
  - `[low]` `[reject]` (blind) the Renderer trait has no facade name without `image` — real, but AD-2 and AC 3 bind the renderer types to `noctis::image::unstable` only and the spec forbids a new feature; Templates are derived only with `image`, so a no-`image` seam is not an everyday configuration and the fix would add a path or feature the contract forbids.
  - `[low]` `[reject]` (blind) stable `Template::view()` names `unstable::ErasedView` — real but deliberate (Design Notes: the derive is the stable path, a hand-written `impl Template` imports from `unstable`); hiding `view()` changes public surface for a rarely hand-written impl.
  - `[low]` `[reject]` (blind) `.renderer(..)` double-wraps an `Arc` — real (`Arc<Arc<..>>`, one extra indirection, `App::renderer()` no longer ptr-equal), but the by-value `R: Renderer` setter plus the `Arc<R>` impl is spec-mandated and the cost is negligible; an `IntoPort`-style trait adds public surface.
  - `[low]` `[patch]` (blind) "the first is kept" and the "`A`, then `B`" wording ignore call order with the image plugin — verified: `ImagePlugin` installs in `build`, so `.theme(..).renderer(Fixed)` reports `first = Fixed`, `second = TakumiRenderer`; patch: document that an explicit `.renderer(..)` is always `first`/kept and the plugin's Renderer is always `second`, and test the reverse call order.
  - `[low]` `[reject]` (blind) `type_name` gives `Arc<dyn Renderer>` or the `pub(crate)` `noctis_image::renderer::TakumiRenderer` — real, but `type_name` is spec-mandated and the message's remedy text is actionable on its own; a naming hook would add trait surface.
  - `[false]` `[reject]` (blind) `RenderJob::key` hashes `upload_budget` and splits caches by tier — a Renderer may degrade its output to the budget, so the budget is an output input and must be in a key that "covers every job field"; the spec pins this deliberately.
  - `[false]` `[reject]` (blind) the key's version is the `noctis-ports` version and breaks caches per release — all crates share `version.workspace = true` (AD-2, root `Cargo.toml` 0.1.0), so it is the Noctis version; FR-36 key semantics include it by design and the golden test states it is re-pinned on a bump.
  - `[low]` `[reject]` (blind) core does not check output format/settings against the job — real, but the reported format names the bytes actually returned (takumi's still fallback legitimately differs from the declared format), so naming stays coherent; a consistency guard adds branches for a Renderer bug not shown.
  - `[false]` `[reject]` (blind) rejected outputs are invisible in metrics — every empty or over-budget output is reported to the `Render` error sink (tested), and the spec deliberately records no rejected reason and no duration for them.
  - `[low]` `[reject]` (blind) `FixedRenderer`'s key ignores its bytes and format — real, but an App holds one Renderer, so two differently-built FixedRenderers never share an App's attachments; the test double follows the spec's `job.key(Self::ID)` and the fix would only matter across processes.
  - `[medium]` `[patch]` (blind) the interaction-budget path is untested; `noctis-rules` untouched — grouped with the vgap budget finding; the `noctis-rules` half is false (rule id and limit unchanged, as the spec states); patch: the pipeline test below.
  - `[low]` `[reject]` (blind) weakened snapshot assertion and cross-crate `#[path]` include — the snapshot that follows renders through `app.renderer()` with the fake's shade, so the identity is still shown; the `#[path]` file is absent only from a packaged test target, which `cargo publish` does not build.
  - `[false]` `[reject]` (blind) `renderer_type: ""` sentinel and `len as u64` vs `try_from` — both fields are written together at the single setter branch, so they cannot disagree; `usize` fits `u64` on every supported target; no named harm.
  - `[low]` `[patch]` (edge) violation reverses the author's call order with `.theme(..)` before `.renderer(X)` — same root cause as the blind call-order row; patched with it.
  - `[low]` `[reject]` (edge) `ImagePlugin::install` starts the render pool and font registry for a build that then fails — real, but only on a misconfigured App whose build fails anyway; the guard adds a branch in the plugin.
  - `[low]` `[reject]` (edge) erased `Arc<dyn Renderer>` reports `alloc::sync::Arc<dyn ..>` — same root cause as the blind type-name row; rejected with it.
  - `[low]` `[reject]` (edge) custom Renderer format/size inconsistent with the job — same root cause as the blind format-check row; rejected with it.
  - `[low]` `[patch]` (edge) `uploaded()` in `renderer_seam.rs` takes the first file and never asserts exactly one — verified (`.next().expect("one uploaded file")`); patch: collect the files and assert there is exactly one.
  - `[medium]` `[patch]` (vgap) `renderer_seam` is missing from the CI `test` job — pre-verified; AGENTS.md requires the disabled CI definitions to stay in sync; patch: add the step next to `render_e2e`.
  - `[medium]` `[patch]` (vgap) the `render_job` budget check is tested only with a budget equal to `Limits::attachment_bytes` — pre-verified; patch: pipeline tests with an interaction `attachment_size_limit` of 10 MiB and a 10 MiB + 1 output (`OverBudget`), and a larger limit accepting an output over 20 MiB.
  - `[low]` `[defer]` (vgap) the template's `tests/adapters.rs` runs in no verification path — pre-verified, pre-existing tracking-mode gap until Story 14.2; recorded under `deferred`.
  - `[false]` `[reject]` (vgap other) the `strong_count` assertion no longer proves `app.renderer()` reaches the fake — the snapshot assertion right after renders through it and matches the fake's shade, which only that Renderer produces.
  - `[false]` `[reject]` (intent 1) the cache key is opt-in, not core-enforced — the spec settled the R2b reading deliberately (AD-15: caching is the Renderer's; core-side cache is in Never).
  - `[false]` `[reject]` (intent 2) a second Upload Budget check against AD-22 — `RenderJob::upload_budget` and `RenderError::OverBudget` were already the Renderer-side budget contract; core now enforces it uniformly while the Responder stays the single message-level ATT-01 check; settled in the Design Notes.
  - `[low]` `[reject]` (intent 3) setter signature change, duplicate violation, double `Arc` — signature and violation are spec-mandated with a stated rationale; the double `Arc` is the blind `IntoPort` row, rejected with it.
  - `[low]` `[reject]` (intent 4) stable signatures (`App::renderer`, `AppBuilder::renderer`, `Template::view`) name unstable types — inherent to a seam whose trait AD-2 places in `unstable`; same root cause as the blind `Template::view` row.
  - `[low]` `[reject]` (intent 5) no-`image` build cannot name the trait — same root cause as the blind feature row; rejected with it.
  - `[false]` `[reject]` (intent 6) breaking removal of stable paths — AC 3 ("renderer types only via `noctis::image::unstable`") and AD-2 require it; the debug snippet moving to `unstable` is the intended consequence.
  - `[false]` `[reject]` (intent 7) the handler-level proof lives in the monorepo test, not the template — the spec's Tasks 6 and 7 place it there deliberately; the template's own run gap is the deferred vgap row.

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

## Auto Run Result

- **Summary:** Story 15.1 opens a Renderer seam. `AppBuilder::renderer(R: Renderer)` takes any custom Renderer, and the renderer-facing types (`Renderer`, `RendererPorts`, `RenderJob`, `RenderOutput`, `TemplateDescriptor`, `ErasedView`, `decode_template`) move to `noctis::image::unstable`. A new `RenderJob::key` gives a custom Renderer FR-36 cache-key semantics. Core now checks the Upload Budget at the render boundary (empty or over-budget output is reported to the `Render` error sink). Setting a Renderer twice, for example an explicit `.renderer(..)` plus the image plugin's TakumiRenderer, is a build violation: the explicit one is reported as `first`, the plugin's as `second`.
- **Files changed:**
  - `crates/noctis-ports/src/render.rs`: Renderer trait, `RenderJob::key`, output types.
  - `crates/noctis-core/src/{app.rs,error.rs,render.rs,pipeline/tests.rs,app/tests.rs,render/tests.rs}`: setter, duplicate-renderer violation, budget check, tests.
  - `crates/noctis-image/src/plugin.rs`: the plugin installs TakumiRenderer through the seam.
  - `crates/noctis/src/lib.rs`: `unstable` re-exports, stable paths removed.
  - `crates/noctis/tests/renderer_seam.rs` (new) and the other facade tests: end-to-end proof that a handler renders through a custom Renderer.
  - `examples/project-template/`: `FixedRenderer` adapter, adapters test, Usage Skill references (adapters, images, testing, app-and-sharding), `AGENTS.md`, API hash.
  - `xtask/public-api.txt`: re-blessed. `docs/discord-rules.md`: wording only. `.github/workflows-disabled/ci.yml`: adds the `renderer_seam` test step.
- **Review findings:** 29 in total: 0 high, 3 medium, 17 low, 9 false.
  - Patched (6 entries in 4 patches; 3 medium, 3 low):
    - call-order wording for `first`/`second` (blind and edge rows), with a test for the reverse call order;
    - `uploaded()` now asserts exactly one file;
    - CI `renderer_seam` step;
    - pipeline tests for the interaction-budget path (10 MiB limit with a 10 MiB + 1 output gives `OverBudget`; a larger limit accepts an output over 20 MiB).
  - Deferred (1): the template's `tests/adapters.rs` runs in no verification path (pre-existing, tracked until Story 14.2).
  - Rejected (22): the trait has no name without `image` (AD-2/AC 3); `Template::view` names an unstable type (deliberate); double `Arc` (spec-mandated setter); `type_name` output (spec-mandated); no format consistency check; `FixedRenderer` key; snapshot assertion and `#[path]` include; erased-type naming duplicate; the `ImagePlugin` installing on a build that then fails. The 9 false rows (budget in the key, version in the key, metrics, sentinel, `strong_count`, opt-in cache, the second budget check, path removal, where the template proof lives) are settled by the spec and its Design Notes. Each reason is in the triage log.
- **Follow-up review recommended:** true. Patched by verdict: high 0, medium 3, low 3. Named risk: the new core budget check in the interaction pipeline (an interaction `attachment_size_limit` that differs from `Limits::attachment_bytes`) and the duplicate-renderer call-order reporting were changed in review and have not had an independent second review.
- **Verification:**
  - Local gates via `tools/story-loop/gates.sh`: all green. They cover fmt, clippy `-D warnings`, workspace tests, the `renderer_seam`/`render_e2e`/`render` tests, the template tests, `api-snapshot`, `skill-check`, `deps-check`, `rules-catalog`, `tw-grammar --check`, `tw_crosscheck`, `cargo deny check` and the MSRV 1.91 check.
  - CI is paused by the maintainer; `cargo hack` is left to the epic gate.
- **Residual risks:**
  - The renderer types are reachable only with the `image` feature.
  - A stable signature (`Template::view`) names an `unstable` type.
  - The template's `tests/adapters.rs` is not run by any gate (deferred).
  - The `RenderJob::key` golden value must be re-pinned on each version bump.
