---
title: 'Story 10.6: Animated Templates'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-5-per-job-caps-and-admission-control.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-69 is half done. Story 10.4 shipped the per-frame view loop (`Frame { index, count }`, takumi `time_ms` per frame). Four pieces are still missing:
- the grammar still rejects `animate-<theme>` and `animate-[…]`, and the Renderer's stylesheet carries no `--animate-*` or `@keyframes`, so CSS-driven animations can't be written and wouldn't play;
- there is no per-call animation (`ctx.render_with`) and no per-call report (`render_report`);
- the fps cap per format is enforced only by the derive;
- a frame whose root declares another size is not detected.

**Approach:**
- **Grammar.** `noctis_image::tw` learns theme animations (`animate-<token>` reads `--animate-<token>`) and arbitrary single animations (`animate-[name_1s_…]`). Both are checked against the Theme's `--animate-*` and `@keyframes` through `ThemeTokens`.
- **Theme.** The Theme validates every `--animate-*` value against the same grammar and checks that its keyframes exist. The takumi stylesheet gains the animations and keyframes.
- **Core.** Core adds `RenderOptions`, `ctx.render_with` and `ctx.render_report`.
- **Ports.** The fps cap per format becomes `RenderCap::Fps`, checked when core creates the job and again in the Renderer.
- **Renderer.** A frame of an animated job whose root declares an explicit pixel size other than the job's size fails with the new `RenderError::SizeMismatch`.

## Boundaries & Constraints

**Always:**
- **Animation grammar (new `crates/noctis-image/src/tw/animation.rs`, takumi-free, `pub(crate)`).**
  - `fn parse(value: &str, sep: Sep) -> Option<&str>` returns the keyframes name of one single animation. `Sep::Underscore` is used for class values and `Sep::Whitespace` (ASCII whitespace runs) for Theme values.
  - The value is split on `sep`. With `Underscore`, an empty segment (a leading, trailing or doubled `_`) is `None`. Each segment must be exactly one of:
    - a **time** `^(0|[1-9][0-9]{0,5})(\.[0-9]{1,3})?(ms|s)$`. The first time is the duration and must be > 0. A second time is the delay, which may be 0. A third time is `None`.
    - a **timing** keyword: `linear|ease|ease-in|ease-out|ease-in-out|step-start|step-end`;
    - an **iteration** count: `infinite` or `[1-9][0-9]{0,3}`;
    - a **direction**: `normal|reverse|alternate|alternate-reverse`;
    - a **fill** mode: `none|forwards|backwards|both`;
    - a **play state**: `running|paused`;
    - the **name**: `[a-z]([a-z0-9-]*[a-z0-9])?`, at most 64 bytes, and not in `RESERVED`.
  - Each category may appear at most once (times at most twice). Exactly one name and at least one time are required. Anything else is `None`: functions (`cubic-bezier(`, `steps(`, `var(`), commas, negative or exponent times, uppercase, `\`, and a fractional or zero iteration count.
  - `RESERVED` is every keyword above plus `initial|inherit|unset|revert|revert-layer|default`.
  - `BUILTIN_ANIMATIONS` moves here as `pub(crate) const BUILTIN: [&str; 4] = ["spin", "ping", "pulse", "bounce"]`. `theme.rs` imports it.
  - `fn is_keyframes_name(&str) -> bool` is the name rule above; `fn is_animation_token(&str) -> bool` is `is_theme_token && !contains('.') && parse::<f32>().is_err()`.
- **`tw` (`tw/mod.rs`).**
  - Add two `Utility` variants (the enum is already `#[non_exhaustive]`):
    - `Animation { token: &'a str }`: draws only when the theme defines `--animate-<token>`;
    - `Keyframes { name: &'a str }`: an arbitrary animation that draws only when the theme defines `@keyframes <name>`.
  - `ThemeTokens` gains `fn contains_animation(&self, token: &str) -> bool { false }` and `fn contains_keyframes(&self, name: &str) -> bool { false }`, both default methods. `Theme` implements them over `animations` and `keyframes`, with exact, case-sensitive matches.
  - In `classify_utility`, when `prefix == "animate"`, `match_parsers` failed and the class is not negative:
    - a `[…]` suffix runs `animation::parse(inner, Underscore)`: a `BUILTIN` name gives `Builtin`, another name gives `Keyframes { name }`, and `None` stays `None`;
    - otherwise `is_animation_token(suffix)` gives `Animation { token }`;
    - in both cases the loop stops, as takumi's does.
  - `is_satisfied` maps `Animation` to `contains_animation` and `Keyframes` to `contains_keyframes`.
  - `__crosscheck::samples()` adds `animate-noctis-tok`, `animate-[noctis-tok_1s_linear_infinite]` and `animate-[spin_1s]`.
