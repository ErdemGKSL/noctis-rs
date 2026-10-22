---
title: 'Story 10.2: Templates with compile-time class validation and a Theme'
type: 'feature'
created: '2026-10-22'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-1-strict-tailwind-v4-grammar-and-takumi-cross-check.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 10.1 shipped the strict Tailwind grammar (`noctis_image::tw`), but nothing uses it. A bot author cannot write an image Template, declare a Theme or get a compile error for a mistyped class; `Module::template::<T>()`, `#[derive(Template)]`, `view!`, `tw!` and `theme!` (all already used by the project template) do not exist (FR-32, FR-66, FR-75, UX-DR2; AD-2, AD-15, AD-22).

**Approach:**
- `noctis-image` (behind `render`) gains a Noctis-owned, takumi-free node tree: `Node`, `Tag`, `Classes`, `Style` (typed setters), `IntoNodes`. It also gains `TemplateView`, `ViewFn` (the `ErasedView` decoder) and the runtime `ClassCheck`.
- `noctis_image::tw::theme` (takumi-free) parses `noctis-theme.css` with cssparser.
- `noctis-macros` (new `image` feature) adds `#[derive(Template)]`, `view!`, `tw!` and `theme!`. Each one validates literal classes with `tw::validate` against the calling crate's theme.
- `noctis-core` adds `Module::template::<T>()` and rejects duplicate Template ids at `App::build`.
- The facade's `image` feature wires everything as `noctis::image`.
- The Usage Skill, the template's profile card, the API snapshot and the rules catalog (new TW-01, TW-02) are updated.

## Boundaries & Constraints

**Always:**
- **Layers (deps-check must pass unchanged in rules).**
  - In `noctis-image`, `tw` (including the new `tw::theme`) stays takumi-free and ungated. Its new ungated deps are `cssparser.workspace = true` (root `[workspace.dependencies]` already has `cssparser = "0.38"`; Cargo.lock already has 0.38.0) and `blake3.workspace = true`.
  - Everything else new in `noctis-image` sits behind `render`. Add `noctis-ports = { workspace = true, optional = true }` and `tracing = { workspace = true, optional = true }`, then set `render = ["dep:takumi", "dep:takumi-core", "dep:noctis-ports", "dep:tracing"]`.
  - In `noctis-macros`, add `noctis-image = { workspace = true, optional = true, default-features = false }` and the feature `image = ["dep:noctis-image"]`. The macros never enable `render`.
  - In the facade `noctis`, add `noctis-image = { workspace = true, optional = true }` and set `image = ["dep:noctis-image", "noctis-image/render", "noctis-macros/image"]`. Without `image`, `cargo tree -p noctis -e normal` shows no `noctis-image`, `cssparser` or `takumi*`.
  - Update the doc comment in `xtask/src/deps_check.rs`: the `tw` module now includes `tw::theme`.
- **Macro hygiene (AD-2).** Macro output names runtime items only through `::noctis::__private::{image, ports}`. The facade adds `pub use noctis_ports as ports;` to `__private` (ungated) and `#[cfg(feature = "image")] pub use noctis_image as image;`.
- **Template contract (AD-22, unchanged ports).** For `#[derive(Template)]` on struct `X`, the macro emits `impl ::noctis::__private::ports::Template for X` with:
  - `ID`, `VERSION`, `SIZE` (`Size::new(w, h)`), `FORMAT` and `ANIMATION` (`None`, or `Some(AnimationSpec::new(Duration::from_millis(ms), fps, fmt))`);
  - `fn view() -> ErasedView { ::noctis::__private::image::erased_view::<Self>() }`.

  `erased_view<T: Template + TemplateView>()` returns `ErasedView::new(ViewFn(decode_view::<T>))`. `decode_view` runs `noctis_ports::decode_template::<T>(data)`, maps an error with `RenderError::failed`, and returns `Ok(value.view(frame))`. `ViewFn` is `pub struct ViewFn(fn(&[u8], Frame) -> Result<Node, RenderError>)` (private field) with `pub fn call(&self, data: &[u8], frame: Frame) -> Result<Node, RenderError>`.
  - `TemplateView` is `pub trait TemplateView { fn view(&self, frame: Frame) -> Node; }`. It carries `#[diagnostic::on_unimplemented(message = "`{Self}` derives `Template` but does not implement `TemplateView`", note = "add `impl TemplateView for {Self} { fn view(&self, frame: Frame) -> Node { view! { .. } } }`")]`.
  - `Serialize + DeserializeOwned` stays enforced by the `Template` supertraits.
