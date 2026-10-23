# Images: Templates, Tailwind, rendering

Covers: the `image` features, `#[derive(Template)]`, `view!` and `tw!`, Tailwind v4
class names and the strict grammar, typed dynamic styles, the Theme file, fonts, emoji,
remote images, animation, output formats and size budget, the render pool, caps and
cache, attaching to messages. Doctrine: UD-2 (images are read-only). FR-32..37, FR-66..70.

## 1. Enable it

```toml
noctis = { version = "0.1", features = ["image", "default-font"] }
# "image-gif"   → GIF/APNG output (animated WebP needs only "image")
# "image-emoji" → bundles Twemoji (colour emoji in Templates)
```

`image` is off by default; it adds ~100 crates and needs a **C compiler**: the WebP
encoder (libwebp) is built from source. With it, `noctis::prelude` brings in
`AppBuilderImageExt`: calling `.image(ImageConfig)` or `.theme(Theme)` on your builder
installs the `ImagePlugin`, which puts the Renderer on the App (`app.renderer()`).
Without either call, `ctx.render` returns `RenderError::NoRenderer`. Fonts are not
registered yet, so text draws no glyphs [planned, Story 10.7]; then a missing font
without `default-font` fails startup [planned, Story 10.7].

## 2. A Template

```rust
use noctis::image::{Frame, Node, Style, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize, Debug)]   // Deserialize: the Renderer decodes the data
#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]
pub struct ProfileCard {
    pub name: String,
    pub subtitle: String,
    pub level_label: String,         // localized text is data: format it in the handler
    pub xp_label: String,
    pub progress: f32,               // 0.0..=1.0
}

impl TemplateView for ProfileCard {
    fn view(&self, _frame: Frame) -> Node {   // Frame::STILL for stills
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl">
                <div tw="flex flex-col gap-3 flex-1">
                    <span tw="text-5xl font-bold text-white">{&self.name}</span>
                    <span tw="text-xl text-muted">{&self.subtitle}</span>
                    <div tw="flex w-full h-6 rounded-full bg-slate-700">
                        <div tw="h-full rounded-full bg-linear-to-r from-indigo-500 to-violet-400"
                             style={Style::new().width_pct(self.progress * 100.0)} />
                    </div>
                </div>
            </div>
        }
    }
}
```

Register it on its Module: `Module::new("profile").template::<ProfileCard>()`. Two
Templates with the same `id` anywhere in the App fail `App::build` with
`BuildViolation::DuplicateTemplate { id, first_module, first_type, second_module,
second_type }` (registering the same type twice counts too). The derive implements
`noctis::image::Template` (consts `ID`, `VERSION`, `SIZE`, `FORMAT`, `ANIMATION`);
`TemplateDescriptor::of::<T>()` is the type-erased record the Renderer keys on, and its
`view` is a `noctis::image::unstable::ViewFn` that decodes the data and calls your
`view` (`ViewFn::call(&data, frame)`; a decode error is `RenderError::Failed` with a
`ValueError`; `ViewFn::is_animated()` says whether the Template declares
`animation(..)`). A struct that lacks `impl TemplateView` or `Deserialize` does not
compile.

`#[template(..)]` keys (any order, each exactly once, unknown keys are errors):

| Key | Takes |
| --- | --- |
| `id` | a string literal of 1 to 64 characters `[a-z0-9._-]`; stable, part of the cache key |
| `version` | an integer literal `0..=4294967295` (`u32`, suffix `u32` allowed); **bump whenever the data or `view` changes**, or cached images go stale |
| `size` | `(width, height)` integer literals, each `1..=4294967295` px |
| `format` | the still encoding: `Png`, `Jpeg { quality: 1..=100 }`, `Webp { quality: 1..=100 }` or `WebpLossless` (`Gif`/`Apng` are not still formats) |
| `animation(..)` | optional: `duration_ms = 1..`, `fps = 1..=90` (`1..=50` for `Gif`), `format = Webp { quality } \| Gif \| Apng`; renders animated by default |

The item must be a struct (named, tuple or unit) without generics or lifetimes. Data
must serialize deterministically (it is part of the render cache key): a field whose
type mentions `HashMap` or `HashSet` anywhere (`Vec<HashMap<..>>`, `Option<HashSet<..>>`)
is a compile error; use `BTreeMap`, `BTreeSet` or `Vec`. The check matches only path
segments named `HashMap`/`HashSet`: type aliases, third-party maps (`FxHashMap`,
`AHashMap`, `DashMap`) and nested structs holding a `HashMap` are not detected, so use
`BTreeMap`/`BTreeSet`/`Vec` throughout Template data. `#[asset]` remote-image fields
are [planned, Story 10.8] and rejected until then.