- **Theme (`tw/theme.rs`, TW-02).**
  - A `--animate-<token>` with `!is_animation_token(token)` is `InvalidToken`.
  - A value that is not `animation::parse(value, Whitespace)` is the new `ThemeErrorKind::InvalidAnimation { name }` (name = the custom property) at the declaration.
  - After the whole file has parsed, every animation whose keyframes name is neither in `BUILTIN` nor a `@keyframes` of the file is the new `ThemeErrorKind::UnknownKeyframes { name, keyframes }` at its declaration. The first such error in file order is the one returned. Store the location next to each animation.
  - A `@keyframes` name runs these checks in order: `BuiltinAnimation` (unchanged), then `!is_keyframes_name` gives the new `ThemeErrorKind::InvalidKeyframesName { name }`, then `Duplicate`.
  - Display texts:
    - `InvalidAnimation`: `` `{name}` is not one animation: write `<keyframes> <duration>[ <delay>][ <timing>][ <count>|infinite][ <direction>][ <fill>][ <play-state>]`, such as `rise 1s ease-out both` (no functions, no commas) ``.
    - `UnknownKeyframes`: `` `{name}` uses `@keyframes {keyframes}`, which this file does not define (built-ins: `spin`, `ping`, `pulse`, `bounce`) ``.
    - `InvalidKeyframesName`: `` `{name}`: a `@keyframes` name must match `[a-z]([a-z0-9-]*[a-z0-9])?`, be at most 64 bytes and not be a CSS keyword ``.
  - Update the module docs and `Theme::parse`'s `# Errors` (reference errors come after the file parses).
- **Stylesheet (`renderer.rs` `theme_css`).**
  - Inside `@theme`, after the namespace tokens, write `  --animate-<name>: <value>;` for every animation in file order.
  - After the closing `}`, write `\n@keyframes <name> {<block>}` for every keyframes block in file order.
- **Ports (`crates/noctis-ports/src/render.rs`).**
  - `RenderCap::Fps` has `as_str` `"fps"` and the doc "the frame rate of an animation, capped per animated format".
  - `AnimatedFormat::max_fps(self) -> u16` is a `pub const fn` returning `Webp` 90, `Gif` 50, `Apng` 90.
  - `AnimationSpec::check_fps(&self, template_id: &str) -> Result<(), RenderError>` returns `OverCap { id, cap: Fps, limit: max_fps, actual: fps }` when `fps > format.max_fps()`.
  - New variant `RenderError::SizeMismatch { id: String, frame: u32, expected: Size, declared: (u32, u32) }`:
    - its Display is `` template `{id}` frame {frame} declares a {w}x{h} root, but every frame of an animation has the Template's size {ew}x{eh} ``;
    - its doc is "a frame of an animation declared an explicit root size other than the job's size (FR-69)".
- **Renderer (`renderer.rs`).**
  - `view_of` runs these checks in order: unknown template → the existing `Unsupported` checks → `spec.check_fps(id)` → `caps.check(job)`.
  - `Shared::frame` runs the root check when `job.animation.is_some()`. It runs right after `view.call` and before the class check.
  - The declared sizes come from:
    - the root `Style`'s `Dimension::Px` width and height;
    - each root class token with no variant (no top-level `:`), after stripping one leading or trailing `!`, of the form `w-[Npx]`, `h-[Npx]` or `size-[Npx]` (both sides), with `N` = `[0-9]{1,9}`.
  - A mismatch is any declared width ≠ `job.size.width()` or any declared height ≠ `job.size.height()`. Then `declared` = (the first mismatching width, else the job's width; the first mismatching height, else the job's height), and `frame` = `Frame::index`.
  - Stills are not checked.
  - Put the pure helper in `node/mod.rs` as `pub(crate) fn root_size_mismatch(&self, expected: Size) -> Option<(u32, u32)>`.
