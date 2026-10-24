# Epic 10 Context: Generated images and GIFs

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author write Tailwind-styled image Templates in Rust, validated at compile time with no Node.js or Tailwind CLI, and get cached PNG, JPEG, WebP, animated WebP, GIF or APNG output rendered on a bounded, dedicated pool, then embed it in Views as Thumbnail or Media Gallery images. Everything sits behind the `image` facade feature, off by default, so bots without images compile no image dependency.

## Stories

- Story 10.1: Strict Tailwind v4 grammar and takumi cross-check
- Story 10.2: Templates with compile-time class validation and a Theme
- Story 10.3: Renderer and bounded render pool
- Story 10.4: Output formats and size budget
- Story 10.5: Per-job caps and admission control
- Story 10.6: Animated Templates
- Story 10.7: Fonts and static assets
- Story 10.8: Remote image fetching through the AssetFetcher
- Story 10.9: Emoji rendering
- Story 10.10: Render cache and single-flight deduplication
- Story 10.11: Pixel snapshots and the reference card benchmark

## Requirements & Constraints

- Unknown Tailwind classes in literal class strings are compile errors; a known v3 name suggests its v4 replacement (e.g. `bg-gradient-to-r` -> `bg-linear-to-r`). Runtime class strings with unknown classes warn once per distinct string, and are an error on `Runtime::for_test()` Apps (tests fail).
- User data enters a Template only as text content, never as class strings or markup; dynamic values (widths, colors, percentages) use typed style setters.
- Rendering never blocks the gateway or tokio workers; it runs on a dedicated bounded pool, a full queue fails fast with a typed error, and a render inside a handler is covered by automatic deferral.
- Per-job caps checked before enqueue (defaults: <= 4 MP per job, <= 1 MP per frame, <= 90 frames, <= 6 s); fps caps GIF 50, WebP/APNG 90. Admission is weighted by frame count so animations cannot starve stills. Jobs whose requester is gone are skipped; per-frame animations stop between frames.
- Size budget = min(Upload Budget, configurable target ~2 MB). Over budget, degrade fps, then size, then quality, then optionally a still frame; typed error if it still does not fit. Report `settings_used`.
- No registered font and `default-font` off is a startup error; fallback fonts with priority and Unicode ranges work without Template changes. Emoji font (Twemoji, `image-emoji`) is opt-in with a startup warning when absent; custom emoji `<:name:id>` render as fetched images, never raw text. Licenses (default font, Twemoji CC-BY-4.0) go in `NOTICE` and `deny.toml`.
- Remote images: declared by the Template, resolved before render, host allow-list (default Discord CDN), timeout, byte cap, bounded concurrency, off the async runtime, cached via `AssetStore`; failures use a configurable fallback unless the image is marked required. Avatar/icon cache keys include id, asset hash and size.
- Identical requests are served from cache; N concurrent identical requests render exactly once (process-local single-flight); upgrades never serve stale images.
- Reference card bench (~934x282 PNG, warm caches) targets p50 <= 10 ms, p99 <= 50 ms with saturated pool; report-only for now.
- Every public API change updates the Usage Skill, project template, API snapshot and passes `skill-check`.

## Technical Decisions

