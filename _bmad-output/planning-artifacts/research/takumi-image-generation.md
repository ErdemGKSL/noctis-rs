# takumi image and GIF generation for vesper

**Status:** research input for `vesper` (Rust Discord bot framework on twilight + tokio; primary UI = Components V2 + generated images).
**Date:** 2026-10-19. **Verified against:** `takumi 2.17.0` (published 2026-10-19), `takumi-core 0.31.0`, `takumi-raster 0.5.8`, `takumi-html 0.5.0`, `taffy 0.15.0`, rustc 1.97.0, Linux x86_64, 4 cores.

Evidence tags: **[S]** read in the crate source in `~/.cargo/registry`, **[R]** compiled and run in a scratch project, **[M]** crates.io metadata, **[I]** inference or recommendation.

Scratch project: `/tmp/claude-0/-home-user/dd8af539-19a8-5b5d-b835-27c3a80d0c45/scratchpad/research-takumi/` (`src/main.rs`, `examples/spin.rs`; outputs `card_builder.png`, `card_html.png`, `card_json.png`, `anim.gif`, `anim.webp`, `anim.png`). This is temporary and will not outlive the session; the important code is quoted below.

---

## 1. TL;DR

- takumi is a pure-Rust, browser-free HTML/CSS renderer: taffy 0.15 for flex/grid/block layout, parley-derived text layout + harfrust shaping, tiny-skia rasterizing. Crate `takumi` is a thin facade over `takumi-core`, `takumi-raster`, `takumi-html` and optional `takumi-svg`. **[S][M]**
- **Tailwind is native**: every node has a `tw` field (`Node::with_tw(TailwindValues)`), HTML input reads a `tw="..."` attribute, and the JSON node format has a `"tw"` key. The utility set follows **Tailwind v4 names** (`bg-linear-to-r`, not v3 `bg-gradient-to-r`). Unknown classes are **silently dropped**. A stylesheet can carry `@import "tailwindcss"` (Preflight), `@theme { ... }` and `@keyframes`. **[S][R]**
- **Animation is native**: CSS `animation` / `@keyframes` and Tailwind `animate-*` (spin, ping, pulse, bounce built in; arbitrary `animate-[name_1s_...]`) are evaluated at a `time_ms`. `write_animation` renders a timeline at a fixed fps and streams it into **GIF, animated WebP or APNG**. Animated GIF/WebP/APNG *inputs* (for example Discord avatars) also play along the timeline. **[S][R]**
- Static outputs: PNG, JPEG, WebP lossy, WebP lossless, ICO. Optional vector SVG output (`svg-backend`). **[S]**
- **Thread-safe in practice**: `Fonts`, `Node`, `ImageSource`, `StyleSheet` are `Send + Sync`; `RenderOptions<'g>` is `Send`. One `&Fonts` was shared by 4 threads rendering 64 cards concurrently. Glyph, Tailwind-parse and intercept caches are process-global and sharded. **[R][S]**
- Measured (release build, 4 cores): 934x282 rank card **~1.7-2 ms render + ~1.5 ms PNG encode** warm; 64 cards in 84 ms on 4 threads (~760 cards/s). 600x300, 2 s @ 25 fps (50 frames): GIF 138 KB in 186 ms, WebP 132 KB in 67 ms, APNG 189 KB in 82 ms. **[R]**
- Gotchas: no bundled default font in Rust (text renders as nothing until you register one); emoji need a registered color font (COLR/bitmap glyphs supported); GIF encoder uses per-frame NeuQuant palettes on full frames (no delta frames, no dithering), so GIF is the biggest and slowest format. **[R][S]**
- There is no JSX/`html!` macro crate for takumi. Options: the `Node` builder API, `from_html` (runtime HTML parse, ~0.25-0.35 ms per card), or serde JSON. **vesper should ship its own `view!` macro that expands to `Node` builder calls at compile time.** **[S][M][I]**

---

## 2. Crates, versions, feature flags

