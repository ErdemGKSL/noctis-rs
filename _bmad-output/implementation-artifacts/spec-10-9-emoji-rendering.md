---
title: 'Story 10.9: Emoji rendering'
type: 'feature'
created: '2026-10-24'
status: 'in-progress'
baseline_revision: '012de2288f5eba6af9fc04d0f03250c3c2bf2fd7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-8-remote-image-fetching-through-the-assetfetcher.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-68 is not implemented. The facade's `image-emoji` feature is an empty placeholder, so Unicode emoji in Template text draw as boxes. Discord custom emoji (`<:name:id>`, `<a:name:id>`) in Template text draw as raw markup. Nothing warns the author about either.

**Approach:**
- **Unicode emoji.** The `image-emoji` feature bundles the Twemoji COLRv0 font (`twemoji-colr-font` 15.0.3 from npm, family "Twemoji Mozilla"). It is registered in the process-scoped `FontRegistry` as a last-resort font. takumi's Blink-style fallback already sends emoji-presentation runs to color fonts first and text runs to non-color fonts first.
- **Startup warning.** With the feature off, `Renderer::register` logs a warning when Templates are registered.
- **Custom emoji, found.** Custom emoji tokens are found by scanning the job's data bytes before pool admission.
- **Custom emoji, fetched.** They are fetched through the 10.8 `AssetFetcher` as `RemoteImage::custom_emoji(id, animated)` from `<cdn>/emojis/<id>.webp?size=128[&animated=true]`, under a separate per-job cap.
- **Custom emoji, drawn.** At lowering, a text node holding a token is split into inline text runs and `1.375em` inline images.

## Boundaries & Constraints

**Always:**
- **Font asset.**
  - File: `crates/noctis-image/fonts/TwemojiMozilla.woff2`, the npm tarball `https://registry.npmjs.org/twemoji-colr-font/-/twemoji-colr-font-15.0.3.tgz` → `package/twemoji.woff2`, used unmodified. Its sha256 must be `598b6867d47954acab46edd6a9773d734b844d221d3a95834da7442d5df64f98` (476456 bytes). Verified 2026-10-24: COLR v0 + CPAL, 1438 cmap entries, name "Twemoji Mozilla", "Version 0.7.0". The package was published 2024-01-28.
  - License text: `crates/noctis-image/fonts/LICENSE-Twemoji.txt`, a verbatim copy of `package/LICENSE`. It holds the OFL-1.1 terms for the font and the CC-BY-4.0 terms for the graphics.
- **Licensing (AC 1).**
  - `noctis-image` `license = "(MIT OR Apache-2.0) AND OFL-1.1 AND CC-BY-4.0"`.
  - `deny.toml`'s exception becomes `{ crate = "noctis-image", allow = ["OFL-1.1", "CC-BY-4.0"] }`, with its comment naming Twemoji.
  - `NOTICE` gains a "Bundled emoji font (the `emoji` feature)" entry in the Inter entry's format: source, version, license, sha256. It carries the attribution "Emoji graphics: Twemoji, Copyright Twitter, Inc and other contributors, CC-BY-4.0 (https://creativecommons.org/licenses/by/4.0/); font build: Mozilla twemoji-colr, packaged by Tilman Vatteroth as twemoji-colr-font 15.0.3; unmodified". Fix the intro sentence to say the license expression.
  - The same attribution, in short form, goes in the feature docs: the `crates/noctis/Cargo.toml` `image-emoji` comment, the `crates/noctis/src/lib.rs` feature list (`:574`), the `noctis-image` Cargo.toml feature comment, and the skill.
- **Features.**
  - `noctis-image`: `emoji = ["render"]`.
  - Facade: `image-emoji = ["image", "noctis-image/emoji"]`.
  - No new crate and no new dependency edge. takumi's `woff2` feature is already on.
