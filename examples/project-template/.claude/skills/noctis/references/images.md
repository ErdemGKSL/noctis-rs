# Images: Templates, Tailwind, rendering

Covers: the `image` features, `#[derive(Template)]`, `view!` and `tw!`, Tailwind v4
class names and the strict grammar, typed dynamic styles, the Theme file, fonts, emoji,
remote images, animation, output formats and size budget, the render pool, caps and
cache, attaching to messages. Doctrine: UD-2 (images are read-only). FR-32..37, FR-66..70.

## 1. Enable it

```toml
noctis = { version = "0.1", features = ["image", "default-font"] }
# "image-gif"   → GIF/APNG output (animated WebP needs only "image")
# "image-emoji" → bundles Twemoji (colour Unicode emoji in Templates; implies "image")
```

`image` is off by default; it adds ~100 crates and needs a **C compiler**: the WebP
encoder (libwebp) is built from source. With it, `noctis::prelude` brings in
`AppBuilderImageExt`: calling `.image(ImageConfig)` or `.theme(Theme)` on your builder
installs the `ImagePlugin`, which puts the Renderer on the App (`app.renderer()`).
Without either call, `ctx.render` returns `RenderError::NoRenderer`. Text needs a font
(§6): `default-font` (implies `image`) bundles Inter 4.1 (OFL-1.1) as the last-resort
family `"Inter"`; without it, register one with `ImageConfig::font`, or an App that
registers a Template fails `App::build` with `[IMG-01] no font is registered`.

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
`animation(..)`, and `ViewFn::declares_assets()` whether it has `#[asset]` fields). A struct that lacks `impl TemplateView` or `Deserialize` does not
compile.

`#[template(..)]` keys (any order, each exactly once, unknown keys are errors):

| Key | Takes |
| --- | --- |
| `id` | a string literal of 1 to 64 characters `[a-z0-9._-]`; stable, part of the cache key |
| `version` | an integer literal `0..=4294967295` (`u32`, suffix `u32` allowed); **bump whenever the data or `view` changes**, or cached images go stale |
| `size` | `(width, height)` integer literals, each `1..=4294967295` px |
| `format` | the still encoding: `Png`, `Jpeg { quality: 1..=100 }`, `Webp { quality: 1..=100 }` or `WebpLossless` (`Gif`/`Apng` are not still formats) |
| `animation(..)` | optional: `duration_ms = 1..`, `fps = 1..=90` (`1..=50` for `Gif`), optional `format = Webp { quality } \| Gif \| Apng` (default `Webp { quality: 80 }`, `AnimatedFormat::DEFAULT`); `Gif`/`Apng` need the `image-gif` feature, else a compile error naming it; renders animated by default |

The item must be a struct (named, tuple or unit) without generics or lifetimes. Data
must serialize deterministically (it is part of the render cache key): a field whose
type mentions `HashMap` or `HashSet` anywhere (`Vec<HashMap<..>>`, `Option<HashSet<..>>`)
is a compile error; use `BTreeMap`, `BTreeSet` or `Vec`. The check matches only path
segments named `HashMap`/`HashSet`: type aliases, third-party maps (`FxHashMap`,
`AHashMap`, `DashMap`) and nested structs holding a `HashMap` are not detected, so use
`BTreeMap`/`BTreeSet`/`Vec` throughout Template data. `#[asset]` and
`#[asset(required)]` fields declare remote images (§7); `#[asset]` takes no other
argument and appears once per field.

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
- Tags: `div`, `span`, `p`, `h1`, `h2`, `h3` (`Tag::{Div, Span, P, H1, H2, H3}`), and
  `<img src={LOGO} tw=".." style={..} />` for a registered `StaticImage` (§6; it
  expands to `img(LOGO, classes).style(..)`). `<img>` has no children (close it with
  `/>`) and needs `src={..}` (an expression in braces, never `src="logo"`); `src` is
  only for `<img>`. `<svg>` is not supported: use a PNG, JPEG or WebP `StaticImage`.
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
  --animate-rise: rise 1.2s ease-out both;   /* → animate-rise (needs @keyframes rise) */
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
  An `--animate-` token also has no `.` and is not a number (`--animate-5` is
  `InvalidToken`).
- An `--animate-<token>` value is exactly one animation, in any order:
  `<keyframes> <duration>[ <delay>][ <timing>][ <count>|infinite][ <direction>][ <fill>][ <play-state>]`.
  Times are `0`..`999999` with up to 3 decimals and `s` or `ms` (the duration must be
  positive; a second time is the delay); timing is `linear`, `ease`, `ease-in`,
  `ease-out`, `ease-in-out`, `step-start` or `step-end`; the count is `infinite` or
  1..=9999; direction `normal|reverse|alternate|alternate-reverse`; fill
  `none|forwards|backwards|both`; play state `running|paused`; each at most once.
  Functions (`cubic-bezier()`, `steps()`, `var()`), commas (several animations) and
  comments are `InvalidAnimation`. Example: `rise 1s ease-out both`.
