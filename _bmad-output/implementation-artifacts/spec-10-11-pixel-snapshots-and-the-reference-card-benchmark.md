---
title: 'Story 10.11: Pixel snapshots and the reference card benchmark'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '2b1cfce38cf2733733716233258cec3178d9b79f'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-10-10-render-cache-and-single-flight-deduplication.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Story 14.2 must generate and commit examples/project-template/tests/snapshots/profile_card.png once the template crate builds.
    evidence: |-
      Story 10.11 reconciles examples/project-template/tests/templates.rs to the real assert_render_snapshot! API but commits no snapshot (the template does not build until 14.2). Under CI=true a missing snapshot fails with "missing snapshot (CI)", so the template's test fails as soon as it joins the gate unless 14.2 runs it locally first and commits the PNG.
    location: >-
      examples/project-template/tests/templates.rs:27
    severity: low
---

<intent-contract>

## Intent

**Problem:** FR-51's last consequence ("Templates can be pixel-snapshot-tested with a tolerance") is missing: the Usage Skill and the template's `tests/templates.rs` show `assert_render_snapshot!` as planned, `TestApp` cannot reach its App's Renderer, and nothing measures NFR-3's reference card or guards renderer upgrades (NFR-8, AD-28). `main.yml` has only stub jobs, and no `budgets.toml` exists.