| Crate | Version (2026-10-19) | Role |
|---|---|---|
| `takumi` | **2.17.0** (MSRV 1.91, edition 2024, MIT OR Apache-2.0) | Facade: `prelude`, `render`, `render_animation`, `write_image`, `write_animation`, `write_animated_{gif,png,webp}`, `measure`, `from_html` |
| `takumi-core` | 0.31.0 | Node tree, CSS/Tailwind style system, layout (taffy 0.15), fonts, image resources, keyframes |
| `takumi-raster` | 0.5.8 | tiny-skia painter and encoders (png 0.18 + zlib-rs, jpeg-encoder, gif 0.14, **libwebp-sys** on native, image-webp on wasm) |
| `takumi-html` | 0.5.0 | `from_html`: html5ever → `Node`, reads `tw`, `style`, `class`, `id`, `dir`, `lang` |
| `takumi-svg` | 0.3.30 | Vector SVG output backend |

Release cadence is very high (2.15.2 → 2.17.0 within a week; hundreds of versions overall). **Pin an exact minor and upgrade deliberately.** [M][I]

Feature flags of `takumi` 2.17.0 [S]:

| Flag | Default | Meaning |
|---|---|---|
| `raster-backend` | yes | the raster renderer and all encoders |
| `woff`, `woff2` | yes | WOFF/WOFF2 font input |
| `svg-source` | yes | SVG `<img>` sources (resvg/usvg vendored in core) |
| `image-decoding` | yes | = `png` + `jpeg` + `webp` + `gif` input decoders |
| `rayon` | yes | parallelism inside the raster backend (frame batches, GIF quantization, libwebp, segmented zlib, blur) |
| `from-html` | no | `takumi-html` (`from_html`, `FromHtml`, `FromHtmlOptions`, `StylePresets`) |
| `svg-backend` | no | `render_svg` |
| `gif`, `webp`, `png`, `jpeg` | via `image-decoding` | individual decoders (a source with no decoder still lays out from its header size) |
| `unstable` | no | `takumi::unstable::{base, raster, html, svg}` full backend crates, no semver guarantee (needed for `ResourceCache`, `set_glyph_cache_max_bytes`) |

Build notes [R]: 161 crates in the lockfile; `libwebp-sys` compiles C via `cc`, so build images need a C compiler. First clean release build takes a few minutes; incremental is instant.

---

## 3. Rust API (as of 2.17.0)

### 3.1 Node tree [S]

```rust
pub struct Node { metadata: NodeMetadata /* tag, class, id, attrs, dir, lang, preset, style, tw */, pub kind: NodeKind }
#[non_exhaustive] pub enum NodeKind { Container { children: Vec<Node> }, Image(ImageData), Text(TextData) }

Node::container(children) / Node::image(data) / Node::text(s)
  .with_tw(TailwindValues) .with_style(Style) .with_class_name(..) .with_tag_name(..)
  .with_id(..) .with_attributes(..) .with_dir(..) .with_lang(..) .with_preset(Style)
```

- `TailwindValues: FromStr` (infallible in practice); parses are cached in a process-global 1 MiB `quick_cache` keyed by the class string, so repeated class strings are cheap.
- `Style::default().with(StyleDeclaration::font_size(...))` gives typed CSS; `Style::from(StyleDeclarationBlock::parse_loosy(css))` takes inline CSS text.
- `Node` implements `Deserialize` (camelCase keys: `type`, `children`, `text`, `src`, `width`, `height`, `tagName`, `className`, `id`, `attributes`, `preset`, `style`, `tw`, `dir`, `lang`), with a max depth guard (`MAXIMUM_DOM_TREE_DEPTH`). This is the same JSON shape the JS package uses. [S][R]
- Images: `Node::image(ImageData::from((Arc<str>::from("avatar"), Some(128.0), Some(128.0))))`. `src` resolves against `RenderOptions::images: HashMap<Arc<str>, ImageSource>` (pre-decoded), or a `data:` URI. `ImageSource::from_bytes(&[u8])` accepts PNG/JPEG/WebP/GIF/SVG; animated GIF/WebP/APNG become `ImageSource::Animated` and advance with `time_ms`. **takumi does not fetch URLs**; the host must download and register images. [S]