- **Registry.**
  - `FontsConfig` gains `emoji: bool`, set to `cfg!(feature = "emoji")` in `plugin.rs:337`.
  - `FontRegistry::init` registers the font after the bundled Inter, with `FontResource::last_resort()` and the family override `"Twemoji"` (`pub(crate) const EMOJI_FAMILY`).
  - A load failure is the build error `[IMG-04] the bundled emoji font could not be loaded: <err>`.
  - `FontRegistry::has_emoji()` reports whether it registered.
  - The emoji font is not a text font: `is_empty()` ignores it, so emoji alone with no text font is still `RenderError::NoFont`.
  - It is never added to `families`/`chain`, so it never outranks author fonts or text-presentation digits.
- **Startup warning (AC 2).** `TakumiRenderer::register(templates)` runs on every App build. When `templates` is non-empty and `!has_emoji()`, it logs once per call: `tracing::warn!(target: "noctis::image", "[IMG-04] the `image-emoji` feature is off: Unicode emoji in Template text render as boxes")`. With no Templates, or with the feature on, nothing is logged.
- **Token grammar (IMG-04).** Discord's markdown rule, ASCII only:
  - the form is `<` + optional `a` + `:` + name + `:` + id + `>`;
  - name is 2..=32 of `[A-Za-z0-9_]`;
  - id is 17..=20 ASCII digits, parsed as a non-zero `u64`;
  - anything else stays literal text.
- **Scanner.** It lives in a new takumi-free, ungated `#[doc(hidden)] pub mod emoji` in `noctis-image`, so `noctis-macros` can use it.
  - `pub struct CustomEmoji { pub id: NonZeroU64, pub animated: bool }` (`Copy`, `Eq`, `Hash`, `Debug`).
  - `pub enum Piece<'a> { Text(&'a str), Emoji(CustomEmoji) }`.
  - `pub fn split(text: &str) -> Split<'_>`, an iterator of `Piece`s. Concatenating its text pieces with the tokens restores the input. Empty text pieces are never yielded.
  - `pub fn scan(bytes: &[u8]) -> Vec<CustomEmoji>`: the distinct `(id, animated)` pairs in order of first appearance, valid on non-UTF-8 input.
  - Both use one byte matcher and allocate nothing when the input has no `<`.
- **Declaring emoji.** In `TakumiRenderer::render`, right after the `#[asset]` plan:
  - `emoji::scan(&job.data)` runs over the postcard data. Strings are contiguous UTF-8 in postcard, so every token in any string field is found. A token-shaped run elsewhere is a harmless superset.
  - The distinct emoji are planned by a new `AssetFetcher` method: the first `max_emoji` in order are fetched as `RemoteImage::custom_emoji(id, animated)` with `required = false` and field `"text"`.
  - The rest map to the fallback, with one warning: `"<n> custom emoji over the per-job cap of <max> draw the fallback"`.
  - Emoji never count against `max_per_job` and never produce `OverCap`, so user text cannot fail a render.
  - When the `#[asset]` plan and the emoji are both empty, admission stays synchronous and byte-for-byte unchanged (10.8). Otherwise everything resolves together through the existing `resolve` (same semaphore, timeout, caps, `AssetStore` cache and warnings), and admission happens after it.
- **`RemoteImage::custom_emoji(id: Id<EmojiMarker>, animated: bool) -> Self`** (public).
  - Adds `Source::Emoji { id: u64, animated: bool }` as the **last** variant, so postcard indices stay stable.
  - `target()` re-validates `id != 0` (`"emoji id 0 is invalid"`).
  - URL: `cdn_url(cdn, "emojis/<id>", 128, animated && job_animated)`.
  - Key: `img:v1:emoji:<id>:128:<a|s>`.
  - `Display`/`Debug`: `emoji/<id>`.