- **`#[template(..)]` grammar.** The attribute is required and appears once. Its keys are unique and appear in any order:
  - `id = "<lit>"` (required): `[a-z0-9._-]{1,64}`.
  - `version = <u32 int lit>` (required): `0..=u32::MAX`, with no suffix other than `u32`.
  - `size = (<w>, <h>)` (required): integer literals `1..=u32::MAX`.
  - `format = Png | Jpeg { quality: N } | Webp { quality: N } | WebpLossless` (required): `N` is `1..=100`.
  - `animation(duration_ms = N, fps = N, format = Webp { quality: N } | Gif | Apng)` (optional; all three keys required): `duration_ms` is `1..=u32::MAX`; `fps` is `1..=90` for Webp and Apng and `1..=50` for Gif.

  Any other key, form, enum variant (an animated format in `format` is the ATT-07 case), duplicate, missing key or out-of-range value is a `compile_error!` spanned on the offending token. Its message names the key, the accepted forms or range and the value.
  - The derive accepts only structs without generic or lifetime parameters.
  - It rejects every field whose type mentions a path segment `HashMap` or `HashSet` at any depth (`Vec<HashMap<..>>`, `Option<std::collections::HashSet<_>>`, …) with: ``Template data must serialize deterministically (it is part of the render cache key): `HashMap` iterates in random order; use `BTreeMap`, `BTreeSet` or `Vec` ``.
  - `#[asset]` is a registered helper attribute. Any use is the error ```#[asset]` remote images are planned (Story 10.8)``.
- **Node model (`noctis-image/src/node/`, re-exported at the `noctis_image` root).**
  - `Node` (`Debug, Clone, PartialEq`) is either an element (`Tag` + `Classes` + `Style` + children) or a text node (`Cow<'static, str>`, plain text that is never parsed).
  - `Tag` is `#[non_exhaustive]` with `Div, Span, P, H1, H2, H3`. Tags carry no default styles.
  - Constructors and builders: `Node::element(Tag)`, `Node::text(impl Into<Cow<'static, str>>)`, `.classes(Classes)` (later call wins), `.style(Style)` (later wins), `.child(impl IntoNodes)` and `.children(impl IntoIterator<Item: IntoNodes>)`. A builder on a text node is a no-op, except `.classes` and `.style`, which apply to text too.
  - Free functions: `div(Classes) -> Node`, `span(Classes) -> Node` and `text(impl Into<Cow<'static, str>>, Classes) -> Node`.
  - `IntoNodes` (sealed) yields 0..n nodes. It covers `Node`, `&str` and `String`/`&String`/`Cow<'static, str>` (text), the integer primitives and `f32`/`f64` (their `Display` text), `Option<T>` of these (None = nothing) and `Vec<Node>`. No `IntoNodes` impl exists for `Classes`, and nothing turns a string into markup.
  - `Classes` (`Debug, Clone, PartialEq, Eq, Default`) has these constructors:
    - `#[doc(hidden)] pub const fn __literal(&'static str)`: only the macros call it, marked checked;
    - `Classes::dynamic(impl Into<Cow<'static, str>>)`: unchecked, and verified by `ClassCheck`. Its docs say: never pass user text;
    - `.and(Classes) -> Classes`: joins with one space and stays checked only if both sides are;
    - `as_str()`.
  - `Style` (`Debug, Clone, Copy, PartialEq, Default`; `Style::new()`) has these setters:
    - `width_px(u32)`, `height_px(u32)` and `gap_px(u32)`;
    - `width_pct(f32)` and `height_pct(f32)`, clamped to `0..=100`;
    - `bg_rgb(u32)`, `color_rgb(u32)` and `border_color_rgb(u32)`, which use the low 24 bits;
    - `opacity(f32)`, clamped to `0..=1`;
    - `translate_px(i32, i32)`;
    - `rotate_deg(f32)`.

    A NaN or infinite `f32` leaves that property unchanged. The last setter on a property wins. Style values override class values for the same property (documented; applied by the 10.3 Renderer). Fields are `pub(crate)`.