- **Core (`crates/noctis-core/src/render.rs`, `responder/ergonomics.rs`, `lib.rs`).**
  - `#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)] pub struct RenderOptions { animation: Option<AnimationSpec> }` with `pub const fn new() -> Self` and `#[must_use] pub const fn animation(self, spec: AnimationSpec) -> Self`.
  - `#[derive(Debug, Clone)] #[non_exhaustive] pub struct RenderReport { pub attachment: Attachment, pub format: OutputFormat, pub settings_used: EncodeSettings }`.
  - Both are re-exported from the `noctis_core` root.
  - The internal `render` takes `RenderOptions` and returns `RenderReport`. After `RenderJob::of`:
    - set `job.animation` to `options.animation` when that is `Some`;
    - then `spec.check_fps(T::ID)`; an `Err` goes through `fail` (reason `over_cap`, reported) and the Renderer is never called.
  - `ctx_methods!` gains `render_with(template, options) -> HandlerResult<Attachment>` and `render_report(template, options) -> HandlerResult<RenderReport>`, next to `render` (= `render_with(t, RenderOptions::new())`), with docs and `# Errors` (add `SizeMismatch` and the fps `OverCap`).
  - `fail` gives `SizeMismatch` no reason; it is reported like `Failed`.
- **Facade (`crates/noctis/src/lib.rs` `pub mod image`).** Re-export `noctis_core::{RenderOptions, RenderReport}`, and update the module docs (animations, `render_with`, fps caps, root size).
- **Macros (`crates/noctis-macros/src/tw.rs` `message`).**
  - An unknown `Animation { token }` adds `` `--animate-{token}` (and its `@keyframes`) ``, giving `[TW-01] … ; or declare `--animate-wiggle` (and its `@keyframes`) in noctis-theme.css`.
  - An unknown `Keyframes { name }` adds `` `@keyframes {name}` ``.
  - The derive's compile-time fps bounds (1..=90, Gif 1..=50) stay as they are.
- **Rules doc.** Update the `docs/discord-rules.md` TW-01 and TW-02 rows for the theme animations, arbitrary animations, the value grammar, the keyframes reference and the keyframes name rule. The rule ids stay the same, and `noctis-rules` needs no change.

**Never:**
- No change to `generated.rs`, `xtask tw-grammar`, the takumi pins or the derive's attribute grammar.
- No configurable fps cap, no `RenderCaps` field for it, and no new metric or reason.
- No comma-separated multi-animation, no timing functions, no `var()` in animations.
- No root-size check for stills, and no check of spacing or percentage utilities (`w-64`, `w-full`).
- No new animated Template in the project template's code (Design Notes).
- No edit to `.cargo/config.toml` or `_bmad/`. No debug builds. No `unsafe`.

## I/O & Edge-Case Matrix

C = `tw::classify`, V(theme) = `tw::validate`, T = `Theme::parse`. R is a renderer test (`renderer/tests.rs`) whose `theme()` becomes `@theme { --color-brand: #5865f2; --animate-fade: fade 1s linear infinite; } @keyframes fade { from { opacity: 0 } to { opacity: 1 } }`.

