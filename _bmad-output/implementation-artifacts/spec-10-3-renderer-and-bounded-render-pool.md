---
title: 'Story 10.3: Renderer and bounded render pool'
type: 'feature'
created: '2026-10-23'
status: 'done'
baseline_revision: '326070f92563f0fa59323b7b92743eb1e846e6ba'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-2-templates-with-compile-time-class-validation-and-a-theme.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Nested rayon work-stealing inside takumi's parallel paths (blur for_each, WebP/GIF frame par_iter) can run other admitted render jobs on a waiting pool thread's stack, reordering completion and growing stack depth with queue depth.
    evidence: |-
      takumi-core blur.rs:92 uses into_par_iter().for_each_init and takumi is built with its rayon feature, so a still render with a blur waits in a rayon join on the bounded pool and may steal top-level spawned renders. Unverified whether stack depth can overflow: settle with a stress test of queue_capacity concurrent blurred (shadow/filter) renders on threads(1), or bound it with ThreadPoolBuilder::stack_size or an in_place_scope boundary (Story 10.5 owns admission).
    location: >-
      crates/noctis-image/src/pool.rs (try_submit / pool.spawn)
    severity: high (unverified)
---

<intent-contract>

## Intent

**Problem:** Story 10.2 shipped Templates, the node tree and `ClassCheck`, but nothing renders them. There is no Renderer, no render pool, no `ImagePlugin`/`.image(..)`/`.theme(..)` and no `ctx.render`. `Registry::templates()` is never handed to `Renderer::register`, and the deferred 10.2 item (wire `ClassCheck`, Deny on `Runtime::for_test()`) is still open (FR-34, FR-61, NFR-4, NFR-5, UX-DR2; AD-15, AD-22, AD-24, AD-29).

**Approach:**
- `noctis-image` (behind `render`) gains:
  - a process-scoped rayon `RenderPool` (a `noctis_core::Resource`) with a bounded admission counter;
  - a per-App takumi `Renderer` that lowers `Node` trees into takumi nodes, applies the Theme and encodes the Template's still format;
  - `ImageConfig`, `ImagePlugin` and the sealed `AppBuilderImageExt` (`.image(..)`, `.theme(..)`).
- Core gains:
  - `ctx.render(t)` on every interaction context, which postcard-encodes `t` into a `RenderJob`, awaits the App's `Arc<dyn Renderer>` and returns `Attachment::rendered(..)`; the existing auto-defer covers the wait;
  - render metrics, the `noctis.render` span and Render Error Sink reports;
  - the `Renderer::register` call at `App::build`.

## Boundaries & Constraints

**Always:**
- **Layers (deps-check passes unchanged in rules).**
  - Rewrite `crates/noctis-image/Cargo.toml` `render` to `render = ["dep:takumi", "dep:takumi-core", "dep:noctis-ports", "dep:noctis-core", "dep:tracing", "dep:rayon", "dep:tokio", "dep:bytes"]`.
  - Add these optional deps: `noctis-core = { workspace = true, optional = true }`, `rayon = { workspace = true, optional = true }` (1.12.0, verified on crates.io; new in Cargo.lock), `tokio = { workspace = true, optional = true, features = ["sync"] }` and `bytes = { workspace = true, optional = true }`.
  - takumi becomes `features = ["raster-backend", "rayon"]`. The `rayon` feature makes takumi's internal parallelism run on whichever pool executes the job: ours.
  - `noctis-core` never depends on `noctis-image`, and `noctis-ui` gets no edge from `noctis-image`.
  - Without `image`, `cargo tree -p noctis -e normal` still shows no `noctis-image`, `takumi*` or `rayon`.
- **Ports (`crates/noctis-ports/src/render.rs`).** Add two `RenderError` variants (the enum is already `#[non_exhaustive]`):
  - `NoRenderer`, with ``#[error("no Renderer is installed: call `.theme(..)` or `.image(..)` (feature `image`) on the AppBuilder")]``. Only core returns it.
  - `Panicked { message: String }`, with `#[error("render panicked: {message}")]`.

  Nothing else in ports changes.
- **Render pool (`noctis-image/src/pool.rs`, `pub(crate)`).**
  - `struct RenderPool` implements `noctis_core::Resource` with `Config = PoolConfig { threads: usize, queue_capacity: usize }` (`Debug, Clone, Copy, PartialEq, Eq`).
  - `init` builds a `rayon::ThreadPoolBuilder` with `num_threads(threads)` and `thread_name(|i| format!("noctis-render-{i}"))`. It stores `Result<ThreadPool, String>`, because `init` is infallible. A build error is reported at install.
  - `try_submit(f) -> Result<oneshot::Receiver<..>, RenderError::QueueFull>` uses a CAS on an `AtomicUsize` that counts admitted jobs (queued plus running), checked `< queue_capacity`. On success it calls `pool.spawn`.
  - The spawned closure runs `f` under `catch_unwind(AssertUnwindSafe(..))`. A panic becomes `RenderError::Panicked { message }`, where `message` is the `&str`/`String` payload or `"<non-string panic>"`.
  - A drop guard decrements the counter in every case, panics included. The closure then sends the result; a dropped receiver is ignored.
  - Process scope: Apps on one `Runtime` share one pool. A different `PoolConfig` is `ResourceConflict`, and each `Runtime::for_test()` has its own pool.