- **`view!` grammar.**
  - The body is exactly one root element.
  - Element: `<tag attr*>child*</tag>` or `<tag attr* />`, where tag ∈ `div span p h1 h2 h3`.
  - Attributes: `tw = "lit"` (validated), `tw = {expr}` (must be `Classes`; type error otherwise) and `style = {expr}` (must be `Style`). Each attribute appears at most once.
  - Children: an element, `"lit"` (text), `{expr}` (`IntoNodes`), `if cond { child* } [else if .. | else { child* }]` and `for pat in expr { child* }`.
  - Expansion uses the builder calls above, and the result is a `Node` expression.
  - Errors, each spanned:
    - an empty body;
    - two roots;
    - a mismatched or missing closing tag;
    - an unknown tag. `img` and `svg` get ``<img>`/`<svg>` are planned (Story 10.7)``;
    - an unknown attribute (``use `tw` for classes`` when it is `class`);
    - a duplicate attribute;
    - bare text that is not a string literal (``text must be a string literal or `{expr}` ``).
- **Class validation at expansion (TW-01).** `tw!("lit")` expands to a `Classes` const expression. It and every `tw = "lit"` run `noctis_image::tw::validate(lit, &theme)`. Each `UnknownClass` becomes one `compile_error!` on the literal: `[TW-01] ` + that class's `InvalidClasses` display line. When `classify` returns a `Theme` verdict, the message adds ``; or declare `<first namespace css_prefix><token>` in noctis-theme.css``. The macros emit errors for all classes at once.
- **Theme file (TW-02, `noctis_image::tw::theme`).**
  - API:
    - `Theme::parse(&str) -> Result<Theme, ThemeError>`;
    - `Theme::empty()`;
    - `#[doc(hidden)] Theme::__embedded(&'static str)`, which panics with "validated at compile time" on error;
    - `digest() -> [u8; 32]`, the blake3 of the source text (empty theme = blake3 of `""`);
    - `animations()` and `keyframes()` (name + raw block text) for Story 10.6;
    - `impl ThemeTokens for Theme`.
  - Input:
    - Comments are allowed.
    - At the top level, only `@theme { … }` (no prelude tokens, so `@theme inline` is an error) and `@keyframes <ident> { … }`.
    - Inside `@theme`, only `--<name>: <value>;`. Each name maps to the longest matching `ThemeNamespace::css_prefix` (so `--font-weight-x` is FontWeight), or to `--animate-` (an animation entry).
    - The remaining token must match the 10.1 token grammar `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` with at most 64 bytes.
    - Values are stored trimmed and verbatim, and must be non-empty.
    - Several `@theme` blocks merge.
    - The file is at most 256 KiB.
  - Errors: `ThemeError` (`thiserror`, `Debug, Clone, PartialEq, Eq`, `#[non_exhaustive]`, public `line()`/`column()` (1-based) and `kind()`), with `ThemeErrorKind` (`#[non_exhaustive]`) variants:
    - `UnexpectedRule`;
    - `NotCustomProperty`;
    - `UnknownNamespace { name }`, whose message lists every prefix;
    - `InvalidToken { name }`;
    - `EmptyValue { name }`;
    - `Duplicate { name }`, for a custom property or a keyframes name;
    - `BuiltinAnimation { name }`, for `@keyframes` or `--animate-` named `spin`, `ping`, `pulse` or `bounce`;
    - `Syntax`;
    - `TooLarge { bytes }`.

    Display: `[TW-02] <line>:<col>: <message>`.
- **Theme lookup in macros.**
  - `tw!`, `view!` and `#[derive(Template)]` (which has no classes of its own and does not read the theme) read `$CARGO_MANIFEST_DIR/noctis-theme.css`. A missing file is an empty theme. An unreadable or invalid file is a `compile_error!` at the call site carrying the `ThemeError` and the path.
  - When the file exists, each expansion that read it emits `const _: &[u8] = include_bytes!("<absolute path>");` so edits rebuild. A process-local cache keyed by path and file bytes is allowed.
  - `theme!()` embeds that file as `{ const _: &[u8] = include_bytes!(..); Theme::__embedded(include_str!(..)) }`. A missing file is `[TW-02] no noctis-theme.css at <path>`.
  - `theme!("rel.css")` resolves relative to the calling file's directory (`proc_macro::Span::call_site().local_file()`). When that returns `None`, it resolves relative to `CARGO_MANIFEST_DIR`. An absolute path is used as is.
- **Runtime check (TW-01 runtime half, `noctis_image::ClassCheck`).**
  - `ClassCheck::new(ClassCheckMode::{Warn, Deny})` (`#[non_exhaustive]` mode).
  - `check(&self, node: &Node, theme: &Theme) -> Result<usize, InvalidClasses>` walks the tree in pre-order and validates only unchecked (`dynamic`) `Classes`.
    - Warn: each invalid string whose blake3 is not in the remembered set (at most 1024 digests, `std::sync::Mutex`, poison recovered with `into_inner`) logs `tracing::warn!(target: "noctis::image", classes, error, "unknown Tailwind class in a runtime class string")`, with `classes` truncated to 512 bytes at a char boundary plus `…`. The call returns `Ok(number newly warned)`. When the set is full, one extra warning says further strings are not logged, and nothing more is remembered.
    - Deny: it returns `Err` for the first invalid string.
  - It is `Send + Sync`. Story 10.3 calls it from the Renderer, with Deny when `Runtime::is_test()`.
- **Core.**
  - `Module::template<T: noctis_ports::Template>(self) -> Self` records `(TemplateDescriptor::of::<T>(), std::any::type_name::<T>())`.
  - `Registry::fold` collects them in fold order, keeps the first of each `ID` and stores the descriptors (`pub(crate) fn templates(&self) -> &[TemplateDescriptor]`, for Story 10.3).
  - Each later one (any version, including the same type twice) is one `BuildViolation::DuplicateTemplate { id, first_module, first_type, second_module, second_type }` with the message ``Template id `{id}` is registered twice: `{first_type}` (Module `{first_module}`) and `{second_type}` (Module `{second_module}`): give each Template its own id``.
- **Facade `noctis::image`** (`#[cfg(feature = "image")]`, with module docs). It re-exports:
  - the macros `Template` (derive), `view`, `tw` and `theme`;
  - the trait `Template` (from ports, same name in the type namespace), `TemplateView`, `Frame`, `Size`, `Format`, `AnimationSpec`, `AnimatedFormat` and `TemplateDescriptor`;
  - `Node`, `Tag`, `Classes`, `Style`, `IntoNodes`, `div`, `span` and `text`;
  - `Theme`, `ThemeError`, `ThemeErrorKind`, `InvalidClasses`, `UnknownClass` and `Suggestion`.

  `noctis::image::unstable` re-exports `ViewFn`, `ClassCheck` and `ClassCheckMode`, and its docs say semver does not cover them. `noctis::prelude` is unchanged.