- **`RemoteConfig::max_emoji(n: usize)`** sets the per-job emoji cap. It accepts 0..=128 (default 32); 0 fetches none and draws the fallback for all. Outside the range, the plugin violation is `[IMG-04] remote max_emoji <v> is out of range: use 0..=128`.
- **Lowering.**
  - `Kind::Text(t)` without any `Piece::Emoji` lowers exactly as today: a text node, no allocation beyond today's.
  - With emoji, it lowers to a container styled `display: inline`, which carries the text node's classes and style. Its children are text nodes for the text pieces and image nodes for the emoji, each styled `display: inline-block; width: 1.375em; height: 1.375em; vertical-align: -0.3em; object-fit: contain`.
  - An emoji is looked up in `Resolved` by `RemoteImage::custom_emoji(id, animated)`, meaning that `(id, animated)` pair; the name is ignored. A failed or over-cap emoji resolves to the fallback.
  - A token with no entry is `RenderError::UndeclaredImage { id, image: "emoji/<id>" }`. That happens only for text that does not come from the data.
- **Compile time first (maintainer decision 4).** A `view!` literal text child for which `split` yields any `Piece::Emoji` is a compile error, spanned on the literal: ``[IMG-04] custom emoji in literal `view!` text are never fetched: put the text in a Template field, or declare `#[asset] e: RemoteImage` (`RemoteImage::custom_emoji(id, animated)`) and draw `<img src={&self.e} />` ``.
- **Same commit (AGENTS.md).** Update the Usage Skill, the template's `AGENTS.md`, the API snapshot and its hash, `docs/discord-rules.md` (new IMG-04 row), `noctis-rules` `IMG_04`, and the rules-catalog evidence.

**Never:**
- Never enable takumi `svg`/`gif` features.
- No other emoji font or format: no CBDT/sbix, no Noto Color Emoji, no COLRv1 (takumi 0.31 draws COLRv0 only).
- No `EmojiFont` config type: the feature alone selects Twemoji.
- Never `cargo update`; `Cargo.lock` gains no package.
- Never fetch emoji named only in view-built text.
- Never parse other markup: no mentions, no timestamps, no `\` escapes.
- No render-cache or single-flight changes (10.10).
- No change to the 10.8 behavior of Templates whose data holds no token.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Token ok | `<:pepe:123456789012345678>`; `<a:pepe:123456789012345678>` | `Emoji { id, animated: false }`; `animated: true` | none |
| Name bounds | 1 char `<:p:…>`; 2 `pp`; 32 chars; 33 chars | text; emoji; emoji; text | stays text |
| Name chars | `pe_pe9`; `pe-pe`; `pépé`; `pe pe`; empty `<::id>` | emoji; text; text; text; text | stays text |
| Id bounds | 16 digits; 17; 20 = `18446744073709551615`; `18446744073709551616`; 21 digits; 17 zeros; `+1234…` | text; emoji; emoji; text (overflow); text; text (id 0); text | stays text |
| Shape | `<A:x1:…>`; missing `>`; `< :pepe:…>`; `<:pepe:…>>`; `<<:pepe:…>` | text; text; text; emoji + `>`; `<` + emoji | stays text |
| Split | `""`; `"plain"`; `"a<:pp:ID>b"`; two adjacent tokens; a token at the start and at the end; `"é<:pp:ID>😀"` | none; `[Text]`; `[Text a, Emoji, Text b]`; `[Emoji, Emoji]`; no empty pieces; UTF-8 boundaries kept | none |
| Scan | the same token twice; static and animated with the same id; non-UTF-8 bytes around a token; no `<` | one entry; two entries; found; empty, no allocation | none |
| Unicode emoji on | `emoji` feature, text `😀` at 64 px | ≥ 200 pixels within ±24 of Twemoji yellow `#FFCC4D` | none |
| Text unaffected | `"Ab12"` rendered with the emoji font registered and without it (two `FontsConfig`s in one test) | identical output bytes | none |
| Emoji off | `FontsConfig { emoji: false }`, text `😀` | no yellow pixels | none |
| Startup warning | `register(&[t])` without emoji; `register(&[])`; `register(&[t])` with emoji | 1 `noctis::image` warning containing `[IMG-04]` and `image-emoji`; none; none | warn only |
| Emoji only | `emoji` on, `default-font` off, no `.font` | `RenderError::NoFont` at register | build fails |
| Custom fetched | data `"ab <:pepe:123456789012345678> cd"`, loopback CDN serves a 4×4 red PNG | 1 request `GET /emojis/123456789012345678.webp?size=128`; red pixels between the `ab` and `cd` ink on the same line | none |
| No raw text | text color green, data `"<:pepe:ID>"` alone | red pixels, 0 green pixels | none |
| Animated emoji | `<a:x1:ID>` in an animated job serving a 2-frame WebP; the same in a still job | `&animated=true`, frames differ; no `animated` param, key `:s` | none |
| Failure | 404, timeout, a GIF body, a not-allow-listed CDN | the fallback is drawn inline, plus a warning naming `emoji/<id>` | never fails the render |
| Cap | 33 distinct with the default 32; `max_emoji(0)` with 3 | 32 requests, the 33rd draws the fallback, 1 cap warning; 0 requests, all fallback | none |
| Cap config | `max_emoji` 0 / 128 / 129 | ok / ok / `[IMG-04] remote max_emoji 129 is out of range: use 0..=128` | no Renderer |
| Separate cap | 16 `#[asset]` images + 20 emoji | succeeds (no `OverCap`) | none |
| Cache | the same emoji rendered twice with a bound `MemoryAssetStore` | 1 request; key `img:v1:emoji:<id>:128:s` | none |
| Undeclared | a view outputs `format!("<:x1:{ID}>")` with an ID that is not in the data | `RenderError::UndeclaredImage { image: "emoji/<ID>" }` | Render sink |
| Lookup ignores name | data `<:pepe:ID>`, view uppercases it | resolved, drawn | none |
| No token | every existing Template and test | unchanged: synchronous admission, identical output | none |
| Constructor | `custom_emoji(Id::new(1), false)` → `Display` `emoji/1`; decoded `Source::Emoji { id: 0 }` | ok; a warning plus the fallback, "emoji id 0 is invalid" | fallback |
| `view!` literal | `view! { <p>"hi <:pepe:123456789012345678>"</p> }`; `"<:p:1>"` | compile error `[IMG-04] custom emoji in literal …`; compiles (not a token) | compile error |