- Pin `takumi = "=2.17.0"` and `takumi-core = "=0.31.0"` exactly in `[workspace.dependencies]`; both bump together. `noctis-image` has empty default features; takumi is behind its `render` feature.
- `noctis_image::tw` is takumi-free and builds with `render` off. `xtask tw-grammar` locates the pinned `takumi-core` via cargo metadata, `syn`-parses `src/style/tw/map.rs` (`FIXED_PROPERTIES`, `PREFIX_PARSERS`, `property_parsers!`, `VAR_TARGETS`; variants per `split_variant`), maps each value parser type through a closed suffix-grammar table in xtask (unknown type or changed layout fails naming it), and writes committed `crates/noctis-image/src/tw/generated.rs` plus theme-token hooks and the v3->v4 rename table; regeneration must be diff-free.
- `tw-crosscheck` CI job: oracle is `takumi_core`'s `TailwindValues::from_str` (recognized = non-empty value list) over samples plus a near-miss corpus; plus smoke renders.
- The grammar is a strict subset of takumi: it rejects non-ASCII, case variants, `inf`/`NaN`/exponent numbers, opacity > 100, negative theme tokens, `url(`/`calc(` arbitrary values, and narrows gradient/shadow var colors to the palette and animations to built-ins; such corpus lines carry `narrowed=<reason>`. Numeric suffixes are never theme tokens; `token/opacity` needs an integer 0..=100.
- `tw-grammar --check` fingerprints takumi's matcher functions, value-parser impls and macro bodies, so a takumi bump fails until re-audited. `Cargo.lock` holds `takumi-raster` 0.5.8 to avoid a second `takumi-core`; re-check on bump. deps-check rule `takumi-gated` keeps takumi optional behind `render` (raster backend only so far).
- Theme: crate's `noctis-theme.css`, parsed with `cssparser` (only custom properties and `@keyframes`); every file a proc macro reads is emitted as `include_bytes!` so edits rebuild. Redefining a built-in animation name is a registration error.
- `#[derive(Template)]` with `#[template(id, version, size, format)]` and optional `animation(..)` implements `noctis_ports::Template` (`ID`, `VERSION`, `SIZE`, `FORMAT`, `ANIMATION`, `view()` -> `ErasedView` with a postcard decoder calling `TemplateView::view(&self, Frame)`); requires `Serialize + Deserialize`; rejects `HashMap` fields. Duplicate Template ids fail `App::build` (`BuildViolation::DuplicateTemplate`); `Module::template::<T>()` records descriptors, handed to `Renderer::register` in 10.3.
- Runtime class strings exist only as `Classes::dynamic(..)`; `tw={expr}` takes `Classes`, never `&str`/`String`, so user text reaches a Template only as text nodes. Runtime check `noctis_image::ClassCheck` (Warn/Deny, warn-once keyed by blake3, max 1024 remembered); the Renderer uses Deny when `Runtime::is_test()`.
- Gradient `from-`/`to-` stops accept palette colors only.
- CSS animations (10.6) widen the strict grammar by hand in `tw/mod.rs` (generated tables untouched): `animate-<token>` -> `Utility::Animation { token }` (needs `--animate-<token>`); `animate-[…]` parsed by takumi-free `tw::animation` (`_`-separated: one keyframes name `[a-z]([a-z0-9-]*[a-z0-9])?`, duration > 0, optional delay, timing keyword, integer/`infinite` count, direction, fill, play state; no functions or commas), `Builtin` for spin/ping/pulse/bounce else `Utility::Keyframes { name }`. Cross-check accepts them when takumi shows `token`/`name: Some(..)`.
- TW-02 tightens: `--animate-*` values must parse with the same grammar (whitespace-separated; `InvalidAnimation`), must reference a file `@keyframes` or built-in, checked after the whole file (`UnknownKeyframes`, with 1-based declaration location); `@keyframes` names match the name regex, <= 64 bytes, not a CSS keyword (`InvalidKeyframesName`). Check order: token, built-in, empty, value, duplicate. The Renderer stylesheet declares `--animate-*` in `@theme`, `@keyframes` after it.
- Fps cap (GIF 50, WebP/APNG 90; `AnimatedFormat::max_fps`, not configurable) is `RenderError::OverCap { cap: RenderCap::Fps }` via `AnimationSpec::check_fps`, checked by core at `ctx.render*` job creation and again in the Renderer before other caps. Core adds `RenderOptions { animation }`, `ctx.render_with`, `ctx.render_report(t, options) -> RenderReport { attachment, format, settings_used }`, re-exported from `noctis::image`.
- All frames share one size: `RenderError::SizeMismatch { id, frame, expected, declared }` checked per frame of animated jobs against root `Style` px width/height and unvariant root `w-/h-/size-[Npx]` classes. Project template keeps its still profile card (CSS animation on a still renders t = 0); the skill carries animated examples.
- Core never depends on `noctis-image`: core postcard-encodes into `RenderJob { template_id, template_version, data, size, format, animation }` and calls `Arc<dyn Renderer>` (port in `noctis-ports`). `noctis-image -> noctis-core` edge exists only behind `render`; `deps-check` proves both.
- Plugin seam: `.image(ImageConfig)` / `.theme(Theme)` via `noctis::image::AppBuilderImageExt`; `ImagePlugin::install` gets the process-scoped rayon pool and font registry from the Runtime (`resource_or_init`) and installs a per-App Renderer via `AppBuilder::renderer(..)`. Conflicting font registries on one Runtime are a `BuildError`; `for_test()` Runtimes get their own.
- Render pool: process-scoped `noctis_image::RenderPool` Resource (rayon, threads `noctis-render-<i>`, `threads` 1..=256 default max(1, cores/2), `queue_capacity` 1..=4096 default 64); admission counts queued+running jobs synchronously at `Renderer::render` call; takumi's `rayon` feature runs inside this pool. Panics are caught on the pool thread -> `RenderError::Panicked { message }`; the pool survives.
- Core owns render observability: `noctis_render_duration_seconds{app, template}`, `noctis_render_queue_depth{app}`, `noctis_render_rejected_total{app, reason = queue_full|unknown_template|unsupported}` in span `noctis.render{app, template}`; renderer failures and panics (not `QueueFull`/`NoRenderer`) go to the `Render` Error Sink.
- `ctx.render(t)` takes the Template by value, returns `HandlerResult<Attachment>`, exists on Command/Component/ModalSubmit/Expired ctx (not Event/Autocomplete); without a Renderer -> `RenderError::NoRenderer`. `App::build` hands templates to `Renderer::register`; rejection -> `BuildViolation::TemplateRejected`. `App::renderer()` exposes the Renderer. `AppBuilderImageExt` is sealed.
- Attachments: `Attachment::rendered(bytes, digest, ext)` names by cache-key digest and marks origin `Rendered`.
- Cache key: `blake3(KEY_SCHEMA, NOCTIS_VERSION, TAKUMI_VERSION, template_id, template_version, theme_digest, postcard(data), sorted asset (id, version) pairs, size, format, animation, encode_settings)`.
- Animation: `AnimationSpec { duration, fps, format }` from Template `ANIMATION` or `ctx.render_with(t, RenderOptions::new().animation(..))`; view called per frame with `Frame { index, count }`; a root size mismatch is a typed error. Default animated format is animated WebP; GIF/APNG behind `image-gif`.
- Pool queued Tasks each own their admission `Release` guard, so skipped, panicked or never-run jobs free their units; a job whose result receiver is dropped before start is skipped (checked via a type-erased closure over `oneshot::Sender::is_closed`).
- Size budget travels in the job (`RenderJob::upload_budget`, filled by `ctx.render` from the interaction's attachment limit); over budget -> `RenderError::OverBudget { budget, smallest }`. The degradation ladder is fixed and cumulative (fps halving, downscale of rendered frames never re-layout, lossy quality steps, optional still frame 0 via `ImageConfig::still_fallback`), at most 9 encodes. `animation(..)` format is optional, default animated WebP quality 80; GIF/APNG gated at compile time by an `on_unimplemented` trait plus render-time `Unsupported`.
- Fonts/static images (10.7): bundled Inter makes `noctis-image` `(MIT OR Apache-2.0) AND OFL-1.1` (crate-scoped `deny.toml` exception, `NOTICE` with sha256). Missing font -> `RenderError::NoFont` at `Renderer::register` (IMG-01). Static images are per App via `ImageConfig::image(StaticImage)` (id grammar/length checked in `const fn StaticImage::new`, PNG/JPEG/WebP by magic bytes, <= 16 MP, decoded once; unregistered `<img>` -> `RenderError::UnknownImage`, IMG-02). `view!` gains `<img src={HANDLE} .. />` and `img(src, classes)`; `<svg>` stays rejected. `TakumiRenderer::new` takes `RendererConfig`.
- `AssetFetcher` (10.8) is the only non-Discord HTTP: `hyper-util` legacy `Client` over `hyper-rustls` (pinned, ring + platform verifier, no new crate), redirects never followed; deps-check rule `asset-http` bans other clients and confines it to `noctis-image/src/remote/fetch.rs`, `render`-gated. takumi never fetches. Allow-list of origins (default Discord CDN/media; http only for loopback); `RemoteConfig::discord_cdn(origin)` override; per-App `RemoteConfig` via `ImageConfig::remote(..)` with bounded timeout, max_bytes, concurrency, max_per_job (`RenderCap::RemoteImages`), max_pixels, cache_ttl, fallback image.
- Core hands the namespaced `AssetStore` to the Renderer via provided port method `Renderer::bind(RendererPorts)` (`#[non_exhaustive]`, room for 10.10 render cache) after `Ports::resolve`. `#[asset]` fields (`RemoteImage`/`Option`/`Vec`, IMG-03 `on_unimplemented`) are resolved before pool admission; new errors `RenderError::RemoteImage`, `UndeclaredImage`. `RemoteImage` stores u64 ids and `([u8;16], bool)` hashes (postcard-safe); CDN sizes power-of-two 16..=4096; animated avatars only for animated jobs. Emoji constructors are 10.9.
- Emoji (10.9): `image-emoji` bundles a COLRv0 Twemoji woff2 as a last-resort family after Inter (never an author font, not counted for IMG-01); crate license becomes `(MIT OR Apache-2.0) AND OFL-1.1 AND CC-BY-4.0`; no config type; feature off warns `[IMG-04]` once per App build when Templates exist. Custom emoji tokens are auto-declared by scanning job postcard data, fetched via `RemoteImage::custom_emoji(id, animated)` (CDN `emojis/<id>.webp?size=128`) under `RemoteConfig::max_emoji` (0..=128, default 32); overflow or fetch failure draws the fallback, so user text never fails a render. A view-built token not in data is `UndeclaredImage`; a literal custom emoji in `view!` text is an `[IMG-04]` compile error.
- Testkit pixel snapshots go through the App's `Renderer` port (no `testkit -> image` edge), decode with `png` 0.18. Bench lives in `noctis-image/benches` against `budgets.toml`.
- The `image` feature docs state the C compiler requirement for the WebP encoder.

## Cross-Story Dependencies

- 10.1 grammar is a prerequisite for 10.2 macros; 10.2 Template contract builds on the Story 3.1 `Template`/`Renderer` ports.
- 10.3 pool and Renderer underpin 10.4-10.10; 10.5 caps sit in 10.3's admission path; 10.6 needs 10.2 Theme keyframes and 10.4 animated encoders.
- 10.9 custom emoji depend on 10.8 AssetFetcher; 10.8 and 10.10 use the Epic 3 `AssetStore`/`RenderCache` ports.
- 10.3 relies on Epic 6 auto-deferral (FR-21) and Epic 9 attachment/Upload Budget handling; 10.11 uses the Epic 8 testkit `TestApp`.
- Reference card and profile card Template are reconciled with `examples/project-template/`; Project Template snapshots join the gate in Story 14.2.