- **Rules catalog.** Add `## 16. Image Templates` after §15 Gateway in `docs/discord-rules.md`, with two `[N]` rows:
  - **TW-01**, known Tailwind classes. Enforcement `Compile-time (literal) / Runtime (dynamic class strings)`. How: `tw!`/`view!` + `ClassCheck`.
  - **TW-02**, theme file grammar. Enforcement `Compile-time (theme file read by the macros) / Runtime (`Theme::parse`)`.

  Also:
  - Add `TW_01 = "TW-01", TW_02 = "TW-02"` after `GW_07` in `crates/noctis-rules/src/rule.rs`.
  - Update the ATT-07 How cell to name `#[template(format = ..)]` and `animation(format = ..)`, and remove the ATT-07 entry from `xtask/rules-pending.toml`.

**Never:**
- No Renderer, takumi lowering, render pool, `ImagePlugin`, `.image(..)`/`.theme(..)`, `ctx.render` or `Renderer::register` call (Story 10.3). No fonts or static assets (10.7), `<img>`, `RemoteImage` or `#[asset]` (10.8), and no widening of the animation grammar (10.6).
- No takumi type in any public non-`unstable` item. No `build.rs`, Node.js or Tailwind CLI. Never hand-edit `tw/generated.rs`.
- No string → markup path. No `IntoNodes for Classes`. No `tw = expr` accepting `&str`/`String`.
- No edit to `.cargo/config.toml` or `_bmad/`. No debug builds.

## I/O & Edge-Case Matrix

The test theme is `crates/noctis/noctis-theme.css`, which defines `--color-brand`, `--color-brand-soft`, `--color-surface`, `--color-muted`, `--font-display` and `@keyframes rise`. "CE" means compile error.