- Its keyframes must be a built-in animation (`spin`, `ping`, `pulse`, `bounce`) or a
  `@keyframes` of the same file, before or after it; this is checked after the whole
  file parsed, and the first animation (in file order) naming missing keyframes is
  `UnknownKeyframes`.
- A `@keyframes` name is `[a-z]([a-z0-9-]*[a-z0-9])?`, at most 64 bytes, and not a CSS
  keyword of the grammar above or `initial`/`inherit`/`unset`/`revert`/`revert-layer`/
  `default` (`@keyframes Rise`, `@keyframes ease`, `@keyframes a_b` are
  `InvalidKeyframesName`).
- A name defined twice is an error, and so is a `@keyframes` or `--animate-` named
  `spin`, `ping`, `pulse` or `bounce` (the built-in animations would render static).
- The file is at most 256 KiB.

`ThemeErrorKind` (non-exhaustive) names the cause: `UnexpectedRule` (anything but
`@theme`/`@keyframes` at the top level, a prelude, an at-rule inside `@theme`),
`NotCustomProperty`, `UnknownNamespace { name }`, `InvalidToken { name }`,
`EmptyValue { name }`, `Duplicate { name }`, `BuiltinAnimation { name }`,
`InvalidAnimation { name }` (an `--animate-*` value outside the grammar),
`UnknownKeyframes { name, keyframes }` (an `--animate-*` value naming keyframes the
file does not define), `InvalidKeyframesName { name }`, `Syntax` (an unclosed block, a
declaration without `:`) and `TooLarge { bytes }`.

A `Theme` also has `Theme::empty()`, `digest()` (part of the render cache key),
`animations()` and `keyframes()` (`(name, body)` pairs), and implements
`ThemeTokens`: `theme.contains(ThemeNamespace::Color, "brand")`,
`theme.contains_animation("rise")` (`--animate-rise`) and
`theme.contains_keyframes("rise")` (`@keyframes rise`); a custom `ThemeTokens` impl
gets `false` defaults for the two animation methods. `ThemeNamespace` has
one variant per namespace above (`Color`, `Spacing`, `Container`, `Text`, `Font`,
`FontWeight`, `Tracking`, `Leading`, `Radius`, `Aspect`, `Breakpoint`) and
`css_prefix()` (`"--color-"`).

## 6. Fonts, static images and emoji

```rust
use noctis::image::{Font, ImageConfig, StaticImage};

static INTER: &[u8] = include_bytes!("../assets/fonts/Inter.ttf");
static KANA: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-kana.woff2");
const LOGO: StaticImage = StaticImage::new("logo", include_bytes!("../assets/logo.png"));

builder.image(
    ImageConfig::new()
        .font(Font::from_static(INTER))                       // first: the default text font
        .font(
            Font::from_static(KANA)
                .family("Kana")                                // instead of the embedded name
                .unicode_range('\u{3041}'..='\u{3096}')        // hiragana only
                .unicode_range('\u{30A1}'..='\u{30FA}'),       // and katakana
        )
        .image(LOGO),                                         // <img src={LOGO} />
);
```

**Fonts** (`[IMG-01]`):

- `Font::from_static(&'static [u8])` (no copy; `include_bytes!`) or
  `Font::from_bytes(impl Into<Arc<[u8]>>)` (a file you read yourself at startup;
  Noctis never reads font paths). Formats: TTF, OTF, TTC, WOFF2 (not WOFF 1).
- **Order is priority.** Text with no `font-*` family draws with the first font; any
  glyph a font lacks, or its `unicode_range` excludes, falls back to the next family
  (in order of first appearance), then to the bundled `"Inter"` (`default-font`, a last
  resort that never outranks your fonts). Mixed-script text needs no Template change.
  A Template's own family (`font-display` → the theme's `--font-display: "Inter",
  sans-serif`) is tried first, and its missing glyphs walk the same chain.
- `.family(name)` registers a font under another family name (1..=128 bytes, no
  control characters; Unicode names are fine). `.unicode_range(a..=b)` adds a
  `char` range (repeatable; none = every codepoint). Author fonts get no generic
  family (`sans-serif`).
- Startup errors (plugin violations, the App does not build): `[IMG-01] font #<i>:
  unicode range U+0062..=U+0061 is inverted`, `[IMG-01] font #<i>: family name ..`,
  `[IMG-01] font #<i> (<len> bytes) could not be loaded: <reason>` (`#<i>` counts the
  `.font` calls from 0). An App that registers a Template with no font at all and
  without `default-font` fails with `BuildViolation::TemplateRejected` holding
  `RenderError::NoFont` (`[IMG-01] no font is registered: add one with
  `ImageConfig::font(..)` or enable the `default-font` feature`).
