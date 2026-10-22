---
title: 'Story 10.1: Strict Tailwind v4 grammar and takumi cross-check'
type: 'feature'
created: '2026-10-22'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/tooling-and-project-layout.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis_image::tw` is an empty module. Noctis has no class grammar, so nothing can reject a Tailwind class that takumi silently drops or reads as an undefined CSS variable. For example, `bg-gradient-to-r` parses in takumi as `var(--color-gradient-to-r)` and draws nothing. Story 10.2's macros and the runtime check both need this grammar (FR-32, FR-66; AD-15, AD-28).

**Approach:**
- `xtask tw-grammar` syn-parses the pinned `takumi-core 0.31.0` Tailwind sources and combines them with a closed suffix-grammar table kept in xtask. From these it writes the committed, takumi-free `crates/noctis-image/src/tw/generated.rs`: data tables, theme-namespace hooks and the v3 → v4 renames.
- A hand-written matcher in `noctis_image::tw` copies takumi's lookup algorithm over those tables and exposes `classify`, `suggest` and `validate`.
- The `render`-gated integration test `tw_crosscheck` checks the grammar against takumi's own `TailwindValues::from_str` and smoke-renders every sample.

## Boundaries & Constraints

**Always:**
- **Pins and features.** `[workspace.dependencies]` keeps `takumi = { version = "=2.17.0", default-features = false }` and `takumi-core = { version = "=0.31.0", default-features = false }`; both are already present, so verify and leave them unchanged. In `crates/noctis-image/Cargo.toml`:
  - add `takumi = { workspace = true, optional = true, features = ["raster-backend"] }` and `takumi-core = { workspace = true, optional = true }`;
  - set `render = ["dep:takumi", "dep:takumi-core"]` and keep `default = []`;
  - add `thiserror.workspace = true`, which is takumi-free.

  `Cargo.lock` stays committed with the new entries. The noctis-core/ports/ui edges are NOT added; they belong to Story 10.3.
- **Takumi-free `tw`.** Nothing under `crates/noctis-image/src/tw/` names takumi. Everything builds and tests with `render` off.
- **xtask `tw-grammar [--check]`** lives in `xtask/src/tw_grammar/`.
  - **Locating the source.** It reads the `takumi-core` pin from the root `Cargo.toml` (`=X.Y.Z`; a missing or non-exact pin is an error). It runs `cargo_metadata::MetadataCommand` with `CargoOpt::AllFeatures` and selects the package `takumi-core` at that exact version. The crate root is the parent of its `manifest_path`.
  - **Extraction.** It parses these files with `syn` (`full`) and extracts:
    - `src/style/tw/map.rs`: the `FIXED_PROPERTIES` keys (alternatives joined by `|` count as separate keys); the `PREFIX_PARSERS` entries, each a prefix (with `|` alternatives) mapped to an ordered list of `PropertyParser::<Variant>(TailwindProperty::<Ctor>)`; the `property_parsers!` invocation triples `Variant(Arg) => ParseType [namespaces]`; and the `VAR_TARGETS` entries, each a prefix mapped to an ordered list of `Namespace::<X>`.
    - `src/style/tw/namespace.rs`: the `Namespace::prefix` match, giving each variant's CSS prefix.
    - `src/style/tw/mod.rs`: the `try_neg!` invocation lists (`try_negative`, `unary`, `grid`), and the built-in breakpoint keys in `Breakpoint::parse`.
    - The `property_parsers` `macro_rules!` body: the hard-coded variant `GradientPosition` and its parse type `TwGradientPosition`. Find it from the `Self::<Ident>(f) => { <Type>::parse_tw_with_arbitrary` arm outside the `$(..)` repetitions.
  - **Fail closed.** An error names the file and the item, then exits with code 1, when any of these happen:
    - an item is missing or has an unexpected token shape;
    - a parse type is missing from the closed table;
    - the closed table has an entry that is not in the source;
    - a `PREFIX_PARSERS` variant is unknown;
    - a namespace is unknown;
    - an algorithm fingerprint differs.
  - **Fingerprints.** For each of these, blake3 its `quote::ToTokens` token string and compare it with the hash recorded in the xtask table:
    - the algorithm functions in `src/style/tw/mod.rs`: `split_variant`, `TailwindValue::parse`, `Breakpoint::parse`, `TailwindProperty::parse`, `parse_prefix_suffix`, `try_neg`, `var_expression`, `parse_opacity_modifier`, `is_ident`, `extract_arbitrary_value`;
    - every `impl TailwindPropertyParser for <T>` block found under `src/**/*.rs`, keyed by `T`.

    On a mismatch, the error prints the item, the old hash and the new hash. Re-audit the matcher, then update the hash by hand.
  - **Output.** It writes `generated.rs` with the header comment `// @generated by \`cargo run --release -p xtask -- tw-grammar\` from takumi-core <ver>. Do not edit.` and a `pub const TAKUMI_CORE_VERSION: &str`. Keys are sorted (FIXED, PREFIXES, VAR_TARGETS, renames); the parser order inside a prefix keeps the source order. The text is piped through `rustfmt --edition 2024`. Running it twice is byte-identical. `--check` compares the generated text with the file on disk and fails without writing anything.
- **The closed suffix table** (`xtask/src/tw_grammar/table.rs`) has one entry per value parse type. There are exactly 50 types: the 49 distinct parse types of `property_parsers!` plus `TwGradientPosition`. Each entry is a Rust expression over `crate::tw::suffix` items, emitted verbatim. The implementer reads each type's `parse_tw` / `FromCss` impl in takumi-core 0.31.0. Narrowing is allowed, widening never is: a suffix the grammar accepts must make takumi accept the class.
- **The matcher** (`crates/noctis-image/src/tw/mod.rs`) copies `TailwindValue::parse` and `TailwindProperty::parse` (mod.rs:397, :1170, :1192):
  1. Split the class with a port of `split_variant`. The variant must be a built-in breakpoint (`sm|md|lg|xl|2xl`, lowercase only) or a theme token (`ThemeNamespace::Breakpoint`). A second variant is never accepted.
  2. Strip one leading `!`, then one trailing `!`.
  3. Look the utility up in FIXED.
  4. Strip one leading `-` (negative).
  5. Scan dash positions from right to left. For a prefix found in PREFIXES, try its parsers in order. The first suffix match wins; when negative, the negation rule applies. A property outside the `try_neg` lists, or a value that cannot be negated (Length `auto`, a grid placement that is not a line), rejects the whole class with no fallthrough.
  6. If no parser matched and the prefix is in VAR_TARGETS, apply `var_expression`'s rules:
     - a suffix starting with `[` → none;
     - `token/op`: only a `Color` group, and `op` must pass `parse_opacity_modifier`;
     - a numeric suffix with a `Spacing` group is the built-in spacing scale, which the suffix grammar already covers.

     When a group survives, this gives `Utility::Theme`. Otherwise go to the next shorter prefix.
  7. Unknown when nothing matched.

  Strict extras:
  - classes are ASCII only, at most 256 bytes;
  - theme tokens match `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`, at most 64 bytes;
  - a negative theme token is Unknown.
- **API in `noctis_image::tw`**:
  - `classify(&str) -> Option<Class<'_>>`, with these types (`#[derive(Debug, Clone, Copy, PartialEq, Eq)]`; the enums are `#[non_exhaustive]`):
    - `Class { variant: Option<Variant>, important, utility: Utility }`;
    - `Variant::{Builtin(&'static str), Custom(&str)}`;
    - `Utility::{Builtin, Theme { namespaces: &'static [ThemeNamespace], token: &str }}`.
  - `ThemeNamespace`, generated from `namespace.rs` plus `Breakpoint`, with `css_prefix()` (`--color-` … `--breakpoint-`).
  - `trait ThemeTokens { fn contains(&self, ns: ThemeNamespace, token: &str) -> bool; }` with the unit struct `NoTheme` (always false).
  - `suggest(&str) -> Option<Suggestion>` and `Suggestion::{Rename(String), Hint(&'static str)}`. A rename keeps the variant and the `!` placement.
  - `validate(classes: &str, theme: &dyn ThemeTokens) -> Result<(), InvalidClasses>`. It splits with `str::split_whitespace`, like takumi. Every class must be Builtin, or every token it needs (the utility token in any of its namespaces, plus a custom variant in `Breakpoint`) must be in `theme`.
  - `InvalidClasses` (`thiserror`, `#[non_exhaustive]`) holds `Vec<UnknownClass { class: String, suggestion: Option<Suggestion> }>`, in first-seen order and deduplicated, read through `.classes()`.
    - Display, one line per class: ``unknown Tailwind class `X` `` + `; use \`Y\` (Tailwind v4)` for a rename, or `; <hint>` for a hint.
- **Renames and hints** are curated in the xtask table and emitted:
  - prefix rename `bg-gradient-to-` → `bg-linear-to-` (the suffixes `t tr r br b bl l tl`);
  - exact rename `overflow-ellipsis` → `text-ellipsis`;
  - hint prefixes `bg-opacity-`, `text-opacity-`, `border-opacity-`: "use an opacity modifier, e.g. `bg-red-500/50`";
  - hint prefixes `space-x-`, `space-y-`: "use `gap-x-*`/`gap-y-*` on a flex or grid parent".

  A unit test proves that every rename target classifies `Builtin` and that no rename or hint source does.

**Never:**
- No Node.js, no Tailwind CLI and no `build.rs` codegen. Never hand-edit `generated.rs`.
- No facade wiring (`noctis::image`, `tw!`, `view!`), no Theme parsing, no runtime warn-once behaviour (Stories 10.2 and 10.3). The facade's public API is unchanged, so there is no skill or template update and no api-snapshot re-bless.
- No `url(...)` arbitrary values: remote images arrive through the typed AssetFetcher (Story 10.8). No `calc(` arbitrary values.
- No edit-distance typo suggestions.

## I/O & Edge-Case Matrix

The verdict column shows `classify` → `validate(.., &NoTheme)`. B = Builtin, T = Theme, U = Unknown; "narrowed" means takumi accepts the class but Noctis rejects it on purpose.

| Scenario | Input | Expected | Error / notes |
|---|---|---|---|
| fixed utility | `flex`, `hidden`, `shadow-2xs` | B → Ok | — |
| spacing | `p-4`, `m-0`, `w-2.5`, `gap-x-96` | B → Ok | — |
| keyword/fraction | `w-full`, `w-1/2`, `items-center` | B → Ok | — |
| palette color + opacity bounds | `bg-red-500`, `/0`, `/100` | B → Ok | — |
| opacity max+1 | `bg-red-500/101` | U | narrowed (takumi has no range check) |
| v4 gradient | `bg-linear-to-r`, `from-red-500`, `via-50%` | B → Ok | — |
| v3 gradient | `bg-gradient-to-r` | T → Err | suggestion Rename `bg-linear-to-r` |
| v3 with variant | `sm:!bg-gradient-to-r` | T → Err | Rename `sm:!bg-linear-to-r` |
| v3 exact | `overflow-ellipsis` | U → Err | Rename `text-ellipsis` |
| hint | `bg-opacity-50`, `space-x-4` | T / U → Err | Hint text |
| unknown prefix | `ring-2`, `divide-y`, `z-10`, `flx` | U → Err | suggestion None |
| theme token | `bg-brand`, `text-brand/50` | T → Err (NoTheme); Ok with a theme that has `Color,"brand"` | — |
| theme multi-ns | `text-title` | T{Text,Color} → Ok if either namespace has `title` | — |
| non-color opacity | `m-brand/50`, `bg-brand/150` | U | takumi agrees (empty) |
| negative | `-m-4`, `-translate-x-1/2`, `-col-start-2` | B → Ok | — |
| negative rejected | `-m-auto`, `-bg-red-500`, `-flex` | U | takumi agrees |
| negative theme | `-bg-brand` | U | narrowed |
| breakpoints | `sm:flex`, `2xl:p-4` | B → Ok | `SM:flex` U, narrowed |
| custom variant | `hover:flex` | T(variant) → Err NoTheme; Ok if `Breakpoint,"hover"` is defined | — |
| stacked/bad variant | `sm:md:flex`, `!sm:flex`, `:flex` | U | takumi agrees |
| important | `!p-4`, `p-4!`, `!p-4!` | B, important=true | `!!p-4` U |
| arbitrary | `w-[10px]`, `bg-[#ff0000]`, `rotate-[45deg]` | B → Ok | — |
| arbitrary rejected | `w-[]`, `w-[calc(1px+1px)]`, `bg-[url(a.png)]` | U | the last two narrowed |
| numeric oddities | `w-inf`, `w-NaN`, `w-1e3`, `w-FULL`, `w-0.1` | U | narrowed |
| unicode | `bg-rød`, `w-４`, `ｆlex` | U | first two narrowed |
| size bounds | 256-byte class / 257 bytes; 64-byte token / 65 | B-or-T / U | — |
| whitespace | `""`, `"  \t\n"`, `"flex\u{3000}p-4"` | Ok (none) / Ok / two classes | — |
| duplicates | `bad bad flex` | Err with one `bad` entry | — |
| xtask unknown type | fixture `property_parsers!` adds `Foo(X) => NewType` | exit 1, names `NewType` and map.rs | — |
| xtask shape drift | fixture without `VAR_TARGETS`, or with a changed `split_variant` | exit 1, names the item (and the hash) | — |
| xtask pin | root pin `0.31` (not `=`) or missing | exit 1 | — |
| xtask --check stale | `generated.rs` edited | exit 1, file untouched | — |

</intent-contract>

## Code Map

- `crates/noctis-image/src/lib.rs`: holds an empty `pub mod tw {}`. Replace it with `pub mod tw;`.
- `crates/noctis-image/Cargo.toml`: the `render = []` placeholder and empty defaults.
- `Cargo.toml` (root): the pins at `takumi`/`takumi-core` (already exact); `syn = "3.0"`, `quote`, `thiserror`, `cargo_metadata = "0.23"` and `blake3` are workspace deps.
- `xtask/src/main.rs`: the subcommand dispatch and `USAGE`. Add `tw-grammar [--check]` using the existing arg-parsing style.
- `xtask/Cargo.toml`: add `syn = { workspace = true, features = ["full"] }` and `quote.workspace = true`.
- `xtask/src/deps_check.rs`: GRAPH/GATED tables and the `render-on` rule (line ~640). Extend it with one rule (below).
- takumi-core 0.31.0 sources, at `cargo metadata` → `manifest_path` (or `~/.cargo/registry/src/*/takumi-core-0.31.0`):
  - `src/style/tw/map.rs`: `property_parsers!` (:56–111), `VAR_TARGETS` (:118), `PREFIX_PARSERS` (:184), `FIXED_PROPERTIES` (:436).
  - `src/style/tw/mod.rs`: `split_variant` :336, `TailwindValue::parse` :397, `Breakpoint::parse` :441, `var_expression` :948, `parse_opacity_modifier` :995, `extract_arbitrary_value` :1007, `is_ident` :1036, `try_neg` :1133, `TailwindProperty::parse` :1170, `parse_prefix_suffix` :1192.
  - `src/style/tw/namespace.rs`: the 10 namespaces.
  - Suffix parsers: `properties/color.rs:438` (palette `name-shade`, `/op` parsed as f32 with no range check), `properties/length.rs:154` (any f32 → spacing, keywords `full px auto 3xs..7xl dvw ...`), `properties/linear_gradient.rs:544` (Angle: `to-*` or f32), `properties/size.rs:152`, `tw/parser.rs`.
  - `takumi_core::style::TailwindValues` is public (`pub use tw::*`); it has `FromStr` and `Debug`. Its inner `TailwindValue { property, breakpoint, important }` is private, so it is visible only through `Debug`.
- `FromCss::VALID_TOKENS` (for example `properties/mod.rs:1417`) lists each CSS type's keyword tokens. It is a starting point for the hand-curated keyword sets, which the cross-check then verifies.
- `takumi` 2.17.0 facade: `takumi::render`, `prelude::{Node, RenderOptions, Viewport, Fonts}`, and `Node::with_tw(TailwindValues)` (takumi-core `layout/node/mod.rs:410`).
- `.github/workflows-disabled/ci.yml`: the job layout (see `deps-check:` at :125). Add `tw-crosscheck` here.
- `AGENTS.md` "Running and verifying": the local gate list.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-image/Cargo.toml`: add the deps and features described in Always.
- `crates/noctis-image/src/tw/suffix.rs`: add the closed `SuffixRule` vocabulary and its matcher (takumi-free):
  - `Keywords(&[&str])`;
  - `Spacing`: `0` or `[1-9]\d{0,3}`, optionally `.25|.5|.75`;
  - `Fraction`: `[1-9]\d?/[1-9]\d?`;
  - `Integer { min, max }`;
  - `Number { .. }`;
  - `Percent`: `\d{1,3}%` (gradient stop positions such as `via-50%`);
  - `Color { current: bool }`: the palette names × `50,100..900,950`, plus `transparent black white` (and `current` when set), with an optional `/0..=100` integer;
  - `Arbitrary(ArbitraryKind)`: Length `-?\d+(\.\d+)?(px|rem|em|%|vw|vh)`, Number, Integer, Angle `deg`, Percentage, Color `#` 3/4/6/8 hex, Ratio, and Keyword-ident;
  - per-rule `Negation` metadata and a `samples()` list for each rule.
- `xtask/src/tw_grammar/{mod.rs,extract.rs,table.rs,emit.rs}`: the generator described in Always. Its unit tests use inline fixture snippets for every xtask row of the matrix, plus a test that the table's key set equals the 50 parse types extracted from the real pinned source when it is present.
- `crates/noctis-image/src/tw/generated.rs`: generate it by running `cargo run --release -p xtask -- tw-grammar` and commit the result.
- `crates/noctis-image/src/tw/mod.rs`: the matcher and API from Always, the docs (`missing_docs` clean), and `#[doc(hidden)] pub mod __crosscheck { pub fn samples() -> Vec<String> }`. The samples are:
  - every FIXED key;
  - every (prefix, parser) × rule sample;
  - one negative sample per negatable entry;
  - `sm:`, `2xl:`, `!x` and `x!` forms of each prefix's first sample;
  - `<p>-noctis-tok` for each VAR_TARGETS prefix, plus `/50` when its first namespace is Color;
  - `brand:flex`.
- `crates/noctis-image/src/tw/tests.rs`: one unit test per matrix row that is not an xtask row, plus the rename and hint invariants.
- `crates/noctis-image/tests/tw_corpus.txt`: at least 60 near-misses, one per line: `<class> <builtin|theme|unknown> [narrowed=<reason>]`, with `#` comments. It covers every v3, typo, unknown-prefix, case, numeric, unicode, arbitrary, negative and variant row of the matrix.
- `crates/noctis-image/tests/tw_crosscheck.rs` (`#![cfg(feature = "render")]`). The oracle parses each class alone with `TailwindValues::from_str` and reads its `Debug`:
  - an empty `inner` → Empty;
  - a `VarUtility` whose fallbacks are all `None` → VarOnly;
  - otherwise → Builtin;
  - separately, `breakpoint: Some(Breakpoint { width: None` → custom variant, and `important: true`.

  A self-test pins these markers on `bg-red-500` (Builtin), `bg-brand` (VarOnly), `foo:flex` (custom) and `!flex`.

  Rules:
  - Every sample and corpus line must agree on variant kind, important and utility (B↔Builtin, T↔VarOnly, U↔Empty).
  - A `narrowed` line must have a grammar verdict U and a non-empty oracle result.
  - A corpus line's grammar verdict must equal its expected verdict.
  - Every mismatch is collected and reported together, naming the class, and the test then fails.
  - **Smoke render.** Each sample renders as `Node::container([]).with_tw(..)` on a 64×64 viewport with `Fonts::default()` inside `catch_unwind`. An `Err` is allowed; a panic fails and names the class.
- `xtask/src/deps_check.rs`: add the rule `takumi-gated`. Every `noctis-image` dependency whose name starts with `takumi` is optional, and only the `render` feature enables it. Add a unit test on a fixture.
- `.github/workflows-disabled/ci.yml`: add the job `tw-crosscheck` (same steps as `deps-check`), which runs `cargo run --release -p xtask -- tw-grammar --check` and `cargo test --release -p noctis-image --features render --test tw_crosscheck`.
- `AGENTS.md`: add those two commands to the gate list.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: the decision lines (already appended by planning).

**Acceptance Criteria:**
- Given a fresh clone, when `cargo run --release -p xtask -- tw-grammar --check` runs, then it exits 0. Running `tw-grammar` leaves `git diff --exit-code crates/noctis-image/src/tw/generated.rs` clean.
- Given `noctis-image` with default features, when `cargo tree -p noctis-image -e normal` runs, then no `takumi*` crate appears. With `--features render`, both pinned versions appear.
- Given `--features render`, when the `tw_crosscheck` test runs, then every sample and corpus class agrees with takumi, and no smoke render panics. A deliberately wrong corpus line (`flex unknown`) fails the test and the output names `flex`.
- Given `validate("p-4 bg-gradient-to-r", &NoTheme)`, when it is displayed, then it reads ``unknown Tailwind class `bg-gradient-to-r`; use `bg-linear-to-r` (Tailwind v4)``.

## Design Notes

- **Why there are three verdicts.** takumi's `VAR_TARGETS` fallback accepts *any* suffix under prefixes like `bg`, `w` or `text` as a CSS variable read (`bg-gradient-to-r` becomes `var(--color-gradient-to-r)`). The "non-empty value list" oracle therefore calls typos recognized. The grammar mirrors this as `Utility::Theme`, and strictness comes from `validate` requiring the token in the Theme. The cross-check compares like with like (T↔VarOnly), so "any disagreement fails" still holds.
- **Narrowing.** The grammar is a subset of takumi: it rejects `w-inf`, case variants, out-of-range opacity, `url()`/`calc()` arbitraries and non-ASCII. Each narrowed corpus line documents itself, and the test proves the line is still a real narrowing. Accepting more than takumi is impossible by construction, because every sample is checked.
- **Fingerprints** stand in for "a file whose shape no longer matches". The value keyword sets cannot be extracted from `FromCss` impls, so they are hand-curated, and the fingerprints force a re-audit on every takumi bump (AD-28: a bump regenerates and passes `tw-crosscheck`).

## Verification

**Commands:**
- `cargo run --release -p xtask -- tw-grammar --check` -- exit 0
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass
- `cargo fmt --all --check`; `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace`; `cargo hack test --release --workspace --each-feature` -- pass
- `cargo run --release -p xtask -- deps-check`, `rules-catalog`, `api-snapshot`, `skill-check` -- pass (api-snapshot unchanged)
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass

## Spec Change Log

## Review Triage Log