| Scenario | Input / State | Expected | Error handling |
|---|---|---|---|
| tw literal ok | `tw!("flex p-4 bg-brand sm:!p-2")` | `Classes` checked, `as_str()` equal to input | — |
| tw empty / blank | `tw!("")`, `tw!("  \t")` | checked `Classes` with that exact text; it validates (no classes) | — |
| tw unknown | `tw!("flx")` | CE `[TW-01] unknown Tailwind class `flx`` on the literal | fixture TW-01 |
| tw v3 | `tw!("bg-gradient-to-r")` | CE with ``; use `bg-linear-to-r` (Tailwind v4)`` | fixture TW-01 |
| tw missing token | `tw!("bg-nope")` | CE adds ``declare `--color-nope` in noctis-theme.css`` | fixture TW-01 (trybuild has no theme: `bg-brand` errors there too) |
| tw several bad | `tw!("flx bad flex")` | two CEs (`flx`, `bad`), deduplicated | — |
| tw non-literal | `tw!(s)`, `tw!(1)` | CE "expected a string literal" | — |
| narrowed stops | `tw!("from-brand")` | CE `[TW-01]` (10.1 palette narrowing) | — |
| theme anim class | `tw!("animate-rise")` | CE `[TW-01]` (10.6) | — |
| view ok | card body (see Tasks) | `Node` equal to the hand-built builder tree | — |
| view empty / two roots | `view!{}`, `view!{<div/><div/>}` | CE "exactly one root element" | — |
| view tags | `<img/>`, `<svg/>`, `<button/>` | CE planned (10.7) / CE unknown tag listing the six | — |
| view closing | `<div></span>`, `<div>` unclosed | CE mismatched / missing closing tag | — |
| view attrs | `class="x"`, `tw="a" tw="b"`, `onclick={f}` | CE ``use `tw` ``, duplicate, unknown | — |
| view tw string expr | `tw={name}` with `name: String` | rustc type error (expected `Classes`) | fixture types-image |
| view text | `{&self.name}` with `"<b tw=\"hidden\">"` | one text node with that exact string | — |
| view children | `if`/`else if`/`else`, `for`, `{Option<Node>::None}`, `{vec_of_nodes}`, `{42u8}`, `{1.5f32}` | branches/iterations/0/n nodes, text `"42"`, `"1.5"` | — |
| view bare text | `<p>Hello</p>` | CE text must be a literal or `{expr}` | — |
| classes join | `tw!("p-4").and(Classes::dynamic("x"))` | `"p-4 x"`, unchecked | — |
| style clamp | `width_pct(-1.0)`, `150.0`, `f32::NAN`, `f32::INFINITY` | 0, 100, unchanged, unchanged | — |
| style opacity | `opacity(1.5)`, `-0.1`, NaN | 1, 0, unchanged | — |
| style rgb | `bg_rgb(0x1_5865F2)` | stored `0x5865F2` | — |
| style rotate | `rotate_deg(-720.0)`, `f32::NEG_INFINITY` | -720, unchanged | — |
| style later wins | `width_px(5).width_pct(50.0)` | width = 50 % | — |
| derive ok | `#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]` | consts equal; `ANIMATION == None`; descriptor id/version | — |
| derive anim | `animation(duration_ms = 2000, fps = 20, format = Webp { quality: 80 })` | `Some(AnimationSpec::new(2 s, 20, Webp{80}))` | — |
| id bounds | `""`, 64 chars, 65 chars, `"Profile"`, `"a:b"`, `"ü"` | CE, ok, CE, CE, CE, CE | — |
| version bounds | `0`, `4294967295`, `4294967296`, `-1`, `1u8`, `"1"` | ok, ok, CE, CE, CE, CE | — |
| size bounds | `(1, 1)`, `(0, 5)`, `(5, 0)`, `(1, 2, 3)`, `[1, 2]` | ok, CE, CE, CE, CE | — |
| quality bounds | `Jpeg { quality: 1 }`, `100`, `0`, `101` | ok, ok, CE, CE | — |
| still vs animated | `format = Gif` / `Apng` | CE not a still format | ATT-07 fixture (`// [ATT-07]`) |
| fps bounds | Webp 1 / 90 / 91 / 0; Gif 50 / 51 | ok / ok / CE / CE; ok / CE | — |
| duration | `duration_ms = 0`, `1` | CE, ok | — |
| anim missing key | `animation(fps = 20, format = Gif)` | CE names `duration_ms` | — |
| keys | missing `#[template]`, missing `size`, `id` twice, `colour = 1` | CE each | fixture types-image |
| item shape | enum, `struct X<T>`, `struct X<'a>` | CE | — |
| HashMap | `HashMap<_, _>`, `Vec<HashMap<..>>`, `Option<std::collections::HashSet<u8>>` | CE (determinism message) | fixture types-image |
| no Deserialize | derive without `Deserialize` | CE naming `Deserialize` and `Template` | fixture types-image |
| no TemplateView | derive without the impl | CE with the on_unimplemented message | fixture types-image |
| asset | `#[asset] a: u8` | CE planned (10.8) | — |
| decoder ok | `descriptor.view.downcast_ref::<ViewFn>()` + `call(&RenderJob::of(&v).data, Frame::STILL)` | `Ok(v.view(STILL))` | — |
| decoder frame | `call(data, Frame { index: 3, count: 10 })` on a Template whose view prints the index | the text node holds `"3"` | — |
| decoder bad | data `[]` for a non-empty struct; valid data + one trailing byte | `Err(RenderError::Failed)` with source `ValueError::Malformed` / `Trailing` | — |
| theme ok | test theme | tokens `(Color, brand)`, `(Font, display)`; `keyframes()` has `rise` | — |
| theme longest prefix | `--font-weight-heavy: 900;` | `(FontWeight, heavy)`, not `(Font, weight-heavy)` | — |
| theme animate | `--animate-rise: rise 1s;` | in `animations()`; `animate-rise` still Unknown | — |
| theme empty file | `""`, comments only | `Theme` with no tokens; digest = blake3(`""`) for `""` | — |
| theme errors | `.card { color: red }`; `@import "x";`; `@theme inline {}`; `@theme { color: red; }`; `--foo-x: 1;`; `--color-Brand: #fff;`; `--color-: #fff;`; 65-byte token; `--color-a: ;`; `--color-a` twice (across two blocks); `@keyframes spin {}`; `--animate-pulse: x;`; `@keyframes rise` twice; `@theme {` unclosed; 256 KiB + 1 byte | `UnexpectedRule` ×3, `NotCustomProperty`, `UnknownNamespace`, `InvalidToken` ×3, `EmptyValue`, `Duplicate`, `BuiltinAnimation` ×2, `Duplicate`, `Syntax`, `TooLarge`; each with the correct 1-based line and column | `tw_02_*` unit tests |
| theme unicode | `--color-rød: #f00;` | `InvalidToken` | — |
| theme at 256 KiB | exactly 262 144 bytes (padding comment) | Ok | — |
| theme in macros | invalid `noctis-theme.css` | CE `[TW-02] …` at the call site | via `theme!("../../fixtures/theme/builtin_keyframes.css")` fixture TW-02 |
| theme! missing | `theme!()` without the file / `theme!("nope.css")` | CE `[TW-02] no noctis-theme.css at …` / not found | — |
| class check warn | Warn; tree with `dynamic("flx")` twice + `dynamic("p-4")` | first `check` → `Ok(1)`, second → `Ok(0)` | `tw_01_*` unit test |
| class check deny | Deny; same tree | `Err` listing `flx` | `tw_01_*` unit test |
| class check literal | a unit test builds `Classes::__literal("flx")` directly | skipped: `Ok(0)` in Warn, `Ok(0)` in Deny (literals are trusted) | — |
| class check cap | Warn; 1025 distinct invalid strings | 1024 remembered, then one "not logged" warning, later new strings → `Ok(0)` | — |
| class check long | 10 000-byte invalid string | logged field ≤ 512 bytes + `…` | — |
| class check theme | `dynamic("bg-brand")` with the test theme / `Theme::empty()` | Ok(0) / Warn 1 | — |
| duplicate template | Module a `.template::<A>()`, Module b `.template::<B>()` with the same `ID` | `BuildViolation::DuplicateTemplate` naming both | — |
| same type twice | `.template::<A>().template::<A>()` | one `DuplicateTemplate` | — |
| templates ok | two ids, nested child Module | build ok; `Registry::templates()` in fold order | — |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/render.rs` -- the unchanged contract: `Template`, `ErasedView::{new, downcast_ref}`, `TemplateDescriptor::of`, `RenderJob::of`, `decode_template`, `Frame::STILL`, `Size::new` (a const that panics on 0), `Format`, `AnimatedFormat`, `AnimationSpec::new` and `RenderError::failed`.
- `crates/noctis-image/src/lib.rs`, `Cargo.toml` -- today only `pub mod tw;` and `render = ["dep:takumi","dep:takumi-core"]`. Add the `#[cfg(feature = "render")]` modules `node`, `template` (TemplateView, ViewFn, erased_view, decode_view) and `check` (ClassCheck).
- `crates/noctis-image/src/tw/mod.rs` -- reuse `validate`, `classify`, `ThemeTokens` (`contains(ns, token)`), `NoTheme`, `InvalidClasses::classes()` and `UnknownClass::{class, suggestion}`; `Display` gives one line per class. The token grammar and the 64-byte limit are there too (reuse the helper and make it `pub(crate)`). Add `pub mod theme;` and re-export `Theme`, `ThemeError` and `ThemeErrorKind`.
- `crates/noctis-image/src/tw/generated.rs:13` -- `ThemeNamespace` (11 variants) and `css_prefix()`. There is no `--animate-` namespace, so the Theme handles it itself.
- `crates/noctis-macros/src/lib.rs`, `Cargo.toml` -- macro entry points. Add the modules `template.rs`, `view.rs`, `tw.rs` and `theme.rs`, behind `#[cfg(feature = "image")]`. Follow existing patterns: `common.rs` and `literal.rs` (literal parsing, errors) and `defaults.rs` (key=value argument parsing with duplicate/unknown-key errors).
- `crates/noctis-core/src/module.rs:137` -- the `Module` fields. Add `templates: Vec<(TemplateDescriptor, &'static str)>` and `pub fn template<T: Template>(mut self) -> Self` with a doc example.
- `crates/noctis-core/src/registry.rs:140-240` -- `fold` (pre-order stack). Collect templates beside `route_entries`. Write `fold_templates` like `fold_routes` (:303). Add `templates: Vec<TemplateDescriptor>` to `Registry`.
- `crates/noctis-core/src/error.rs:303` -- `DuplicateRoute` is the model (doc example asserting the message). `BuildViolation` is `Clone + PartialEq + Eq`, so it stores only `&'static str` fields.
- `crates/noctis/src/lib.rs` -- `__private` at :2026; `pub mod ports` at :1813; macro re-exports at :1713-1731; feature docs at :553-559. Add `pub mod image` under the feature.
- `crates/noctis/Cargo.toml` -- `image = []` today. Add `exclude = ["noctis-theme.css"]` under `[package]`.
- `crates/noctis/tests/ui.rs` -- the featureless suite and the `harness` module. Add `#[test] fn ui_image()` gated on `all(feature = "image", not(any(<every other optional feature>)))`. It runs `compile_fail("tests/ui-image/**/*.rs")` and `compile_fail("tests/types-image/**/*.rs")` with the same `harness` asserts. Widen the `harness` cfg so it also compiles for that gate.
- `xtask/src/rules_catalog/mod.rs` -- the fixture layout rules: `tests/ui-image/<area>/<ID>_<slug>.rs` with `.stderr`, no other file types in a suite; F11/F12 registration needs `compile_fail("tests/ui-image/**/*.rs")`. `tests/types-image/` is not a ui suite.
- `xtask/rules-pending.toml` -- the ATT-07 entry (story 10.4). Remove it.
- `docs/discord-rules.md:267` (ATT-07 row) and §15 (:288) -- §16 goes after it.
- `crates/noctis-rules/src/rule.rs:38` -- the `rule_ids!` list in catalog order; a test compares it with the catalog.
- `examples/project-template/src/modules/profile/view.rs:64-110` -- `ProfileCard` and its view. `src/modules/profile/mod.rs:33` -- `.template::<ProfileCard>()`. `src/lib.rs:39` -- `.theme(noctis::image::theme!())` (10.3). `noctis-theme.css` -- its comment claims `from-brand` and `animate-rise` work.
- `examples/project-template/.claude/skills/noctis/references/images.md` §2-§5; `SKILL.md:23` (API hash) and §3.5 (:250).
- `xtask/src/deps_check.rs:9-12,96` -- the label comment of the `macros -> image` edge.
- `tools/story-loop/gates.sh` -- the local gate list; no new gate. The `ui_image` suite runs in `cargo hack test --each-feature` (`--features image`).