### 3.2 Render entry points [S]

```rust
let opts = RenderOptions::builder()          // typed-builder
    .viewport(Viewport::new((934, 282)))     // 1 to 64 megapixels, else Error::InvalidViewport
    .node(node)
    .fonts(&fonts)                           // &'g Fonts
    .images(images)                          // HashMap<Arc<str>, ImageSource>
    .stylesheet(Arc::new(StyleSheet::parse(css)?)) // optional: @keyframes, @theme, selectors, @import "tailwindcss"
    .time_ms(0)                              // animation clock for still renders
    // also: font_families, lang, dithering, draw_debug_border, resource_cache
    .build();
let bitmap: Bitmap = takumi::render(opts)?;  // RGBA8
takumi::write_image(&bitmap, &mut Vec::new(), OutputFormat::Png)?;
// OutputFormat::{Png, Jpeg{quality}, WebP{quality}, WebPLossless, Ico}; content_type() gives MIME
takumi::measure(opts)?;                      // layout only, returns MeasuredNode
```

PNG encoding is smart: it samples rows to choose filter/level and writes an **8-bit palette PNG when the image has at most 256 colors** (all the test cards came out as colormap PNGs). [S][R]

### 3.3 HTML input with Tailwind [S][R]

```rust
use takumi::{from_html, prelude::*};
let node = from_html(r#"<div tw="flex flex-col items-center bg-linear-to-br from-violet-600 to-indigo-900">
  <img src="avatar" tw="w-24 h-24 rounded-full" /><h1 tw="text-5xl font-bold m-0">Welcome</h1></div>"#,
  FromHtmlOptions::default())?;   // builder: presets (Chromium UA styles by default), tailwind_property ("tw"), max_depth
```

`<style>` blocks are dropped by `from_html`; pass CSS through `RenderOptions::stylesheet` instead.

### 3.4 Fonts [S][R]

```rust
let mut fonts = Fonts::default();                    // EMPTY in Rust: no bundled fallback (JS has one)
fonts.register(FontResource::new(FontSource::from_static(bytes)))?; // TTF/OTF/TTC/WOFF/WOFF2
// FontSource::from_shared(Arc<dyn AsRef<[u8]> + Send + Sync>) for runtime-loaded bytes
// FontResource: .override_info(FontOverride{family_name,..}) .generic_family(..) .unicode_range(..) .last_resort()
fonts.unregister_family("Inter");                    // 2.17.0
```

- Verified: with no font registered, text draws **nothing** (0 dark pixels). vesper must ship or require a default font. [R]
- `unicode_range` subsets (Google Fonts style) supported since 2.16. Variable fonts, `opsz`, synthetic bold/italic/small-caps supported. [S]
- Emoji: color glyphs from COLR layers and embedded bitmap glyphs are drawn. With only Inter registered, 🎉 rendered as tofu. Register a color emoji font (e.g. Noto Color Emoji COLRv1 or Twemoji COLR) as `GenericFamily` emoji / last-resort. There is no satori-style "load emoji as image from CDN" hook. [S][R]
- `Fonts` is a cheap-to-clone registry. Each render takes a render-local snapshot (`Rc<RefCell<Fonts>>`), so concurrent renders never contend on the registry. Mutating requires `&mut Fonts`, so put the registry behind `ArcSwap<Fonts>` if runtime registration is needed. [S]

### 3.5 Animation [S][R]

```rust
let scene = SequentialScene::builder().options(render_options).duration_ms(2000).build();
takumi::write_animation(&[scene], 25, AnimationFormat::Gif(AnimatedGifOptions::default()), &mut out)?;
// AnimationFormat::{Gif(AnimatedGifOptions{loop_count}), WebP(AnimatedWebpOptions{lossless, quality, speed 0..=6, blend, dispose, loop_count}), Apng(AnimatedPngOptions{loop_count})}
// eager variant: render_animation(&scenes, fps) -> Vec<AnimationFrame>, then write_animated_gif/png/webp
// manual frames: AnimationFrame::new(bitmap, duration_ms) for code-driven animation (e.g. counters)
```