## 3. `view!` syntax

```rust
view! {
    <div tw="flex flex-col" style={Style::new().gap_px(8)}>
        "Title: "                                        // a string literal is a text node
        {&self.title}                                    // {expr}: anything IntoNodes
        if self.boosting { <span tw="text-pink-400">{&self.boost_label}</span> }
        else if self.trial { "trial" } else { "free" }
        for badge in &self.badges { {badge_node(badge)} } // fn(&T) -> Node components
        <p tw={tw!("text-sm").and(Classes::dynamic(level_class))} />
    </div>
}
```

- Exactly one root element; `view!` is an expression of type `Node`.
- Tags: `div`, `span`, `p`, `h1`, `h2`, `h3` (`Tag::{Div, Span, P, H1, H2, H3}`).
  `<img>` and `<svg>` are [planned, Story 10.7] and rejected with that message.
- Attributes, each at most once: `tw="literal"` (checked like `tw!`, §4) or
  `tw={expr}` of type `Classes`, and `style={expr}` of type `Style`. `class=` is an
  error that points to `tw`; other attributes are errors.
- Children: elements, string literals, `{expr}`, `if cond { .. }` with `else if` /
  `else`, and `for pat in expr { .. }`. `{expr}` takes any `IntoNodes`: `Node`,
  `Vec<Node>`, `&str`, `String`, `&String`, `Cow<'static, str>`, integers, `f32`/`f64`
  (as text) and `Option<T>` (`None` adds nothing). Text children are always text:
  `"<b>"` renders the characters, never markup.
- **User text enters only as a text child** (FR-32), never as classes or styles.

The builder API builds the same tree: `div(classes)`, `span(classes)`,
`text(content, classes)`, `Node::element(Tag::P)`, `Node::text(..)`, then
`.classes(..)`, `.style(..)`, `.child(impl IntoNodes)` and `.children(iter)`; for
example `div(tw!("flex gap-2")).child(text(&name, tw!("text-xl")))`. A `view!` tree and
its builder twin compare equal (`Node: PartialEq`), which is how to unit-test a view.

## 4. Classes: `tw!`, Tailwind v4, strict grammar

- `tw!("flex p-4")` is a `const`-usable `Classes` checked at `cargo check`; `tw="…"`
  in `view!` is checked the same way. Each unknown class is one `[TW-01]` error on the
  literal, e.g. ``[TW-01] unknown Tailwind class `bg-nope`; or declare `--color-nope`
  in noctis-theme.css``; with a known v3 name the error suggests the v4 one.
- Classes are **Tailwind v4** in Noctis's strict grammar (`noctis::image::tw`):
  built-in utilities, variants (`sm:`, `md:`, …, or a custom `--breakpoint-*`),
  `!important`, opacity modifiers (`bg-black/50`), plus the tokens of your theme
  (§5). takumi alone would silently drop an unknown class. A failed runtime check
  is an `InvalidClasses`, whose `classes()` are `UnknownClass`es (`class()`,
  `suggestion() -> Option<&Suggestion>`); a `Suggestion` is `Rename(v4 class)` or
  `Hint(text)`.
- v3 names the grammar suggests:
  | v3 | v4 |
  | --- | --- |
  | `bg-gradient-to-r` (any side) | `bg-linear-to-r` |
  | `overflow-ellipsis` | `text-ellipsis` |
  | `bg-opacity-50`, `border-opacity-*`, … | an opacity modifier: `bg-black/50` (hint) |
  | `space-x-4` / `space-y-*` | `gap-x-*` / `gap-y-*` on a flex parent (hint) |

  General Tailwind v4 advice the error does not spell out: `shrink-0`/`grow` (not
  `flex-shrink-0`/`flex-grow`), `shadow-xs`/`rounded-xs`/`blur-xs` for v3's `-sm`,
  `outline-hidden`, `ring-3`.
- Layout is flexbox/grid (taffy): use `flex`, `flex-col`, `items-*`, `justify-*`,
  `gap-*`, `grid grid-cols-*`. Set explicit sizes on the root (`w-full h-full`).
- Classes chosen at run time: `Classes::dynamic(text)` (a known class name from your
  own code, such as a tier → `"text-amber-400"` match) and `a.and(b)` (joins with a
  space); `as_str()` reads the text. **Never pass user text to `Classes::dynamic`.**
  Dynamic strings are checked when rendered: the Renderer runs
  `noctis::image::unstable::ClassCheck` before drawing (`ClassCheckMode::Warn` logs each
  invalid string once on target `noctis::image` and renders; on `Runtime::for_test()`
  Apps, so in every `TestApp`, `ClassCheckMode::Deny` fails the render with
  `RenderError::Failed` whose source is the `InvalidClasses`).