</intent-contract>

## Code Map

- `crates/noctis-image/src/lib.rs` `:36`: add `pub mod emoji;` (ungated, `#[doc(hidden)]`), next to `pub mod tw`.
- `crates/noctis-image/src/emoji.rs` (+ `emoji/tests.rs`): new scanner.
- `crates/noctis-image/src/fonts.rs`:
  - `FontsConfig` `:236`, `FontRegistry::init` `:251-284`, `register_bundled` `:287` (the pattern to copy for `register_emoji` under `#[cfg(feature = "emoji")]`, with a no-op otherwise);
  - add `has_emoji()`;
  - the module doc `:1-10` names the emoji font.
- `crates/noctis-image/src/plugin.rs` `:337-340`: `emoji: cfg!(feature = "emoji")`. `check_config` gains the `max_emoji` range through `RemoteConfig::validate`.
- `crates/noctis-image/src/remote.rs`:
  - `Source` `:70` (append `Emoji`);
  - the constructors `:137-205` (add `custom_emoji`);
  - `target` `:220-280`, `Display` `:312-345`, `cdn_url` `:296`.
- `crates/noctis-image/src/remote/config.rs`: constants `:17-22`, the field, setter and default `:72-165`, `validate` `:167`, the doc table `:30-62`.
- `crates/noctis-image/src/remote/fetch.rs`:
  - `plan` `:153-185`: add the emoji planning (capped, overflow mapped to the fallback);
  - `Resolved` `:403-430`: insert fallback entries;
  - reuse the warning helper.
- `crates/noctis-image/src/renderer.rs`:
  - `declared` `:257-268` and `render` `:279-307`: emoji scan, plan, and choice of the synchronous or resolved path;
  - `register` `:301-317`: the warning.