- Several `SequentialScene`s form a timeline (scene A for 1 s, then scene B). All scenes must share one viewport size.
- `write_animation` streams: it renders a batch of frames in parallel on the rayon pool (one per rayon thread; each worker prepares its own scene because `PreparedScene` is `!Send`) and encodes them in order, holding only a batch of raw frames in memory.
- fps caps: `AnimationFormat::max_fps()` = **50 for GIF** (10 ms delay units, 20 ms floor), **90 for WebP/APNG**. Higher fps fails with `AnimationFrameRateTooHigh` before rendering.
- Tailwind `animate-spin|ping|pulse|bounce` work **with no stylesheet** (verified: all frames differ). `animate-wiggle` (unknown token) is a no-op until `--animate-wiggle` and `@keyframes wiggle` exist. Arbitrary `animate-[slide_1s_ease-in-out_infinite_alternate]` + `@keyframes slide` in the stylesheet works, including `alternate`. In one test, redefining `--animate-spin` + `@keyframes spin` in `@theme` produced a static result; **do not shadow the built-in animation names**. [R]
- GIF encoding: `gif::Frame::from_rgba_speed(.., 28)` per frame, own palette per frame, full frames (no inter-frame diffing or transparency optimization), no dithering for the GIF palette. Gradients band. [S]

### 3.6 Caches and globals [S]

| Cache | Scope | Budget | Control |
|---|---|---|---|
| Glyph outlines + masks | process-global, sharded, shared across threads | 8 MiB default | `glyph_cache::set_glyph_cache_max_bytes` (only via `unstable`), call before first render |
| Tailwind class-string parse | process-global | 1 MiB | none |
| Text intercepts | process-global | fixed | none |
| `ResourceCache` (decoded images, SVG rasters, parsed stylesheets) | per handle, `Clone` shares it, thread-safe `quick_cache` | 64 MiB default | `RenderOptions::resource_cache(ResourceCache::new(bytes))`; the type is reachable only via `unstable` |
| Pre-decoded `images` map | per render (moved into an `Rc`) | caller | pass `ImageSource` clones (they are `Arc`s) |

There is no output (encoded bytes) cache; that is vesper's job (§5.5).

---

## 4. What I ran and what worked [R]

`cargo add takumi --features gif,webp,png,jpeg,from-html` + `serde_json`, release build, run on 4 cores. All of the following compiled on the first try and produced correct output (checked visually):

| Test | Result |
|---|---|
| Compile-time asserts `Fonts: Send+Sync`, `Node: Send+Sync`, `ImageSource: Send+Sync`, `StyleSheet: Send+Sync`, `RenderOptions<'static>: Send` | pass |
| Register Inter Regular + Bold OTF | 1-4.4 ms total |
| Rank card via `Node` builder + `with_tw` (avatar SVG, gradient progress bar `bg-linear-to-r from-indigo-500 to-pink-500 w-[67%]`, `rounded-3xl`, flex) 934x282 | first: render 4.4 ms, PNG 3.4 ms; warm: **render 1.7-2.0 ms, PNG 1.4-1.5 ms (15 KB), WebP q85 6-10 ms (9 KB)** |
| Same card with v3 name `bg-gradient-to-r` | gradient **silently missing** |
| Welcome card via `from_html` with `tw` attributes 800x400 | parse 0.25-0.36 ms, render+PNG 5-6 ms, 20 KB |
| Card from JSON (`{"type":"container","tw":"...","children":[...]}`) via serde | works |
| `animate-[slide_1s_ease-in-out_infinite_alternate]` + `@keyframes slide` (transform + opacity) and `animate-spin`, 600x300, 2 s @ 25 fps = 50 frames | **GIF 138 KB / 186 ms; WebP lossy q80 132 KB / 67 ms; APNG 189 KB / 82 ms**; frame-by-frame check: motion and alternate direction correct, 40 ms delays, infinite loop |
| 64 rank cards + PNG on 4 threads sharing one `&Fonts` (`std::thread::scope`) | **84 ms total (~760/s)** |
| Built-in `animate-spin/ping/pulse/bounce` without CSS | animate; unknown `animate-wiggle` static |
| Text with empty `Fonts` | nothing drawn |
| Emoji 🎉 with only Inter registered | tofu box |

