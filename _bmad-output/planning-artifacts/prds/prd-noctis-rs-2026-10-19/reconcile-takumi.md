# Reconciliation: PRD vs `research/takumi-image-generation.md`

**Date:** 2026-10-19. **PRD:** `prd.md` (draft). **Input:** `../../research/takumi-image-generation.md` (takumi 2.17.0, takumi-core 0.31.0).
Scope: capabilities and constraints the PRD misses or states wrongly. Pure implementation detail (pool types, cache crates, macro internals) is left to architecture.
Severity: **High** = PRD requirement is incomplete in a way that ships broken output; **Med** = missing requirement with user-visible effect; **Low** = clarification.

---

## K-1 (High) Tailwind v4 class names, silent drops, and dynamic class strings

**Research (§1, §4, §5.2):** takumi follows **Tailwind v4 names** (`bg-linear-to-r`, not v3 `bg-gradient-to-r`) and **silently drops unknown classes** (verified: the v3 gradient just disappears). Arbitrary values (`w-[67%]`, `animate-[slide_1s_...]`) are supported. Dynamic values formatted into class strings are parsed at runtime and still dropped silently when wrong. User-provided text must never reach class strings (class injection).
**PRD:** FR-32 makes unknown classes in **literal** strings compile errors. It says nothing about v4 naming, about dynamic class strings, or about injection.

**Suggested FR-32 consequences:**
> - Class names follow Tailwind v4. The compile error for an unknown class suggests the v4 replacement for known v3 names (e.g. `bg-gradient-to-r` → `bg-linear-to-r`).
> - Dynamic values (widths, colours, percentages) are set through typed style setters, not by formatting class strings. A runtime class string that contains an unknown class is logged as a warning once per distinct string (and is an error in the testing kit).
> - User-supplied text can only enter a Template as text content, never as a class string or markup.

## K-2 (High) Remote images: takumi never fetches URLs

**Research (§3.1, §5.3):** takumi resolves `src` only against a pre-decoded image map or `data:` URIs; **the host must download and decode every remote image before rendering**. Recommendations: bounded concurrency, timeouts, a size cap, a fallback image, cache keys that include the avatar hash and size, decode off the async runtime. Animated GIF/WebP avatars play along the animation timeline.
**PRD:** FR-35 says remote images "are fetched and cached through the AssetStore Adapter" with no rules for failure, size, hosts or timing.

**Suggested FR-35 consequences:**
> - A Template declares the remote images it uses; Noctis resolves all of them before the render starts.
> - Each fetch has a timeout and a byte cap. A failed, oversized or undecodable image is replaced by a configurable fallback image and logged; it never fails the whole render unless the Template marks it required.
> - Remote fetches are limited to an allow-list of hosts (default: Discord CDN), so Template data can't make the bot fetch arbitrary URLs.
> - Discord avatar and icon helpers key the cache by user/guild id, asset hash and size, so a changed avatar invalidates naturally; animated avatars animate in animated output.

## K-3 (High) Emoji and font fallback: missing glyphs render as tofu, missing fonts as nothing

**Research (§3.4, §4):** Rust takumi has **no bundled font**: with no font, text draws nothing (FR-35 covers this). With only a Latin font, emoji render as **tofu boxes**; a colour emoji font (COLR or bitmap, e.g. Twemoji COLR ~1 MB, Noto Color Emoji COLRv1 ~4 MB) must be registered. There is **no satori-style hook to load emoji as images from a CDN**, so Discord custom emoji (`<:name:id>`) are never rendered from text. CJK and other scripts also need registered fallback fonts. Supported inputs: TTF/OTF/TTC/WOFF/WOFF2, `unicode_range` subsets, variable fonts.
**PRD:** FR-35 makes the emoji font opt-in `[ASSUMPTION]` but doesn't state what happens without it, nor how custom emoji appear.

**Suggested FR-35 consequences:**
> - Without an emoji font, Noctis logs a warning at startup if any Template is registered, and the Usage Skill documents that emoji then render as boxes.
> - A bot author can register fallback fonts with a priority order and Unicode ranges (e.g. CJK) without changing Templates.
> - Discord custom emoji in Template text are rendered as images (fetched like K-2) or are a documented non-feature; they are never shown as raw `<:name:id>` text.

## K-4 (Med) Animation authoring is missing; only output formats are specified

**Research (§3.5, §5.6):** Two modes: CSS-driven (Tailwind `animate-spin|ping|pulse|bounce`, arbitrary `animate-[...]` with user `@keyframes`) and code-driven (one View per frame, e.g. a counting XP bar). Multiple scenes form a timeline and must share one viewport. Built-in animation names must **not** be shadowed (redefining `spin` produced a static result). FPS caps: **50 for GIF**, **90 for WebP/APNG**.
**PRD:** FR-33 lists animated formats only; no way to express an animation, no stylesheet/`@keyframes` input.

**Suggested new FR (in 4.5):**
> **FR-x: Animated Templates.** A bot author can produce an animation either from Tailwind/CSS animations in a Template (with custom keyframes supplied through a stylesheet) or by rendering one Template state per frame.
> - Requesting an fps above the format's cap (GIF 50, WebP/APNG 90) is a typed error before rendering.
> - Redefining a built-in animation name is a registration error.
> - All scenes of one animation share one size; a mismatch is a typed error.

## K-5 (Med) Resource caps per render job