## Tasks & Acceptance

**Execution:**
- `Cargo.toml` (root) -- verify that `cssparser = "0.38"` and `blake3 = "1.8"` exist in `[workspace.dependencies]`, and leave them unchanged.
- `crates/noctis-image/Cargo.toml` -- add the deps and the `render` feature from Always.
- `crates/noctis-image/src/tw/theme.rs` (+ `tw/theme/tests.rs`) -- the Theme parser, its API and `ThemeError`. Add one `tw_02_*` unit test per theme row of the matrix (all errors, longest prefix, animate, empty, the 256 KiB boundary, unicode, digest) and `Theme: ThemeTokens` tests.
- `crates/noctis-image/src/node/{mod.rs,classes.rs,style.rs,into_nodes.rs}` (+ `tests.rs`) -- the node model. Add unit tests for every style, classes and IntoNodes row.
- `crates/noctis-image/src/template.rs` -- `TemplateView`, `ViewFn`, `erased_view` and `decode_view` (both `#[doc(hidden)] pub`).
- `crates/noctis-image/src/check.rs` (+ tests) -- `ClassCheck`. Add the `tw_01_*` unit tests for warn-once, deny, the cap, long-string truncation (assert the returned counts and a pure `pub(crate) fn log_excerpt(&str) -> Cow<str>` truncation helper; no log capture), literal skip and theme tokens.
- `crates/noctis-image/src/lib.rs` -- module wiring, re-exports and docs (`missing_docs` clean).
- `crates/noctis-macros/Cargo.toml`, `src/{template.rs,view.rs,tw.rs,theme.rs,lib.rs}` -- the four macros, the `#[template]` parser, the HashMap scan, the theme lookup and `include_bytes!` tracking, and the `[TW-01]`/`[TW-02]` diagnostics. Add unit tests for the pure parsers: the attribute grammar rows and the view grammar errors.
- `crates/noctis-core/src/{module.rs,registry.rs,error.rs}` -- `Module::template`, the fold, `DuplicateTemplate` (with a doc example) and `Registry::templates()`. Add registry unit tests for the three core rows.
- `crates/noctis/Cargo.toml`, `src/lib.rs` -- the features, `exclude`, `__private` and `pub mod image` (+ `unstable`) with docs. The feature docs (:556) now say `image` enables Templates, `view!`/`tw!`/`theme!` and the Renderer seam (the Renderer arrives in 10.3).
- `crates/noctis/noctis-theme.css` -- the test theme from the matrix header.
- `crates/noctis/tests/template.rs` (`#![cfg(feature = "image")]`) -- facade integration tests for the derive, decoder, view, tw, theme, Module/App rows, the card copy and the user-text-is-text row. The card copy is the template's `ProfileCard` without `avatar`/`<img>`, and its `view` must equal a hand-built builder tree. Add `Module::template` duplicate tests through `App::builder()` with a tokio runtime, following the existing facade tests.
- `crates/noctis/tests/ui-image/tw/TW-01_unknown_class.rs` + `.stderr` -- `tw!("flx")`, `tw!("bg-gradient-to-r")`, `tw!("bg-brand")` (no theme in trybuild) and a `view!` with `tw="w-FULL"`.
- `crates/noctis/tests/ui-image/tw/TW-02_builtin_keyframes.rs` + `.stderr` -- `theme!("../../fixtures/theme/builtin_keyframes.css")`.
- `crates/noctis/tests/fixtures/theme/builtin_keyframes.css` -- `@keyframes spin {}`.
- `crates/noctis/tests/ui-image/att/ATT-07_still_format.rs` + `.stderr` -- `format = Gif`, with `// [ATT-07]` on that line.
- `crates/noctis/tests/types-image/{template_hashmap,template_missing_deserialize,template_missing_view,template_attribute,view_tw_string}.rs` + `.stderr` -- the fixture rows of the matrix.
- `crates/noctis/tests/ui.rs` -- the `ui_image` test and the harness gating.
- `docs/discord-rules.md`, `crates/noctis-rules/src/rule.rs`, `xtask/rules-pending.toml` -- TW-01 and TW-02, the ATT-07 cell, and removing the pending entry.
- `xtask/src/deps_check.rs` -- the doc comment only.
- `examples/project-template/src/modules/profile/view.rs` -- reconcile the card:
  - the imports equal the shipped names;
  - the gradient becomes `bg-linear-to-r from-indigo-500 to-violet-400`;
  - the `#[asset] avatar` field and `<img>` stay, marked `// [planned, Story 10.8]` and `// [planned, Story 10.7]`;
  - every other class validates.

  Fix `examples/project-template/noctis-theme.css` comments: no `from-brand`, and `animate-rise` is `[planned, Story 10.6]`.