Core of the working builder code:

```rust
fn tw(n: Node, classes: &str) -> Node { n.with_tw(TailwindValues::from_str(classes).unwrap()) }

tw(Node::container([
    tw(Node::image(ImageData::from((Arc::<str>::from("avatar"), Some(128.0), Some(128.0)))),
       "rounded-full w-32 h-32 border-4 border-indigo-400"),
    tw(Node::container([
        tw(Node::text(name), "text-4xl font-bold text-white"),
        tw(Node::text(format!("Level {level}")), "text-xl text-slate-300"),
        tw(Node::container([tw(Node::container([]),
            &format!("h-full rounded-full bg-linear-to-r from-indigo-500 to-pink-500 w-[{pct:.0}%]"))]),
           "w-full h-6 rounded-full bg-slate-700 flex"),
    ]), "flex flex-col gap-3 flex-1"),
]), "flex flex-row items-center gap-8 w-full h-full p-10 bg-slate-900 rounded-3xl")
```

Compared with the current TS stack (`packages/satori`: satori → SVG → resvg → sharp WebP, fonts in a module-level array, no caching, no pool, static images only), takumi does it in one step, natively, with Tailwind and animation, and an order of magnitude less latency per card. [S][I]

---

## 5. Design recommendations for vesper [I]

### 5.1 Cargo feature

```toml
[features]
default = []
image = ["dep:takumi", "dep:rayon"]          # static PNG/WebP/JPEG + Tailwind
image-gif = ["image", "takumi/gif"]          # animated GIF output + GIF inputs
image-html = ["image", "takumi/from-html"]   # runtime HTML templates (html5ever)
image-default-font = ["image"]               # embeds Inter (+ a COLR emoji font) via include_bytes!

[dependencies]
takumi = { version = "=2.17", default-features = false, features = ["raster-backend", "png", "jpeg", "webp", "woff2", "svg-source", "rayon"], optional = true }
```

- Keep `image` off by default so non-image bots skip ~100 extra crates and the libwebp C build.
- Everything image-related lives in `vesper::image` behind `#[cfg(feature = "image")]`; the Components V2 builder only gains an `attach(RenderedImage)` method behind the same flag. Never re-export takumi types in vesper's stable surface except through a `vesper::image::takumi` escape hatch, because takumi releases weekly.
- Pin `=2.17` (or `~2.17`) and bump deliberately with snapshot tests (§5.8).

### 5.2 Template API: a `view!` macro over a thin builder

There is no `html!`-for-takumi crate. vesper should provide two layers.

**Builder layer (plain Rust, no macro magic)**, a tiny wrapper that makes the takumi builder terse and keeps vesper's API stable:

```rust
use vesper::image::prelude::*;

fn rank_card(u: &RankData) -> View {
    div("flex items-center gap-8 w-full h-full p-10 bg-slate-900 rounded-3xl").children([
        img(Asset::avatar(u.user_id), "w-32 h-32 rounded-full border-4 border-indigo-400"),
        div("flex flex-col gap-3 flex-1").children([
            text(&u.name, "text-4xl font-bold text-white"),
            text(format!("Level {}", u.level), "text-xl text-slate-300"),
            div("w-full h-6 rounded-full bg-slate-700 flex").child(
                div("h-full rounded-full bg-linear-to-r from-indigo-500 to-pink-500")
                    .style(width_pct(u.progress)),   // typed dynamic style, no format! into class strings
            ),
        ]),
    ])
}
```

**Macro layer** (`vesper-macros`, proc macro) giving JSX-like ergonomics while expanding to the builder at compile time, so no HTML parsing at runtime:

```rust
view! {
    <div tw="flex items-center gap-8 w-full h-full p-10 bg-slate-900 rounded-3xl">
        <img src={Asset::avatar(u.user_id)} tw="w-32 h-32 rounded-full" />
        <span tw="text-4xl font-bold text-white">{&u.name}</span>
        if u.boosting { <span tw="text-pink-400">"Booster"</span> }
        for badge in &u.badges { <Badge badge={badge} /> }   // components = fn(props) -> View
    </div>
}
```

Macro responsibilities:
- **Validate Tailwind classes at compile time** against takumi's parser (call `TailwindValue::parse` from the proc macro via `takumi-core`, or a generated allow-list). Because takumi drops unknown classes silently (the `bg-gradient-to-r` trap), a compile error such as "unknown utility `bg-gradient-to-r`; did you mean `bg-linear-to-r`?" is a big ergonomic win.
- Pre-parse static class strings once (`LazyLock<TailwindValues>` per call site) so the runtime only clones an `Arc`.
- Dynamic values go through typed style setters or `tw={format!(..)}` (runtime parse, still cached by takumi).

Also accept, for bots that want designer-editable templates: `Template::html(str)` (feature `image-html`, ~0.3 ms parse, cache the parsed `Node` and clone it) and `Template::json(serde_json::Value)`.

`View` is a newtype around `takumi::Node` plus the set of `Asset` keys it references, so the renderer knows which images to resolve before rendering.

### 5.3 Font and asset registry

```rust
pub struct ImageAssets {
    fonts: ArcSwap<Fonts>,                               // read lock-free per render; swap on (rare) registration
    static_images: HashMap<Arc<str>, ImageSource>,       // logos, backgrounds: decoded once at startup
    remote: moka::future::Cache<AssetKey, ImageSource>,  // avatars, guild icons, fetched over HTTP, TTL + byte weigher
    stylesheet: Arc<StyleSheet>,                         // vesper defaults: @theme tokens, extra @keyframes
}
```

- Builder: `.font_file(path)`, `.font_bytes(&'static [u8])`, `.google_font("Inter", weights)` (download once at build or startup; register subsets with `unicode_range`), `.emoji(EmojiFont::Twemoji | NotoColor)`, `.image("logo", bytes)`, `.theme_css(str)`.
- Fail fast at startup if no font is registered (takumi draws no text otherwise), unless `image-default-font` is on.
- Avatars: fetch `cdn.discordapp.com/avatars/{id}/{hash}.png?size=128` (or `.gif` for animated) with the bot's HTTP client, bounded concurrency, timeouts, a size cap and a fallback image. Key the cache by `(user_id, avatar_hash, size)` so hash changes invalidate naturally. Decode with `ImageSource::from_bytes` off the async runtime.
- Pass `ResourceCache` (via `unstable`) shared across renders, and size the glyph cache at startup (`set_glyph_cache_max_bytes`, e.g. 32-64 MiB for many guilds and CJK text).

### 5.4 Render pool with backpressure

Rendering is CPU-bound (2-10 ms stills, 50-500 ms animations), and takumi itself fans out on the **current rayon pool** (frame batches, GIF quantize, libwebp, zlib, blur). Recommendation:

- A **dedicated `rayon::ThreadPool`** (`image_threads`, default `num_cpus - 1`, named `vesper-render-N`) owned by the `Renderer`. Jobs run inside `pool.spawn(...)`, so takumi's inner `par_iter`s stay in that pool and never touch tokio workers or the global rayon pool. Result returned via `tokio::sync::oneshot`.
- Avoid `spawn_blocking` as the primary mechanism: its pool is large (512) and unbounded in CPU terms, and nested rayon calls would land on the global pool.
- **Admission control**: a `tokio::sync::Semaphore` with `queue_capacity` permits (for example 4x threads) acquired *before* submit. `render()` waits (bounded by a timeout), `try_render()` fails immediately with `RenderError::Overloaded`. Separate weights: a still costs 1 permit, an animation costs `ceil(frames / 8)` permits, so a burst of GIFs cannot starve stills. Optionally two lanes (still / animated) with their own semaphores.
- Interaction flow: with a fresh cache miss and a busy queue, **defer the interaction first** (type 5/6) and then edit the original response with the attachment; the 3 s ACK window must never depend on render latency. Expose this as the default in the command context (`ctx.reply_image(...)` auto-defers when estimated wait > ~1.5 s).
- Cancellation: if the caller drops the future, set an `AtomicBool` checked between frames when vesper drives frames itself (§5.6); a takumi `render` call cannot be interrupted mid-frame.
- Metrics via `tracing`: queue depth, wait time, render time, encode time, bytes, cache hit rate.
- Horizontal scale: rendering is stateless apart from fonts and assets, so at "millions of guilds" scale the same `Renderer` trait can be backed by a remote render service (gRPC/HTTP) on separate machines. Keep `Renderer` a trait: `LocalRenderer` (rayon) and user-provided `RemoteRenderer`.