- **Data-driven values never go into class strings.** Use the `Style` setters (all
  `const`; the last setter on a property wins; a NaN or infinite `f32` leaves the
  property unchanged; style overrides a class of the same property):
  `Style::new()`, `.width_px(u32)`, `.width_pct(f32)` and `.height_pct(f32)` (clamped
  to `0..=100`), `.height_px(u32)`, `.gap_px(u32)`, `.bg_rgb(0xRRGGBB)`,
  `.color_rgb(..)`, `.border_color_rgb(..)` (the low 24 bits are used),
  `.opacity(f32)` (clamped to `0..=1`), `.translate_px(i32, i32)`, `.rotate_deg(f32)`.

## 5. Theme (`noctis-theme.css`)

At the crate root (next to `Cargo.toml`); `tw!`, `tw="…"` and `view!` read it at
compile time and rebuild when it changes. No file means an empty theme (built-in
classes only). `noctis::image::theme!()` embeds it as a `Theme` value for runtime
(`theme!("path/to/file.css")` takes a path relative to the calling source file); both
fail the build on a missing or invalid file. After creating the file in a crate that
was built without one, touch a source file or run `cargo clean -p <crate>` once so the
macros re-read it.

```css
/* comments anywhere */
@theme {
  --color-brand: #5865f2;      /* → bg-brand, text-brand, border-brand, text-brand/50 */
  --color-surface: #11131a;
  --font-display: "Inter", sans-serif;   /* → font-display */
  --font-weight-heavy: 900;    /* longest prefix wins: --font-weight-, not --font- */
  --animate-rise: rise 1.2s ease-out both;   /* animate-rise is [planned, Story 10.6] */
}
@keyframes rise { from { opacity: 0 } to { opacity: 1 } }
```

Rules (each violation is `[TW-02] <line>:<column>: <what>` at compile time, or a
`ThemeError` from `Theme::parse(css)` with `line()`, `column()` and a `ThemeErrorKind`
from `kind()`):

- The top level holds only `@theme { … }` (no prelude: `@theme inline` is an error)
  and `@keyframes <name> { … }`; several `@theme` blocks merge.
- `@theme` holds only `--<namespace><token>: <value>;`. Namespaces: `--color-`,
  `--spacing-`, `--container-`, `--text-`, `--font-`, `--font-weight-`, `--tracking-`,
  `--leading-`, `--radius-`, `--aspect-`, `--breakpoint-`, and `--animate-`. The token
  is `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`, at most 64 bytes; the value must not be empty.
- A name defined twice is an error, and so is a `@keyframes` or `--animate-` named
  `spin`, `ping`, `pulse` or `bounce` (the built-in animations would render static).
- The file is at most 256 KiB.

`ThemeErrorKind` (non-exhaustive) names the cause: `UnexpectedRule` (anything but
`@theme`/`@keyframes` at the top level, a prelude, an at-rule inside `@theme`),
`NotCustomProperty`, `UnknownNamespace { name }`, `InvalidToken { name }`,
`EmptyValue { name }`, `Duplicate { name }`, `BuiltinAnimation { name }`, `Syntax`
(an unclosed block, a declaration without `:`) and `TooLarge { bytes }`.

A `Theme` also has `Theme::empty()`, `digest()` (part of the render cache key),
`animations()` and `keyframes()` (`(name, body)` pairs), and implements
`ThemeTokens`: `theme.contains(ThemeNamespace::Color, "brand")`. `ThemeNamespace` has
one variant per namespace above (`Color`, `Spacing`, `Container`, `Text`, `Font`,
`FontWeight`, `Tracking`, `Leading`, `Radius`, `Aspect`, `Breakpoint`) and
`css_prefix()` (`"--color-"`).

## 6. Fonts and emoji

```rust
builder.image(
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
use noctis::image::RenderError;

// In the handler: render, then place the Attachment (§10).
let card: Attachment = match ctx.render(ProfileCard { .. }).await {   // by value
    Ok(card) => card,
    Err(err) => match err.as_error().downcast_ref::<RenderError>() {
        Some(RenderError::QueueFull) => return ctx.reply_text("Busy, try again soon.").await,
        _ => return Err(err),
    },
};
```

- `ctx.render(t).await?` (on `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx` and
  `ExpiredCtx`) returns an `Attachment` named by the output digest
  (`<16 hex>.png`/`.jpg`/`.webp`) in the Template's still `format`. Always render **in
  the handler**, never in a render function; the auto-defer covers a render slower
  than the ack deadline (type 5, then the edit carrying the file).