| Scenario | Input / State | Expected | Error handling |
|---|---|---|---|
| parse happy | `rise_1s`, `rise_1.5s_ease-in-out_infinite_alternate_both_running_200ms`, `1s_rise` (order free), `rise_0.001s` | `Some("rise")` | — |
| parse times | `rise_0s`; `rise_0ms`; `rise_1s_0s` (delay 0); `rise_999999s` (6 digits); `rise_1000000s`; `rise_1.2345s`; `rise_-1s`; `rise_1e3s`; `rise_1s_2s_3s`; `rise_1S` | None; None; Some; Some; None; None; None; None; None; None | — |
| parse counts | `rise_1s_1`, `_9999`, `_10000`, `_0`, `_1.5`, `_infinite_2` | Some, Some, None, None, None, None (2 counts) | — |
| parse names | no name (`1s_linear`); two names (`a_b_1s`); `none_1s` (fill, no name); `Rise_1s`; a 64-byte name / 65-byte name; `ri-se_1s`; `rise-_1s`; `ease_1s` alone (keyword, no name) | None; None; None; None; Some / None; Some; None; None | — |
| parse separators | `_rise_1s`, `rise__1s`, `rise_1s_`, empty, `rise 1s` with Underscore; `rise  1s\t` with Whitespace | all None; Some | — |
| parse rejects | `rise_1s_cubic-bezier(0,0,1,1)`, `rise_1s,b_1s`, `var(--x)_1s`, `rise_1s_steps(4)`, `rïse_1s` | None | — |
| class theme animation | C(`animate-fade`), C(`animate-fade-in`), C(`animate-a_b`) | `Animation{token}` with `fade`, `fade-in`, `a_b` | — |
| class token edges | C(`animate-5`), C(`animate-a.b`), C(`animate-`), C(`-animate-fade`), C(`animate-Fade`) | None (numeric, dot, empty, negative, uppercase) | — |
| class builtins | C(`animate-spin`), C(`animate-none`), C(`sm:animate-pulse`), C(`!animate-bounce`) | Builtin (variant and important kept) | — |
| class arbitrary | C(`animate-[fade_1s_linear_infinite]`); C(`animate-[spin_2s]`); C(`animate-[fade]`); C(`animate-[]`); C(`animate-[fade_1s`); C(`animate-[fade_1s_cubic-bezier(0,0,1,1)]`) | `Keyframes{fade}`; Builtin; None; None; None; None | — |
| validate | V(`animate-fade`) and V(`animate-[fade_1s]`) with R's theme; same with `NoTheme`; V(`animate-[spin_1s] animate-spin`, NoTheme) | Ok; `InvalidClasses` listing both; Ok | — |
| suggestion text | `tw!("animate-wiggle")`, `tw!("animate-[wiggle_1s]")` on the facade test theme | `[TW-01] unknown Tailwind class `animate-wiggle`; or declare `--animate-wiggle` (and its `@keyframes`) in noctis-theme.css`; `…; or declare `@keyframes wiggle` in noctis-theme.css` | compile error (new trybuild `tests/ui-image/tw/TW-01_unknown_animation.rs`) |
| theme ok | T(`@theme { --animate-rise: rise 1s ease-out both; } @keyframes rise { from { opacity: 0 } }`); forward and backward keyframes order; `--animate-s: spin 2s linear infinite` (built-in reference, no keyframes) | Ok; `animations()` yields `("rise", "rise 1s ease-out both")` | — |
| theme invalid value | T(`--animate-x: x;`), T(`--animate-x: rise 1s, b 2s;`), T(`--animate-x: rise cubic-bezier(0,0,1,1) 1s;`), T(`--animate-x: rise 1s /* c */;`) | `InvalidAnimation{name:"--animate-x"}` at the declaration's line:col | `ThemeError` |
| theme unknown keyframes | T(`@theme { --animate-a: nope 1s; }`); two bad ones | `UnknownKeyframes{name:"--animate-a", keyframes:"nope"}` at 1:10; the first in file order | `ThemeError` |
| theme token rules | `--animate-a.b: …`, `--animate-5: …` | `InvalidToken` | `ThemeError` |
| keyframes name rules | `@keyframes Rise {}`, `@keyframes ease {}`, `@keyframes none {}`, `@keyframes a_b {}`, a 65-byte name; `@keyframes spin {}` | `InvalidKeyframesName`; `BuiltinAnimation` (unchanged, TW-02 builtin AC) | `ThemeError` / compile error |
| ported theme tests | `theme/tests.rs` rows that declare `--animate-rise: rise 1s` without keyframes, and `--animate-a: x` | add `@keyframes rise {}`; the duplicate row still fails `Duplicate` first | — |
| stylesheet | `theme_css` of R's theme | contains `--animate-fade: fade 1s linear infinite;` inside `@theme` and `@keyframes fade {from { opacity: 0 } to { opacity: 1 }}` after it; `theme_stylesheet` is Ok | — |
| CSS plays (theme) | R, a 4×4 Template `animate-fade w-full h-full bg-brand` that ignores the frame; `Shared::frame` at `time_ms` 0, 500, 999 | alpha at (0,0): ≤ 10, 118..=138, ≥ 245 | — |
| CSS plays (arbitrary / builtin) | the same with `animate-[fade_1s_linear_infinite]`; `animate-pulse` at 0 and 1000 ms | same alphas; pulse alpha 255 at 0 ms and 118..=138 at 1000 ms | — |
| CSS animated output | R renders the fade Template with `animation: 1 s, 10 fps, WEBP80` | animated WebP (`ANIM`), `settings_used.frames == 10`; the view received `Frame{0..10, 10}` | — |
| still with CSS | the fade Template as a still | renders frame at t=0 (alpha ≈ 0); no error | — |
| fps cap | `check_fps` with Gif fps 50 / 51; Webp 90 / 91; Apng 90 / 91; fps `u16::MAX` Webp | Ok / `OverCap{Fps, 50, 51}`; Ok / `{90, 91}`; Ok / `{90, 91}`; `{90, 65535}` | typed |
| fps cap order (renderer) | R: fps 0 → `Unsupported`; Gif fps 60 without `gif` → `Unsupported` (gif first); Webp fps 120, 7 s → `OverCap{Fps}` (before Duration); admitted() unchanged | as listed | typed, before admission |
| size mismatch | R animated 8×8 job; root `Style::width_px(4)` on frame 2 only; root class `w-[9px]`; `size-[8px]` (ok); `sm:w-[4px]` (ignored); `!h-[3px]`; `w-full`/`w-64` (ignored); `width_px(8).height_px(8)` (ok); `width_pct(50)` (ignored) | `SizeMismatch{frame:2, expected:8×8, declared:(4,8)}`; `{frame:0, declared:(9,8)}`; Ok; Ok; `{declared:(8,3)}`; Ok; Ok; Ok | typed, reported |
| size mismatch still | a still Template whose root is `width_px(4)` | renders (no check) | — |
| ports display | `SizeMismatch` with `card`, frame 3, 934×282, (500, 282); `RenderCap::Fps.as_str()` | `template `card` frame 3 declares a 500x282 root, but every frame of an animation has the Template's size 934x282`; `"fps"` | — |
| core options | Fake records the last job: `render_with(Card, RenderOptions::new().animation(spec(20 fps Webp)))`; `render_with(t, RenderOptions::new())` on an animated Template; `RenderOptions::default() == new()` | job.animation == Some(spec); the Template's own `ANIMATION`; equal | — |
| core fps | `render_with(Card, ….animation(Gif at 60 fps))` | `OverCap{Fps,50,60}`; Fake never called; `noctis_render_rejected_total{reason="over_cap"}` 1; one Render sink report | typed |
| core report | `render_report(Card, RenderOptions::new())` with Fake Ok | `attachment` named `<16 hex>.png`, `format == Png`, `settings_used == Fake's` | — |
| core size mismatch | Fake answers `SizeMismatch` | handler error; no rejected reason; one Render sink report | typed |
| e2e | `crates/noctis/tests/render_e2e.rs`: a slash handler calls `ctx.render_with(Dot, RenderOptions::new().animation(AnimationSpec::new(500 ms, 10, AnimatedFormat::DEFAULT)))` | the response carries one `.webp` attachment | — |
| facade | `crates/noctis/tests/render.rs`: a derived CSS Template with `tw="animate-rise …"` (facade theme gains `--animate-rise: rise 1s ease-out both;`) renders animated through `app.renderer()`; `Theme::parse("@keyframes spin {}")` kind is `BuiltinAnimation`; `AnimatedFormat::Gif.max_fps() == 50` | as listed | — |