```rust
#[async_trait] // or RPITIT
pub trait Renderer: Send + Sync + 'static {
    async fn render(&self, job: RenderJob) -> Result<RenderedImage, RenderError>;
}
pub struct RenderJob { pub view: View, pub size: (u32, u32), pub output: Output, pub cache_key: Option<CacheKey> }
pub enum Output { Png, WebP { quality: u8 }, Jpeg { quality: u8 }, Gif { fps: u8, duration: Duration }, AnimatedWebP { fps: u8, duration: Duration, quality: u8 } }
pub struct RenderedImage { pub bytes: bytes::Bytes, pub mime: &'static str, pub filename: String, pub width: u32, pub height: u32 }
```

### 5.5 Render cache

- Key = `xxh3_128(template_id, template_version, serialized data, size, output, asset versions, vesper+takumi version)`. Prefer hashing the *template inputs* (props struct deriving `Hash` or `serde` bytes) over hashing the `Node` tree; it is cheaper and stable. Fallback for ad-hoc views: hash `View` via its serde form.
- Pluggable adapter trait, async, bytes-oriented:

```rust
pub trait RenderCache: Send + Sync + 'static {
    fn get(&self, key: &CacheKey) -> impl Future<Output = Option<CachedImage>> + Send;
    fn put(&self, key: CacheKey, image: CachedImage, ttl: Duration) -> impl Future<Output = ()> + Send;
}
```

  Built-ins: `NoCache`, `MemoryCache` (moka, byte-weighted, default 64-256 MiB), and an optional `RedisCache` behind a feature. Users can implement S3/R2 for cross-shard sharing.