**Approach:**
- `noctis-testkit` gains `TestApp::renderer()`, a `Tolerance` type and an `assert_render_snapshot!` macro. The macro renders a Template through the App's `Renderer` port as a still PNG, decodes it with `png` 0.18 and compares it with a stored PNG.
- `noctis-image` gets a reference card (a copy of the template's profile card) and a custom-harness bench. The bench reports p50/p99 against a new `budgets.toml`.
- The facade gets the card's pixel snapshot and an FR-49 Media Gallery test.
- CI files gain the snapshot gate, the bench job and the latest-takumi job.

## Boundaries & Constraints

**Always:**
- **Layering (AD-24, spine graph).**
  - `noctis-testkit` gains normal dependencies `noctis-ports.workspace = true` (an allowed edge; move it out of `[dev-dependencies]`) and `png.workspace = true` (0.18.1, already in `Cargo.lock`).
  - It never depends on `noctis-image`, and it renders only through `Arc<dyn Renderer>`.
  - `noctis-image` gains no `noctis`, `noctis-macros` or `noctis-testkit` dependency of any kind. Its new dev-dependency is `toml.workspace = true` (1.1.8 is already locked). `xtask deps-check` passes.
- **`TestApp::renderer(&self) -> Option<&Arc<dyn Renderer>>`.** Capture `app.renderer().cloned()` right after `builder.build()` (`app.rs:181`), before `run_until` moves the App, and store it in a new `renderer` field. It mirrors `App::renderer()`: `Some` when a Renderer was installed (`ImagePlugin`, or `builder.renderer(..)`).
- **`Tolerance`** (`noctis-testkit/src/snapshot.rs`, `#[derive(Debug, Clone, Copy, PartialEq)]`):
  - Fields: `max_ratio: f64` and `channel: u8`.
  - `pub const EXACT` (0.0, 0) is also `Default`.
  - `#[track_caller] pub fn ratio(max_ratio: f64) -> Self` panics with `tolerance must be a finite ratio in 0.0..=1.0, got {x}` unless `x.is_finite() && (0.0..=1.0).contains(&x)`. `-0.0` is accepted.
  - `#[must_use] pub const fn channel(self, delta: u8) -> Self`, plus the getters `max_ratio()` and `channel_delta()`.
  - `impl From<f64> for Tolerance` is `ratio(x)`.
  - A pixel *differs* when any of its R, G, B or A values differ by more than `channel`. A snapshot *matches* when the sizes are equal and `differing as f64 <= max_ratio * total as f64`.
- **Macro.** `#[macro_export] macro_rules! assert_render_snapshot` has two arms:
  - `($renderer:expr, $name:expr, $template:expr $(,)?)`, which uses `Tolerance::EXACT`;
  - `(…, tolerance = $t:expr $(,)?)`, which converts with `::core::convert::Into::<$crate::Tolerance>::into($t)`.
  - Both expand to `$crate::__private::render_snapshot($renderer, $name, $template, tol, ::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/tests/snapshots")).await`, so the macro is only valid inside an async fn.
  - `__private` is `#[doc(hidden)] pub mod`. The function panics on every failure, like `assert_eq!`.
  - The facade re-exports `assert_render_snapshot` and `Tolerance` from `noctis::testkit`.
- **Rendering for a snapshot.**
  - `RenderJob::of(&template)`, then set `format = Format::Png`, `animation = None` and `upload_budget = u64::MAX`.
  - The view is therefore called once with `Frame::STILL`, and a JPEG/WebP or animated Template snapshots its still PNG pixels.
  - The Template must be registered on the App (`Module::template::<T>()`). Otherwise the Renderer's own `UnknownTemplate` error is the panic message.
- **Comparison.**
  - Decode both PNGs with `png` 0.18 (`Transformations::EXPAND | STRIP_16`) and normalize Gray, GrayAlpha, Rgb and Rgba 8-bit to RGBA8. Missing alpha becomes 255.
  - Store the renderer's bytes verbatim; never re-encode them.
  - On a match, delete a stale `<name>.new.png`.
- **Files and environment.**
  - The path is `<dir>/<name>.png`. Create `dir` when it is missing.
  - A name must be 1..=64 bytes of `[A-Za-z0-9_-]`, checked before rendering.
  - On a mismatch or a decode failure, write the actual bytes to `<name>.new.png`.
  - The env flags are `NOCTIS_UPDATE_SNAPSHOTS` and `CI`. Each is "on" when it is set and not `""`, `"0"` or `"false"`. The macro wrapper reads them into a `SnapshotEnv { update, ci }` struct, which the testable inner fn takes as a parameter.
  - Update mode always writes the snapshot and passes, and it wins over CI.
- **Reference card.**
  - It lives in `crates/noctis-image/benches/reference_card/card.rs`, a hand-written Template that mirrors exactly what `#[derive(Template)]` and `view!` emit (see Design Notes): `ReferenceCard`, id `"reference.card"`, version 1, `Size::new(934, 282)`, `Format::Png`, no animation.
  - Its fields are those of `ProfileCard`: `name`, `subtitle`, `level_label`, `xp_label`, `progress: f32` and an `#[asset]`-equivalent `avatar: RemoteImage` (`TemplateAssets` with `DECLARES = true`, which declares `"avatar"`, not required).
  - Its view is the template's view (`examples/project-template/src/modules/profile/view.rs`), node for node.
  - Each class string is a `pub const`, and all of them are listed in `pub const CLASS_STRINGS: [&str; 9]`.
  - `pub fn theme() -> Result<Theme, ThemeError>` parses `include_str!("noctis-theme.css")`, a verbatim copy of the template's theme.
  - `pub fn configure_with(builder: &mut AppBuilder, config: ImageConfig) -> Result<&mut AppBuilder, ThemeError>` adds `.theme(theme()?)`, `.image(config.font(Font::from_static(INTER)))` (`INTER` = `include_bytes!("../../fonts/InterVariable.woff2")`) and `.module(Module::new("reference").template::<ReferenceCard>())`. `pub fn configure(builder)` is `configure_with(builder, ImageConfig::new())`; the bench calls `configure_with` with its threads and queue capacity.
  - `pub fn sample(xp: u32) -> ReferenceCard` returns name `"Deniz"`, subtitle `"they/them"`, `"Level 7"`, `format!("{xp} / 100 XP")`, progress 0.42 and `RemoteImage::placeholder()`.
  - The file uses only `noctis_core`, `noctis_image`, `noctis_ports` and `serde` paths, so the bench and the facade test can both compile it.
- **Bench** (`crates/noctis-image/benches/reference_card.rs`).
  - Manifest: `[[bench]] name = "reference_card"`, `harness = false`, `test = true`, `required-features = ["render"]`.
  - It runs under `#[tokio::main]` (multi-thread).
  - `T = available_parallelism / 2` clamped to 1..=256. Use `ImageConfig::threads(T)` and `queue_capacity(max(64, 2*T).min(4096))`.
  - Full mode (args contain `--bench`):
    1. Warm up with 100 untimed renders, using xp `1_000..1_100`.
    2. Run `T` closed-loop client tasks that share an atomic counter until 1000 timed samples are taken. Each sample has a distinct xp `≥ 2_000`, so there is never a cache hit or a single-flight merge.
    3. Each sample is the wall time of `renderer.render(job).await` alone (postcard encoding excluded). Any error panics.
    4. Print threads, clients, samples, min, mean, p50, p99 and max in ms, then each budget line.
  - Percentiles are nearest-rank on the sorted samples: `sorted[ceil(p·n) − 1]`.
  - Budgets come from `concat!(env!("CARGO_MANIFEST_DIR"), "/../noctis-core/benches/budgets.toml")`, table `[reference_card]`:
    - `p50_ms` and `p99_ms` are required, finite and > 0, with p50 ≤ p99; a TOML integer or float is accepted;
    - `gate` is an optional bool and defaults to false;
    - an unknown key is an error; other tables are ignored.
  - Over budget: print `reference_card: <stat> <x> ms is over its <b> ms budget (report only)` and exit 0. With `gate = true`, exit 1.
  - Smoke mode (no `--bench`, which is what `cargo test` runs):
    - run the self-checks of the percentile and budget parsers on the matrix rows;
    - parse the real `budgets.toml`;
    - render 3 distinct samples and assert a 934×282 PNG (decoded with `png`), `OutputFormat::Png` and pairwise-distinct digests.
  - `#![allow(clippy::print_stdout, reason = "a bench reports on stdout")]`, plus `expect_used`/`unwrap_used` allows with reasons.
- **Budgets file.** `crates/noctis-core/benches/budgets.toml` has a header comment (AD-25; report-only until Story 14.3 / PRD Q3) and:

  ```toml
  [reference_card]
  p50_ms = 10.0
  p99_ms = 50.0
  gate = false
  ```
- **Same commit (AGENTS.md).** Update:
  - the Usage Skill;
  - the template's `tests/templates.rs` and `AGENTS.md`;
  - the facade docs;
  - `xtask/public-api.txt` and the `SKILL.md` `API hash`.

**Never:**
- No criterion, no new crate version, no `cargo update`. `Cargo.lock` changes only by the new edges (testkit→png and noctis-ports, noctis-image→toml).
- No edit to the takumi pins, no `docs/discord-rules.md` change and no new rule id.
- `budgets.toml` never gates in this story (`gate = false`). The latest-takumi job never fails the workflow.
- No snapshot for the Project Template's own card. Its crate does not build until Story 14.2, so only reconcile its test source.
- Never run `.github/scripts/latest-takumi.sh` locally: it edits `Cargo.toml`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Identical | stored == rendered (10×10) | pass; stale `.new.png` removed | none |
| Ratio boundary | 1 of 100 pixels differs, `tolerance = 0.01` | pass (1 ≤ 1.0) | none |
| Ratio max+1 | 2 of 100 differ, 0.01 | fail; message has 2/100, 2.00 %, the tolerance and the `.new.png` path; `.new.png` == rendered bytes | panic |
| Exact default | 1 pixel differs, no tolerance arm | fail | panic |
| Ratio 1.0 | every pixel differs, same size | pass | none |
| Channel delta | R differs by 3 with `Tolerance::ratio(0.0).channel(3)`; by 4 | pass; fail | panic |
| Alpha only | only A differs by 1, EXACT | fail | panic |
| Size mismatch | stored 10×10, rendered 10×11, ratio 1.0 | fail naming both sizes | panic |
| Downscaled | `settings_used.size` ≠ `T::SIZE` | fail "the size budget downscaled the render" | panic |
| Color types | stored Gray8 / Rgb8 / 16-bit RGBA of the same pixels as an RGBA8 render | pass (normalized) | none |
| Corrupt stored | `b"junk"` at `<name>.png` | fail "cannot decode stored snapshot"; `.new.png` written. In update mode: overwritten, pass | panic |
| Missing, local | no file, `ci = false` | file written (dir created), bytes == renderer bytes, pass | none |
| Missing, CI | no file, `ci = true` | fail "missing snapshot (CI)"; `.new.png` written; no `<name>.png` | panic |
| Update | differing stored, `update = true` (also with `ci = true`) | overwritten, pass | none |
| Env parsing | unset, `""`, `"0"`, `"false"` → off; `"1"`, `"true"`, `"yes"` → on | as stated | none |
| Names | `"a"`, 64 × `a`, `"profile_card-2"` | accepted | none |
| Bad names | `""`, 65 bytes, `"a/b"`, `".."`, `"a b"`, `"x.new"`, `"kart.ş"` | panic before rendering, naming the rule | panic |
| Bad tolerance | `-0.1`, `1.0000001`, `NaN`, `inf` | panic from `Tolerance::ratio`; `-0.0` and `0.0` accepted | panic |
| No renderer | `None` | panic "no Renderer: build the App with the `image` feature and `.image(..)`/`.theme(..)`" | panic |
| Render error | the Renderer returns `UnknownTemplate` | panic with the error's `Display` | panic |
| Non-PNG output | a fake returns `OutputFormat::Webp` / `Apng` | fail "a pixel snapshot needs a still PNG" | panic |
| Forced job | Template with `Format::Jpeg{80}` + `ANIMATION = Some(..)` | the fake records `format == Png`, `animation == None`, `upload_budget == u64::MAX` | none |
| TestApp renderer | builder with `.renderer(fake)` / plain builder | `Some` (same `Arc`, `ptr_eq`) / `None` | none |
| Percentile | n=1 [5] → p50=p99=5; n=100 values 1..=100 shuffled → p50=50, p99=99; n=1000 → p99 = 990th smallest; n=0 | as stated; n=0 is `Err` | bench error |
| Budgets valid | the table above; `p50_ms = 10` integer; `gate` missing | parsed; gate false | none |
| Budgets invalid | file missing; no `[reference_card]`; `p50_ms` missing; 0; −1; `nan`; `inf`; p50 = 60 > p99 = 50; `gate = "no"`; key `p95_ms` | `Err` naming the file and key; the bench exits non-zero | bench error |
| Over budget | p99 above budget, `gate=false` / `gate=true` | report-only line, exit 0 / exit 1 | none / exit 1 |
| Card classes | every `CLASS_STRINGS` entry with the card theme | `tw::validate` Ok | test failure |
| Card distinct | `sample(1)` vs `sample(2)` rendered | different digests (no cache hit) | none |

</intent-contract>

## Code Map

- `crates/noctis-testkit/Cargo.toml`: add `noctis-ports` and `png` to `[dependencies]` and remove the `noctis-ports` dev-dependency line.
- `crates/noctis-testkit/src/app.rs` `:79-94` (`TestApp` fields), `:149-232` (`build`; `builder.build()` at `:181`), `:340-350` (getters `http_fake`/`gateway_fake`: add `renderer` beside them).
- `crates/noctis-testkit/src/lib.rs` `:1-50`: crate docs bullet, `mod snapshot;`, `pub use snapshot::Tolerance;`, `#[doc(hidden)] pub mod __private`.
- `crates/noctis-testkit/src/render.rs`: the existing JSON snapshots. Keep it as is; the new code goes in `snapshot.rs` (+ `snapshot/tests.rs`).
- `crates/noctis-testkit/tests/`: add `render_snapshot.rs`. It is an e2e test of the macro with a fake `Renderer` on a `TestApp`, which writes and commits `tests/snapshots/fake_card.png` (a tiny PNG).
- `crates/noctis-ports/src/render.rs`:
  - `RenderJob` `:88-128` (`of`, pub fields);
  - `Renderer` `:578-594`;
  - `RenderOutput` `:347`, `EncodeSettings` `:334`, `Format` `:182`.
- `crates/noctis-core/src/app.rs` `:677` (`App::renderer`), `module.rs:399` (`Module::template`).
- `crates/noctis-image/src/renderer/tests.rs` `:20-48` (the hand-written Template pattern), `:279-295` (`decode_png`).
- `crates/noctis-macros/src/template.rs` `:200-300` (what the derive emits: `Template` + `TemplateAssets`).
- `crates/noctis-macros/src/view.rs` `:410-500` (how `view!` lowers tags, `tw`, `style`, `<img>` and text children).
- `crates/noctis-image/src/lib.rs` `:100-120` (the public node API: `div`, `span`, `img`, `Classes`, `Style`, `erased_view`, `TemplateAssets`, `AssetField`, `DeclaredImages`), `node/classes.rs:21` (`Classes::__literal`), `tw/mod.rs:270` (`validate`), `plugin.rs:110-210,400-450` (`ImageConfig`, `AppBuilderImageExt`), `remote.rs:235` (`RemoteImage::placeholder`).
- `crates/noctis-image/Cargo.toml`: `[[bench]]`, and `toml` in `[dev-dependencies]`.
- New files:
  - `crates/noctis-image/benches/reference_card.rs`;
  - `crates/noctis-image/benches/reference_card/card.rs`;
  - `crates/noctis-image/benches/reference_card/noctis-theme.css`;
  - `crates/noctis-core/benches/budgets.toml`;
  - `crates/noctis/tests/reference_card.rs` (`#![cfg(all(feature = "image", feature = "testkit"))]`, `#[path = "../../noctis-image/benches/reference_card/card.rs"] mod card;`);
  - `crates/noctis/tests/snapshots/reference_card.png` (produced by the first run, then committed).
- `crates/noctis/tests/render_e2e.rs` `:60-120`: the pattern for a command that renders into a `MediaGallery` and reads the recorded upload.
- `crates/noctis/src/lib.rs`:
  - `:3384-3432` (testkit module docs: add a "Pixel snapshots" bullet);
  - `:3464-3469` (re-export `Tolerance`, `assert_render_snapshot`);
  - `:602` (the feature list line, if it lists contents).
- Skill (`examples/project-template/.claude/skills/noctis/`):
  - `references/testing.md` `:1-7` (drop "planned" for pixel snapshots), `:248-261` (§6, rewrite);
  - `references/images.md` `:813-817` (§11);
  - `SKILL.md` `:23` (API hash), `:498`.
- `examples/project-template/tests/templates.rs` (reconcile), `examples/project-template/AGENTS.md` `:26`.
- `.github/workflows-disabled/ci.yml` `:55-69` (Test job), `:143-151` (`tw-crosscheck`).
- `.github/workflows-disabled/main.yml` (replace the stubs), plus the new `.github/scripts/latest-takumi.sh`.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-testkit/Cargo.toml`, `src/snapshot.rs` (+ `src/snapshot/tests.rs`), `src/lib.rs`, `src/app.rs`:
   - `Tolerance`, the macro, `__private::render_snapshot` and the testable inner `check` (taking the renderer, name, job, declared size, tolerance, dir `&Path` and `SnapshotEnv`, returning `Result<Outcome, SnapshotError>`);
   - name validation, PNG normalization, the env parsing and `TestApp::renderer`.
   - Unit tests cover every testkit matrix row (fake `Renderer` with canned PNGs built by `png::Encoder`, a per-test unique dir under `std::env::temp_dir()`).
2. `crates/noctis-testkit/tests/render_snapshot.rs`: a TestApp with `.renderer(fake)` and a registered hand-written Template. Assert `app.renderer()` is `Some`/`ptr_eq`, and on a plain builder `None`. Call `assert_render_snapshot!` (first run writes `tests/snapshots/fake_card.png`; commit it).
3. `crates/noctis-core/benches/budgets.toml`: as specified.
4. `crates/noctis-image/benches/reference_card/{card.rs,noctis-theme.css}` and `benches/reference_card.rs`, plus `Cargo.toml` `[[bench]]` and the `toml` dev-dependency: the card, the bench, the smoke self-checks for the Percentile / Budgets / Over budget / Card distinct rows.
5. `crates/noctis/tests/reference_card.rs`:
   - `reference_card_classes_are_valid` (Card classes row);
   - `reference_card_matches_snapshot` (`TestApp::build(card::configure(&mut App::builder())?)`, `assert_render_snapshot!(app.renderer(), "reference_card", card::sample(42), tolerance = 0.01)`, then `app.stop()`);
   - `reference_card_in_a_media_gallery` (FR-49): a `/card` command `ctx.render`s `sample(42)` into a `MediaGallery` with alt text. Assert one upload named `<16 hex>.png`, which decodes to 934×282, and that the gallery item's URL is `attachment://<that name>`.
   - Run it once to create `tests/snapshots/reference_card.png`, then commit it.
6. `crates/noctis/src/lib.rs`: the docs and re-exports.
7. Skill:
   - `testing.md` §6 documents: both macro forms; `Tolerance` (ratio, channel, EXACT, `f64`); the forced still PNG; registration; the file path, `.new.png`, `NOCTIS_UPDATE_SNAPSHOTS`, the `CI` behaviour; the name rule; unique names per crate; committing snapshots; that fonts should be registered explicitly for determinism; `RemoteImage::placeholder()` for no network; that the test must be `#[tokio::test(flavor = "multi_thread")]`; and `TestApp::renderer()`.
   - Update `images.md` §11 and the header line.
   - Re-bless with `cargo run --release -p xtask -- api-snapshot --bless` and copy the hash into `SKILL.md`.
8. `examples/project-template/tests/templates.rs`: `#[tokio::test(flavor = "multi_thread")]`, `TestApp::build(configure(&mut App::builder())).await?`, the macro, then `app.stop().await?`. Keep the NOTE header (the template builds in Story 14.2). In `AGENTS.md` `:26`, name `assert_render_snapshot!` and `tests/snapshots/`.
9. `.github/workflows-disabled/ci.yml`:
   - rename `tw-crosscheck` to "Takumi cross-check and pixel snapshots" and add `cargo test --release -p noctis --features image,testkit --test reference_card` (required on every push, takumi bumps included);
   - add `cargo test --release -p noctis-image --features render --bench reference_card` (smoke) to the Test job.
10. `.github/workflows-disabled/main.yml`:
    - triggers: `push: branches: [main]` + `workflow_dispatch`;
    - `benches` job: checkout, toolchain, rust-cache, `cargo bench -p noctis-image --features render --bench reference_card` (cargo bench has no `--release`; the bench profile inherits release), a comment that Stories 12.6 and 14.3 add their benches;
    - `latest-takumi` job: `continue-on-error: true`, runs `bash .github/scripts/latest-takumi.sh`.
11. `.github/scripts/latest-takumi.sh` (`set -euo pipefail`):
    1. Read the newest `takumi` and `takumi-core` versions from `cargo search <name> --limit 1`.
    2. Rewrite the two `=x.y.z` pins in the root `Cargo.toml` (`:57-58`), then `cargo update -p takumi -p takumi-core`.
    3. Run, each recorded pass/fail into `$GITHUB_STEP_SUMMARY` without stopping: `cargo build --release -p noctis-image --features render`, `cargo run --release -p xtask -- tw-grammar --check`, the `tw_crosscheck` test and the `reference_card` snapshot test.
    4. Exit 0.
12. `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: the planner already logged the decisions.

**Acceptance Criteria:**
- Given a `TestApp` built from a builder whose `ImagePlugin` installed a Renderer, when a test calls `assert_render_snapshot!(app.renderer(), "name", template, tolerance = ..)`, then the PNG is rendered through `Renderer::render` (testkit has no `noctis-image` dependency: `deps-check` passes and `cargo tree -p noctis-testkit -e normal` shows none), decoded with `png` 0.18 and compared within the tolerance. The test panics beyond it, and the first run writes `tests/snapshots/name.png`.
- Given the reference card, when `cargo bench -p noctis-image --features render --bench reference_card` runs, then it prints p50 and p99 with the pool saturated and each against `budgets.toml`. Over budget it only reports (exit 0, `gate = false`).
- Given `cargo test --release --workspace --all-features`, when it runs, then the bench's smoke mode, the testkit snapshot tests and `crates/noctis/tests/reference_card.rs` pass against the committed `reference_card.png`.
- Given a takumi bump commit, when CI runs, then `ci.yml`'s cross-check job requires `tw_crosscheck` and the reference card pixel snapshot, and `main.yml`'s report-only `latest-takumi` job builds and tests against the newest takumi.
- Given the new public API, when the story is done, then the Usage Skill documents pixel snapshots with no "planned" marker for them, `tests/templates.rs` uses the real API, and `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 34 findings — high 0, medium 4, low 21, false 9, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) `latest-takumi.sh` never checks that the `sed` pin rewrite matched, so a reformatted pin line yields a false green against the old takumi — verified at `latest-takumi.sh:32-35` (no check after sed); patch: fail loudly in the summary when the new pins are absent.
  - `[low]` `[reject]` (blind) `cargo search --limit 1` may rank another crate first or report a pre-release — `cargo search takumi`/`takumi-core` rank the exact name first today (checked); the pre-release case is hypothetical and the job is report-only, and the fix (crates.io API) adds machinery.
  - `[low]` `[reject]` (blind) latest-takumi runs only build, grammar, tw_crosscheck and the snapshot, not the full noctis-image suite — the spec's step 11 and AD-28 name exactly these checks; any real bump commit runs the full `ci.yml` Test job anyway.
  - `[low]` `[reject]` (blind) bench results go only to the job log, not `$GITHUB_STEP_SUMMARY` — report-only until Story 14.3 and visible in the log; adding summary plumbing is an enhancement, not a defect.
  - `[low]` `[reject]` (blind) a failing `.new.png` write replaces the real diagnosis with `Io` — needs a read-only or full checkout, rare and still loud; the fix adds error-combining branches.
  - `[low]` `[reject]` (blind) float tolerance boundary (`0.29*100 = 28.999…`) rejects exactly-at-ratio counts — the code implements the spec's formula verbatim; it bites only when `ratio*total` is an exact integer, unlikely in everyday use.
  - `[low]` `[reject]` (blind) `impl From<f64> for Tolerance` can panic, against C-CONV — mandated by the spec; out-of-range is a programmer error reported with `#[track_caller]`, and changing it reshapes public API.
  - `[low]` `[reject]` (blind) `Outcome` is discarded, so Written/Updated pass silently; `check_name` runs twice — silent update is the documented update-mode behaviour; the double check is harmless; the fix adds output nobody asked for.
  - `[low]` `[reject]` (blind) fully transparent pixels with different RGB count as differing — the spec defines differing on raw RGBA; a spurious failure would be loud and fixed with update mode.
  - `[low]` `[patch]` (blind) test gaps: no test reaches `DecodeRendered`, palette/low-bit expansion, `Io` paths or the `render_snapshot` panics — grouped with the vgap DecodeRendered and panic-path gaps; patch: add the undecodable-render test (palette/Io parts not patched).
  - `[low]` `[patch]` (blind) the reference card is tied to the template's view and theme only by a comment — verified the theme files are identical and the template's `view.rs` contains every class literal today, but nothing enforces it; patch: a drift test in `crates/noctis/tests/reference_card.rs`.
  - `[low]` `[patch]` (blind) the skill says "register fonts explicitly" while the template's `configure` relies on the bundled `default-font` — verified at `testing.md:259,307` and `images.md:822` vs `templates.rs:17`; patch: reword the skill so the bundled default font counts as deterministic.
  - `[low]` `[defer]` (blind) no `profile_card.png` is committed for the template, so its test will fail "missing snapshot (CI)" once Story 14.2 builds it — intent and spec exclude the template's own snapshot until 14.2; deferred so 14.2 generates and commits it.
  - `[low]` `[reject]` (blind) T clients on T threads understate saturated latency; large xp labels widen text — the spec's Design Notes choose clients = threads deliberately (no queue wait); the label width difference is negligible.
  - `[low]` `[patch]` (edge) with `NOCTIS_UPDATE_SNAPSHOTS=1`, the second `fake_card` call in `tests/render_snapshot.rs` stores shade 122 and the next normal EXACT shade-120 run fails — verified at `render_snapshot.rs:89-96`; patch: run the shade-122 call first so update mode leaves shade 120 stored.
  - `[medium]` `[patch]` (edge) sed pin rewrite unchecked in `latest-takumi.sh` — same root cause as the blind sed finding; same patch.
  - `[false]` `[reject]` (edge) `cargo search` ranks another crate first for `takumi` — `cargo search takumi --limit 3` returns `takumi` first (checked 2026-10-24), and the sed filter keys on the exact name.
  - `[false]` `[reject]` (edge) a failed `cargo update` leaves later steps passing misleadingly — the later `cargo build`/`test` run without `--locked` and re-resolve against the rewritten `=` pins, so they fail too rather than pass.
  - `[false]` `[reject]` (edge) `percentile` with pct > 100 indexes out of bounds — the private fn is called only with the constants 50 and 99.
  - `[low]` `[reject]` (edge) a non-NotFound read error of the stored file writes no `.new.png` — the spec writes `.new.png` only on a mismatch or decode failure; the `Io` error is loud and rare.
  - `[low]` `[reject]` (edge) names differing only by case collide on case-insensitive filesystems — unlikely in practice, and a new name rule adds surface the spec did not ask for.
  - `[low]` `[reject]` (edge) env values `no`/`off`/`FALSE` count as on — the spec's matrix fixes the off set (`""`, `"0"`, `"false"`); setting the update flag to "off" is unusual.
  - `[low]` `[reject]` (edge) T clients for T pool threads is not a saturated pool — grouped with the blind saturation finding; deliberate spec design.
  - `[medium]` `[patch]` (vgap) the `render_snapshot` panic path is never exercised, so a regression that swallows errors would pass every test — pre-verified; patch: `#[should_panic]` tests calling `render_snapshot` with a differing render and with no Renderer.
  - `[low]` `[patch]` (vgap) `SnapshotEnv::from_env`'s variable names are untested — pre-verified; patch: have `from_env` delegate to a `pub(crate)` lookup-taking helper and unit-test that it reads `NOCTIS_UPDATE_SNAPSHOTS` and `CI` (no env mutation, `unsafe_code` is denied).
  - `[low]` `[patch]` (vgap) the undecodable-render (`DecodeRendered`) branch has no test — pre-verified; patch: `an_undecodable_render_fails_and_writes_new_png` under `LOCAL` and update mode.
  - `[false]` `[reject]` (intent) on CI the first run does not write the snapshot — deliberately and correctly settled by the spec's CI guard (Design Notes, matrix "Missing, CI").
  - `[false]` `[reject]` (intent) the reference card is a hand-lowered copy, not macro output — the spec's Design Notes settle this deliberately (noctis-image cannot depend on the facade); drift risk is handled by the drift-test patch.
  - `[false]` `[reject]` (intent) "pool saturated" measured with clients = threads — deliberately settled in the spec's Design Notes.
  - `[false]` `[reject]` (intent) `tolerance = 0.01` lets a takumi bump change up to 1 % of pixels unnoticed — FR-51 requires a tolerance and the spec fixes 0.01 for this test deliberately.
  - `[false]` `[reject]` (intent) `examples/` Template snapshots are not exercised by the gate — the only `examples/` Template is the Project Template's, which the intent defers to Story 14.2.
  - `[low]` `[patch]` (intent) `templates.rs` and the skill example are not compile-checked, and `templates.rs` says "default font" while the skill says "register fonts explicitly" — the compile-check part is settled by the spec's Never rule (template builds in 14.2); the font conflict is real and grouped with the blind font finding; same patch.
  - `[medium]` `[patch]` (intent) latest-takumi can report pass without testing the newest takumi — same root cause as the blind sed finding; same patch.
  - `[false]` `[reject]` (intent) `$crate::__private` in `assert_render_snapshot!` vs AD-2's `::noctis::__private` rule — AD-2 targets proc-macro output; `$crate` resolves through the facade's re-export for bot authors who depend only on `noctis`.

