---
title: 'Story 10.7: Fonts and static assets'
type: 'feature'
created: '2026-10-24'
status: 'in-progress'
baseline_revision: 'b57e676b64cc8ca729aab9a8313fdffca014d1b9'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-6-animated-templates.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-35 is not implemented. The Renderer draws with an empty `Fonts::default()`, so text renders invisibly. `default-font` is an empty facade feature. There is no font, fallback or Unicode-range registration. `<img>` is rejected by `view!` ("planned (Story 10.7)"), so a Template cannot show a logo.

**Approach:**
- **Fonts.** `ImageConfig::font(Font)` registers fonts in priority order. Each `Font` can carry a family override and Unicode ranges. They go into a process-scoped `FontRegistry` Resource (AD-24). The Renderer hands takumi that registry plus the family priority list as the fallback chain, so mixed-script text needs no Template change.
- **Bundled font.** `default-font` bundles Inter 4.1 (OFL-1.1) as a last-resort family `"Inter"`.
- **No-font error.** With no font at all, registering a Template fails `App::build` with `[IMG-01]`.
- **Static images.** `StaticImage` handles (PNG/JPEG/WebP, `&'static` bytes) are registered with `ImageConfig::image(..)` and decoded once at build. `view!` gains `<img src={HANDLE} />`. Rules IMG-01 and IMG-02 join the catalog.

## Boundaries & Constraints

**Always:**
- **Font priority.** Priority = order of the `ImageConfig::font` calls. The fallback chain is the families in order of first appearance, deduplicated, passed per render as takumi `RenderOptions::font_families` (only when non-empty). Author fonts get no generic family. The bundled font is registered last with `FontResource::last_resort()` under the family override `"Inter"`, and only with `default-font`.
- **Font registry Resource.** `FontRegistry` implements `noctis_core::Resource` with `Config = FontsConfig { fonts: Vec<Font>, bundled: bool }`.
  - `bundled` is `cfg!(feature = "default-font")`, set by the plugin.
  - Equality is order-sensitive (order is priority).
  - `Debug` of `Font`/`FontsConfig`/`StaticImage` never prints bytes: family, ranges, byte length and the first 8 hex chars of the blake3 digest.
  - `init` never fails: load errors are stored and read with `build_error()`, as `RenderPool` does.
- **Static images are per App** (held by the Renderer), decoded once at install with takumi `ImageSource::from_bytes`. Lowering clones the decoded source into `ImageSourceInput::Loaded`, and takumi never fetches.
- **Ids, formats, caps.**
  - The id grammar is checked in `const fn StaticImage::new`.
  - The format is sniffed by magic bytes, never by guessing (GIF, SVG, BMP and ICO are rejected even though the `image` crate could decode some).
  - Dimensions are read from the header (`image::ImageReader::with_guessed_format().into_dimensions()`) before decoding, so the cap is checked first.
- **Plugin order.**
  - Install order: range checks (now including font config) → pool → `FontRegistry` → static images → stylesheet → Renderer.
  - Every failure is a plugin violation whose message starts with `[IMG-01]`/`[IMG-02]`, except `ResourceConflict`.
- **Memory (maintainer decision 5).** `Font::from_static` uses `FontSource::from_static` (no copy); `StaticImage` borrows `&'static` bytes. Fonts and images are not secrets: no `Zeroizing`.
- **Licenses.**
  - `noctis-image`'s license becomes `(MIT OR Apache-2.0) AND OFL-1.1`, because the package ships the font files.
  - `deny.toml` accepts OFL-1.1 only as a crate-scoped exception for `noctis-image`.
  - `crates/noctis-image/NOTICE` records every font file with its source, version, license and sha256.
- **Same commit (AGENTS.md).** Re-bless the API, update the Usage Skill, the template, `docs/discord-rules.md` and `noctis-rules`.