- `crates/noctis-image/src/lower.rs` `:47`: `Kind::Text` splitting through `emoji::split`. The inline style goes through `takumi::prelude::Style::from_str`, as `inline_style` does.
- `crates/noctis-image/src/remote/test_server.rs`: reuse it for the loopback CDN in the renderer tests (`renderer/tests/` holds the 10.8 remote tests; add `renderer/tests/emoji.rs`).
- `crates/noctis-image/tests/fixtures/fonts/noto-sans-jp-119-400-normal.woff2`: a text font for tests that must pass without `default-font` (`cargo hack --each-feature`).
- `crates/noctis-macros/src/view.rs` `:434` (`Child::Text(lit)`): call `noctis_image::emoji::split(&lit.value())` and reject any `Piece::Emoji`. Tests go in its test module (`:560`+).
- `crates/noctis/Cargo.toml` (`image-emoji`) and `crates/noctis/src/lib.rs` `:574` (feature docs) plus the image module docs `:1961-2100`.
- `crates/noctis-rules/src/rule.rs` `:78`: `IMG_04 = "IMG-04"`.
- `docs/discord-rules.md`: after `:308`, the IMG-04 row.
- Skill and template:
  - `examples/project-template/.claude/skills/noctis/references/images.md` §1 `:13`, §6 `:314-315`, §7;
  - `references/troubleshooting.md` `:186` (the nonexistent `EmojiFont::Twemoji`);
  - `references/limits-cheatsheet.md`;
  - `SKILL.md` `:23` (the API hash);
  - `examples/project-template/AGENTS.md` `:69`;
  - `examples/project-template/Cargo.toml` `:29` (the comment is already right).
- takumi facts (read-only, takumi-core 0.31.0):
  - `resources/glyph.rs:429`: COLRv0 only;
  - `fonts/font_cache.rs:19-36`: emoji priority prefers `is_color` fonts, text priority non-color;
  - `resources/font.rs:351`: the fallback bucket is the chain plus the last-resort families;
  - `fonts/font_fallback_iterator.rs`: font-group fonts are tried first, so a glyph an author font has (©, ™) draws from it.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-image/fonts/{TwemojiMozilla.woff2,LICENSE-Twemoji.txt}`, `NOTICE`, `Cargo.toml` (license, `emoji` feature, comment), `deny.toml` -- the asset and licensing -- AC 1.
- `crates/noctis-image/src/emoji.rs` + `emoji/tests.rs` -- the scanner; tests `img_04_*` for every grammar, split and scan row -- token rules.
- `crates/noctis-image/src/fonts.rs` (+ `fonts/tests.rs`), `plugin.rs` -- emoji registration, `has_emoji`, the NoFont row.
- `crates/noctis-image/src/remote.rs`, `remote/config.rs`, `remote/fetch.rs` (+ their tests) -- `custom_emoji`, `max_emoji`, the emoji plan and fallbacks -- the constructor, cap and config rows.
- `crates/noctis-image/src/renderer.rs`, `lower.rs` (+ `renderer/tests/emoji.rs`, `lower/tests.rs`) -- the scan, resolve and inline lowering, plus the startup warning (captured with `tracing-subscriber`, as `remote/fetch/tests.rs` does). Gate Unicode pixel tests with `#[cfg(feature = "emoji")]`; they register the Noto kana fixture so they pass with and without `default-font` -- the fetched, raw-text, animated, failure, cache, undeclared, no-token, Unicode and warning rows.
- `crates/noctis-macros/src/view.rs` (+ tests) and `crates/noctis/tests/ui-image/img/IMG-04_literal_custom_emoji.{rs,stderr}` (generate with `TRYBUILD=overwrite`, then review) -- the literal check.
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` -- the feature wiring and docs.
- `crates/noctis/tests/emoji.rs` (new, `#![cfg(feature = "image")]`) -- end to end through `App::build` on `Runtime::for_test()`, with `RemoteConfig::new().discord_cdn("http://127.0.0.1:<port>")`:
  - a Template whose `String` field holds a custom emoji renders the served image (red pixel);
  - `#[cfg(feature = "image-emoji")]`: `😀` renders yellow.