## Design Notes

- **Why a hand-written card.** `#[derive(Template)]` and `view!` expand to `::noctis::__private::…` (`template.rs:206`, `view.rs:417`). A `noctis` dev-dependency of `noctis-image` would unify `render` into every `noctis-image` test build, and it would compile the crate twice. The card therefore mirrors the expansion by hand:
  - `impl Template` with `fn view() -> ErasedView { erased_view::<Self>() }`;
  - `impl TemplateAssets { const DECLARES: bool = true; fn declare(&self, out) { <RemoteImage as AssetField>::declare(&self.avatar, "avatar", false, out) } }`;
  - nodes built with `div`/`span`/`img` and `Classes::__literal(CONST)`.

  `__literal` skips the runtime check exactly as macro literals do, and the classes test replaces the compile-time TW-01 check. The card's header comment says that it copies `examples/project-template/src/modules/profile/view.rs` and must be kept in step.
  Sketch:
  ```rust
  pub const ROOT: &str = "flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl";
  // AVATAR, COLUMN, NAME, SUBTITLE, LEVEL_ROW, XP, TRACK, FILL likewise (9 strings)
  div(Classes::__literal(ROOT)).children([
      img(&self.avatar, Classes::__literal(AVATAR)),
      div(Classes::__literal(COLUMN)).children([/* … */]),
  ])
  ```