- Errors are a `HandlerError` wrapping a `RenderError`, found with
  `err.as_error().downcast_ref::<RenderError>()`:
  - `QueueFull`: the render queue is full; show a "busy, try again" notice;
  - `NoRenderer`: no `.theme(..)`/`.image(..)` on the builder;
  - `UnknownTemplate { id, version }`: not registered with `Module::template` (or a
    version skew);
  - `Unsupported { id, why }`: animated Templates are not rendered yet;
  - `Failed(source)`: decode, class check (Deny), takumi or encode failure (a canvas
    over 64 MP fails here);
  - `Panicked { message }`: the view panicked; the pool survives.
- The pool: `builder.image(ImageConfig::new().threads(4).queue_capacity(128))`.
  `threads` is 1..=256 (default half the available parallelism, at least 1),
  `queue_capacity` 1..=4096 (default 64), counting queued plus running renders;
  out-of-range values fail `App::build` with a plugin violation. Renders run on this
  dedicated rayon pool (`noctis-render-<i>` threads), never on tokio workers or rayon's
  global pool.
- The pool is **process-scoped**: every App on one `Runtime` shares it, so they must
  pass an equal `ImageConfig`; another one fails `build` with
  `BuildViolation::ResourceConflict` naming `RenderPool`. Each `Runtime::for_test()`
  has its own pool.
- `ImagePlugin` is added by `AppBuilderImageExt::image`/`theme` (each call updates
  the one plugin; the last call of each kind wins, order does not matter, never a
  `DuplicatePlugin`). `app.has_renderer()` / `app.renderer()` expose the result.
- At `App::build` the Renderer receives every `Module::template` registration; a
  hand-written `Template` (view not from `#[derive(Template)]`) or an animated one fails
  the build with `BuildViolation::TemplateRejected`. Templates registered without any
  Renderer only log a warning on target `noctis::render`.
- The Render Error Sink (`error_sink(ErrorOrigin::Render, ..)`) gets every render
  failure (`ReportKind::Error`) and panic (`ReportKind::Panic`), except `QueueFull` and
  `NoRenderer`. A failure your handler also propagates with `?` is reported once more
  by the handler's own origin.
- Observability: span `noctis.render{app, template}`; metrics
  `noctis_render_duration_seconds{app, template}`, `noctis_render_queue_depth{app}`
  (this App's `ctx.render` calls awaiting a render; a cancelled call stops counting
  although its admitted job still runs until Story 10.5) and `noctis_render_rejected_total{app, reason}`
  (`queue_full`, `unknown_template`, `unsupported`).
- On `Runtime::for_test()` Apps an unknown `Classes::dynamic` class fails the render
  (Deny, §4).
- Planned: `ctx.render_with(t, RenderOptions::new().animation(..))` and
  `render_report()` [planned, Story 10.6]; per-job caps (≤ 4 MP per job, ≤ 1 MP per
  frame, ≤ 90 frames, ≤ 6 s; fps ≤ 50 GIF, ≤ 90 WebP/APNG), admission weighting and
  skipping a job whose interaction is gone [planned, Story 10.5]; the size budget
  [planned, Story 10.4].

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
settings used are in `render_report()`. [planned, Story 10.4]

## 9. Cache

Identical requests hit the `RenderCache` port; N concurrent identical requests render
once. The key covers Noctis + renderer version, template id + version, theme, data,
asset versions, size, format and settings — so upgrades never serve stale images, but
**you** must bump `version` when the view changes. Attachment names derive from the key.

## 10. Placing images (UD-2)

```rust
MediaGallery::new([MediaItem::new(cx.attach(&self.card)).alt(s.profile.card_alt(&name))])  // wide
Section::new([TextDisplay::new(title)], Thumbnail::new(cx.attach(&self.badge)).alt(alt))         // small
```

- Wide cards, charts, tables → Media Gallery (1 component, 0 text chars). 1 wide image
  (≈ 2:1) or 2 side by side.
- Icons, avatars, badges → Thumbnail.
- Always alt text on the Thumbnail/MediaItem itself (`.alt(..)`, or `alt!("…")` for a
  fixed text): a rendered image without it fails Strict `validate` with `[ATT-06]`
  (Lenient/`cost`: `MissingAlt` warning). Never actionable info in the image; put
  buttons next to it. Images are uploaded once and kept by id on later edits
  (`components-v2.md` §6).
- Light and dark themes: use opaque card backgrounds or mid-contrast palettes.

## 11. Testing

`noctis::testkit::assert_render_snapshot!(app.renderer(), "name", template, tolerance = 0.01)`
on a `TestApp` built from your own `configure` (its ImagePlugin installs the Renderer;
default font, no network). See `testing.md`.

## 12. Unstable

Renderer internals (takumi types) are only under `noctis::image::unstable`, outside
semver. Don't use them in normal bots.