- `examples/project-template/.claude/skills/noctis/references/images.md` -- rewrite §2-§5 to the shipped API:
  - the derive keys and ranges, HashMap, `#[asset]` planned;
  - the `view!` grammar, the six tags (`img`/`svg` planned), children forms, `tw={Classes}`;
  - `tw!`, `Classes::{dynamic, and}` with the "never user text" rule;
  - the exact `Style` setters and clamping;
  - the TW-01/TW-02 errors, theme file rules (namespaces, longest prefix, `--animate-`, keyframes, built-ins), `theme!()`/`theme!("path")` and the rebuild note;
  - `Module::template` + DuplicateTemplate.

  Remove any v3 → v4 table row the grammar does not suggest, or mark it as general Tailwind advice. Also mention `Theme::parse`, `ThemeError`, `ThemeErrorKind`, `IntoNodes`, `Tag`, `div`/`span`/`text`, `TemplateDescriptor`, `InvalidClasses`, `UnknownClass`, `Suggestion`, `ViewFn`, `ClassCheck` and `ClassCheckMode` (all mentioned by name for skill-check). Update `SKILL.md` §3.5 to match and its `API hash` line.
- `xtask/public-api.txt` -- re-bless with `cargo run --release -p xtask -- api-snapshot --bless`. Put the printed hash in SKILL.md.
- `.github/workflows-disabled/ci.yml` -- no new gate. Confirm that its each-feature job covers `image`; if a job lists features explicitly, add `image`.