- **Why a custom harness.** NFR-3 asks for p50/p99 under a saturated pool. Criterion reports per-iteration statistics of a single caller, and it is not in `Cargo.lock`. Closed-loop clients = pool threads keep every thread busy without measuring queue wait. `test = true` lets `cargo test` run the smoke mode, so the matrix rows are covered by the gates.
- **Why a forced still PNG.** PNG is lossless, so a diff means a pixel change rather than encoder noise, and one comparison path covers every Template. Animated output is not compared frame by frame (documented).
- **CI guard.** On GitHub (`CI=true`) a forgotten snapshot would otherwise be written and passed, which would make the NFR-8 gate vacuous.

## Verification

**Commands** (in `/home/claude/lanes/a/noctis-rs`):
- `cargo fmt --all --check`: clean.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: clean (includes the bench).
- `cargo test --release -p noctis-testkit`: pass.
- `cargo test --release -p noctis-image --features render --bench reference_card`: smoke passes.
- `cargo test --release -p noctis --features image,testkit --test reference_card`: passes. The second run passes against the committed PNG.
- `cargo bench -p noctis-image --features render --bench reference_card`: prints the report. Record p50/p99 in the Auto Run Result.
- `tools/story-loop/bg.sh cargo test --release --workspace --all-features`: pass.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog`, `-- skill-check` and `-- tw-grammar --check`: pass.
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`, `cargo deny check` and `cargo +1.91 check --release --workspace`: pass.
- `git diff Cargo.lock`: only the new dependency-list lines (no new package or version).
- `bash tools/story-loop/gates.sh`: ALL-OK (this is the epic's last story, so it includes `cargo hack`).

## Auto Run Result

- `cargo bench -p noctis-image --features render --bench reference_card` (cloud container, T = 2 threads, 2 clients, 1000 samples): min 4.530 ms, mean 7.711 ms, **p50 6.518 ms**, **p99 15.934 ms**, max 25.287 ms; both within budget (10 / 50 ms, report only).
- Committed snapshots: `crates/noctis/tests/snapshots/reference_card.png` (first run, then re-run twice against it) and `crates/noctis-testkit/tests/snapshots/fake_card.png`.
- `*.new.png` is ignored in the repository and in the Project Template.

## Auto Run Result

- **Summary:** Adds FR-51 pixel snapshots to `noctis-testkit` (`assert_render_snapshot!`/`render_snapshot`, `Tolerance`, `SnapshotEnv` with `NOCTIS_UPDATE_SNAPSHOTS`/`CI` matrix, `.new.png` on mismatch, no writes on CI), a committed reference-card snapshot test in the facade, a `reference_card` bench in `noctis-image` with budgets in `crates/noctis-core/benches/budgets.toml`, and the report-only latest-takumi job (`.github/scripts/latest-takumi.sh`, disabled workflows).
- **Files changed:**
  - `crates/noctis-testkit/src/snapshot.rs`, `snapshot/tests.rs`, `tests/render_snapshot.rs`, `tests/snapshots/fake_card.png`, `src/lib.rs`, `src/app.rs`, `Cargo.toml` — snapshot API, tests and fixture.
  - `crates/noctis/src/lib.rs`, `tests/reference_card.rs`, `tests/snapshots/reference_card.png` — facade re-export, reference-card snapshot and template drift test.
  - `crates/noctis-image/benches/reference_card.rs`, `benches/reference_card/{card.rs,noctis-theme.css}`, `Cargo.toml`; `crates/noctis-core/benches/budgets.toml` — reference-card benchmark and budgets.
  - `.github/scripts/latest-takumi.sh`, `.github/workflows-disabled/{ci.yml,main.yml}` — latest-takumi job and gates.
  - `examples/project-template/` (skill SKILL.md, images.md, testing.md, AGENTS.md, .gitignore, tests/templates.rs); `xtask/public-api.txt` — template, Usage Skill and API snapshot.
- **Review findings:** 34 findings (high 0, medium 4, low 21, false 9). Patched: 4 medium (sed pin rewrite unchecked x3 same root cause; `render_snapshot` panic path untested) and 6 low (DecodeRendered test, reference-card drift test, font wording in skill, update-mode shade ordering, `from_env` variable-name test, font conflict grouped). Deferred: 1 (template `profile_card.png` to Story 14.2). Rejected: 23, each with its recorded reason in the Review Triage Log (spec-settled design, hypothetical or report-only cases).
- **Follow-up review recommended:** true — first pass patched 4 medium entries; unverified risk: the new pin-rewrite check in `latest-takumi.sh` only runs in the disabled CI job and is not exercised by any local gate.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy, tests, deps-check, tw-grammar, tw_crosscheck, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV); CI paused by maintainer.
- **Residual risks:** latest-takumi script untested until CI returns; the Project Template's own snapshot is missing until Story 14.2; bench budgets are report-only until Story 14.3.