</intent-contract>

## Code Map

- `crates/noctis-image/src/tw/mod.rs`:
  - `Utility` at `:72-86`, `ThemeTokens` at `:88-93`;
  - `is_satisfied` at `:244-261`;
  - `classify_utility` at `:313-338`: the generic loop. Add the `animate` branch after the `match_parsers` miss;
  - `__crosscheck::samples` at `:420-462`;
  - `is_theme_token` at `:393`.
- `crates/noctis-image/src/tw/theme.rs`:
  - `BUILTIN_ANIMATIONS` at `:47` (move it);
  - `animations: Vec<(String, String)>` (add the location);
  - `parse_top_level` `@keyframes` arm at `:184-207`;
  - `declare` at `:240-260`;
  - `ThemeErrorKind` at `:333-378`, with Display at `:385-431`.
  - Tests: `tw/theme/tests.rs` (rows at `:75`, `:233`, `:388` need keyframes).
- `crates/noctis-image/src/tw/tests.rs`: the grammar tests. Its `Tokens` `ThemeTokens` impl at `:10` keeps working through the default methods.
- `crates/noctis-image/tests/tw_crosscheck.rs`:
  - `Verdict` (add `Animation` and `Keyframes`), `verdict_of` at `:79` and `compare` at `:88`;
  - an `Animation{token}` agrees when the oracle says Builtin and its debug contains `token: Some("{token}")`;
  - a `Keyframes{name}` agrees when the oracle says Builtin and its debug contains `name: Some("{name}")`;
  - the corpus parser at `:130-170` takes the words `animation` and `keyframes`.
