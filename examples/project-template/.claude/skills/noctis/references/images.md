# Images: Templates, Tailwind, rendering

Covers: the `image` features, `#[derive(Template)]`, `view!` and `tw!`, Tailwind v4
class names and the strict grammar, typed dynamic styles, the Theme file, fonts, emoji,
remote images, animation, output formats and size budget, the render pool, caps and
cache, attaching to Views. Doctrine: UD-2 (images are read-only). FR-32..37, FR-66..70.

## 1. Enable it

```toml
noctis = { version = "0.1", features = ["image", "default-font"] }
# "image-gif"   → GIF/APNG output (animated WebP needs only "image")
# "image-emoji" → bundles Twemoji (colour emoji in Templates)
```

`image` is off by default; it adds ~100 crates and the native WebP encoder needs a C
compiler. With it, `noctis::prelude` brings in `AppBuilderImageExt`: calling
`.image(ImageConfig)` or `.theme(Theme)` on your builder installs the `ImagePlugin`,
which puts the Renderer on the App. Without either call, `ctx.render` returns a typed
error. Without a registered font and without `default-font`, startup fails (text
would otherwise render invisibly).

## 2. A Template

```rust
use noctis::image::{Frame, Node, RemoteImage, Style, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize, Debug)]   // Deserialize: the Renderer decodes the data
#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]
pub struct ProfileCard {
    pub name: String,
    pub subtitle: String,
    pub level_label: String,         // localized text is data: format it in the handler
    pub xp_label: String,
    pub progress: f32,
    #[asset]                         // remote image, fetched + cached before rendering
    pub avatar: RemoteImage,
}

impl TemplateView for ProfileCard {
    fn view(&self, _frame: Frame) -> Node {   // Frame::STILL for stills
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl">
                <img src={&self.avatar} tw="w-40 h-40 rounded-full border-4 border-brand" />
                <div tw="flex flex-col gap-3 flex-1">
                    <span tw="text-5xl font-bold text-white">{&self.name}</span>
                    <div tw="flex w-full h-6 rounded-full bg-slate-700">
                        <div tw="h-full rounded-full bg-linear-to-r from-brand to-brand-soft"
                             style={Style::new().width_pct(self.progress * 100.0)} />
                    </div>
                </div>
            </div>
        }
    }
}
```

Register it on its Module: `.template::<ProfileCard>()` (App build then checks fonts,
theme tokens and keyframes).

`#[template(...)]`: `id` (stable, part of the cache key), `version` (**bump whenever
`view` changes**, or cached images go stale), `size = (w, h)` in px, `format` (the
still encoding: `Png` | `Jpeg { quality }` | `Webp { quality }` | `WebpLossless`), and
optionally `animation(duration_ms = 2000, fps = 20, format = Webp)` (animated
`Webp { quality }` | `Gif` | `Apng`) to render animated by default. Data must serialize deterministically: the derive rejects `HashMap`
fields (use `Vec` or `BTreeMap`).

## 3. `view!` syntax [planned]

```rust
view! {
    <div tw="flex flex-col">
        {&self.title}                                   // text node (always escaped text)
        if self.boosting { <span tw="text-pink-400">{&self.boost_label}</span> }
        for badge in &self.badges { {badge_node(badge)} }   // fn(&T) -> Node components
        <img src={&self.avatar} tw="w-16 h-16" />
        <div style={Style::new().width_pct(p).bg_rgb(0x5865F2)} />
    </div>
}
```

Elements: `div`, `span`, `img`, `p`, `h1`–`h3`, `svg` (inline literal). User text can
only enter as a text node, never as a class string or markup (FR-32).

`tw!("…")` gives a validated class set for the builder API:
`div(tw!("flex gap-2")).child(text(&name, tw!("text-xl")))`.

## 4. Tailwind: v4 names, strict grammar

- Classes are **Tailwind v4**. Literal classes are checked at compile time against
  Noctis's strict grammar plus your theme tokens; unknown classes are compile errors.
  takumi alone would silently drop them.
- Common v3 → v4 renames (the error suggests these):
  | v3 | v4 |
  | --- | --- |
  | `bg-gradient-to-r` | `bg-linear-to-r` |
  | `flex-shrink-0` / `flex-grow` | `shrink-0` / `grow` |
  | `shadow-sm` / `shadow` | `shadow-xs` / `shadow-sm` |
  | `rounded-sm` / `rounded` | `rounded-xs` / `rounded-sm` |
  | `blur-sm` / `blur` | `blur-xs` / `blur-sm` |
  | `outline-none` | `outline-hidden` |
  | `ring` | `ring-3` |
  | `bg-opacity-50` | `bg-black/50` (opacity modifier) |
  | `overflow-ellipsis` | `text-ellipsis` |
- Layout is flexbox/grid (taffy): use `flex`, `flex-col`, `items-*`, `justify-*`,
  `gap-*`, `grid grid-cols-*`. Set explicit sizes on the root (`w-full h-full`).
- **Dynamic values never go into class strings.** Use typed setters:
  `Style::new().width_pct(f32)`, `.height_px(u32)`, `.bg_rgb(u32)`, `.color_rgb(u32)`,
  `.opacity(f32)`, `.translate_px(x, y)`, `.rotate_deg(f32)` [planned]. A runtime class
  string with an unknown class logs a warning once and fails tests under the testkit.

## 5. Theme (`noctis-theme.css`)

At the crate root; the macros read it at compile time and `.theme(noctis::image::theme!())`
embeds it for runtime:

```css
@theme {
  --color-brand: #5865f2;      /* → bg-brand, text-brand, from-brand, border-brand */
  --color-surface: #11131a;
  --font-display: "Inter", sans-serif;   /* → font-display */
  --animate-rise: rise 1.2s ease-out both;  /* → animate-rise */
}
@keyframes rise { from { opacity: 0 } to { opacity: 1 } }
```

Redefining a built-in animation (`spin`, `ping`, `pulse`, `bounce`) is a registration
error (it would render static).

## 6. Fonts and emoji

```rust
App::from_env()?.image(
    ImageConfig::new()
        .font(FontSource::bytes(include_bytes!("../assets/fonts/Inter.ttf")))
        .font(FontSource::file("assets/fonts/NotoSansJP.ttf").unicode_range("U+3000-9FFF"))
        .fallback_order(["Inter", "Noto Sans JP"])
        .emoji(EmojiFont::Twemoji)          // needs "image-emoji"
        .image("logo", include_bytes!("../assets/logo.png")), // static images: <img src="logo">
)
```

- TTF/OTF/WOFF/WOFF2; fallbacks by priority and Unicode range; no Template changes.
- Without an emoji font, emoji render as boxes and startup warns. Discord custom emoji
  (`<:name:id>`) in text render as images fetched like remote images.
- Fonts, emoji and the render pool are **process-scoped**: the first App's config wins;
  a conflicting second App config is a `BuildError`.

## 7. Remote images

- Declare them as `#[asset] field: RemoteImage` (or `#[asset(required)]` to fail the
  render instead of using the fallback).
- Constructors: `RemoteImage::avatar(&user, 256)` (keyed by user id + avatar hash +
  size, so a changed avatar invalidates naturally; animated avatars animate in
  animated output), `RemoteImage::guild_icon(&guild, 128)`, `RemoteImage::emoji(id,
  animated)`, `RemoteImage::url(url)` (host must be allow-listed), `RemoteImage::placeholder()`.
- Fetched only by Noctis's `AssetFetcher`: host allow-list (Discord CDN by default;
  extend with `ImageConfig::allow_host("i.imgur.com")`), byte cap, timeout, bounded
  concurrency, cached through the `AssetStore` port. Failures use the fallback image
  and are logged.

## 8. Rendering

```rust
let card: Attachment = ctx.render(ProfileCard { .. }).await?;                    // template defaults
let anim = ctx.render_with(card_data, RenderOptions::new()
        .animation(AnimationSpec::new(Duration::from_secs(2), 20, AnimatedFormat::Webp { quality: 80 })))
    .await?;
let report = anim.render_report();   // format, size, fps, quality actually used, cache hit
```

- Always render **in the handler** (or its `load` fn), never in `View::render`; the
  auto-defer covers slow renders.
- Runs on a dedicated bounded rayon pool, never on tokio workers. A full queue fails
  fast: `RenderError::Overloaded` (show a "busy, try again" notice).
- Per-job caps, checked before queueing: ≤ 4 MP per job, ≤ 1 MP per frame, ≤ 90
  frames, ≤ 6 s; fps ≤ 50 GIF, ≤ 90 WebP/APNG (`RenderError::CapExceeded`).
- If the interaction is gone, the queued job is skipped; animations stop between frames.
- Without a Renderer (feature off or replaced by none) `ctx.render` returns a typed error.

### Animation

- CSS-driven: `animate-spin|ping|pulse|bounce`, your theme `animate-*`, or arbitrary
  `animate-[slide_1s_ease-in-out_infinite_alternate]` with `@keyframes slide` in the
  theme. Render with an animated `format`, `fps` and `duration`.
- Code-driven: one data value, one Template state per frame. The view receives
  `Frame { index, count }` and computes that frame's state, for example
  `let value = self.target * (frame.index + 1) / frame.count;`. Render it animated with
  `ctx.render_with(card, RenderOptions::new().animation(AnimationSpec::new(Duration::from_secs(2), 20, AnimatedFormat::Webp { quality: 80 })))`
  [planned]. Every frame has the job's size; a root node with a different explicit
  size is a typed error.
- Default animated format: animated WebP (smaller and ~3× faster than GIF); GIF with
  `image-gif` for maximum compatibility. Design GIF Templates with flat colours.

### Size budget

Budget = min(Upload Budget, target ≈ 2 MB). Over budget, Noctis lowers fps, then size,
then quality, then falls back to a still frame; still too big → typed error. The
settings used are in `render_report()`.

## 9. Cache

Identical requests hit the `RenderCache` port; N concurrent identical requests render
once. The key covers Noctis + renderer version, template id + version, theme, data,
asset versions, size, format and settings — so upgrades never serve stale images, but
**you** must bump `version` when the view changes. Attachment names derive from the key.

## 10. Placing images (UD-2)

```rust
MediaGallery::new([MediaItem::new(cx.attach(&self.card)).alt(s.profile.card_alt(&name))])  // wide
Section::new([Text::new(title)], Thumbnail::new(cx.attach(&self.badge)).alt(alt))         // small
```

- Wide cards, charts, tables → Media Gallery (1 component, 0 text chars). 1 wide image
  (≈ 2:1) or 2 side by side.
- Icons, avatars, badges → Thumbnail.
- Always alt text. Never actionable info in the image; put buttons next to it.
- Light and dark themes: use opaque card backgrounds or mid-contrast palettes.

## 11. Testing

`noctis::testkit::assert_render_snapshot!(app.renderer(), "name", template, tolerance = 0.01)`
on a `TestApp` built from your own `configure` (its ImagePlugin installs the Renderer;
default font, no network). See `testing.md`.

## 12. Unstable

Renderer internals (takumi types) are only under `noctis::image::unstable`, outside
semver. Don't use them in normal bots.