- `crates/noctis-rules/src/rule.rs`, `docs/discord-rules.md` -- the **IMG-04** Emoji row:
  - **Limit/value:** the token grammar; size 128; `max_emoji` 0..=128 (32); the Twemoji COLRv0 font (`image-emoji`) **[N]**.
  - **Source:** [REF §Message Formatting](https://discord.com/developers/docs/reference#message-formatting) and [§Image Formatting](https://discord.com/developers/docs/reference#image-formatting).
  - **Enforcement:** `Compile-time / App-build / Runtime`.
  - **How:** the fixture; the `max_emoji` violation and the startup warning; the scan, the fallback and `UndeclaredImage`.
  - Evidence: the fixture plus the `img_04_*` tests.
- Skill and template:
  - `images.md`: rewrite the §6 emoji paragraph with the feature, the warning text, last-resort behavior, author-font precedence, attribution advice for bots that post Twemoji images, and that a registered COLRv0 font of your own also works;
  - add a §7 "Custom emoji" subsection: automatic from Template data, the cap, the fallback, size, `custom_emoji` with `<img>`, the literal compile error, `UndeclaredImage`, admission after fetch;
  - troubleshooting: the row becomes `image-emoji`, and an IMG-04 row is added;
  - the cheatsheet gets IMG-04; the template's `AGENTS.md` gets one sentence;
  - re-bless `xtask/public-api.txt` and copy the hash into `SKILL.md`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions already logged by the planner.
- `.github/workflows-disabled/ci.yml` -- confirm only: there is no new gate, `--each-feature` covers `image-emoji`.

**Acceptance Criteria:**
- Given `image-emoji`, when Template text contains `😀`, then the PNG holds Twemoji-yellow pixels (pixel test), and `NOTICE`, `LICENSE-Twemoji.txt`, the `image-emoji` feature docs and the skill carry the CC-BY-4.0 Twemoji attribution. `cargo deny check` passes, and `noctis-image`'s license is `(MIT OR Apache-2.0) AND OFL-1.1 AND CC-BY-4.0`.
- Given `image-emoji` off and an App that registers a Template, when `App::build` runs, then exactly one `noctis::image` warning containing `[IMG-04]` says emoji render as boxes, and the Usage Skill documents it.
- Given Template data text containing `<:name:id>` or `<a:name:id>`, when it renders, then the emoji is fetched once through the AssetFetcher (allow-list, cache, caps) and drawn inline on the text line. No glyph of the markup is drawn, and a failure draws the fallback.
- Given the new public API, when the story is done, then `skill-check`, `api-snapshot`, `rules-catalog` and `deps-check` pass, and `Cargo.lock` adds no package.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why a byte scan of `job.data`.** The scan needs no decode, no ViewFn change and no derive change. Every string in the data is covered automatically, so authors annotate nothing, and the emoji are known before admission, as AD-15 requires ("declared by the Template, including custom emoji"). Token-shaped bytes outside strings only add capped, allow-listed fetches. Text the view builds itself is never fetched, and drawing it is a typed `UndeclaredImage`.
- **Why a separate cap with fallback overflow.** The emoji come from user text. With `OverCap`, any user could break a card by posting 17 emoji.
- **Why last-resort registration.** takumi tries author fonts, then (for emoji runs) color fallback fonts. So digits, `#` and `*`, which Twemoji maps for keycaps, keep the text font, and author fonts keep priority.
- **Size 128.** That is Discord's maximum custom emoji resolution, sharp up to about 90 px glyphs. The `1.375em` box matches Discord's inline emoji.

## Verification

**Commands** (run in `/home/claude/lanes/a/noctis-rs`):
- `sha256sum crates/noctis-image/fonts/TwemojiMozilla.woff2` -- `598b6867…4f98`
- `cargo fmt --all --check` -- clean
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release -p noctis-image --features render,emoji` and `--features render` -- pass
- `cargo test --release --workspace --all-features` -- pass
- `TRYBUILD=overwrite cargo test --release -p noctis --test ui --features image` once, review the new `.stderr`, then run it again without `TRYBUILD` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- pass
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass
- `git diff --stat Cargo.lock` -- no new `[[package]]`
- `bash tools/story-loop/gates.sh` -- ALL-OK