- Fonts are **process-scoped**: one font registry per `Runtime`, frozen by the first
  App. Every App on that Runtime must register equal fonts in the same order (an equal
  `Font` has the same bytes, family and ranges); another list, a reordered one or none
  is `BuildViolation::ResourceConflict` naming `FontRegistry`. Each
  `Runtime::for_test()` has its own registry.
- `Font`'s `Debug` prints the family, ranges, byte length and a digest prefix, never
  the bytes.

**Static images** (`[IMG-02]`):

- `StaticImage::new(id, &'static [u8])` is a `const fn`; the id matches
  `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`, at most 64 bytes. In a `const` item a bad id is a
  compile error (`[IMG-02] static image id must match ..`); elsewhere it panics.
- Register each with `ImageConfig::image(LOGO)` (per App; repeating the same handle is
  a no-op). They are decoded once when the App builds: PNG, JPEG or WebP only (sniffed
  by magic bytes; GIF, SVG, BMP, ICO are rejected), at most 16 MP (16777216 pixels).
  Plugin violations: `[IMG-02] static image `<id>`: unsupported format: use PNG, JPEG
  or WebP`, `.. could not be decoded: ..`, `.. is <w>x<h>: at most 16777216 pixels`,
  `[IMG-02] static image id `<id>` is registered twice with different bytes`.
- Draw it with `<img src={LOGO} />` or `noctis::image::img(LOGO, classes)`
  (`Node::image(LOGO)`). Unsized it takes the image's own size; `tw`/`style` size it
  like any node (`tw="w-40 h-40 rounded-full"`). Put it in an `items-start`/`items-center`
  flex row or give it a size, or a flex parent stretches it.
- An `<img>` whose image the App did not register (or registered with other bytes)
  fails the render with `RenderError::UnknownImage { id, image }`.

