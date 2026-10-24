---
title: 'Story 11.5: Runtime Locale overlays through LocaleSource'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
baseline_revision: '62f2c5fcae3c1d1e7ba9ef3ae8997455adb59518'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-11-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/ownership-and-lifetimes.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The `LocaleSource` port (`noctis-ports/src/locale.rs`, default `NoLocaleOverlay`) and `AppBuilder::locale_source(..)` exist, but nothing ever calls `load`: translators cannot fix or ship content text without a redeploy, and nothing rejects an overlay of a published command text or one whose placeholders differ from the compiled set (FR-42, FR-38, AD-16, AD-1).

**Approach:** At startup (`App::run_until`/`run`, before the startup Publish and any shard), the App calls `LocaleSource::load` once per locale it has a file for, validates every pair against the merged `AppStrings` (content keys only, compiled placeholder set, no duplicates), and on success installs the overlays into each owner catalog's process-wide overlay slot, which both the App's `Translate` path (`Text::Key`, `reply_text`, `defaults(..)` keys, notices) and the `i18n!`-generated `Strings::of(..)` accessors read. Any violation aborts startup with a typed `Error::LocaleOverlay`.

## Boundaries & Constraints

**Always:**

- **When.** `App::run_with` (behind `run`, `run_until`, `TestApp::build`) loads overlays after the token check and before `publish_at_startup`, inside the existing biased `tokio::select!` with `shutdown` (a shutdown during the load returns `Ok(())` with the existing "shutdown started/complete" logs). CLI Publish modes (`--publish*`) never load (overlays cannot touch command texts). An App with no catalogs (`AppStrings::catalogs()` empty) never calls `load`.
- **Which locales.** One `load(code)` call per locale in the App's available set (`AppLocales::available()`: every locale with a file in any merged catalog), sequentially in `Locale::ALL` order, with `code` = the `Locale`'s `Display` (`"en-US"`, `"tr"`). The first `PortError` stops loading: `LocaleOverlayError::Load { locale, source }`; nothing is installed.
- **Validation** (pure, in `noctis-i18n`, over all loaded locales before anything is installed), per `(locale, key, value)` pair, first matching rule wins:
  1. the same `key` more than once in one locale's `Vec` → one `OverlayViolation::DuplicateKey { locale, key }` per `(locale, key)`; no other check runs for that key in that locale;
  2. first dot-segment of `key` (`key.split('.').next()`) is `cmd`, `cmdword`, `ctxcmd` or `noctis` → `ReservedKey { locale, key }` (so `cmd`, `cmd.x`, `noctis` are reserved; `cmdx.y`, `CMD.x` are not);
  3. `key` is not a content key of its owner catalog (`AppStrings::owner(key)` is `None`, or the owner's content list lacks it, e.g. the owner was built without an overlay slot) → `UnknownKey { locale, key }`;
  4. the **set** of `noctis_rules::placeholder_names(value)` differs from the set of `owner.placeholders(key)` → `Placeholders { locale, key, expected, found }` (`expected` = compiled names in compiled order; `found` = the value's unique names in first-appearance order). Order and repetition do not matter (`{n} {n}` matches `{n}`).
  - Everything else is accepted verbatim: empty values (`""` when no placeholders), any Unicode, any length (Discord limits are checked at send time by the Responder, as for file values), literal braces (`{}`, `{a b}`, unclosed `{x`), a value equal to the compiled one.
  - Violations are sorted by `Locale::ALL` index, then key bytes, then rule order above; `AlreadyInstalled` ones follow, in catalog registration order. Every violation is reported (no early stop).
- **Install.** Only when there are no violations. Overlays are grouped by owner catalog. A catalog whose group is empty is not touched. A catalog whose slot already holds overlays: equal entries (same `(locale, key, value)` set) is `Ok`, different is `OverlayViolation::AlreadyInstalled { origin }` and nothing of this load is installed (checked for every catalog before any `set`). After a successful install, one `info` on target `noctis::i18n`: `"locale overlays installed"` with `values` (count) and `catalogs` (count).
- **Semantics.** An overlay value for locale `L` acts as if written in `L`'s file of its owner catalog (added when that file lacks the key or the catalog has no `L` file). Lookup for `(L, key)` becomes: overlay(L), file(L), overlay(L.sibling()), file(L.sibling()), overlay(default), file(default). This applies identically to `Catalog::translate` (hence `AppStrings`, Responder `Text::Key`, `reply_text`, `DefaultText::Key`, notices, webhook keys) and to `Strings::of(L)` accessors. Keys outside the content list (`noctis.*` overrides) keep the compiled chain; a content key's row index is found by binary search in `content`. Rows are built for all 32 locales (a locale whose chain holds no overlay keeps the compiled row: `None`). Locale resolution (`AppLocales`, Story 11.3) is unchanged. `Catalog::get` and `AppStrings::get` (Publish's lookup) stay compiled-file-only.
- **Process-wide.** The overlay slot is a `static` emitted per `i18n!` invocation, so overlays are process-wide per catalog and set at most once; every App in the process (and `Strings::of` anywhere) sees them. Strings are owned by the slot: **no `Box::leak`**.
- **New public API (facade root, ungated, like `StartError`):**
  - `Error::LocaleOverlay(LocaleOverlayError)` (`#[error(transparent)]`, `#[from]`).
  - `noctis_core::LocaleOverlayError` (`Debug`, `thiserror`, `#[non_exhaustive]` enum): `Load { locale: Locale, #[source] source: PortError }` (Display ``the LocaleSource could not load the `{locale}` overlay``), `Invalid(Vec<OverlayViolation>)` (Display `{n} locale overlay violation(s)` then one `\n  - {violation}` line each, like `BuildError`); plus `fn violations(&self) -> &[OverlayViolation]` (empty for `Load`).
  - `OverlayViolation` (defined in `noctis-i18n`, re-exported by `noctis-core` and the facade root; `Debug, Clone, PartialEq, Eq`, `#[non_exhaustive]`, hand-written `Display` + `std::error::Error`, no new dependency). Variants and Display:
    - `DuplicateKey { locale: Locale, key: String }`: ``locale `tr`: overlay key `shop.title` appears more than once``
    - `ReservedKey { locale, key }`: ``locale `tr`: overlay key `cmd.ban.description` is reserved: `cmd.*`, `cmdword.*`, `ctxcmd.*` and `noctis.*` cannot be overlaid (published command texts change only with a redeploy)``
    - `UnknownKey { locale, key }`: ``locale `tr`: overlay key `shop.titel` is not a content key of any catalog``
    - `Placeholders { locale, key, expected: Vec<String>, found: Vec<String> }`: ``locale `tr`: overlay of `shop.left` must use the placeholders `{n}`; missing `{n}`, extra `{count}` `` (same `braces`/`(none)` shape and `; missing …, extra …` parts as `i18n!`'s translation check, `model.rs:845-860`)
    - `AlreadyInstalled { origin: &'static str }`: ``catalog `bot/locales` already holds different overlays in this process (overlays install once per process)``
    - Every variant and field documented; doctests for `LocaleOverlayError` Display and one `OverlayViolation` Display.
- **Hidden API** (`#[doc(hidden)]`, for `i18n!` output and tests): `noctis_i18n::OverlaySlot` (`const fn new()`, manual `Debug` printing `installed: bool`, `Sync`); `Catalog::__with_overlay(self, slot: &'static OverlaySlot, content: &'static [&'static str]) -> Self` (`const fn`; `content` = content keys sorted by key bytes = row order); `Catalog::__overlay_row(&self, Locale) -> Option<&'static [&'static str]>`; `AppStrings::install_overlays(&self, overlays: &[(Locale, Vec<(String, String)>)]) -> Result<(), Vec<OverlayViolation>>` (validation + install as above). `Catalog::__new` keeps its signature (all 48 existing call sites unchanged); such a catalog has no slot and no content list.
- **Template, skill, snapshot, docs** ship in the same commit (AGENTS.md).

**Never:**

- No overlay of `cmd.*`, `cmdword.*`, `ctxcmd.*` or `noctis.*`; no change to Publish JSON, `diff.rs`, dispatch or `AppLocales`' available set.
- No reload or periodic refresh, no load in CLI Publish modes, no per-App overlay copy, no `Box::leak`, no `unsafe`.
- No graceful degradation: a failed or invalid load fails startup (decision below).
- No change to `LocaleSource`'s signature or `NoLocaleOverlay`; no new rule id in `docs/discord-rules.md`; no new dependency.
- No debug builds, no trybuild `pass` suites, no weakening of `.cargo/config.toml`.

## I/O & Edge-Case Matrix

Catalog `C` = `bot/locales`, default en-US, files en-US `{greet.hello: "Hello", shop.left: "{n} left", pos.x: "{0}/{1}", cmd.greet.description: "Greet"}`, tr `{greet.hello: "Merhaba"}`; available set `{en-US, en-GB(no), tr}` = `{en-US, tr}` unless a row says otherwise. "ov L k=v" = `load(L)` returns `(k, v)`.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Fix a translation | ov tr `greet.hello=Selam` | tr `reply_text(keys::GREET_HELLO)` and `Strings::of(Tr).greet.hello()` → `Selam`; en-US unchanged `Hello` | none |
| 2 | Add a missing translation | ov tr `shop.left={n} kaldı` | `Strings::of(Tr).shop.left(3)` → `3 kaldı` (was `3 left`) | none |
| 3 | Default-locale overlay | ov en-US `greet.hello=Hi` | en-US `Hi`; `de` (no file) → `Hi`; tr keeps `Merhaba` (own file wins) | none |
| 4 | Sibling chain | C also has en-GB file without `greet.hello`; ov en-US `greet.hello=Hi` | en-GB → `Hi` (file(en-GB) lacks it, overlay(en-US) next) | none |
| 5 | Overlay for locale with no file in owner | catalog D (other origin) has a `de` file; ov de `greet.hello=Hallo` (owner C has no de file) | de → `Hallo` | none |
| 6 | Not loaded locale | `pt-BR` not in available set | `load("pt-BR")` never called; exactly one call per available locale, in `Locale::ALL` order | none |
| 7 | No catalogs | App without `.locales` | `load` never called; startup unchanged | none |
| 8 | Default port | `NoLocaleOverlay` | all loads empty; no slot touched; no `info` log | none |
| 9 | Empty value | ov tr `greet.hello=""` | accepted; tr text `""` (Responder validates at send) | none |
| 10 | Unicode / long value | ov tr `greet.hello=😀 Çağ` / 10 000 chars | accepted verbatim | none |
| 11 | Repeated placeholder | ov tr `shop.left={n}-{n}` | accepted; `shop.left(2)` → `2-2` | none |
| 12 | Positional | ov tr `pos.x={1} / {0}` ok; `{0}` only; `{00}/{1}`; `{0}/{1}/{2}` | ok / Placeholders missing `{1}` / missing `{0}`, extra `{00}` / extra `{2}` | none / `Err` ×3 |
| 13 | Missing placeholder | ov tr `shop.left=kaldı` | `Placeholders { locale: Tr, key: "shop.left", expected: ["n"], found: [] }`, Display `… placeholders `{n}`; missing `{n}`` | `Err` |
| 14 | Extra / renamed placeholder | ov tr `greet.hello=Hi {name}`; `shop.left={count}`; `shop.left={N}` | expected `(none)` extra `{name}`; missing `{n}`, extra `{count}`; missing `{n}`, extra `{N}` | `Err` |
| 15 | Literal braces | ov tr `greet.hello=a {} {a b} {x` | accepted (no placeholders found) | none |
| 16 | Reserved families | keys `cmd.greet.description`, `cmdword.greet`, `ctxcmd.k.name`, `noctis.expired`, `cmd`, `noctis` | one `ReservedKey` each, even when the key exists in a file | `Err` |
| 17 | Not reserved | `cmdx.y`, `CMD.greet` | `UnknownKey` (not `ReservedKey`) | `Err` |
| 18 | Unknown keys | `shop.titel`, `""`, `" greet.hello"`, `greet`, `greet.hello.x`, `çay.x` | `UnknownKey` each | `Err` |
| 19 | Catalog without slot | owner built with `Catalog::__new` only | `UnknownKey` | `Err` |
| 20 | Duplicate in one locale | ov tr `greet.hello` twice (equal or not, or a duplicate with bad placeholders) | one `DuplicateKey`, no `Placeholders` for it | `Err` |
| 21 | Same key, two locales | ov en-US and tr `greet.hello` | both accepted | none |
| 22 | Several problems | reserved in tr, unknown in en-US, mismatch in tr | all reported, sorted (en-US first, then tr by key bytes); nothing installed (row 1 values absent) | `Err` |
| 23 | Load failure | `load("tr")` returns `PortError::Unavailable` | `Error::LocaleOverlay(LocaleOverlayError::Load { locale: Tr, .. })`, `source()` is the PortError; later locales not loaded; nothing installed | `Err` |
| 24 | Second App, equal overlays | same process, same catalog, identical overlays | `Ok`, no change | none |
| 25 | Second App, different overlays | same catalog, other values | `Invalid([AlreadyInstalled { origin: "bot/locales" }])`, first overlays kept | `Err` |
| 26 | Second App, no overlays | same catalog, empty loads | `Ok`; it sees the first App's overlays (process-wide) | none |
| 27 | Shutdown during load | `load` never resolves, shutdown fires | `run_until` returns `Ok(())` | none |
| 28 | Publish unaffected | overlays installed; `AppProbe::commands()` / dry-run | byte-identical JSON; `Catalog::get` unchanged | none |
| 29 | Strings before install | `Strings::of(Tr)` taken before startup | compiled text; a later `Strings::of(Tr)` sees overlays | none |
| 30 | CLI publish mode | `run_with_args(["--publish", "--dry-run"])` with an invalid overlay | `load` not called; mode succeeds | none |
| 31 | Startup order | invalid overlay + `publisher(true)` | fails before any HTTP request to Discord (no startup Publish, no `GET /gateway/bot`) | `Err` |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/locale.rs` -- `LocaleSource` trait (keep signature); rewrite docs: called once per available locale at startup, content keys only, validation and failure behavior. `memory/trivial.rs` `NoLocaleOverlay` unchanged.
- `crates/noctis-i18n/src/catalog.rs` -- `Catalog` (`:43-150`: fields, `__new`, `get`, `placeholders`, private `template`, `Translate`), `AppStrings` (`:170-300`: `owner`, `get`, `Translate`). Add the `overlay: Option<&'static OverlaySlot>` and `content: &'static [&'static str]` fields, `__with_overlay`, `__overlay_row`, overlay-aware `template`, `AppStrings::install_overlays`. Tests at `:300+` show the `static X: Catalog = Catalog::__new(..)` pattern; overlay tests add `static SLOT: OverlaySlot = OverlaySlot::new();` and `.__with_overlay(&SLOT, &[..])`, one slot per test (tests run in parallel).
- New `crates/noctis-i18n/src/overlay.rs` -- `OverlaySlot` (two `std::sync::OnceLock`s: owned entries `Box<[(Locale, u32 key index, Box<str>)]>` sorted; then 32 rows `Box<[Option<Box<[&'static str]>>]>` built from `slot.entries.get()` (a `&'static` borrow, since the slot is a `static`) and the catalog's compiled files; `None` = no overlay in that locale's chain), `OverlayViolation` + `Display` + `Error`, validation helper. `lib.rs:28` re-exports (`pub use overlay::{OverlaySlot, OverlayViolation}`), crate docs list.
- `crates/noctis-rules/src/key.rs:143` `placeholder_names` -- reuse for `found`.
- `crates/noctis-macros/src/i18n/emit.rs:170-300` `expand` -- emit `static SLOT: #i18n::OverlaySlot = #i18n::OverlaySlot::new();`, `const CONTENT: &[&str] = &[<model.content keys in order>];`, `CATALOG = Catalog::__new(..).__with_overlay(&SLOT, CONTENT)`, and in `row(locale)` first `if let Some(row) = CATALOG.__overlay_row(locale).and_then(|r| <&[&str; K]>::try_from(r).ok()) { return row; }`. `model.content` is already sorted by key (`model.rs:87`). Extend the emit tests (`:330+`) to assert the tokens.
- `crates/noctis-macros/src/i18n/model.rs:845-860` -- the placeholder mismatch message shape to mirror (do not share code across crates).
- `crates/noctis-core/src/error.rs:19-50` `Error` (add `LocaleOverlay`), next to `StartError` (`:640+`, stays `Copy`): add `LocaleOverlayError`. `lib.rs:230` re-exports; facade `crates/noctis/src/lib.rs:588-601` root re-export list and the crate-docs error list (`:25`).
- `crates/noctis-core/src/app.rs`:
  - `:128` `App` -- add `strings: Arc<noctis_i18n::AppStrings>` (the same `Arc` also coerced into `AppShared.strings` at `:2431`).
  - `:2399-2431` `build()` -- keep the concrete `Arc`.
  - `:779-806` `run_with` -- call a new crate-private `async fn load_locale_overlays(&self) -> Result<(), LocaleOverlayError>` first inside the startup `select!` arm (before `publish_at_startup`); the arm's error type becomes `Error`.
  - `:640-680` `run_mode_to` -- untouched (no load).
  - `self.ports().locale_source` (`ports.rs:109`) is the port; `self.shared.locales.available()` (`locale.rs:239`, currently `#[cfg(test)]`: drop that cfg; `LocaleSet::contains`) the locale set; `locale.rs:131-160` for iterating `Locale::ALL`.
  - Update `run_until` docs ("Checks, in order …") and the `Errors` section.
- `crates/noctis-core/src/app/tests.rs` -- unit tests of `load_locale_overlays` with a stub `LocaleSource` (call recording, rows 6-8, 23, 27 with `tokio::time::pause` or a pending future + shutdown, 31 via `TestServer` request log) using `Catalog::__new(..).__with_overlay(..)` statics.
- `crates/noctis-testkit/src/app.rs:146-230` -- `TestApp::build` surfaces `TestError::Start(Error::LocaleOverlay(..))` already (the run task's error); doc line only.
- `crates/noctis/tests/i18n_app.rs` -- pattern for the new `i18n_overlays.rs` (`mod strings { noctis::i18n!(dir = ..); }`, `TestApp`, `.locale("tr")`, `reply_text`).
- `.github/workflows-disabled/ci.yml:65` -- add `--test i18n_overlays`.
- `docs/discord-rules.md:78,82,110,122` -- the "runtime-loaded locales" / "`LocaleSource` adapter loaded at startup" parentheticals become "merged locale catalogs; `LocaleSource` overlays never carry reserved keys (Story 11.5)". Keep each tier head word (`Compile-time`, `App-build`) unchanged: `xtask/src/rules_catalog/catalog.rs:338` parses them.
- Skill `examples/project-template/.claude/skills/noctis/`: `references/i18n.md` §8 (`:436-441`), `references/adapters.md:22,40` (LocaleSource row), `references/app-and-sharding.md` (startup order), `SKILL.md:23` API hash. Template `examples/project-template/AGENTS.md` (one overlay line).

## Tasks & Acceptance

**Execution:**
- [ ] `crates/noctis-i18n/src/overlay.rs` (new), `catalog.rs`, `lib.rs` -- `OverlaySlot`, `OverlayViolation`, `__with_overlay`, `__overlay_row`, overlay-aware `Catalog::template`, `AppStrings::install_overlays`, docs; unit tests for Matrix 1-5, 9-22, 24-26, 28-29 at this level, each named after its scenario with a `/// Story 11.5 Matrix N` doc comment, one `static` slot per test -- the validation and install core.
- [ ] `crates/noctis-macros/src/i18n/emit.rs` -- emit the slot, `CONTENT`, `__with_overlay`, the overlay-first `row()`; token tests -- typed accessors see overlays.
- [ ] `crates/noctis-core/src/error.rs`, `lib.rs` -- `Error::LocaleOverlay`, `LocaleOverlayError` (+ `violations()`), re-export `OverlayViolation`; doctests -- the typed error.
- [ ] `crates/noctis-core/src/app.rs` -- concrete `Arc<AppStrings>` on `App`, `load_locale_overlays`, its call in `run_with`, docs -- the startup load.
- [ ] `crates/noctis-core/src/app/tests.rs` -- tests for Matrix 6-8, 23, 27, 30, 31 -- startup behavior.
- [ ] `crates/noctis-ports/src/locale.rs` -- trait docs -- the port contract.
- [ ] `crates/noctis/src/lib.rs` -- root re-exports `LocaleOverlayError`, `OverlayViolation`, crate-docs error list -- facade surface.
- [ ] `crates/noctis/tests/i18n_overlays.rs` (`#![cfg(all(feature = "i18n", feature = "testkit"))]`) + `crates/noctis/tests/fixtures/i18n/overlay/{en-US,tr}.yaml` (first line a `#` comment naming Story 11.5; content as the Matrix's `C`) -- through `TestApp` with `.locales(..)` and `.locale_source(stub)`: Matrix 1 (a command replying `reply_text(keys::GREET_HELLO)` and another replying `Strings::of(ctx.user_locale()).greet.hello()`, both `Selam` for a tr user), 2, 13 and 16 (`TestApp::build` returns `TestError::Start(Error::LocaleOverlay(LocaleOverlayError::Invalid(v)))` with the exact violations), 23. Every installing test uses its own `i18n!` module (own slot) -- the outermost surface for AC 1-2.
- [ ] `.github/workflows-disabled/ci.yml:65` -- add `--test i18n_overlays` -- gate sync.
- [ ] `docs/discord-rules.md` -- the four parentheticals -- docs truth.
- [ ] Skill + template: `i18n.md` §8 rewritten (startup load per available locale, content keys only and why, the four checks with example messages, process-wide semantics and the chain, failure = `Error::LocaleOverlay`, a `LocaleSource` example impl returning `Vec<(String, String)>` from a map, "no hot reload; restart to pick up edits"); `adapters.md` LocaleSource row; `app-and-sharding.md` startup order; the skill names `LocaleOverlayError` (both variants, `violations()`), `OverlayViolation` (every variant) and `Error::LocaleOverlay` so `skill-check` finds them; template `AGENTS.md` line; then `cargo run --release -p xtask -- api-snapshot --bless` and set `SKILL.md`'s API hash -- AC 3.
- [ ] `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- append any implementation decision as a `(decision)` line.

**Acceptance Criteria:**
- Given an App with `.locales(catalog())` and a `LocaleSource` returning valid content-key overlays, when it starts (`TestApp::build`), then handlers' `ctx.reply_text(key)`, `Text::Key`s and `Strings::of(locale)` accessors render the overlay values in that locale (and through the sibling/default chain), while Publish JSON is unchanged.
- Given a `LocaleSource` returning any `cmd.*`, `cmdword.*`, `ctxcmd.*` or `noctis.*` key, when the App starts, then startup fails with `Error::LocaleOverlay(LocaleOverlayError::Invalid(..))` containing `OverlayViolation::ReservedKey` naming locale and key, before any Discord request, and nothing is installed.
- Given an overlay value whose placeholder set differs from the compiled set, when it is loaded, then startup fails with `OverlayViolation::Placeholders` naming the key, the locale, the expected and the found placeholders.
- Given the finished story, when the gates run, then the Usage Skill documents overlays, the API snapshot matches, the skill's API hash equals it, and `skill-check` and `rules-catalog` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

These decisions are logged in the architecture memlog.

- **Typed accessors see overlays (process-wide slot).** `Strings::of` is the main content API (skill §3); overlays that skip it would fail the story's "ship text without a redeploy". `Strings::of` has no App, so the overlay lives in a per-`i18n!` `static` slot. A `OnceLock` inside a `static` yields `&'static` borrows, so the rows borrow the slot's own strings with no leak. Set-once per process: a second App with equal overlays is fine, a different one is `AlreadyInstalled` (loud, never silent last-wins). Tests must give each installing case its own catalog.
- **Startup, not `build()`.** `build()` is sync and the port is async; loading in `run_with` before the startup Publish keeps a bad overlay from reaching Discord and surfaces through `TestApp::build`. CLI Publish modes skip it: only reserved keys reach Publish, and those are never overlaid.
- **Fail fast.** A load error or any violation fails startup with a typed error. Overlays are fixes the operator chose to ship; running silently with stale text would hide the failure. Degrading gracefully would be a later, opt-in story.
- **Reserved set includes `ctxcmd.*` and `noctis.*`.** `ctxcmd.*` is published (skill §8 already says so). `noctis.*` overrides are framework notices checked against `FRAMEWORK_KEYS`; keeping them compile-time-only keeps one rule (`content keys only`) and the slot keyed by content rows.
- **Which locales.** Only locales with a file somewhere: resolution (11.3) never picks a locale outside that set, so an overlay for it could never show. Adding a new language still needs a (possibly empty) locale file, which is documented.
- **Set comparison** mirrors `i18n!`'s translation check, so an overlay is valid exactly when the same value in a non-default file would compile.

## Verification

**Commands** (run in `/home/claude/lanes/b/noctis-rs`; this is Epic 11's last story, so `cargo hack` runs):
- `cargo fmt --all` -- expected: no diff.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean.
- `cargo test --release --workspace --all-features` -- expected: all pass, incl. `i18n_overlays` and `noctis-i18n` unit tests.
- `cargo test --release -p noctis --features i18n,testkit --test i18n_app --test i18n_modules --test i18n_locales --test i18n_commands --test i18n_overlays` -- expected: pass.
- `cargo test --release -p noctis --features i18n --test ui` and `cargo test --release -p noctis --test ui` -- expected: pass (no `.stderr` drift from the emit change).
- `cargo hack test --release --workspace --each-feature` -- expected: pass.
- `cargo run --release -p xtask -- deps-check`, `-- tw-grammar --check`, `-- rules-catalog`, `-- skill-check` -- expected: ok.
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: match (install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if missing).
- `cargo deny check` -- expected: ok.
- `cargo +1.91 check --release --workspace` -- expected: ok.
- `test ! -d target/debug` -- expected: no debug profile.