**Research (§3.2, §5.6, §5.8):** takumi accepts viewports from 1 to 64 megapixels. Recommended Noctis caps: ≤ 4 MP per job, `max_frames` (90), `max_pixels_per_frame` (1 MP), `max_duration` (6 s). Rendering can't be interrupted mid-frame; code-driven animations can be cancelled between frames.
**PRD:** FR-34 bounds the pool and queue but not the size of a single job, so one large GIF can occupy the pool.

**Suggested FR-34 consequences:**
> - Each job is checked against configurable caps (pixels, frames, duration) before it is queued; a job over a cap fails with a typed error.
> - Animated jobs weigh more than still jobs in admission control, so a burst of animations can't starve stills.
> - When the requester gives up (e.g. the interaction expired), a queued job is dropped and a code-driven animation stops between frames.
> - Rendering parallelism inside takumi stays inside the render pool and never uses tokio worker threads or a global thread pool shared with the author.

## K-6 (Med) Render cache key must include more than "Template, data and size"

**Research (§5.5):** The key should cover Template id **and version**, data, size, **output format/settings**, versions of referenced **assets** (e.g. avatar hash), and the Noctis + takumi versions. Filenames should derive from the key (unique filename per render, which also fits Components V2 attachment advice).
**PRD:** FR-36 keys on "same Template, data and size".

**Suggested FR-36 text:**
> Identical render requests (same Template and Template version, data, size, output format and settings, and referenced asset versions) are served from the RenderCache Adapter. Upgrading Noctis or the renderer never serves a stale cached image. Concurrent identical requests run one render and share the result. The attachment filename is derived from the cache key.

## K-7 (Med) FR-33 size-budget loop needs the real budget and an order

**Research (§5.6):** Budget = interaction `attachment_size_limit` (default 20 MiB), but target far less (< 1-2 MB) for latency. Degrade order: lower fps → smaller viewport → lower lossy quality → still frame. GIF is the largest and slowest format (per-frame palettes, no delta frames, no dithering, banding on gradients); animated WebP was ~2.8× faster to encode and slightly smaller.
**Suggested FR-33 consequences:**
> - The size budget is the smaller of the interaction's upload limit and a configurable target (default ~2 MB).
> - Lowering follows a documented order and ends with a still frame as an optional last step; the settings actually used are reported with the image.

## K-8 (Med) Stylesheet and theme tokens

**Research (§3.2, §5.3):** A stylesheet can carry `@import "tailwindcss"` (Preflight), `@theme { ... }` design tokens and `@keyframes`. `from_html` drops `<style>` blocks; CSS goes through the render options.
**PRD:** No way to define brand colours/tokens shared across Templates.

**Suggested FR-32 consequence:**
> - A bot author can register a theme (Tailwind v4 `@theme` tokens and keyframes) once on the App, and Templates can use the theme's class names; compile-time validation accepts theme tokens declared in the build.

## K-9 (Low) Runtime HTML / JSON templates (decision needed)

**Research (§3.3, §5.2):** takumi can build a Template at runtime from HTML with `tw` attributes (~0.3 ms parse, depth-guarded) or from JSON. Useful for designer-editable or per-guild custom cards, but class validation then happens only at runtime.
**PRD:** Templates are Rust-only; §7 doesn't exclude runtime templates.
**Suggested:** either add an opt-in feature ("A bot author can load a Template from HTML or JSON at runtime; unknown classes are reported as warnings; input depth is bounded") or list it in §8.2 as deferred.

## K-10 (Low) Versioning and build constraints

**Research (§2, §5.1, §5.8):** takumi releases weekly (2.15.2 → 2.17.0 in a week): pin an exact minor, don't expose takumi types in Noctis's stable API (only through an explicit unstable escape hatch), and snapshot-test images on every bump. The native WebP encoder (`libwebp-sys`) needs a **C compiler** at build time; the image feature adds ~100 crates, so it should be off by default. MSRV 1.91 (NFR-7 already reflects this).
**Suggested §6 / NFR-7 / NFR-8 additions:**
> - Renderer types are Noctis's own; takumi types are reachable only through a clearly unstable module and are not covered by semver.
> - CI runs pixel snapshot tests (with a tolerance) for the Project Template's Templates; they must pass before a renderer upgrade is merged.
> - The image feature is off by default; the docs state that it needs a C compiler.

## K-11 (Low) Global renderer memory belongs in the memory budget

**Research (§3.6):** Process-global caches: glyph cache (8 MiB default, tunable only via takumi's unstable API, must be set before the first render), Tailwind parse cache (1 MiB), resource cache (64 MiB default per handle).
**Suggested NFR-1 note:** Renderer caches have documented, configurable byte budgets and are reported separately from per-guild memory.

## K-12 (Low) Alt text for rendered images

**Research (§5.7):** Alt text (≤ 1024) is the only accessible text for a generated image.
**Suggested FR-30 consequence:** Attaching a rendered image to a Thumbnail or Media Gallery requires alt text (strict mode) or warns (lenient mode).

## Confirmed (no change needed)

- NFR-3 card target (p50 ≤ 10 ms, p99 ≤ 50 ms) is consistent with the measured ~2 ms render + ~1.5 ms PNG warm on 4 cores; WebP still encode is slower (6-10 ms at q85), so the benchmark should state the output format.
- FR-35 no-font startup failure matches the verified "no font = no text" behaviour.
- FR-37 Renderer seam and §7 "no remote render service in v1" match the research's open question 3.