**Unicode emoji** (`[IMG-04]`). With the `image-emoji` feature, Noctis bundles the
Twemoji Mozilla COLRv0 font (twemoji-colr-font 15.0.3) and registers it as a
**last-resort** emoji font, after every `ImageConfig::font` and the bundled Inter. Emoji
(`😀`, `👍🏽`, flags, ZWJ sequences) then draw in colour with no Template change; your
own fonts keep priority (a glyph your font has, such as `©` or `™`, draws from it), and
plain digits, `#` and `*` keep the text font. Without the feature, Unicode emoji draw as
boxes, and an App that registers a Template logs one warning at build (target
`noctis::image`): ``[IMG-04] the `image-emoji` feature is off: Unicode emoji in Template
text render as boxes``. The emoji font is not a text font: with `image-emoji` but no
`default-font` and no `.font(..)`, `App::build` still fails with `[IMG-01] no font is
registered`. Any COLRv0 colour font you register with `ImageConfig::font` works too
(takumi draws COLRv0 only: no CBDT/sbix bitmaps, no COLRv1, so not Noto Color Emoji).
**Attribution:** the emoji graphics are Twemoji, Copyright Twitter, Inc and other
contributors, licensed CC-BY-4.0 (https://creativecommons.org/licenses/by/4.0/); the
font build is Mozilla twemoji-colr (OFL-1.1). A bot that posts rendered images
containing Twemoji should credit them, e.g. in its about/help text: "Emoji graphics by
Twemoji (CC-BY 4.0)". Discord custom emoji are §7 "Custom emoji".

## 7. Remote images

Avatars, guild icons and other remote pictures are **declared** by the Template and
fetched by Noctis before the render; takumi never fetches, and Noctis never fetches an
image no field declares.

```rust
use noctis::image::{Frame, Node, RemoteImage, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]
pub struct ProfileCard {
    pub name: String,
    #[asset]                       // a failure draws the fallback and logs a warning
    pub avatar: RemoteImage,
    #[asset(required)]             // a failure fails the render instead
    pub server: RemoteImage,
    #[asset]
    pub badge: Option<RemoteImage>,  // `None` declares nothing
    #[asset]
    pub friends: Vec<RemoteImage>,   // each element (`friends[0]`, ..) is declared
}

impl TemplateView for ProfileCard {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface">
                <img src={&self.avatar} tw="w-40 h-40 rounded-full" />
                <span tw="text-5xl text-white">{&self.name}</span>
            </div>
        }
    }
}

// in the handler:
let card = ProfileCard {
    name: user.name.clone(),
    avatar: RemoteImage::avatar(&user, 256),
    server: RemoteImage::guild_icon(&guild, 128),
    badge: None,
    friends: vec![],
};
```

- **Field types.** An `#[asset]` field is `RemoteImage`, `Option<RemoteImage>` or
  `Vec<RemoteImage>` (tuple-struct fields work too); anything else is the compile
  error ``[IMG-03] `#[asset]` fields are `RemoteImage`, `Option<RemoteImage>` or
  `Vec<RemoteImage>` ``. `#[asset(optional)]`, `#[asset = ..]` and
  `#[asset(required, required)]` are ``[IMG-03] `#[asset]` takes no argument or
  `required` ``; two `#[asset]` on one field are an error too.
- **Drawing.** `<img src={&self.avatar} />` (or `img(&self.avatar, classes)`;
  `img`/`Node::image` take `impl Into<ImageSrc>`: a `StaticImage` or a `RemoteImage`, by
  value or by reference). Drawing a remote image that no `#[asset]` field holds fails
  with `RenderError::UndeclaredImage { id, image }`
  (``[IMG-03] template `<id>` draws remote image `<image>`, which no `#[asset]` field
  declares``).
- **Constructors.**
  - `RemoteImage::avatar(&user, size)`: `<cdn>/avatars/<id>/<hash>.webp?size=<size>`, or
    the default avatar `<cdn>/embed/avatars/<i>.png` when the user has none (`(id >> 22) % 6`
    for discriminator 0, else `discriminator % 5`).
  - `RemoteImage::guild_icon(&guild, size)`: `<cdn>/icons/<id>/<hash>.webp?size=<size>`;
    a guild without an icon gives `RemoteImage::placeholder()`.
  - `RemoteImage::url(text) -> Result<RemoteImage, RemoteUrlError>`: at most 2048
    printable ASCII bytes (also after canonicalization), `https://` or `http://`, no
    user info or fragment (`RemoteUrlError::{Empty, TooLong, InvalidCharacter, Scheme, UserInfo,
    Fragment, NoHost, Malformed, InvalidHost}`). The scheme and host are lowercased;
    the path and query are kept. Its origin must be allow-listed when it renders;
    plain `http://` can be allow-listed only for a loopback host, so any other
    `http://` URL draws the fallback (with a warning).
  - `RemoteImage::placeholder()`: always draws the fallback, with no fetch and no log,
    even in a required field.
  - `size` is normalized to a Discord CDN size: the next power of two, clamped to
    16..=4096 (`200` → `256`, `0` → `16`, `5000` → `4096`).
- **Animated avatars and icons.** An `a_` hash requests `&animated=true` only when the
  job is animated (an `animation(..)` Template or `ctx.render_with(.., animation)`); the
  avatar then animates with the frames (animated WebP is drawn at each frame's time).
  A still render requests the still image. GIF avatars are never requested.
- **Allow-list and limits** (`RemoteConfig`, set with `ImageConfig::remote(..)`; checked
  at `App::build`, an invalid value is a plugin violation ``[IMG-03] remote <name> <v>
  is out of range: use <range>`` or ``[IMG-03] remote origin `<s>` is invalid: <reason>``):

  ```rust
  use std::time::Duration;
  use noctis::image::{ImageConfig, RemoteConfig, StaticImage};

  const NO_AVATAR: StaticImage = StaticImage::new("no-avatar", include_bytes!("../assets/no-avatar.png"));

  let config = ImageConfig::new().remote(
      RemoteConfig::new()
          .allow_origin("https://i.imgur.com")      // scheme://host[:port], optional trailing `/`
          .timeout(Duration::from_secs(3))           // 100 ms..=60 s, default 5 s
          .max_bytes(4 * 1024 * 1024)                // 1024..=67108864, default 8 MiB
          .concurrency(8)                            // 1..=256, default 8 (per App)
          .max_per_job(16)                           // 1..=64, default 16
          .max_pixels(4_194_304)                     // 1..=16777216, default 4 MP
          .cache_ttl(Duration::from_secs(86_400))    // 1 s..=30 days, default 1 day
          .fallback(NO_AVATAR),                      // default: a 1x1 #475569 pixel
  );
  ```

  The default allow-list is `https://cdn.discordapp.com` and
  `https://media.discordapp.net`. An origin is `https`, or `http` only for `127.0.0.1`,
  `[::1]` and `localhost` (else "plain http is only for loopback hosts"); a lowercase
  `[a-z0-9.-]` host or an IP literal; no user info, path, query, fragment or wildcard.
  Scheme, host and effective port must match exactly (`https://cdn.discordapp.com:444`
  and `https://cdn.discordapp.com.evil.com` are not allowed). Redirects are never
  followed (a 3xx is a failure). `RemoteConfig::discord_cdn(origin)` changes the origin
  avatar and icon URLs are built on (a media proxy; a loopback server in tests); it is
  always allow-listed. Duplicates are ignored. The fallback is checked with the IMG-02
  rules at build.
- **Resolution.** Before the job enters the render pool, every declared image (deduplicated)
  is fetched with a `GET` (`user-agent: noctis/<version>`, no cookies, no proxy) under one
  per-App concurrency permit; the timeout covers the permit wait, the request and the
  body; a `content-length` over `max_bytes` fails before the body; the body must be PNG,
  JPEG or WebP (GIF, SVG, BMP, ICO and HTML fail) with `width * height <= max_pixels`;
  it is decoded on tokio's blocking pool. More than `max_per_job` distinct images is
  `RenderError::OverCap { cap: RenderCap::RemoteImages, .. }` before any fetch.
  Templates with `#[asset]` fields are admitted to the queue **after** their fetches,
  so their `QueueFull` can arrive after fetch time; Templates without them are admitted
  at the call. Dropping the `ctx.render` future aborts pending fetches.
- **Failures.** A blocked or failed image (not allow-listed, timeout, `HTTP 404`,
  `redirect not followed`, over the byte or pixel cap, unsupported format, not
  decodable, I/O error) logs one warning (target `noctis::image`, fields `template`,
  `field`, `image`, `reason`; the image is shown without its URL query) and draws the
  fallback. In an `#[asset(required)]` field it fails the render with
  `RenderError::RemoteImage { id, image, reason }`
  (``[IMG-03] template `<id>` requires remote image `<image>`, which could not be used:
  HTTP 404``), reported to the Render Error Sink like any render failure.
- **Caching.** Fetched bytes go to the App's namespaced `AssetStore` (`App::builder()
  .asset_store(..)`; the bounded in-memory default otherwise) with TTL `cache_ttl`,
  under `img:v1:avatar:<user>:<hash>:<size>:<a|s>`, `img:v1:icon:<guild>:<hash>:<size>:<a|s>`,
  `img:v1:default-avatar:<i>` or `img:v1:url:<blake3 of the URL>`. A changed avatar is a
  new hash, so a new key. Cached bytes are checked again on read (bad bytes are
  refetched); a store error logs a warning and never fails the render; failures are
  never cached.
- **Testing.** In unit tests, `RemoteImage::placeholder()` keeps a Template renderable
  without network; for end-to-end tests point `RemoteConfig::discord_cdn("http://127.0.0.1:<port>")`
  at a local server.

### Custom emoji

Discord custom emoji tokens in Template **data** render as fetched images, automatically,
with or without `image-emoji` (`[IMG-04]`):

```rust
#[derive(Template, Serialize, Deserialize)]
#[template(id = "quote.card", version = 1, size = (800, 200), format = Png)]
pub struct QuoteCard { pub text: String }          // e.g. msg.content, unchanged

impl TemplateView for QuoteCard {
    fn view(&self, _frame: Frame) -> Node {
        view! { <p tw="text-3xl text-white">{&self.text}</p> }   // "gg <:pepe:123456789012345678>"
    }
}
```

- **Grammar.** Discord's markdown form, ASCII only: `<:name:id>` (static) or
  `<a:name:id>` (animated); `name` is 2..=32 of `[A-Za-z0-9_]`, `id` is 17..=20 digits
  (a non-zero `u64`). Anything else stays literal text (`<:p:1>`, `<A:x:..>`, `< :x:..>`).
  Mentions, timestamps and `\` escapes are not parsed.
- **Found automatically.** Before the render, Noctis scans the Template's serialized
  data: every token in **any** `String` field (also nested, in `Vec`s and `Option`s) is
  found, deduplicated by `(id, animated)` (the name is ignored), and fetched through the
  same fetcher as `#[asset]` images (allow-list, timeout, byte and pixel caps,
  concurrency, `AssetStore` cache) from `<cdn>/emojis/<id>.webp?size=128`, plus
  `&animated=true` for an `<a:..>` emoji in an animated job (it then animates). Cache
  key: `img:v1:emoji:<id>:128:<a|s>`. Nothing is annotated.
- **Drawn inline.** A text node holding a token draws as an inline run: the text around
  it, and each emoji as a `1.375em` square image (Discord's inline size) on the same
  line, vertically aligned like Discord. The markup itself is never drawn. Size the text
  with `text-*` classes as usual; the emoji follows the font size.
- **Cap.** `RemoteConfig::max_emoji(n)` (0..=128, default 32; out of range is the plugin
  violation ``[IMG-04] remote max_emoji <v> is out of range: use 0..=128``): the first
  `n` distinct emoji of a job are fetched; the rest draw the fallback with one warning
  (`<n> custom emoji over the per-job cap of <max> draw the fallback`). `max_emoji(0)`
  fetches none. Emoji never count against `max_per_job` and never fail a render, so user
  text cannot break a card.
- **Failures** (404, timeout, GIF body, not decodable, ..) draw the fallback inline with
  the usual warning naming `emoji/<id>`.
- **Admission.** A Template whose data holds a token is admitted to the queue after its
  fetches, like one with `#[asset]` fields; data without a token keeps call-order
  admission.
- **Literal text is a compile error.** `view! { <p>"hi <:pepe:123456789012345678>"</p> }`
  fails with ``[IMG-04] custom emoji in literal `view!` text are never fetched: put the
  text in a Template field, or declare `#[asset] e: RemoteImage`
  (`RemoteImage::custom_emoji(id, animated)`) and draw `<img src={&self.e} />` ``.
- **Text the view builds itself** (`format!("<:x:{id}>")` with an id that is not in the
  data) is never fetched: drawing it fails with `RenderError::UndeclaredImage { id, image:
  "emoji/<id>" }`. Transforming data text (`to_uppercase()`, trimming) is fine: the
  lookup uses only the id and the `a` flag.
- **An emoji as an image.** `RemoteImage::custom_emoji(id: Id<EmojiMarker>, animated)`
  in an `#[asset]` field, drawn with `<img src={&self.emoji} tw="w-16 h-16" />`, e.g. a
  big reaction emoji. Its display (in logs and errors) is `emoji/<id>`.

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
  (`<16 hex>.png`/`.jpg`/`.webp`) in the Template's still `format`; an animated
  Template (`animation(..)`) renders animated, ext `webp` (default), `gif` or `png`
  (APNG). The output always fits the size budget (below), so it never fails ATT-01 at
  upload. The budget is per file (ATT-01); ATT-02 (25 MiB per request) still limits a
  whole message, and the 2 MiB default keeps up to 10 renders under it. Always render **in
  the handler**, never in a render function; the auto-defer covers a render slower
  than the ack deadline (type 5, then the edit carrying the file).
- Errors are a `HandlerError` wrapping a `RenderError`, found with
  `err.as_error().downcast_ref::<RenderError>()`:
  - `QueueFull`: the render queue is full; show a "busy, try again" notice;
  - `NoRenderer`: no `.theme(..)`/`.image(..)` on the builder;
  - `UnknownTemplate { id, version }`: not registered with `Module::template` (or a
    version skew);
  - `Unsupported { id, why }`: an animation with `fps` 0 or a duration under 1 ms, or
    GIF/APNG output without `image-gif` (a hand-built `RenderJob`; the derive already
    refuses both at compile time);
  - `OverCap { id, cap, limit, actual }`: the job is over one of the App's per-job
    caps (below) or its animation over the format's fps cap; `cap` is a `RenderCap`
    (`JobPixels`, `FramePixels`, `Frames`, `Duration`, `Fps`, `RemoteImages`;
    `cap.as_str()` is `job_pixels`/`frame_pixels`/`frames`/`duration_ms`/`fps`/
    `remote_images`), `limit` and `actual` are pixels, frames, whole milliseconds,
    frames per second or distinct remote images (§7). It never entered the
    queue; make the Template smaller or shorter, lower the fps, or raise the cap (the
    fps cap is fixed: `AnimatedFormat::max_fps()`, GIF 50, WebP/APNG 90);
  - `SizeMismatch { id, frame, expected, declared }`: frame `frame` of an animation
    returned a root that declares an explicit pixel size `declared` (`(w, h)`) other
    than the Template's `expected` size (below, "Animation");
  - `OverBudget { budget, smallest }`: even the smallest encoding of the size budget's
    ladder (`smallest` bytes) is over `budget`; shrink the Template, lower its size or
    quality, or keep `still_fallback` on;
  - `UnknownImage { id, image }`: the view drew a `StaticImage` the App did not
    register with `ImageConfig::image` (§6);
  - `RemoteImage { id, image, reason }`: an `#[asset(required)]` remote image could
    not be used (§7);
  - `UndeclaredImage { id, image }`: the view drew a `RemoteImage` no `#[asset]` field
    declares (§7);
  - `NoFont`: only from `App::build` (a Template with no font registered, §6);
  - `Failed(source)`: decode, class check (Deny), takumi or encode failure;
  - `Panicked { message }`: the view panicked; the pool survives.
- The pool: `builder.image(ImageConfig::new().threads(4).queue_capacity(128))`.
  `threads` is 1..=256 (default half the available parallelism, at least 1),
  `queue_capacity` 1..=4096 (default 64), counting admission units of queued plus
  running renders; out-of-range values fail `App::build` with a plugin violation.
  Renders run on this dedicated rayon pool (`noctis-render-<i>` threads), never on
  tokio workers or rayon's global pool.
- Admission is weighted: a still weighs 1 unit, an animation `ceil(frames / 8)` units
  (at least 1, at most the animation share). A quarter of `queue_capacity`, floored, is
  reserved for stills, so animations hold at most the other three quarters (default 64:
  16 reserved, 48 for animations, so a 90-frame animation weighs 12 and four are admitted
  at once). A free pool thread always takes a queued still before any animation. A job
  that does not fit fails at once with `QueueFull`; there is no waiting queue.
- Per-job caps per App: `ImageConfig::new().caps(RenderCaps::new().frames(120))`.
  `RenderCaps::new()` (= `default()`) is 4 MP (`job_pixels`, 4_000_000, `w*h` of every
  job), 1 MP per frame (`frame_pixels`, 1_000_000, animations only), 90 `frames` and 6 s
  `duration` (whole ms); setters `.job_pixels(u64)`, `.frame_pixels(u64)`,
  `.frames(u32)`, `.duration(Duration)`. Limits are inclusive and checked before
  admission in that order (pixels, then duration, then frames), after the fps/duration
  and `image-gif` checks; the first exceeded is the `OverCap`. Out-of-range caps fail
  `App::build`: `render cap job_pixels {n} is out of range: use 1..=67108864` (same for
  `frame_pixels`), `render cap frames 0 is out of range: use at least 1`,
  `render cap duration {d:?} is out of range: use at least 1 ms`. Caps are not pool
  settings: Apps differing only in them share one pool.
- Giving up: dropping the `ctx.render` future (a cancelled handler, the shutdown grace,
  your own `tokio::time::timeout` or `select!`) cancels the render: a queued job is
  skipped and frees its units, and a running one stops before its next frame or encode
  attempt. An expired interaction token does **not** cancel a render (public messages
  fall back to bot-token routes); wrap `ctx.render` in `tokio::time::timeout` when you
  want a deadline.
- The size budget per App: `ImageConfig::new().size_target(1_048_576)` (bytes, at
  least 1024, default 2 MiB = 2_097_152; below 1024 `App::build` fails with the plugin
  violation `render size target {n} is out of range: use at least 1024 bytes`) and
  `.still_fallback(false)` (default `true`). These two are not pool settings: Apps
  differing only in them share one pool.
- The pool is **process-scoped**: every App on one `Runtime` shares it, so they must
  pass equal `threads` and `queue_capacity`; another one fails `build` with
  `BuildViolation::ResourceConflict` naming `RenderPool`. Each `Runtime::for_test()`
  has its own pool.
- `ImagePlugin` is added by `AppBuilderImageExt::image`/`theme` (each call updates
  the one plugin; the last call of each kind wins, order does not matter, never a
  `DuplicatePlugin`). `app.has_renderer()` / `app.renderer()` expose the result.
- At `App::build` the Renderer receives every `Module::template` registration; a
  hand-written `Template` (view not from `#[derive(Template)]`) fails the build with
  `BuildViolation::TemplateRejected`. Templates registered without any
  Renderer only log a warning on target `noctis::render`.
- The Render Error Sink (`error_sink(ErrorOrigin::Render, ..)`) gets every render
  failure (`ReportKind::Error`) and panic (`ReportKind::Panic`), except `QueueFull` and
  `NoRenderer`. A failure your handler also propagates with `?` is reported once more
  by the handler's own origin.
- Observability: span `noctis.render{app, template}`; metrics
  `noctis_render_duration_seconds{app, template}`, `noctis_render_queue_depth{app}`
  (this App's `ctx.render` calls awaiting a render; a cancelled call stops counting, and
  its job is skipped while queued or stops before its next frame) and
  `noctis_render_rejected_total{app, reason}` (`queue_full`, `unknown_template`,
  `unsupported`, `over_cap`; an fps over the format's cap counts as `over_cap`).
  `OverCap` and `SizeMismatch` are also reported to the Render Error Sink
  (`SizeMismatch` and `OverBudget` have no rejected reason).
- On `Runtime::for_test()` Apps an unknown `Classes::dynamic` class fails the render
  (Deny, §4).
- Per-call options: `ctx.render_with(t, RenderOptions::new().animation(spec))` renders
  this call as the animation `spec` (an `AnimationSpec`), in place of the Template's
  own `ANIMATION` or its still image; `RenderOptions::new()` (= `default()`) changes
  nothing, and `ctx.render(t)` is `ctx.render_with(t, RenderOptions::new())`.
  `ctx.render_report(t, options)` returns a `RenderReport` (non-exhaustive) with
  `attachment`, `format` (`OutputFormat`) and `settings_used` (`EncodeSettings`, after
  any degradation to fit the budget). Both live on the same contexts as `ctx.render`
  and return the same errors.
- The fps cap per format: GIF 50, WebP and APNG 90 (`AnimatedFormat::max_fps()`;
  `AnimationSpec::check_fps(id)`). Core checks it before calling the Renderer (the
  Renderer is never called), and the takumi Renderer checks it again for a hand-built
  `RenderJob`, after the fps/duration and `image-gif` checks and before the caps. The
  derive already bounds `fps` at compile time (1..=90, GIF 1..=50).

### Animation

- CSS-driven: `animate-spin|ping|pulse|bounce` (built in), `animate-<token>` for a
  theme `--animate-<token>` (with its `@keyframes`), or an arbitrary single animation
  `animate-[slide_1s_ease-in-out_infinite_alternate]` (the §5 value grammar with `_`
  for spaces) with `@keyframes slide` in the theme; `animate-[spin_2s_linear]` needs
  no theme. An undeclared one is a `[TW-01]` compile error naming the missing
  declaration: "or declare `--animate-wiggle` (and its `@keyframes`) in
  noctis-theme.css" for `animate-wiggle`, "or declare `@keyframes wiggle`" for
  `animate-[wiggle_1s]`. Render with an animated `format`,
  `fps` and `duration` (the Template's `animation(..)` or `ctx.render_with`): takumi
  draws each frame at its start time, so the frames follow the keyframes. A still
  draws the animation at 0 ms (for `rise … both`, the invisible first keyframe), so
  give CSS-animated Templates an `animation(..)`.

  ```rust
  #[derive(Template, Serialize, Deserialize)]
  #[template(id = "badge.pop", version = 1, size = (128, 128), format = Png,
             animation(duration_ms = 1200, fps = 20))]
  struct Badge;

  impl TemplateView for Badge {
      fn view(&self, _frame: Frame) -> Node {
          view! { <div tw="animate-rise w-full h-full rounded-full bg-brand" /> }
      }
  }
  ```
- Code-driven: one data value, one Template state per frame. The view receives
  `Frame { index, count }` and computes that frame's state, for example
  `let value = self.target * (frame.index + 1) / frame.count;`. Declare
  `animation(duration_ms = 2000, fps = 20)` on the Template and render it with
  `ctx.render(card).await?` as usual: the view is called once per frame, on the
  render pool. Frame `i` starts at `i*1000/fps` ms and lasts until the next one (the
  last ends at `duration_ms`); `count` is `ceil(duration_ms * fps / 1000)` (2000 ms at
  20 fps: 40 frames). Keep `count` within the frames cap (default 90) and
  `duration_ms` within the duration cap (default 6000), and each frame within 1 MP,
  or the render fails before it is queued with `RenderError::OverCap` (§8, "Per-job
  caps"). Override or add the animation per call with
  `ctx.render_with(card, RenderOptions::new().animation(AnimationSpec::new(Duration::from_secs(2), 20, AnimatedFormat::DEFAULT)))`;
  the fps must stay within the format's cap (GIF 50, WebP/APNG 90).
- Every frame has the Template's size. A frame whose root declares another explicit
  pixel size fails the render with `RenderError::SizeMismatch { frame, declared, .. }`:
  the root's `Style::width_px`/`height_px`, or a root class `w-[Npx]`, `h-[Npx]` or
  `size-[Npx]` without a variant. Relative sizes (`w-full`, `w-64`, `width_pct`) and
  child nodes are not checked, and neither are stills. Keep the root at
  `w-full h-full` and animate children.
- Default animated format: animated WebP (smaller and ~3× faster than GIF); GIF with
  `image-gif` for maximum compatibility. Design GIF Templates with flat colours.

### Size budget

Every render fits `budget = min(Upload Budget, ImageConfig::size_target)`. The Upload
Budget is the interaction's `attachment_size_limit` (what Discord says this upload
accepts), else `Limits::attachment_bytes` (20 MiB); `ctx.render` writes it into
`RenderJob::upload_budget` for you (`RenderJob::of` alone sets
`RenderJob::DEFAULT_UPLOAD_BUDGET`, 20_971_520). When the requested encoding is over
budget, the Renderer walks one fixed, cumulative ladder and returns the first attempt
that fits (each step keeps the earlier steps' most degraded values):

1. the requested settings (quality `q` of `Jpeg`/`Webp`/animated `Webp`; none for
   `Png`, `WebpLossless`, `Gif`, `Apng`);
2. fps (animated only): `F/2`, then `F/4`, each kept only when it is ≥ 5 and lower
   than the current fps (20 → 10 → 5; 90 → 45 → 22; 9 stays 9);
3. size: 3/4, then 1/2 of the Template size (floored, each side ≥ 1; the frames are
   resampled, not re-laid out, so the layout never changes);
4. quality (lossy only): 75, 50, 25, each kept only when lower than the current one;
5. a still frame (animated only, when `still_fallback` is on): frame 0 at the last
   size, in the Template's still `format` (quality `min(q, 25)` for a lossy one).

If no attempt fits, the render fails with `RenderError::OverBudget { budget, smallest
}`. What was actually encoded is in `RenderOutput::settings_used`
(`EncodeSettings { size, quality, fps, frames }`; `frames` is 1 for a still or the
still fallback, whose `RenderOutput::format` is the still format), read through
`app.renderer()`:

```rust
use noctis::image::RenderJob;

let job = RenderJob::of(&card)?;                                 // upload_budget 20 MiB
let out = app.renderer().expect("a Renderer").render(job).await?;
assert!(out.bytes.len() as u64 <= 2_097_152);                    // default size_target
println!("{:?} {}", out.settings_used, out.format.extension());
```

`ctx.render` returns only the `Attachment`; `ctx.render_report(t, RenderOptions::new())`
returns the same through a handler as a `RenderReport` (`report.attachment`,
`report.format`, `report.settings_used`).

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