- `crates/noctis-image/tests/tw_corpus.txt`:
  - `:101` `animate-wiggle unknown narrowed=builtin-animations-only` becomes `animate-wiggle animation`;
  - add `animate-[wiggle_1s] keyframes`, `animate-[spin_1s] builtin`, `animate-5 unknown narrowed=numeric-token`, `animate-[wiggle] unknown narrowed=needs-duration`, `animate-[wiggle_1s_cubic-bezier(0,0,1,1)] unknown narrowed=animation-functions` and `animate-[a_1s,b_1s] unknown narrowed=single-animation`;
  - a `narrowed=` line must be one takumi accepts; drop any that takumi reports Empty.
- `crates/noctis-image/src/renderer.rs`:
  - `theme_css` at `:77-89`;
  - `view_of` at `:123-148`;
  - `Shared::frame` at `:247-275`, where the existing bitmap-size `Failed` check stays.
  - Tests: `renderer/tests.rs` `theme()` at `:176`, the `template!` macro at `:21`, `the_theme_stylesheet_declares_every_token` at `:512`, and `decode_png`/`pixel` helpers at `:242-262` (a takumi `Bitmap` exposes `as_raw()`, RGBA).
- `crates/noctis-image/src/node/mod.rs` (`Style`'s `width`/`height: Option<Dimension>` in `node/style.rs`; `Classes::as_str`): add `root_size_mismatch`, and test it in `node/tests.rs`.
- `crates/noctis-ports/src/render.rs`: `AnimatedFormat` `:205`, `AnimationSpec` `:225`, `RenderCap` `:331-361`, `RenderError` `:364-430`, tests at the bottom.
- `crates/noctis-core/src/render.rs`: `render` `:28-58`, `fail` `:98-147`. Tests: `render/tests.rs` (the `Fake` at `:59` gains `job: Mutex<Option<RenderJob>>` and `calls: AtomicUsize`; the `Answer` table gains `SizeMismatch`).
- `crates/noctis-core/src/responder/ergonomics.rs:458-525`: `render` in `ctx_methods!`. `crates/noctis-core/src/lib.rs:199` is `mod render;`; add the `pub use`.
- `crates/noctis-macros/src/tw.rs`: `message` `:29-56`; test `:136-146` flips (`animate-rise` is now valid with that theme).
- `crates/noctis/src/lib.rs`: image docs `:1958-2000` and re-exports `:2044-2053`.
- `crates/noctis/noctis-theme.css`: add `--animate-rise: rise 1s ease-out both;` (keyframes `rise` already exist).
- `crates/noctis/tests/render.rs` (helpers `app()`/`render()`), `crates/noctis/tests/render_e2e.rs` (TestApp flow), and `crates/noctis/tests/ui-image/tw/` with fixtures in `tests/fixtures/theme/`.
- takumi-core 0.31.0 facts (read-only, `~/.cargo/registry/src/*/takumi-core-0.31.0`):
  - `TwAnimation::parse_tw` (`style/tw/parser.rs:333`) maps any ident token to `var(--animate-<token>)`;
  - `Animations::parse_tw_with_arbitrary` (`style/properties/animation.rs:421`) decodes `_` to a space;
  - `find_keyframes` (`style/animation.rs:122`) looks in the stylesheet, then the built-in spin/ping/pulse/bounce;
  - the stylesheet parser accepts top-level `@keyframes` and `@theme`.
- Skill: `examples/project-template/.claude/skills/noctis/references/images.md`:
  - `:182` drop "[planned]";
  - §5 theme rules `:190-210` (the value grammar, the keyframes reference, the name rule and the new kinds);
  - §8 errors `:270-290` (the fps `OverCap`, `SizeMismatch`);
  - the observability note;
  - `:346-347` Planned becomes shipped: `render_with`, `render_report`, `RenderOptions`, `RenderReport` and the fps caps;
  - Animation `:349-369`: CSS-driven (the grammar, a still renders t = 0), code-driven, `render_with`, and the root size rule;
  - `:406`.
- Also update `SKILL.md` (API hash `:23`; `skill-check` needs every new public name mentioned), `references/limits-cheatsheet.md` (the fps caps), `examples/project-template/AGENTS.md:65` (`render_with`/`render_report`, the fps caps, `animate-*`) and `examples/project-template/noctis-theme.css` (the comment at `:1-4` drops "[planned, Story 10.6]").

## Tasks & Acceptance

**Execution:**
- `crates/noctis-image/src/tw/animation.rs` (+ unit tests) -- the grammar, `RESERVED`, `BUILTIN`, the name and token predicates -- every parse row of the Matrix.
- `crates/noctis-image/src/tw/mod.rs` (+ `tw/tests.rs`) -- the `Utility` variants, the `ThemeTokens` defaults, the `animate` branch, `is_satisfied`, the samples -- the class and validate rows.
- `crates/noctis-image/src/tw/theme.rs` (+ `theme/tests.rs`) -- the value validation, the deferred keyframes reference, the keyframes name rule, three kinds, docs, the ported rows -- the theme rows.
- `crates/noctis-image/tests/tw_crosscheck.rs`, `tw_corpus.txt` -- the animation verdicts and the corpus lines; the cross-check passes.
- `crates/noctis-ports/src/render.rs` -- `RenderCap::Fps`, `max_fps`, `check_fps`, `SizeMismatch` and the tests -- the ports rows.
- `crates/noctis-image/src/node/mod.rs` (+ `node/tests.rs`), `renderer.rs` (+ `renderer/tests.rs`) -- `root_size_mismatch`, `theme_css`, the order in `view_of`, the frame check -- the stylesheet, CSS-plays, fps-order and size rows.
- `crates/noctis-core/src/{render.rs,responder/ergonomics.rs,lib.rs}` (+ `render/tests.rs`) -- `RenderOptions`, `RenderReport`, `render_with`, `render_report`, the fps check and `fail` -- the core rows.
- `crates/noctis-macros/src/tw.rs` -- the animation hints and the flipped test.
- `crates/noctis/src/lib.rs`, `crates/noctis/noctis-theme.css`, `crates/noctis/tests/{render.rs,render_e2e.rs}`, `crates/noctis/tests/ui-image/tw/TW-01_unknown_animation.{rs,stderr}` and `TW-02_unknown_keyframes.{rs,stderr}` (+ `tests/fixtures/theme/unknown_keyframes.css`) -- re-exports, docs and the facade, e2e and trybuild rows. Generate `.stderr` with `TRYBUILD=overwrite`, then review it.
- `docs/discord-rules.md` -- the TW-01 and TW-02 rows.
- `examples/project-template/{noctis-theme.css,AGENTS.md}` and `.claude/skills/noctis/{SKILL.md,references/images.md,references/limits-cheatsheet.md}` -- document both animation styles, the grammar, the caps (fps per format plus `RenderCaps`), `render_with`/`render_report`, `RenderOptions`/`RenderReport`, `SizeMismatch`, and the new `ThemeErrorKind`s.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; copy the printed hash into `SKILL.md`.
- `.github/workflows-disabled/ci.yml` -- confirm only; no new gate.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- log only the decisions made during implementation.

**Acceptance Criteria:**
- Given a Template whose literal classes use `animate-spin`, `animate-<theme>` (with `--animate-<theme>` and its `@keyframes` in `noctis-theme.css`) or `animate-[<keyframes>_1s_…]`, when it compiles and renders to an animated format, then `cargo check` accepts it, takumi receives the Theme's animations and keyframes, and the frames follow the keyframes over `time_ms`. An undeclared animation or keyframes is a `[TW-01]` compile error naming the missing declaration.
- Given `ctx.render_with(t, RenderOptions::new().animation(spec))` or a Template `ANIMATION`, when it renders, then the view runs once per frame with `Frame { index, count }`, and `ctx.render_report` returns the attachment plus `format` and `settings_used`.
- Given a frame rate above the format's cap (GIF 50, WebP/APNG 90), when core creates the job, then the handler gets `RenderError::OverCap { cap: RenderCap::Fps, .. }` and the Renderer is never called; a hand-built `RenderJob` gets the same error from the takumi Renderer before admission.
- Given a code-driven animation whose frame root declares an explicit pixel size other than the job's, when it renders, then the result is `RenderError::SizeMismatch` naming the frame, and it is reported to the Render sink.
- Given a Theme that defines `@keyframes spin` (or `--animate-spin`), when it is parsed by `theme!()` or `Theme::parse`, then it fails with `[TW-02]` `BuiltinAnimation`; `.theme(..)` only takes a parsed Theme.
- Given the finished story, when `skill-check`, `api-snapshot`, `rules-catalog`, `deps-check` and `tw-grammar --check` run, then all of them pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Already shipped (no work).**
  - Story 10.4 shipped the per-frame view loop: `Frame { index, count }` with count = `span_count(duration, fps)`, and takumi `time_ms` = the span start.
  - Story 10.2 shipped `BuiltinAnimation` at parse time and at compile time (`theme!`, the trybuild fixture `TW-02_builtin_keyframes`). `.theme(Theme)` takes a parsed `Theme`, so "registration fails" is that parse. The story adds only the regression rows.
  - The derive already bounds `fps` (1..=90, Gif 1..=50). This story adds the run-time half for `render_with` and hand-built jobs.
- **Why the fps cap is `OverCap { cap: Fps }`.** It is a per-job cap checked before enqueue (AD-15 lists it with the caps), so it reuses the typed error, the `over_cap` reason and the Render sink. It is fixed per format, not a `RenderCaps` knob. GIF stores delays in centiseconds, so 50 fps is its floor of 20 ms.
- **Why the Theme validates `--animate-*` values.** TW-01's promise is that only classes that draw pass. If `animate-rise` passes while `--animate-rise: rsie 1s` names no keyframes, the output renders static. So the Theme parses each value with the same grammar as `animate-[…]`, and checks the keyframes reference after the whole file, so order does not matter. The grammar is a strict subset (one animation, keywords only) for the same reason the class grammar is narrowed. `cubic-bezier`/`steps` can be added later without breaking anyone.
- **Keyframes names.** The rule `[a-z]([a-z0-9-]*[a-z0-9])?` (not a CSS keyword) holds because a name is reachable only through the grammar, and takumi matches keyframes names case-insensitively. A lowercase-only rule also closes `@keyframes Spin` shadowing the built-in `spin`.
- **Root size.** "Explicit size" means the root's `Style` pixel width or height, or an arbitrary `w-/h-/size-[Npx]` root class. Those are the only per-frame size inputs a view controls; spacing and percentage utilities are relative. Stills are unchanged, because a still has one scene.
- **`render_report`.** It was planned since Story 10.4 (memlog) and exposes `settings_used` per call. It takes `RenderOptions` too, so there is one entry point for options.
- **Project template.** The profile card stays a still. A CSS animation on a still renders its t = 0 frame (for `rise … both` that frame is invisible), so the skill shows animated examples instead of changing the card. The template's `noctis-theme.css` already holds a valid `--animate-rise` with its keyframes.

## Verification

**Commands** (run in `/home/claude/lanes/a/noctis-rs`):
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` and `cargo test --release --workspace --all-features` -- pass (includes `tw_crosscheck`, the trybuild `ui-image` suite and `render_e2e`)
- `cargo hack test --release --workspace --each-feature` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `bash tools/story-loop/gates.sh` -- ALL-OK