- **Single-flight**: coalesce concurrent identical keys (for example 50 users clicking the same leaderboard) so one render serves all waiters (moka's `get_with`/`try_get_with` gives this for the memory tier).
- Optional **CDN URL cache**: after the first upload, Discord returns the attachment URL. For content that does not change, vesper can store `key → attachment CDN URL` and reference the URL instead of re-uploading. Discord CDN URLs are signed and expire (~24 h), so treat this as a short-TTL optimization only.
- Filenames derive from the key (`rank-<hex12>.webp`), which also satisfies the Components V2 "unique filename per render" advice.

### 5.6 GIF and animation strategy

- **Prefer animated WebP** for animated output: in the test it was 2.8x faster to encode than GIF, slightly smaller, full color with alpha. Discord renders animated WebP in Media Gallery and Thumbnail (see `discord-components-v2.md`). Offer GIF for maximum client compatibility and as an explicit choice.
- Defaults: 12-20 fps, 1-3 s loops, 600x200 to 800x400 viewport. Cap fps at `AnimationFormat::max_fps()` (50 GIF / 90 WebP) and vesper's own limits (`max_frames` default 90, `max_pixels_per_frame` default 1 MP, `max_duration` 6 s).
- Two animation modes in the API:
  1. **CSS-driven** (default): `Animation::css(duration, fps)` plus Tailwind `animate-*` or user `@keyframes`, rendered with `write_animation` (streams, parallel batches).
  2. **Code-driven**: `Animation::frames(n, |i, t| view)` for data animations (counting XP up, progress filling). vesper renders each `View` to a `Bitmap` in the pool and feeds `write_animated_*`; it can check cancellation between frames.
- **Size budget loop**: Discord's default upload limit is 20 MiB per file (interaction `attachment_size_limit` gives the actual value); target far less for latency (< 1-2 MB). Strategy: estimate bytes after the first batch; if the projection exceeds the budget, re-render with lower fps, then smaller viewport, then lossy WebP quality, then fall back to a still frame. Return the chosen settings in `RenderedImage` metadata.
- GIF quality caveats: per-frame palette and no dithering means gradients band and flat UI looks fine. Design GIF templates with flat colors, or use WebP. If GIF size matters, a later improvement is post-processing with `gifski`-style or frame-diff optimization; takumi does not do inter-frame diffing.
- Animated avatars: pass the animated GIF/WebP avatar as an `ImageSource`; takumi plays it along the timeline. Choose the timeline duration as a multiple of the avatar loop to avoid a jump at the seam.

### 5.7 Attaching to Components V2

- `RenderedImage` implements vesper's `Attachment` trait: `filename()`, `bytes()`, `mime()`. In twilight terms it becomes `twilight_model::http::attachment::Attachment { filename, file, id, description }` sent as `files[n]`, with the message carrying `flags: IS_COMPONENTS_V2`.
- Builders reference it by filename: `MediaGallery::item(Media::attachment(&img).alt("Rank card"))`, `Section::accessory(Thumbnail::attachment(&img))` → `url: "attachment://rank-3f9c1a2b7d4e.webp"`. Because the reference is created from the `RenderedImage` value, a typo in the filename is impossible.
- On edits, vesper's attachment manager (already planned in `discord-components-v2.md` §2.6) emits the `attachments` keep-list and new `files[n]` automatically; a re-render with new data gets a new hash-based filename.
- Alt text: require `alt` (max 1024 chars) on gallery items in debug builds or lint for it; it is the only accessible text for a generated image.
- Use Thumbnail for small square renders (badges, avatars with frames), Media Gallery for cards and charts (1 component regardless of item count).

### 5.8 Testing and safety

- Snapshot tests for templates: render to PNG and compare against stored images with a small per-pixel tolerance (takumi's own tests do this); run them when bumping takumi.
- Untrusted input: user-provided text goes only into text nodes (never into `from_html` strings or class strings) to avoid markup and class injection. Clamp user-controlled sizes. `from_html` has a `max_depth` guard; JSON `Node` deserialization has the same depth limit.
- Viewport sanity: takumi rejects canvases over 64 MP; vesper should enforce a much lower cap (for example 4 MP) per job.

---

## 6. Open questions

1. Ship an emoji font in `image-default-font` (Twemoji COLR ~1 MB vs Noto Color Emoji COLRv1 ~4 MB), or require the bot author to choose?
2. Should the compile-time Tailwind validation depend on `takumi-core` inside the proc macro (exact parity, heavier build) or on a generated class list (lighter, can drift)?
3. Remote render service: is it in scope for v1, or only the `Renderer` trait seam?
4. Confirm on real clients that animated WebP plays inside Thumbnail on all platforms (mobile included) before making it the default animated format.

## 7. Sources

- crates.io API: <https://crates.io/api/v1/crates/takumi> (versions, features, MSRV, license)
- Crate source in `~/.cargo/registry/src/*/`: `takumi-2.17.0/{src/lib.rs, README.md, CHANGELOG.md, tests/fixtures/animation.rs, tests/fixtures/animated_image_sources.rs}`, `takumi-raster-0.5.8/src/{render.rs, write.rs}`, `takumi-html-0.5.0/src/lib.rs`, `takumi-core-0.31.0/src/{layout/node/mod.rs, layout/node/input.rs, style/tw/*.rs, resources/{font.rs, image.rs, glyph_cache.rs}}`
- Homepage / docs: <https://takumi.kane.tw>, repository <https://github.com/kane50613/takumi> (GitHub API was not reachable from this session; everything above comes from the published crate source)
- Current TS implementation: `/home/user/schwi-v2/packages/satori/src/{index.ts, utils/toImage.ts}`
- Components V2 limits and attachment rules: `_bmad-output/planning-artifacts/research/discord-components-v2.md`