**Acceptance Criteria:**
- Given a bot crate with `noctis = { features = ["image"] }`, a `noctis-theme.css` that defines `--color-brand`, and `#[derive(Template, Serialize, Deserialize)] #[template(id = "card", version = 1, size = (600, 200), format = Png)]` with a `TemplateView` impl that uses `view! { <div tw="flex bg-brand"> {&self.name} </div> }`, when `cargo check` runs, then it compiles. After `bg-brand` is changed to `bg-gradient-to-r`, `cargo check` fails with `[TW-01] unknown Tailwind class `bg-gradient-to-r`; use `bg-linear-to-r` (Tailwind v4)` pointing at the literal. Editing `noctis-theme.css` to remove `--color-brand` makes the next `cargo check` fail without touching any `.rs` file.
- Given that Template, when `TemplateDescriptor::of::<Card>().view` is downcast to `noctis::image::unstable::ViewFn` and called with `RenderJob::of(&card)?.data` and `Frame::STILL`, then it returns `Ok` with a `Node` equal to `card.view(Frame::STILL)`.
- Given two mounted Modules that each call `.template::<T>()` for Templates sharing one id, when `App::build` runs, then it returns a `BuildError` containing exactly one `DuplicateTemplate` naming both types and Modules.
- Given default features, when `cargo tree -p noctis -e normal` runs, then neither `noctis-image`, `cssparser` nor any `takumi` crate appears. With `--features image`, all of them appear.
- Given `cargo hack test --release --workspace --each-feature`, when the `image` run executes `ui_image`, then every `ui-image` and `types-image` fixture fails to compile with its blessed `.stderr`, and `rules-catalog` reports TW-01, TW-02 and ATT-07 covered with nothing pending for them.
- Given the finished story, when `skill-check` and `api-snapshot` run, then both pass, with the new SKILL.md hash.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why a Noctis-owned node tree.** The template already imports `Node`/`Style` from stable `noctis::image`, while AD-2 keeps takumi types under `unstable`. Owning the tree also gives three properties:
  - the "user data is only text" guarantee holds by type: `tw={..}` takes `Classes`, text children become text nodes, and no constructor turns a string into markup;
  - views can be unit-tested without takumi;
  - Story 10.3 can lower the tree into takumi nodes in one place.
- **Runtime half without a Renderer.** "When it renders" needs the 10.3 Renderer. This story ships the check as `ClassCheck`, with Warn/Deny semantics proven by unit tests. 10.3 calls it with `Deny` on `Runtime::for_test()` Apps and adds the end-to-end testkit assertion. Literal classes are trusted at runtime, because they were validated against the crate's own theme file.
- **Trybuild has no theme.** Trybuild compiles fixtures in its own project directory, so `CARGO_MANIFEST_DIR` there has no `noctis-theme.css`. Positive theme-token tests therefore live in facade integration tests (`crates/noctis/noctis-theme.css`). TW-02's compile-time fixture uses `theme!("path")`, which resolves like `include_str!`.
- **Example expansion (sketch):**
  ```rust
  view! { <div tw="flex">{&self.name}</div> }
  // =>
  { const _: &[u8] = include_bytes!("/abs/noctis-theme.css");
    ::noctis::__private::image::Node::element(::noctis::__private::image::Tag::Div)
        .classes(::noctis::__private::image::Classes::__literal("flex"))
        .child(&self.name) }
  ```

## Verification

**Commands:**
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace --all-features` and `cargo test --release --workspace` -- pass
- `cargo hack test --release --workspace --each-feature` -- pass (includes `ui_image`)
- `TRYBUILD=overwrite cargo test --release -p noctis --features image --test ui` -- run once to bless the new `.stderr` files, then review them
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot` (after `--bless`), `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `cargo tree -p noctis -e normal | grep -E 'noctis-image|cssparser|takumi'` -- no output; with `--features image` -- matches
- `bash tools/story-loop/gates.sh` -- ALL-OK