**Never:**
- No remote images, `#[asset]` or `RemoteImage` (Story 10.8).
- No emoji font or its warning (10.9).
- No cache-key change (10.10).
- No `<svg>` support or takumi `svg-source`.
- No takumi `woff` (v1) or `gif` features.
- No new internal crate.
- No generic-family registration for author fonts.
- No font/image loading from file paths at run time (authors read files themselves and use `Font::from_bytes`).
- Never run `cargo update`: the lock may only gain entries; `takumi-raster` stays 0.5.8 and `takumi-core` stays one version (0.31.0).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| No font, no Template | `.image(ImageConfig::new())`, no Templates, `default-font` off | builds; renderer installed | none |
| No font + Template | as above + `Module::template::<T>()` | `App::build` Err: `BuildViolation::TemplateRejected` whose message starts `[IMG-01] no font is registered` and names `ImageConfig::font` and `default-font` | `TakumiRenderer::register` returns `RenderError::NoFont` |
| Bundled only | `default-font` on, no `.font` | builds; Latin text draws ink (non-background pixels) | none |
| Bundled + author fonts | `default-font` on + `.font(a)` | chain = [a's families], then "Inter" as last resort | none |
| Family by name | author font `.family("Inter")` (or the bundled one) and class `font-display` (crate theme `--font-display: "Inter", sans-serif`) | family `Inter` draws; its missing glyphs walk the chain | none |
| Fallback (pixel) | fonts [Inter, JP400]; text `あ` | bitmap == the `[JP400]`-only render of `あ`; != the `[Inter]`-only render | none |
| Priority (pixel) | fonts [JP400 "Kana Regular", JP700 "Kana Bold"] vs the reverse; text `あ` | each order's bitmap == its first font's solo render; the two differ | none |
| Unicode range (pixel) | [JP700 `.family("Hira Bold").unicode_range('\u{3041}'..='\u{3096}')`, JP400] | `あ` == the JP700 solo render; `ア` (U+30A2) == the JP400 solo render | none |
| Range single / full | `'a'..='a'`; `'\0'..=char::MAX` | accepted | none |
| Range inverted | `'b'..='a'` | plugin violation `[IMG-01] font #<i>: unicode range U+0062..=U+0061 is inverted` | no Renderer |
| Ranges empty | no `unicode_range` call | font serves every codepoint | none |
| Family override bounds | `""`; 128 bytes; 129 bytes; contains `\u{7}`; `"源ノ角"` | `""`, 129 bytes and control characters → `[IMG-01]` violation; 128 bytes and Unicode names OK | no Renderer |
| Font format | TTF/OTF/TTC/WOFF2 | loaded | none |
| Bad font | WOFF1 (`wOFF`); empty; 3 bytes; `b"\0\x01\0\0garbage"`; a file that registers no family | `[IMG-01] font #<i> (<len> bytes) could not be loaded: <takumi reason or "it contains no font face">` | the `FontRegistry::build_error` path |
| Same Runtime, equal fonts | App1 and App2 with `ImageConfig::new().font(a)` on one Runtime | both build; the same `Arc<FontRegistry>` | none |
| Same Runtime, conflict | App2 has [b], [a, b] reordered, or no fonts while App1 has [a] | `BuildViolation::ResourceConflict` whose `resource` ends with `FontRegistry` | the build fails |
| Test Runtimes | App2 on another `Runtime::for_test()` with [b] | builds with its own registry | none |
| Image id valid | `"a"`, `"logo.v2_x-1"`, 64 bytes | const OK | none |
| Image id invalid | `""`, 65 bytes, `"Logo"`, `"-a"`, `"a-"`, `"a b"`, `"ä"` | const panic `[IMG-02] static image id must match [a-z0-9]([a-z0-9._-]*[a-z0-9])? and be at most 64 bytes`; in a `const` item it is a compile error (trybuild) | — |
| Image format | PNG, JPEG, WebP bytes (from `takumi::write_image`) | decoded, drawable | none |
| Image unsupported | `GIF89a…`, `<svg …>`, `BM…`, empty | `[IMG-02] static image `<id>`: unsupported format: use PNG, JPEG or WebP` | no Renderer |
| Image corrupt | PNG magic + truncated body | `[IMG-02] static image `<id>` could not be decoded: <reason>` | no Renderer |
| Pixel cap (`check_pixels`) | 4096×4096 and 16_777_216×1 OK; 4097×4096 and 16_777_217×1 rejected; `u32::MAX`² never overflows (u64) | `[IMG-02] static image `<id>` is <w>x<h>: at most 16777216 pixels` | no Renderer |
| Image duplicate | the same `StaticImage` twice / the same id with other bytes | idempotent / `[IMG-02] static image id `<id>` is registered twice with different bytes` | no Renderer |
| `<img>` draws (pixel) | a 2×2 red PNG, a 4×4 white root, `<img src={RED} />` without size | pixel (0,0) red, (3,3) white | none |
| `<img>` sized | `tw="w-4 h-4"` / `style={Style::new().width_px(4)}` | classes and Style apply as on any node | none |
| Unregistered image | `<img src={OTHER}>` not in the config, or the same id with other bytes | `RenderError::UnknownImage { id: <template>, image: <image id> }`, reported to the Render sink like `Failed` | the render fails |
| `view!` img | `<img src={L} tw="…" style={..} />` | `::noctis::image::img(L, classes).style(..)` | none |
| `view!` img errors | `<img src={L}></img>`; `<img tw="w-4" />`; `src="logo"`; two `src`; `src` on `<div>`; `<svg/>` | errors: "`<img>` has no children: close it with `/>`"; "`<img>` needs `src={..}` (a `StaticImage`)"; "`src` takes an expression in braces: `src={LOGO}`"; the existing duplicate-attribute error; "`src` is only for `<img>`"; "`<svg>` is not supported: use `<img src={..} />` with a PNG, JPEG or WebP `StaticImage`" | compile error |

</intent-contract>

## Code Map

- `crates/noctis-image/src/renderer.rs`:
  - `Shared` `:40-55`: `fonts: Fonts` and its "Story 10.7" doc;
  - `TakumiRenderer::new` `:113-136` already takes 7 arguments; group them in a `pub(crate) struct RendererConfig`;
  - `register` `:212-226`: add the `NoFont` check when `templates` is non-empty and the registry has no family and no bundled font;
  - `Shared::frame` `:247-288`: `.fonts(&self.fonts)` gains `.font_families(..)`, and `lower(&node)` becomes `lower(&node, &self.images)`.
- `crates/noctis-image/src/renderer/tests.rs`: `renderer_budget` `:188-237` builds the Renderer and registers every Template, so it needs a test `FontRegistry` (Inter from `../fonts/`).
- `crates/noctis-image/src/plugin.rs`:
  - `ImageConfig` `:76-160` (derives `Debug, Clone, PartialEq, Eq`; add `fonts: Vec<Font>` and `images: Vec<StaticImage>`, plus the `font` and `image` setters and docs);
  - `install` `:177-270`.
  - `RenderPool` (`pool.rs`) is the Resource pattern to copy: `resource_or_init`, `build_error`.
- `crates/noctis-image/src/lower.rs` `:25-40`: `Kind` match. Add `Kind::Image(src)` → `takumi::prelude::Node::image(ImageData { src: ImageSourceInput::Loaded(source.clone()), width: None, height: None })`. Unknown → `RenderError::UnknownImage`.
- `crates/noctis-image/src/node/mod.rs`:
  - `Kind` `:68-71` (add `Image(StaticImage)`);
  - `child`/`children`/`child_nodes` (no-op / empty for images);
  - builders `div`/`span`/`text` `:228-240` (add `img`).
  - `check.rs` (`ClassCheck`) must also visit an image node's classes.
- `crates/noctis-image/src/lib.rs` `:31-59`: the module list and re-exports. Add `mod fonts; mod assets;` behind `render`, re-export `Font`, `StaticImage` and `img`, and update the crate docs.
- `crates/noctis-image/Cargo.toml`:
  - license;
  - takumi features `["raster-backend", "rayon", "woff2", "png", "jpeg", "webp"]`;
  - feature `default-font = ["render"]`.
- `crates/noctis-ports/src/render.rs` `:392-450`: `RenderError` (`#[non_exhaustive]`). Add `NoFont` and `UnknownImage { id: String, image: String }`, with tests at the bottom.
- `crates/noctis-core/src/render.rs`: `fail` (`:160`+) has wildcard arms, so nothing changes there; extend its doc to say `UnknownImage` is handled like `Failed`.
- `crates/noctis-core/src/runtime.rs` `:39-80` (read-only): `Resource`, `ResourceConflict` (its `resource` is `type_name`).
- `crates/noctis-macros/src/view.rs`:
  - grammar doc `:1-13`, `TAGS` `:26`, `Element` `:38`;
  - attribute parsing in `parse_element`;
  - `tag_of` `:197-208` (the "planned (Story 10.7)" message);
  - tests `:450-460` (`err(quote!(<img/>))`).
- `crates/noctis/Cargo.toml` `:25`: `default-font = ["image", "noctis-image/default-font"]`.
- `crates/noctis/src/lib.rs`: feature docs `:571`, image docs `:1958-2000`, re-exports `:2044-2062`.
- `crates/noctis/tests/render.rs`:
  - `builder` `:~230`, `app_with` `:244`;
  - `render_e2e.rs` `:139-145`;
  - `template.rs` `:24` (doc "planned `<img>`"), `:256`;
  - `ui.rs` (`tests/ui-image/**/*.rs` already globs subdirectories).
- `crates/noctis-rules/src/rule.rs` `:77`: `TW_01`, `TW_02`. Append `IMG_01 = "IMG-01", IMG_02 = "IMG-02"` (a test compares `ALL` with the catalog).
- `docs/discord-rules.md` §16 `:300-306`: the TW rows. `xtask/src/rules_catalog/mod.rs` `:1-40` says what evidence each tier needs: `App-build`/`Runtime` → a test name containing `img_01`/`img_02`; `Compile-time` → a trybuild fixture whose `.stderr` contains `[IMG-02]`.
- `deny.toml` `:41-44` `exceptions`.
- takumi facts (read-only, `~/.cargo/registry/src/*/`):
  - `takumi-core-0.31.0/src/resources/font.rs`:
    - `Fonts::register(FontResource) -> Result<Vec<RegisteredFamily>, FontError>` (`RegisteredFamily.name` is public);
    - `FontResource::new(..).override_info(FontOverride { family_name, .. }).unicode_range(UnicodeRangeSet).last_resort()`;
    - `FontSource::from_static` / `from_shared`.
  - `UnicodeRange::new(u32, u32)`; `UnicodeRangeSet: FromIterator`, which silently drops inverted ranges (hence our check).
  - `takumi-raster-0.5.8/src/render.rs:55`: `font_families: Option<FontFamily>` is the per-render fallback chain (`FontFamily::from_names`). The default `font-family` is generic `sans-serif`, which resolves only through generic registrations, so text without a family walks the chain.
  - `ImageSource::from_bytes` (`resources/image.rs:491`); `ImageData { src, width, height }` and `ImageSourceInput::Loaded` (`layout/node/mod.rs:67,154`). Everything is re-exported from `takumi::prelude`.
- Skill and template:
  - `examples/project-template/.claude/skills/noctis/references/images.md`: §1 `:20-22` (the "[planned, Story 10.7]" font sentences), §3 `:106` (`<img>`/`<svg>`), §6 `:239-256` (aspirational `FontSource`/`fallback_order`; rewrite to the real API), §8 errors;
  - `SKILL.md` (API hash line), `references/limits-cheatsheet.md`;
  - `examples/project-template/AGENTS.md`;
  - `examples/project-template/src/modules/profile/view.rs:94` (the comment becomes "`<img>` ships for static images; `RemoteImage` is [planned, Story 10.8]").

## Tasks & Acceptance

**Execution:**
- **Font files** (exact commands; check each sha256):
  - `curl -sS -o /tmp/inter.tgz https://registry.npmjs.org/inter-ui/-/inter-ui-4.1.1.tgz`, then extract `package/variable/InterVariable.woff2` (sha256 `693b77d4f32ee9b8bfc995589b5fad5e99adf2832738661f5402f9978429a8e3`) to `crates/noctis-image/fonts/InterVariable.woff2`, and `package/LICENSE.txt` to `crates/noctis-image/fonts/LICENSE-Inter.txt`.
  - `curl -sS -o /tmp/jp.tgz https://registry.npmjs.org/@fontsource/noto-sans-jp/-/noto-sans-jp-5.3.1.tgz`, then extract `package/files/noto-sans-jp-119-400-normal.woff2` (sha256 `2258b057683e7040e5149a9469077fd8ed47e73ca656f84fc3c4f9bc86976a1e`), `noto-sans-jp-119-700-normal.woff2` (sha256 `250d4554209267dafcdf9f045b70808de6157837663f7baa8090a2f425e0928a`) and `package/LICENSE` (as `LICENSE-NotoSansJP.txt`) to `crates/noctis-image/tests/fixtures/fonts/`.
  - Subset 119 holds kana: U+3042 and U+30A2 are present, Latin is absent, and the 400/700 outlines differ. Inter has no U+3042.
- `crates/noctis-image/NOTICE`, `Cargo.toml`, `deny.toml` -- NOTICE lists the 3 files (source URL, version, OFL-1.1, copyright, sha256); the license, takumi features and `default-font` feature as in the Code Map; the exception `{ crate = "noctis-image", allow = ["OFL-1.1"] }` with a comment -- licenses.
- `crates/noctis-ports/src/render.rs` -- add `NoFont` and `UnknownImage`, plus unit tests of their messages:
  - `NoFont` displays "[IMG-01] no font is registered: add one with `ImageConfig::font(..)` or enable the `default-font` feature";
  - `UnknownImage` displays "template `{id}` uses static image `{image}`, which is not registered: add it with `ImageConfig::image(..)`".
- `crates/noctis-image/src/fonts.rs` (+ `fonts/tests.rs`):
  - `Font` (`from_static`, `from_bytes(impl Into<Arc<[u8]>>)`, `family`, `unicode_range(RangeInclusive<char>)` appending; `PartialEq` on digest + family + ranges; redacted `Debug`);
  - `FontsConfig`, `FontRegistry` (Resource; `build_error`, `is_empty`, `families`), `validate` (the family and range rows);
  - the bundled `DEFAULT_FONT` behind `cfg(feature = "default-font")`;
  - tests are named `img_01_*` and cover every font row of the Matrix.
- `crates/noctis-image/src/assets.rs` (+ `assets/tests.rs`):
  - `StaticImage` (`const fn new` with an `[IMG-02]` panic, `id()`, `Copy`, redacted `Debug`);
  - `pub(crate) const fn valid_image_id`, `sniff`, `check_pixels`, and `StaticImages` (decode, dedupe, lookup with a byte check);
  - tests named `img_02_*` for every image row (PNG/JPEG/WebP made with `takumi::write_image`; leaked bytes are fine in tests).
- `crates/noctis-image/src/node/mod.rs`, `check.rs`, `lower.rs` (+ their tests) -- `Kind::Image`, `Node::image`, `img`, the class check, and lowering with the images map.
- `crates/noctis-image/src/renderer.rs` (+ `renderer/tests.rs`) -- `RendererConfig`, fonts and families per render, `NoFont` in `register` (test `img_01_register_without_font_fails`), `UnknownImage`; existing tests get a test font.
- `crates/noctis-image/src/plugin.rs` (+ `plugin/tests.rs`) -- the setters, docs and doctest, and the install order; tests for the conflict, `ptr_eq` sharing and `for_test` isolation rows.
- `crates/noctis-image/src/lib.rs` -- modules, re-exports, docs.
- `crates/noctis-macros/src/view.rs` (+ tests) -- `<img>` grammar and the error rows.
- `crates/noctis/Cargo.toml`, `src/lib.rs` -- the `default-font` mapping, docs, and re-exports `Font`, `StaticImage`, `img`.
- `crates/noctis/tests/fonts.rs` (new, `#![cfg(feature = "image")]`) -- one `Glyph { text: String }` Template, 48×48 PNG, `tw="flex w-full h-full items-center justify-center bg-white text-black text-3xl"`. Apps on `Runtime::for_test()`. Covers the fallback, priority, range and `<img>` pixel rows, the two-App rows, `img_01_no_font_fails_startup` (`cfg(not(feature = "default-font"))`), and `default_font_draws_text` (`cfg(feature = "default-font")`).
- `crates/noctis/tests/render.rs`, `render_e2e.rs`, `template.rs` -- every App that installs the ImagePlugin and registers a Template adds `.font(Font::from_static(include_bytes!("../../noctis-image/fonts/InterVariable.woff2")))` (also with `default-font`, which dedupes); update the planned-`<img>` doc comments. Check the `crates/noctis/src/lib.rs` doctests the same way.
- `crates/noctis/tests/ui-image/img/IMG-02_invalid_image_id.{rs,stderr}` -- `const L: StaticImage = StaticImage::new("Logo", b"");`. Generate it with `TRYBUILD=overwrite`, then review it.
- `crates/noctis-rules/src/rule.rs`, `docs/discord-rules.md` §16 -- add the rows:
  - **IMG-01** Fonts. Enforcement `App-build`. How: plugin violations and `RenderError::NoFont` → `TemplateRejected`.
  - **IMG-02** Static images. Enforcement `Compile-time (const evaluation) / App-build / Runtime`. How: `const fn` panic, plugin violations, `RenderError::UnknownImage`.
- Skill, template and API:
  - `examples/project-template/{AGENTS.md, src/modules/profile/view.rs}` and `.claude/skills/noctis/{SKILL.md, references/images.md, references/limits-cheatsheet.md}`: document `Font`, the priority and fallback chain, Unicode ranges, `default-font` (implies `image`, bundled Inter as last resort, OFL-1.1), the process-scoped registry and its conflicts, `StaticImage`/`ImageConfig::image`/`<img>`/`img`, IMG-01/02, `NoFont`, `UnknownImage`, the formats and the 16 MP cap. Drop every "[planned, Story 10.7]".
  - `xtask/public-api.txt`: re-bless it, then copy the printed hash into `SKILL.md`.
- `.github/workflows-disabled/ci.yml` -- confirm only; no new gate.

**Acceptance Criteria:**
- Given no registered font and `default-font` off, when an App with a Template registered starts (`App::build`), then it fails with a `BuildError` holding `TemplateRejected` whose message starts with `[IMG-01] no font is registered`. With `default-font` on, the same App builds and its rendered text has ink, `crates/noctis-image/NOTICE` records Inter (OFL-1.1), and `cargo deny check` passes with the crate-scoped OFL-1.1 exception.
- Given fallback fonts registered with a priority order and Unicode ranges (the Japanese kana fixture), when the unchanged `Glyph` Template renders mixed-script text through `app.renderer()`, then every glyph comes from the first matching font in priority order, proven by exact bitmap equality with single-font renders.
- Given fonts registered through `.image(ImageConfig::new().font(..))`, when `ImagePlugin::install` runs, then the registry is the Runtime's `FontRegistry` Resource. A second App on that Runtime with equal fonts shares it, one with conflicting fonts fails with `BuildViolation::ResourceConflict`, and an App on another `Runtime::for_test()` gets its own.
- Given a `StaticImage` registered with `ImageConfig::image`, when a Template renders `<img src={IMG} />`, then its pixels appear in the output. An unregistered image fails the render with `RenderError::UnknownImage`, and an invalid id in a `const` is a `[IMG-02]` compile error.
- Given the new public API, when the story is done, then the Usage Skill documents font and asset registration, and `skill-check`, `api-snapshot`, `rules-catalog`, `deps-check` and `cargo deny check` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **One `.font()` list instead of a separate `fallback_font`.** In takumi, a Template's `font-*` family is tried first, and any glyph it lacks walks the fallback chain. With order = priority, "fallback fonts" are simply the later fonts, and the first font serves text with no family. One list keeps a single source of priority.
- **Bundled font.** It is a last resort named `"Inter"`, so the crate theme's `--font-display: "Inter", sans-serif` matches. It never outranks author fonts, and Latin text never renders invisibly. Inter 4.1's `InterVariable.woff2` (352 KB) is used as published. WOFF2 needs takumi `woff2`, which adds only `wuff` (MIT): `brotli-decompressor` 5.0.3 is already in the lock. Converting the file to TTF would need an unpinned toolchain.
- **Licensing without a new crate.** The font ships inside `noctis-image`, so its license expression is made accurate (`AND OFL-1.1`), and deny gets a crate-scoped exception ("accepted in `deny.toml`"). The JP fixtures sit under `noctis-image/tests/fixtures/fonts` for the same reason; facade tests `include_bytes!` them through `../../noctis-image/`.
- **Why the CJK example is kana.** A full CJK font is about 16 MB. `@fontsource/noto-sans-jp` ships per-range subsets, and subset 119 (kana, 42 KB, in 400 and 700) gives a real CJK script plus two visually different weights for the priority test.
- **Static images.** They are per App, because nothing in the spine makes them process-scoped and only fonts conflict. `const fn new` puts the id check at compile time where possible (maintainer decision 4). The handle carries its bytes, so the Renderer verifies that the registered bytes match (`ptr::eq` first, then content). PNG, JPEG and WebP decoders are takumi features whose `image`/`png`/`image-webp` crates are already in the graph; JPEG adds `zune-jpeg`. `<svg>` stays out: `svg-source` pulls in usvg, and a raster `StaticImage` covers logos.
- **Typed error at build.** Core cannot name fonts, and the check needs the folded Templates, so it lives in `Renderer::register` (`RenderError::NoFont`). Core already wraps that as `TemplateRejected`; the `[IMG-01]` prefix makes the message greppable.

## Verification

**Commands** (run in `/home/claude/lanes/a/noctis-rs`):
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` and `cargo test --release --workspace --all-features` -- pass
- `cargo hack test --release --workspace --each-feature` -- pass (`image` alone runs `img_01_no_font_fails_startup`; `default-font` alone runs `default_font_draws_text`)
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `cargo tree -p noctis-image --features render -i takumi-core` -- a single 0.31.0
- `bash tools/story-loop/gates.sh` -- ALL-OK