- **Renderer (`noctis-image/src/renderer.rs`).**
  - `pub(crate) struct TakumiRenderer` holds:
    - `Arc<RenderPool>`;
    - `Arc<Theme>`;
    - the takumi `StyleSheet` built once from the Theme (`@theme { --<css_prefix><token>: <value>; … }` over every namespace's tokens, via `StyleSheet::parse_list`; add a `pub(crate)` token iterator to `Theme`);
    - `ClassCheck` (Deny when `runtime.is_test()`, else Warn);
    - a `takumi Fonts` (`Fonts::default()`; fonts are Story 10.7). If `Fonts` is not `Send + Sync`, use a `thread_local!` `Fonts` per pool thread instead, and say so in a comment;
    - `RwLock<HashMap<&'static str, (u32, ViewFn)>>`.
  - `register(&self, &[TemplateDescriptor])` replaces the map. A descriptor whose `view.downcast_ref::<ViewFn>()` is `None` gives ``Unsupported { id, why: "its view was not made by `#[derive(Template)]`" }``. A `ViewFn::is_animated()` one gives `Unsupported { id, why: "animated Templates are not rendered yet" }`. register returns the first such error; otherwise `Ok(())`.
  - `render(&self, job)` does all of the following **synchronously, before it returns the future**:
    1. It looks up `job.template_id`. A missing id, or a stored version other than `job.template_version`, is `UnknownTemplate { id, version }`.
    2. `job.animation.is_some()` is `Unsupported`.
    3. It calls `try_submit`. A rejection returns a ready `Err(QueueFull)` future.

    So admission order is call order, which keeps the queue tests deterministic.
  - The pool closure:
    1. `ViewFn::call(&data, Frame::STILL)`.
    2. `ClassCheck::check(&node, &theme)`. In Deny, an `Err` is `RenderError::failed(InvalidClasses)`.
    3. Lower the tree.
    4. `takumi::render` with `Viewport::new((w, h))`, the fonts and the stylesheet. A takumi error is `RenderError::failed(err)`.
    5. Check that the bitmap is `w × h`, else `failed`.
    6. `takumi::write_image` into a `Vec` with `Png → Png`, `Jpeg{q} → Jpeg{Quality::new(q)}`, `Webp{q} → WebP{Quality::new(q)}` and `WebpLossless → WebPLossless`.
    7. Return `RenderOutput { bytes, digest: blake3(bytes), format: job.format.into(), settings_used: EncodeSettings { size, quality: Some(q) for Jpeg/Webp else None, fps: None, frames: 1 } }`.

    The future awaits the oneshot. A closed channel is `failed("render pool dropped the job")`.
- **Lowering (`noctis-image/src/lower.rs`).**
  - An element becomes `takumi Node::container(children)`. A text node becomes `Node::text(..)`.
  - Non-blank `Classes` become `.with_tw(TailwindValues::from_str(..))`. An `Err` is `failed`.
  - A non-default `Style` becomes takumi style declarations.
  - Tags set no tag name and no preset (10.2: tags carry no default styles).
  - **Style wins over classes for the same property.** If takumi applies `tw` after `with_style`, lower `Style` into arbitrary-value classes appended after the author's classes instead. Either way, the test in the Matrix decides.
- **ImageConfig / ImagePlugin / AppBuilderImageExt (`noctis-image/src/plugin.rs`).**
  - `ImageConfig` (`Debug, Clone, PartialEq, Eq`, `Default` = `new()`) has the setters `#[must_use] threads(usize)` and `queue_capacity(usize)`. The defaults are `threads = max(1, available_parallelism / 2)` (1 when unknown) and `queue_capacity = 64`.
  - `ImagePlugin` (`Debug`, fields private, no public constructor) holds `ImageConfig` and `Theme`. Its `install`:
    1. Checks `threads ∈ 1..=256` and `queue_capacity ∈ 1..=4096`. Each miss is one `BuildViolation::plugin::<ImagePlugin>("render pool threads {n} is out of range: use 1..=256")` (similarly for `queue capacity`), and install stops.
    2. Calls `resource_or_init::<RenderPool>`. An `Err` is `report(BuildViolation::ResourceConflict(e))`. A failed pool build is a plugin violation carrying rayon's message.
    3. Parses the stylesheet. An error is a plugin violation: `the Theme is not valid CSS for the renderer: {err}`.
    4. Calls `app.renderer(Arc::new(TakumiRenderer{..}))`.
  - `pub trait AppBuilderImageExt: sealed::Sealed` is implemented only for `AppBuilder`. It has `fn image(&mut self, config: ImageConfig) -> &mut Self` and `fn theme(&mut self, theme: Theme) -> &mut Self`.
    - Each call updates the existing `plugin_mut::<ImagePlugin>()` or adds one with defaults (`ImageConfig::new()`, `Theme::empty()`).
    - The last call of each kind wins, order does not matter, and there is never a `DuplicatePlugin`.
- **`ViewFn`** (unstable) becomes `ViewFn { decode: fn(..), animated: bool }`. `erased_view::<T>()` sets `animated = T::ANIMATION.is_some()`. Add `pub fn is_animated(&self) -> bool`. The existing `call` is unchanged.
- **Core.**
  - Move `renderer` from `App` into `AppShared` (`Option<Arc<dyn Renderer>>`). `has_renderer()` keeps working. Add `pub fn renderer(&self) -> Option<&Arc<dyn Renderer>>` with a doctest.
  - In `build`, after the fold:
    - with a renderer, `renderer.register(registry.templates())`; an `Err(e)` pushes the new `BuildViolation::TemplateRejected { message: String }` (``#[error("the Renderer cannot render the registered Templates: {message}")]``, where `message` is `e` with its source chain joined by `": "`);
    - with no renderer but `!templates().is_empty()`, `tracing::warn!(target: "noctis::render", templates = n, "Templates are registered but no Renderer is installed; ctx.render will fail: call .theme(..) or .image(..)")`.

    Remove the 10.2 `expect(dead_code)` on `templates`.
  - New `crates/noctis-core/src/render.rs` holds `pub(crate) async fn render<T: Template>(inner: &CtxInner, template: T) -> HandlerResult<Attachment>`:
    1. `inner.app` is `None`, or it has no renderer: `Err(HandlerError::new(RenderError::NoRenderer))`. No metric and no report.
    2. `RenderJob::of(&template)`. A `ValueError` is `RenderError::failed`, reported.
    3. Inside `tracing::info_span!("noctis.render", app, template = T::ID)`: increment `noctis_render_queue_depth{app}`, await `renderer.render(job)`, then decrement.
    4. Ok: record `noctis_render_duration_seconds{app, template}`, then `Attachment::rendered(out.bytes, out.digest, out.format.extension())`. A `UiError` is `failed`, reported.
    5. Err: `QueueFull` increments `noctis_render_rejected_total{app, reason="queue_full"}`. `UnknownTemplate` and `Unsupported` increment it with `unknown_template` / `unsupported`.
    6. Every error except `QueueFull` and `NoRenderer` is reported to the `ErrorOrigin::Render` sink. `Panicked` is `ReportKind::Panic(&message)`; the others are `ReportKind::Error(&handler_error)`, with app, shard, guild_id, module, handler = the ctx path and extensions, like `worker.rs:506`.
    7. It returns that same `HandlerError`, which wraps the `RenderError`.
  - `ctx_methods!` adds `pub fn render<T: noctis_ports::Template>(&self, template: T) -> impl Future<Output = HandlerResult<noctis_ui::Attachment>> + Send + '_`, with docs and a doctest (`no_run` or compile-only). This covers `CommandCtx` (menus too), `ComponentCtx`, `ModalSubmitCtx` and `ExpiredCtx`.
  - `observe.rs`: add the three `METRICS` rows, with these labels:
    - `noctis_render_duration_seconds`: `app`, `template`;
    - `noctis_render_queue_depth`: `app`, this App's renders in flight;
    - `noctis_render_rejected_total`: `app`, `reason` (`queue_full`, `unknown_template`, `unsupported`).

    Update the doc table, the span list (`noctis.render{app, template}`), `catalog_static` (19 rows) and the label policy (`template` allowed).
- **Facade.**
  - `noctis::image` re-exports `ImageConfig`, `ImagePlugin`, `AppBuilderImageExt` and `RenderError`.
  - `noctis::prelude` adds `#[cfg(feature = "image")] pub use crate::image::AppBuilderImageExt;`.
  - The feature docs (lib.rs:561) and the `Cargo.toml` comment say that `image` enables the Renderer and the render pool, and that it needs a C compiler (libwebp, the WebP encoder). Drop the "arrives in Story 10.3" text.
- **Docs.** In the `docs/discord-rules.md` TW-01 How cell, the runtime check is shipped: drop `[planned, Story 10.3]`.

**Never:**
- No render on tokio workers, on `rayon`'s global pool, or on any pool the author can reach; no `spawn_blocking`.
- No admission weighting, per-job caps, skipping a dropped requester (10.5), size budget or animated encoding (10.4/10.6), fonts (10.7), assets (10.8), emoji (10.9) or cache (10.10). No `ctx.render_with`/`RenderOptions` (10.6).
- No `ctx.render` on `EventCtx` or `AutocompleteCtx`. No takumi type in a stable public item. No metric label carrying a user, guild or channel id, or free text.
- No edit to `.cargo/config.toml` or `_bmad/`. No debug builds.

## I/O & Edge-Case Matrix

Unless a row says otherwise, the setup is a `Runtime::for_test()` App with `.image(..)` and a theme of `--color-brand: #5865f2`. A Template `Card` (`size = (64, 32)`, `format = Png`, root `<div tw="flex w-full h-full bg-brand">{&self.name}</div>`) is registered. "R" is `app.renderer().unwrap()`.

| Scenario | Input / State | Expected | Error handling |
|---|---|---|---|
| threads bounds | `threads(0)`, `1`, `256`, `257` | violation, ok, ok, violation (message names the value and `1..=256`) | `BuildError` |
| queue bounds | `queue_capacity(0)`, `1`, `4096`, `4097` | violation, ok, ok, violation | `BuildError` |
| defaults | `ImageConfig::new()` | `== ImageConfig::default()`; threads ≥ 1; queue 64 | — |
| ext only theme | `.theme(t)` | one `ImagePlugin` with default config; `has_renderer()` | — |
| ext only image | `.image(c)` | `Theme::empty()`; `has_renderer()` | — |
| ext order/repeat | `.theme(a).image(c1).theme(b).image(c2)` | one plugin, theme `b`, config `c2`; no `DuplicatePlugin` | — |
| shared pool | two Apps, one Runtime, equal config | build ok; one `RenderPool` (`Arc::ptr_eq` of `resource_or_init` in a unit test) | — |
| pool conflict | second App on that Runtime with `threads(n+1)` | `BuildViolation::ResourceConflict` naming `RenderPool` | `BuildError` |
| test runtimes | two `Runtime::for_test()` Apps with different configs | both build | — |
| register ok | derived still Templates (0, 1, 2) | build ok | — |
| register foreign | hand-written `Template` whose `view()` is `ErasedView::new(())`, id `x` | `TemplateRejected` whose message contains `` `x` `` and `#[derive(Template)]` | `BuildError` |
| register animated | derived Template with `animation(..)` | `TemplateRejected` ("animated … not rendered yet") | `BuildError` |
| no renderer | Template registered, no `.image`/`.theme` | build ok + one `noctis::render` warning | — |
| render png | `R.render(RenderJob::of(&Card{name:"Ada"}))` | PNG signature; decoded 64×32; `format == Png`; `digest == blake3(bytes)`; `settings_used == {size 64×32, quality None, fps None, frames 1}` | — |
| theme color | same | pixels (0, 0) and (63, 31) are (0x58, 0x65, 0xF2, 0xFF) | — |
| deterministic | the same job twice | identical bytes | — |
| formats | `Jpeg{1}`, `Jpeg{100}`, `Webp{1}`, `Webp{100}`, `WebpLossless` | bytes start `FF D8` / `RIFF….WEBP`; ext `jpg`/`webp`; quality `Some(q)` for lossy, `None` for lossless | — |
| min size | zero-field Template `size = (1, 1)` | 1×1 PNG | — |
| over takumi canvas | `size = (10000, 10000)` | `Err(Failed)` with the takumi source; no panic; the pool still renders | — |
| style wins | `<div tw="w-full h-full bg-red-500" style={Style::new().bg_rgb(0x00FF00)}>` | pixel (0, 0) is (0, 0xFF, 0, 0xFF) | — |
| unicode text | name `"Ada 😀 ğüş"` | `Ok` | — |
| unknown id | job with `template_id = "nope"` | `Err(UnknownTemplate{id:"nope", ..})` | — |
| version skew | `Card` job with `template_version + 1` | `Err(UnknownTemplate{version})` | — |
| animated job | `Card` job with `animation = Some(..)` | `Err(Unsupported)` | — |
| bad data | `data = []`; valid data + 1 byte | `Err(Failed)`, source `ValueError::Malformed` / `Trailing` | — |
| view panics | view does `panic!("boom")`, then `panic_any(5u8)` | `Err(Panicked{message:"boom"})`, `"<non-string panic>"`; the next `Card` render is `Ok` with `threads(1)` | — |
| queue full | `threads(1)`, `queue_capacity(2)`; a Template whose view blocks on a test gate; calls a, b, then c | `c` resolves at once to `QueueFull`; after the gate opens, a and b are `Ok`; then d is `Ok` (counter released) | — |
| panic frees slot | `queue_capacity(1)`: a panicking render, then `Card` | second is `Ok` | — |
| pool thread | view records `thread::current().name()` and `tokio::runtime::Handle::try_current().is_err()` | name starts `noctis-render-`; no tokio handle | — |
| deny classes | `for_test` App; view with `Classes::dynamic("flx")` | `Err(Failed)` whose source downcasts to `InvalidClasses` naming `flx` | — |
| warn classes | App on `Runtime::global()` (default config), same view | `Ok` | — |
| dynamic ok | `Classes::dynamic("bg-brand")` on `for_test` | `Ok`, brand pixels | — |
| ctx no renderer | TestApp command calls `ctx.render(card)`, no plugin | `HandlerError` whose `as_error()` is `RenderError::NoRenderer`; no Render sink report | — |
| ctx fast | TestApp + fake Renderer answering at once; handler responds with `MediaGallery[MediaItem(r).alt("Card")]` | one type-4 callback with `files[0]` named `<16 hex of digest>.png` | — |
| ctx slow (FR-21) | fake Renderer sleeping 300 ms; `ack_deadline(50 ms)` | first callback exactly `{"type":5}`, then `PATCH …/messages/@original` carrying the file | — |
| ctx real | facade TestApp with `.theme(theme!())` + `Card`; same handler | the uploaded file is a valid 64×32 PNG | — |
| ctx panic → sink | facade TestApp, panicking Template, custom Render sink | sink gets `ReportKind::Panic("boom")` with `origin == Render` and `handler == "<command path>"`; the next `/card` works | — |
| ctx error → sink | fake Renderer returns `Failed` / `UnknownTemplate` | one `ReportKind::Error` report each | — |
| metrics ok | core unit test, fake Renderer `Ok`, local recorder | `noctis_render_duration_seconds{app, template="card"}` one sample; queue depth +1 then −1 (ends at 0) | — |
| metrics rejected | fake returns `QueueFull` / `UnknownTemplate` / `Unsupported` | `noctis_render_rejected_total{reason}` `queue_full` / `unknown_template` / `unsupported` = 1; QueueFull is not reported to the sink | — |
| span | core unit test with a capturing subscriber | one `noctis.render` span with fields `app`, `template` | — |
| default build | `cargo tree -p noctis -e normal` | no `noctis-image`, `takumi`, `rayon` | — |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs` -- `RenderError` (add 2 variants plus tests), `Renderer`, `RenderJob::of`, `RenderOutput`, `EncodeSettings`, `OutputFormat::extension`, `From<Format> for OutputFormat`.
- `crates/noctis-image/{Cargo.toml,src/lib.rs}` -- the render feature and deps; add the modules `pool`, `renderer`, `lower`, `plugin` (all `#[cfg(feature = "render")]`) and re-export `ImageConfig`, `ImagePlugin` and `AppBuilderImageExt`.
- `crates/noctis-image/src/template.rs:41-70` -- `ViewFn` / `erased_view`: add `animated`.
- `crates/noctis-image/src/check.rs` -- `ClassCheck::{new, check}`, `ClassCheckMode::{Warn, Deny}`; reuse as is.
- `crates/noctis-image/src/node/{mod.rs,style.rs,classes.rs}` -- the tree to lower. `Style` fields are `pub(crate)` (`width/height: Option<Dimension{Px,Percent}>`, `gap`, `bg`, `color`, `border_color`, `opacity`, `translate`, `rotate`).
- `crates/noctis-image/src/tw/theme.rs:88` -- `Theme { tokens: [BTreeMap<String,String>; 11], .. }`; add `pub(crate) fn custom_properties()` that yields `(css_prefix + token, value)`. `ThemeNamespace::css_prefix` is in `tw/generated.rs:13`.
- `crates/noctis-image/tests/tw_crosscheck.rs:221` -- a working takumi call (`RenderOptions::builder().viewport(Viewport::new((w,h))).fonts(&fonts).node(..).build()`, `Node::container`, `.with_tw(TailwindValues)`).
- takumi sources (read only): `~/.cargo/registry/src/*/takumi-raster-0.5.8/src/{render.rs:27-146,write.rs:35-84,398}` and `takumi-core-0.31.0/src/{style/selector.rs:1393 (StyleSheet::parse_list, `@theme` = `:root`),layout/node/mod.rs:308-410,error.rs:89}`. takumi caps the canvas at 64 MP.
- `crates/noctis-core/src/runtime.rs:39,185` -- `Resource`, `resource_or_init`; `is_test()`.
- `crates/noctis-core/src/app.rs` -- the `renderer` field (:140, :1272, :2213); `has_renderer` (:499); the `renderer(..)` setter (:2013); `build` (:2071; register after `Registry::fold`, :2148); `plugin`/`plugin_mut`/`report` (:1937-1990).
- `crates/noctis-core/src/handler.rs:203` -- `AppShared`; add `renderer`.
- `crates/noctis-core/src/registry.rs:126-133,280-287` -- `templates()`; remove the `expect(dead_code)`.
- `crates/noctis-core/src/error.rs:140,450` -- `BuildViolation` (add `TemplateRejected` with a doc example, like `Plugin`).
- `crates/noctis-core/src/command.rs:100` -- `CtxInner` (`app`, `path`, `module`, `shard`, `guild_id`, `extensions()`).
- `crates/noctis-core/src/responder/ergonomics.rs:377` -- `ctx_methods!`; add `render`.
- `crates/noctis-core/src/worker.rs:506-560,575` -- `ErrorReport` construction and `panic_message`.
- `crates/noctis-core/src/sink.rs` -- `ErrorOrigin::Render` exists, and `ErrorSinks::report`.
- `crates/noctis-core/src/observe.rs`, `observe/tests.rs:59,108` -- the metric catalog and the label policy.
- `crates/noctis-core/src/app/tests.rs:459` -- the existing `FakeRenderer`/`RendererPlugin`; reuse it for the core tests.
- `crates/noctis-testkit/tests/attachments.rs` -- TestApp and the multipart assertions pattern (`callback`, `files()`); `commands.rs:443-480` -- the `ack_deadline` type-5 pattern.
- `crates/noctis/src/lib.rs:548-565` (feature docs), `:1949` (`pub mod image`), `:2041` (prelude); `crates/noctis/Cargo.toml` (dev-deps).
- `crates/noctis/tests/template.rs` -- existing image tests and the derive patterns.
- `xtask/src/deps_check.rs:96-101` -- the `GATED` table already allows `noctis-image -> noctis-core` behind `render`.
- `examples/project-template/src/lib.rs:39` (`.theme(theme!())`), `src/modules/profile/handlers.rs:214` (`ctx.render(ProfileCard{..}).await?`, by value): both match the shipped signatures.
- Skill: `examples/project-template/.claude/skills/noctis/references/images.md` §1, §4 (:150), §8 (:243-260); `app-and-sharding.md:91,117,138,639-660,707,732,784-789`; `SKILL.md:23` (API hash).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ports/src/render.rs` -- add `NoRenderer` and `Panicked`, with Display tests.
- `crates/noctis-image/Cargo.toml` -- the deps and features from Always. Dev-deps: `png.workspace = true`, `tokio = { workspace = true, features = ["macros", "rt"] }` and `serde`.
- `crates/noctis-image/src/template.rs` -- `ViewFn.animated` and `is_animated`, with doctest updates.
- `crates/noctis-image/src/tw/theme.rs` -- the `pub(crate)` custom-property iterator, with a unit test.
- `crates/noctis-image/src/pool.rs` (+ tests) -- `RenderPool`, `PoolConfig`, `try_submit`, the panic capture and the counter guard. Unit tests: full queue, panic frees the slot, thread name, `Arc::ptr_eq` for an equal config, conflict for a different one.
- `crates/noctis-image/src/lower.rs` (+ tests) -- Node → takumi. Unit tests: element/text/classes/style lowering, and `TailwindValues` parsing of a literal.
- `crates/noctis-image/src/renderer.rs` (+ tests) -- `TakumiRenderer`: register, render, encode and settings. Unit tests for every renderer row of the Matrix that needs no App.
- `crates/noctis-image/src/plugin.rs` (+ tests) -- `ImageConfig`, `ImagePlugin`, `AppBuilderImageExt` (sealed), and the install checks.
- `crates/noctis-image/src/lib.rs` -- module wiring, re-exports and crate docs (no "follow in later stories" for the Renderer).
- `crates/noctis-core/src/{handler.rs,app.rs,error.rs,registry.rs}` -- `AppShared.renderer`, `App::renderer()`, the register call, the warning and `TemplateRejected`. App unit tests: register Ok/Err with the fake renderer, and the no-renderer warning path.
- `crates/noctis-core/src/render.rs` (+ `render/tests.rs`) and `responder/ergonomics.rs` -- `ctx.render`. Unit tests: the metrics, span, sink and NoRenderer rows (local `metrics` recorder, current-thread runtime, the fake Renderer).
- `crates/noctis-core/src/observe.rs`, `observe/tests.rs` -- the 3 rows, the docs and the policy.
- `crates/noctis-testkit/tests/render.rs` -- the ctx no renderer / fast / slow / error-sink rows with a hand-written `noctis_ports::Template` and a fake `Renderer` set by `.renderer(..)`.
- `crates/noctis/Cargo.toml` -- dev-deps `png.workspace = true` and `blake3.workspace = true`; the feature comment (C compiler).
- `crates/noctis/src/lib.rs` -- the re-exports, prelude and feature docs.
- `crates/noctis/tests/render.rs` (`#![cfg(feature = "image")]`) -- the plugin/config/pool/register/render/formats/pixels/style/deny/warn/queue/panic/thread rows with derived Templates. Use one `Runtime::for_test()` per test (except the warn row) and a `#[tokio::test(flavor = "multi_thread")]` runtime.
- `crates/noctis/tests/render_e2e.rs` (`#![cfg(all(feature = "image", feature = "testkit"))]`) -- the ctx real and ctx panic → sink rows through `noctis::testkit::TestApp`.
- `docs/discord-rules.md` -- the TW-01 cell.
- `examples/project-template/.claude/skills/noctis/references/images.md` -- rewrite §8 Rendering to the shipped API:
  - `ctx.render(t).await?` → `Attachment`;
  - the `RenderError` variants: `QueueFull` (show a "busy" notice; replace `Overloaded`), `NoRenderer`, `UnknownTemplate`, `Unsupported`, `Failed` and `Panicked`, found through `err.as_error().downcast_ref::<RenderError>()`;
  - `ImageConfig::new().threads(..).queue_capacity(..)` with its ranges and defaults;
  - the process-scoped pool and `ResourceConflict`;
  - `ImagePlugin`, `AppBuilderImageExt` and `App::renderer()`;
  - `TemplateRejected`, and the Render Error Sink, which gets failures and panics but not `QueueFull`;
  - the metrics and span;
  - Deny on `Runtime::for_test()`.

  Mark `render_with`/`RenderOptions`/`render_report`, the caps, the skip-if-gone behavior and the size budget `[planned, Story 10.4/10.5/10.6]`. Note that text draws no glyphs until fonts `[planned, Story 10.7]`. Fix §4 :150-153 (shipped) and §1 (C compiler).
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- drop `[planned]` from `.image(ImageConfig)` (:138); add `app.renderer()` (:91); move the `noctis.render` span and the render metrics from planned to shipped (:707, :732); make the plugin text (:789) match. In `SKILL.md`, mention `ctx.render` in §3.5 and update the API hash. Also mention `ViewFn::is_animated`, `RenderError::NoRenderer`/`Panicked` and `BuildViolation::TemplateRejected` (skill-check lists anything missing).
- `examples/project-template/AGENTS.md` -- in the Templates bullet (:64), add: render inside the handler with `ctx.render(card).await?`; a `RenderError::QueueFull` deserves a busy notice.
- `xtask/public-api.txt` -- run `cargo run --release -p xtask -- api-snapshot --bless`, and put the printed hash in `SKILL.md`.
- `.github/workflows-disabled/ci.yml` -- no new gate. Confirm that the jobs which run `--all-features` cover `render_e2e.rs`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the decisions are already logged by planning; append implementation-time decisions only.

**Acceptance Criteria:**
- Given a bot with `noctis = { features = ["image"] }` whose builder calls `.theme(noctis::image::theme!())` through the prelude's `AppBuilderImageExt`, and a mounted `Module::new("cards").template::<Card>()`, when `App::build` runs, then it succeeds, `app.has_renderer()` is true, and `Renderer::register` received `Card`'s descriptor (an animated or hand-written Template instead fails the build with `TemplateRejected`).
- Given a `TestApp` command handler that calls `ctx.render(Card { .. }).await?` and responds with that attachment in a Media Gallery, and a render slower than the ack deadline, when the command runs, then Discord first receives exactly `{"type":5}` and then the edit carrying a file named by the output digest. With the real ImagePlugin, the file is a PNG of the Template's size.
- Given the render queue is full, when another render is requested, then it fails at once with `RenderError::QueueFull`, `noctis_render_rejected_total{reason="queue_full"}` increments, and renders admitted earlier still complete.
- Given a Template whose view panics, when it renders, then the handler receives `RenderError::Panicked`, the Render Error Sink receives a `Panic` report, and the next render on the same pool succeeds.
- Given a `Runtime::for_test()` App, when a view uses an unknown `Classes::dynamic` class, then the render fails with `InvalidClasses`. On `Runtime::global()` the same view renders.
- Given default features, when `cargo tree -p noctis -e normal` runs, then no image crate appears, and `deps-check` passes with `noctis-image -> noctis-core` behind `render` only.
- Given the finished story, when `skill-check` and `api-snapshot` run, then both pass with the new hash.

## Spec Change Log

## Review Triage Log

### 2026-10-22 — Review pass
- verdicts: 37 findings — high 0, medium 2, low 19, false 15, maybe-false 1
- findings:
  - `[low]` `[reject]` blind: Failed/Panicked renders appear in no metric — the spec fixes the three rejection reasons; failures and panics reach the Render sink with app/handler; a new catalog row is more than a direct correction.
  - `[false]` `[reject]` blind: duration recorded before `Attachment::rendered` can fail — `Attachment::rendered` fails only on an invalid extension and `OutputFormat::extension()` returns fixed alphanumeric constants, so the wrap cannot fail.
  - `[false]` `[reject]` blind: non-QueueFull errors reported twice — deliberately settled in Design Notes (AD-29, documented in images.md §8).
  - `[low]` `[patch]` blind: `noctis_render_queue_depth` under-reports when a `ctx.render` future is dropped — real: `InFlight` follows the caller future while the admitted job keeps running, contradicting the catalog text "queued on or running in the render pool"; patch rewords the catalog/skill text (grouped with edge InFlight + edge claim + vgap other).
  - `[low]` `[reject]` blind: `register` stops at the first rejected Template — spec-mandated ("returns the first such error"); several hand-written/animated Templates in one build is rare and collecting adds logic.
  - `[false]` `[reject]` blind: pool created before stylesheet validation causes ResourceConflict on retry — a stylesheet failure is fixed by changing the Theme, not the ImageConfig, so a retry uses the same PoolConfig and reuses the pool; range errors already stop before the pool.
  - `[low]` `[reject]` blind: rayon build error stored permanently, per-submit Failed path unreachable — spec-mandated storage; install already rejects; no user harm named.
  - `[maybe-false]` `[defer]` blind: nested rayon work-stealing via takumi's rayon feature — takumi blur.rs uses par_iter in still renders, so stealing is plausible; whether stack depth can overflow is unverified (deferred, high unverified).
  - `[medium]` `[patch]` blind: `the_theme_stylesheet_declares_every_token` only asserts `is_ok()` — same root cause as vgap gap 1; patch adds a non-first-token pixel test and inspects the CSS text.
  - `[false]` `[reject]` blind: Warn mode not log-captured — warn-once per distinct string is covered by `check/tests.rs::tw_01_warn_once_per_distinct_string`; the renderer holds one `ClassCheck` and the render tests prove Warn renders.
  - `[low]` `[reject]` blind: global-Runtime test relies on being alone in its binary — no current flake; the invariant is documented; moving binaries is more than a direct fix.
  - `[low]` `[reject]` blind: `ctx.render` only tested via CommandCtx — the method is generated identically by `ctx_methods!` for every ctx; the deferred-update path is the existing auto-defer.
  - `[low]` `[reject]` blind: QueueFull not driven through `ctx.render` in TestApp — counter and sink exclusion covered by core unit test, real pool fast-fail by pool/facade tests (spec matrix rows).
  - `[false]` `[reject]` blind: `App::renderer()` exposes `&Arc` — the signature is mandated by the spec.
  - `[low]` `[reject]` blind: `ImagePlugin` has no constructor, `ImageConfig` no getters — no constructor is mandated; no caller needing getters named.
  - `[false]` `[reject]` blind: template `src/` lacks `.theme`/`ctx.render` — `examples/project-template/src/lib.rs:42` calls `.theme(noctis::image::theme!())` and `modules/profile/handlers.rs:215` calls `.render(ProfileCard{..})` with the `image` feature on.
  - `[low]` `[patch]` blind: SKILL.md says hand-written/animated Templates always fail the build — true only with the takumi Renderer; patch qualifies the sentence.
  - `[low]` `[reject]` blind: 64 MP limit and "megapixels" substring tie docs/tests to takumi — accurate for the pinned takumi; an upgrade fails the test loudly; a Noctis pre-check is Story 10.5 work.
  - `[low]` `[patch]` edge: default `threads` exceeds 256 on hosts with >513 CPUs — `ImageConfig::new` uses `(n/2).max(1)` with no upper clamp, so the default config fails `App::build`; patch clamps to `THREADS.end()`.
  - `[low]` `[reject]` edge: ImagePlugin silently replaces an explicit `.renderer(..)` — real but contradictory configuration is rare and a guard needs a new public AppBuilder accessor.
  - `[low]` `[reject]` edge: one Renderer Arc reused by a second AppBuilder replaces the first App's views — requires passing `app.renderer()` into another builder, an unsupported pattern; spec says register replaces the map.
  - `[low]` `[patch]` edge: `InFlight` guard scope — same root cause as the blind queue-depth finding; doc patch.
  - `[low]` `[reject]` edge: quality above 100 misreported in `settings_used` — reachable only through the unstable `erased_view` in a hand-written Template; derived Templates validate quality.
  - `[low]` `[reject]` edge: deep Node tree recursion can overflow the stack — needs a pathologically deep author-built tree; a depth guard adds branches.
  - `[false]` `[reject]` edge: ci.yml gains a step the spec said was unneeded — the `all-features` job only builds (`cargo build --all-features`), so the dedicated `render_e2e` step is required and keeps CI in sync per AGENTS.md.
  - `[low]` `[patch]` edge: queue-depth catalog claim — same root cause as the blind queue-depth finding; doc patch.
  - `[medium]` `[patch]` vgap: only the first Theme token is ever drawn — pre-verified gap; patch adds a `bg-surface` pixel test on the facade theme and a CSS-text assertion.
  - `[low]` `[patch]` vgap: Failed/Panicked not checked to stay out of rejected counter and duration histogram — pre-verified gap; patch extends the existing sink test with a local recorder.
  - `[low]` `[patch]` vgap other: queue-depth gauge vs documented meaning — same root cause as the blind queue-depth finding; doc patch.
  - `[false]` `[reject]` intent D1: slow render auto-defer shown only with a fake Renderer — the spec's "ctx slow" row deliberately uses a fake; the real path is covered by "ctx real".
  - `[false]` `[reject]` intent D2: full-queue metric shown only with a fake — the spec's "metrics rejected" row is a core unit test by design.
  - `[false]` `[reject]` intent D3: queue depth measures the App — settled in Design Notes ("Core owns observability").
  - `[false]` `[reject]` intent D4: queue cap counts running jobs — the spec defines admission as queued plus running.
  - `[false]` `[reject]` intent D5: typed error via downcast — settled in Design Notes ("Errors are HandlerResult").
  - `[false]` `[reject]` intent D6: double report / sink only via ctx.render — settled in Design Notes (core reports; the Renderer port knows no App).
  - `[false]` `[reject]` intent D7: default build checked by gates — the spec's Verification runs `cargo tree` and `deps-check`.
  - `[false]` `[reject]` intent D8: rendering narrower than the full story — the spec's Never list assigns these to Stories 10.4-10.10.

## Design Notes

- **Core owns observability.** The pool is shared across Apps, but every metric needs the `app` label, and the Renderer port knows no App. So core measures around `renderer.render(..)`. `noctis_render_queue_depth{app}` is therefore "this App's renders in flight (queued or running)". The Renderer stays App-agnostic, apart from its Theme.
- **Errors are `HandlerResult`**, like `ctx.respond`: `?` works in handlers, and the `RenderError` is found by `downcast_ref`. A failure that the handler propagates is reported twice, once by the Render sink and once by the handler's own origin. That is documented, and it matches AD-29 ("sinks receive every … error").
- **Eager admission.** `render()` admits and spawns before it returns the future, so call order is admission order (deterministic tests). A dropped future leaves the job running; skipping it is Story 10.5.
- **Still formats now.** takumi's `write_image` already encodes all four `Format`s, and an attachment needs bytes. Story 10.4 adds the size budget, animated formats and degradation.
- **Theme via `@theme`.** takumi's stylesheet parser reads `@theme { … }` as `:root`, so theme utilities (`bg-brand`) resolve their `var(--color-brand)` the same way the grammar checked them.

## Verification

**Commands:**
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace --all-features` and `cargo test --release --workspace` -- pass
- `cargo hack test --release --workspace --each-feature` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `cargo tree -p noctis -e normal | grep -E 'noctis-image|takumi|rayon'` -- no output
- `bash tools/story-loop/gates.sh` -- ALL-OK

## Auto Run Result

- Summary: added the takumi-backed `Renderer` (Node lowering, Theme stylesheet via `@theme`, still formats PNG/JPEG/WebP/AVIF), a bounded shared rayon render pool with queue-plus-running admission (fast-fail `QueueFull`), `ImagePlugin`/`ImageConfig`, `ctx.render(..)` on every ctx with auto-defer, and core-owned render metrics (`noctis_render_queue_depth`, rejected counter, duration histogram) plus the Render sink.
- Files changed: `crates/noctis-image/src/{renderer,pool,plugin,lower}.rs` (+tests) renderer, pool, plugin, Node lowering; `crates/noctis-image/src/tw/theme.rs` Theme stylesheet; `crates/noctis-ports/src/render.rs` Renderer port additions; `crates/noctis-core/src/observe.rs`, `render/tests.rs` metrics and sink; `crates/noctis/src/lib.rs`, `Cargo.toml`, `tests/render.rs`, `tests/render_e2e.rs` facade wiring and tests; `crates/noctis-testkit/tests/render.rs` TestApp coverage; `examples/project-template/` skill, references and AGENTS.md; `docs/discord-rules.md`; `xtask/public-api.txt`, `xtask/src/api_snapshot.rs` re-blessed snapshot.
- Review findings: 37 (high 0, medium 2, low 19, false 15, maybe-false 1). Patched 10 entries (medium 2 sharing one root cause, low 8): non-first Theme token pixel test plus CSS-text assertion; Failed/Panicked kept out of rejected counter and duration histogram; queue-depth gauge wording fixed (four grouped entries); default `threads` clamped to 256; SKILL.md sentence on hand-written/animated Templates qualified to the takumi Renderer. Deferred 1: nested rayon work-stealing through takumi's par_iter (stack depth unverified). Rejected 26, each with its recorded reason in the Review Triage Log (spec-mandated behaviour, settled Design Notes, or work owned by Stories 10.4-10.10).
- Follow-up review recommended: false. The two medium patches are one root cause (untested non-first Theme token) fixed by one direct test; no high was patched.
- Verification: `tools/story-loop/gates.sh` ALL-OK (fmt, clippy -D warnings, workspace tests, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91); `cargo tree -p noctis -e normal` shows no noctis-image/takumi/rayon in the default build. CI is paused by the maintainer.
- Residual risks: nested rayon work-stealing inside the pool (deferred); a dropped `ctx.render` future leaves its job running until Story 10.5; Failed/Panicked renders have no metric row; ImagePlugin silently replaces an explicit `.renderer(..)`.
