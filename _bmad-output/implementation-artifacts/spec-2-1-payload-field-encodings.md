---
title: 'Story 2.1: Payload field encodings'
type: 'feature'
created: '2026-10-19'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-codec` is an empty crate, so no Payload field type has an encoding, a known worst-case width or a round-trip guarantee, and bot authors cannot make their own types Payload fields (FR-16, NFR-9; AD-5, AD-6, AD-25; CID-01, CID-07).

**Approach:** Add to `noctis-codec` the `Field` trait (`const MAX_LEN: Option<usize>` + `encode`) and the lifetime-parameterised `FieldDecode<'de>` trait, base62/zig-zag integer, `bool`, `Id<T>`, `Option<T>`, `BoundedStr<N>`, `String`, `Cow<'a, str>` and `Vec<T>` encodings with `!` escaping, a typed `FieldError`, and public round-trip / worst-case-width check helpers; re-export them from the facade as `noctis::codec` and document them in the Usage Skill.

## Boundaries & Constraints

**Always:**
- Base62 alphabet is `0-9A-Za-z` (digit value = index; `"47"` = 255). Integers are minimal-digit base62 (no padding, `0` → `"0"`); signed `iN` maps to `uN` by zig-zag (`(n << 1) ^ (n >> (BITS-1))`), so `-1` → `"1"`, `1` → `"2"`, `iN::MIN` → `uN::MAX`.
- Widths exactly: `bool` 1, `u8`/`i8` 2, `u16`/`i16` 3, `u32`/`i32` 6, `u64`/`i64`/`twilight_model::id::Id<T>` (any marker `T`) 11, `Option<T>` = `T::MAX_LEN + 1` (`None` if `T` is unbounded or `+1` overflows), `BoundedStr<N>` = `2N` (`None` on overflow), `String`/`Cow<'a, str>`/`Vec<T>` = `None`.
- `bool`: `false` → `"0"`, `true` → `"1"`. `Option<T>`: `None` → `"0"`, `Some(v)` → `"1"` + `encode(v)`.
- String escape table (all string-like fields): printable ASCII `0x20..=0x7E` passes through except `!` → `!0`, `,` → `!1`, `:` → `!2`, `~` → `!3`. `String`/`Cow` additionally write any other char (control `0x00..=0x1F`, `0x7F`, non-ASCII) as `!u` + exactly 6 lowercase hex digits of its scalar value (`'é'` → `!u0000e9`). `BoundedStr<N>` holds printable ASCII only, so it never emits `!u`.
- `Vec<T>` (`T: Field + for<'x> FieldDecode<'x>`): concatenate `encode(e) + ","` per element, then apply the `String` escape to the whole (`[]` → `""`, `[1u8, 2]` → `"1!12!1"`, `[""]` (Vec<String>) → `"!1"`).
- Every encoding is ASCII, printable (`0x20..=0x7E`), never contains `,`, `:` or `~`, and is ≤ `MAX_LEN` when bounded (CID-07). This is the documented contract for author impls and is what the check helpers verify.
- Decoding is total and canonical: `decode(s)` never panics; if it returns `Ok(v)` then `encode(v) == s` exactly (so leading zeros, `-`/`+`, unknown or unnecessary escapes, uppercase hex and over-long inputs are errors). Length guards run before arithmetic; arithmetic is checked.
- Fixed-width fields (`bool`, all integers, `Id<T>`, `BoundedStr<N>`, `Option` of these) decode from `&str` without heap allocation: they are `Copy` and `encode` writes through a stack buffer. `Cow<'de, str>` decodes `Cow::Borrowed` when the input contains no `!`.
- `FieldError` and `BoundedStrError` are `Copy`, `thiserror` enums, `#[non_exhaustive]`; every public item has rustdoc.
- Public API change ⇒ same commit updates `xtask/public-api.txt` (`--bless`), the skill's `API hash`, `references/routes-payloads-refs.md`, the template's `AGENTS.md` and a template test (AGENTS.md policy).

**Never:**
- No layout code: no `F`/route/`S`/`E`/`N`/`T` handling, no `ArrayString<100>` encoder, no `EncodedId`, and none of the 8 layout constants (`FMT_LEN` … `TAG_LEN`) — Story 2.2 owns them and `xtask rules-catalog` fails if any appears while `custom-id-constants` is pending.
- No `Route`/`Payload`/`PayloadEnum` traits or derives, no `sum_fields`/`assert_fits` (Story 2.5); no Refs, tags, `getrandom`, `blake3` (2.3/2.4); no dhat test (2.6).
- No `Field` impls for `usize`/`isize`/`char`/`f32`/`f64`/tuples/arrays.
- No `unsafe`; no `unwrap`/`expect` outside tests; no new facade feature; do not re-export `Id` or add `noctis::id` (later story).
- Do not name any test with the tokens `cid_04`, `cid_05` or `cid_06` and do not add `// [CID-xx]` doctest tags other than `// [CID-07]` (they would satisfy pending catalog entries of other stories and fail `rules-catalog`).

## I/O & Edge-Case Matrix

Encode (`encode(v)` → string):

| Scenario | Input | Output |
|---|---|---|
| bool | `false` / `true` | `"0"` / `"1"` (worst case width 1) |
| u8 zero / one digit max / two digits | `0` / `61` / `62` | `"0"` / `"z"` / `"10"` |
| u8 worst | `255` | `"47"` (len 2 = MAX_LEN) |
| i8 | `0` / `-1` / `1` / `127` / `-128` | `"0"` / `"1"` / `"2"` / `"46"` / `"47"` (worst, len 2) |
| u16 / i16 worst | `65535` / `i16::MIN` | `"H31"` / `"H31"` (len 3) |
| u32 / i32 worst | `u32::MAX` / `i32::MIN` | `"4gfFC3"` / `"4gfFC3"` (len 6) |
| u64 / i64 worst | `u64::MAX` / `i64::MIN`; `i64::MAX` | `"LygHa16AHYF"` (len 11); `"LygHa16AHYE"` |
| Id | `Id::<GenericMarker>::new(u64::MAX)` / `Id::new(1)` | `"LygHa16AHYF"` (len 11) / `"1"` |
| Option | `None::<u8>` / `Some(255u8)` / `Some(None::<u8>)` / `Some(Some(0u8))` | `"0"` / `"147"` (len 3) / `"10"` / `"110"` |
| BoundedStr plain / empty | `"abc"` / `""` | `"abc"` / `""` |
| BoundedStr specials | `"a,b!c:d~"` (N=8) | `"a!1b!0c!2d!3"` |
| BoundedStr worst | `"!!!!"` (N=4) | `"!0!0!0!0"` (len 8 = 2N) |
| BoundedStr N=0 | `""` | `""`, MAX_LEN `Some(0)` |
| Option<BoundedStr<2>> worst | `Some(",,")` | `"1!1!1"` (len 5) |
| String unicode / control / space | `"é😀"` / `"\n"` / `" "` | `"!u0000e9!u01f600"` / `"!u00000a"` / `" "` |
| Vec | `Vec::<u8>::new()` / `vec![1u8, 2]` / `vec![String::new()]` / `vec![vec![1u8]]` | `""` / `"1!12!1"` / `"!1"` / `"1!01!1"` (inner `"1!1"` + `","`, escaped) |

Decode (`T::decode(s)`):

| Scenario | Type / input | Result |
|---|---|---|
| Empty int/bool/Option | `u8` `""`, `bool` `""`, `Option<u8>` `""`, `Id` `""` | `Err(Empty)` |
| Zero / max | `u8` `"0"` / `"47"` | `Ok(0)` / `Ok(255)` |
| Max+1 | `u8` `"48"`, `u8` `"zz"`, `u16` `"H32"`, `u64` `"LygHa16AHYG"`, `u64` `"zzzzzzzzzzz"` | `Err(Overflow)` |
| Over width | `u8` `"100"`, `u64` 12 chars, `bool` `"11"` | `Err(TooLong { max, actual })` (e.g. `max: 2, actual: 3`) |
| Leading zero | `u8` `"00"`, `"01"`; `i8` `"007"` (len>2 → TooLong first) | `Err(LeadingZero)` / `TooLong` |
| Bad digit | `u8` `"-1"`, `"+1"`, `" 1"`, `"1 "`, `"é"` | `Err(InvalidChar { index })` with byte index of first bad char (`0`,`0`,`0`,`1`,`0`) |
| Signed | `i8` `"47"` / `"48"`; `i64` `"LygHa16AHYF"` | `Ok(-128)` / `Err(Overflow)`; `Ok(i64::MIN)` |
| Id zero | `Id<T>` `"0"` | `Err(ZeroId)` |
| bool other | `"2"`, `"a"` | `Err(InvalidChar { index: 0 })` |
| Option tag | `"2"` / `"0x"` / `"1"` (`Option<u8>`) | `InvalidChar{index:0}` / `TrailingInput{index:1}` / `Err(Empty)` (inner empty) |
| Option nested | `Option<Option<u8>>` `"10"` / `"110"` | `Ok(Some(None))` / `Ok(Some(Some(0)))` |
| Option index | inner error | index-bearing variants (`InvalidChar`, `BadEscape`, `TrailingInput`) are offset by the 1-char tag (`"1-"` for `Option<u8>` → `InvalidChar{index:1}`); others pass through unchanged (`"1100"` → `TooLong{max:2, actual:3}`) |
| BoundedStr ok | `<8>` `"a!1b!0c!2d!3"` | `Ok("a,b!c:d~")` |
| BoundedStr raw special | `"a,b"`, `"a:b"`, `"a~b"` | `Err(InvalidChar { index: 1 })` |
| Escape errors | `"a!"` (dangling), `"!4"`, `"!a"`, `"!u0000e9"` (BoundedStr: `!u` not allowed) | `Err(BadEscape { index })` = index of the `!` |
| Non-printable raw | `"\n"`, `"é"`, `"\u{7f}"` | `Err(InvalidChar { index })` |
| BoundedStr capacity | `<2>` `"abc"` / `<2>` `"!0!0"` / `<2>` `"!0!0!0"` | `OverCapacity{capacity:2, actual:3}` / `Ok("!!")` / `TooLong{max:4, actual:6}` (raw length checked first) |
| BoundedStr N=0 | `""` / `"a"` | `Ok("")` / `TooLong{max:0, actual:1}` |
| String `!u` | `"!u0000e9"` / `"!u0000E9"` / `"!u00e9"` / `"!u000041"` / `"!u00d800"` / `"!u110000"` | `Ok("é")` / `BadEscape` (uppercase) / `BadEscape` (short) / `BadEscape` (non-canonical: `A` is plain) / `BadEscape` (surrogate) / `BadEscape` (> 0x10FFFF) |
| Cow borrow | `Cow<str>` `"abc"` / `"a!1"` | `Cow::Borrowed("abc")` / `Cow::Owned("a,")` |
| Vec | `Vec<u8>` `""` / `"1!12!1"` / `"1!12"` / `"1"` / `"!1"` | `Ok([])` / `Ok([1,2])` / `Err(BadList)` (no trailing terminator) / `Err(BadList)` / `Err(Empty)` (element `""` for u8) |
| Arbitrary | any `&str` incl. 101+ chars, NUL, emoji, every type | never panics; `Ok(v)` ⇒ `encode(v) == input` |

`BoundedStr` construction:

| Scenario | Input | Result |
|---|---|---|
| ok / empty / exactly N | `BoundedStr::<4>::new("ab")` / `""` / `"abcd"` | `Ok` |
| N+1 | `new("abcde")` (N=4) | `Err(BoundedStrError::TooLong { capacity: 4, actual: 5 })` |
| non-ASCII / control / DEL | `"aé"` / `"a\tb"` / `"\u{7f}"` | `Err(InvalidChar { index: 1, ch: 'é' })` / `{index:1, ch:'\t'}` / `{index:0, ch:'\u{7f}'}` (checked before length) |
| specials allowed | `",!:~"` (N=4) | `Ok` |
| const literal bad | `const T: BoundedStr<4> = BoundedStr::from_static("çay");` | compile error (const-eval panic) — `compile_fail` doctest `// [CID-07]` |
| const literal too long | `BoundedStr::<2>::from_static("abc")` in a const | compile error (second `compile_fail` doctest) |

Check helpers (`noctis_codec::check`):

| Scenario | Call | Result |
|---|---|---|
| good value | `round_trip(&255u8)` | `Ok(())` |
| author impl whose encoding has `,` / non-ASCII / > MAX_LEN / decode differs / decode fails | `round_trip(&v)` | `Err(CheckError::ReservedChar{index,ch})` / `NotPrintable{index,ch}` / `TooLong{max,actual}` / `Mismatch{encoded}` / `Decode{encoded, error}` |
| worst case | `worst_case(&u8::MAX)` / `worst_case(&1u8)` / `worst_case(&String::new())` | `Ok(())` / `Err(WidthMismatch{max:Some(2), actual:1})` / `Err(WidthMismatch{max:None, actual:0})`; `worst_case` also runs `round_trip` first |
| panicking forms | `assert_round_trip(&v)` / `assert_worst_case(&v)` | panic with the `CheckError` `Display` on failure |

</intent-contract>

## Code Map

- `crates/noctis-codec/src/lib.rs` -- only module docs today; becomes module declarations + root re-exports (`Field`, `FieldDecode`, `FieldError`, `BoundedStr`, `BoundedStrError`, `pub mod check`). Keep the "internal; use `noctis`" note.
- `crates/noctis-codec/Cargo.toml` -- no deps yet; add `thiserror.workspace = true`, `twilight-model.workspace = true` (0.17.1, verified on crates.io: ISC licence — allowed in `deny.toml` l.27 — `rust_version` 1.89 ≤ MSRV 1.91; not yet in `Cargo.lock`), `[dev-dependencies] proptest.workspace = true` (1.12.0 in lock).
- `Cargo.toml` (root) l.~35 -- `twilight-model = "0.17.1"` and `thiserror = "2.0"` already in `[workspace.dependencies]`; workspace lints (`unsafe_code = deny`, `rust_2018_idioms` warn ⇒ elided lifetimes in paths are errors under `-D warnings`, pedantic, `unwrap_used`/`expect_used` warn) apply.
- `xtask/src/deps_check.rs` l.56-63, l.389 -- facade → any internal crate is allowed; `noctis-codec → noctis-rules` allowed but not needed here. External deps are not checked.
- `xtask/src/rules_catalog/mod.rs` l.554-625 -- `codec_constants` scans `crates/noctis-codec/src` for `pub const NAME: <int>`; only the 8 catalog names matter, so never define them here. l.470-490: doctest `// [ID]` tags (after code, in a `compile_fail` block in `crates/*/src`) count as by-construction evidence; `#[test]` fn names citing a snake id count as unit evidence.
- `xtask/rules-pending.toml` -- the `[[rule]] id = "CID-07" evidence = "by-construction" story = "2.1"` entry must be removed in this story (the new doctest satisfies it; leaving it fails P2). Leave the `custom-id-constants` check and all other entries.
- `crates/noctis/src/lib.rs` l.20-122 -- facade: crate docs + `pub mod rules` with doctests and `pub use noctis_rules::…` (pattern to copy for a new `pub mod codec`); `#![deny(missing_docs)]`. `crates/noctis/Cargo.toml` has only `noctis-rules` — add `noctis-codec.workspace = true`.
- `xtask/public-api.txt` + `xtask api-snapshot [--bless]` (Story 1.8) -- inlines facade re-exports of internal crates; needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0 (both installed here).
- `examples/project-template/.claude/skills/noctis/SKILL.md` l.22 -- `API hash:` line = blake3 of `xtask/public-api.txt`; `xtask skill-check` requires it and requires every new item's last path segment (types, variants, fields like `index`/`capacity`, fns, `MAX_LEN`) to appear as a whole identifier somewhere in the skill's Markdown.
- `examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md` l.80-103 -- §2 "Field types and widths" already names `noctis::codec::Field`; l.98-99 says author impls are "[planned]" — replace with the real contract.
- `examples/project-template/AGENTS.md` "Conventions that differ from defaults" -- add one line on custom Payload fields; `examples/project-template/tests/` -- tracking-only build (CI `template-build`, Story 14.2), files must still lex for `skill-check`'s doctrine lint.
- `crates/noctis-rules/src/key.rs` l.23, `crates/noctis-rules/src/component.rs` l.66-80 -- existing `compile_fail` doctests with `// [ID]` tags to copy.
- Previous story (1.8, done): its gates list (Verification) is reused; follow-up risk noted there (re-export inlining) means: if `api-snapshot` output for `noctis::codec` looks wrong (missing items, `noctis_codec` paths), stop and fix xtask rather than hand-edit the snapshot.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-codec/Cargo.toml` -- add deps listed in Code Map -- encodings need `Id<T>` and typed errors.
- `crates/noctis-codec/src/lib.rs` -- declare `mod base62; mod escape; mod error; mod field; mod bounded_str; pub mod check;`, re-export `Field, FieldDecode, FieldError, BoundedStr, BoundedStrError` at the root, document the escape table and canonical-decode rule in the crate docs -- single entry point for 2.2+.
- `crates/noctis-codec/src/base62.rs` (`pub(crate)`) -- alphabet, `encode_u64` into a `[u8; 11]` stack buffer returning the used slice as `&str`, `decode_u64(raw, max_len) -> Result<u64, FieldError>` (empty → `Empty`, `len > max_len` → `TooLong`, bad char → `InvalidChar{index}`, `len > 1 && starts '0'` → `LeadingZero`, checked mul/add → `Overflow`), zig-zag helpers -- shared by integers and later Ref/tag stories.
- `crates/noctis-codec/src/error.rs` -- `FieldError` variants: `Empty`, `InvalidChar { index: usize }`, `LeadingZero`, `Overflow`, `TooLong { max: usize, actual: usize }`, `OverCapacity { capacity: usize, actual: usize }`, `ZeroId`, `BadEscape { index: usize }`, `TrailingInput { index: usize }`, `BadList`; `#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]`, `#[non_exhaustive]`, messages lower-case without trailing period.
- `crates/noctis-codec/src/field.rs` -- `pub trait Field { const MAX_LEN: Option<usize>; fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result; }` and `pub trait FieldDecode<'de>: Field + Sized { fn decode(raw: &'de str) -> Result<Self, FieldError>; }`, with rustdoc stating the implementor contract (Boundaries "Every encoding is …" + canonical decode) and a doc example of an author newtype delegating to `u8`; impls for `bool`, `u8..u64`, `i8..i64` (one `macro_rules!` for each family), `Id<T>`, `Option<T>`, `String`, `Cow<'a, str>`, `Vec<T>` -- AD-6 widths and AD-5 encodings.
- `crates/noctis-codec/src/escape.rs` (`pub(crate)`) -- `escape(s, allow_unicode, out)`, `unescape(raw, allow_unicode) -> Result<Cow<'_, str>, FieldError>` (borrowed when no `!`), shared by `BoundedStr`, `String`, `Cow`, `Vec`.
- `crates/noctis-codec/src/bounded_str.rs` -- `BoundedStr<const N: usize>` (private `[u8; N]` zero-filled tail + `len: usize`; derives `Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord`; manual `Debug` as `BoundedStr("…")`, `Display`, `Default`, `AsRef<str>`, `TryFrom<&str>`, `FromStr`, `PartialEq<str>`, `PartialEq<&str>`); `new(&str) -> Result<Self, BoundedStrError>`, `pub const fn from_static(&'static str) -> Self` (panics — in const context a compile error — on invalid input; `# Panics` doc), `as_str` (via `core::str::from_utf8`, returning `""` on the unreachable error branch, no `unwrap`), `len`, `is_empty`, `const CAPACITY: usize = N`; `BoundedStrError { TooLong { capacity, actual }, InvalidChar { index, ch } }`; two `compile_fail` doctests (non-ASCII line ends `// [CID-07]`; over-capacity) plus a passing doctest of the same `const` with valid input; `Field`/`FieldDecode` impls.
- `crates/noctis-codec/src/check.rs` -- `CheckError` (`NotPrintable { index, ch }`, `ReservedChar { index, ch }`, `TooLong { max, actual }`, `Decode { encoded: String, error: FieldError }`, `Mismatch { encoded: String }`, `WidthMismatch { max: Option<usize>, actual: usize }`; `Debug, Clone, PartialEq, Eq, thiserror::Error`, `#[non_exhaustive]`); `round_trip<T>(&T) -> Result<(), CheckError>` and `worst_case<T>(&T) -> Result<(), CheckError>` with `T: Field + for<'de> FieldDecode<'de> + PartialEq + Debug`; `assert_round_trip`, `assert_worst_case` (panic with `Display`, `#[track_caller]`) -- FR-16 last consequence: authors' own tests use these.
- `crates/noctis-codec/src/*` unit tests (`#[cfg(test)] mod tests`) -- one test per built-in type asserting the worst-case value encodes to exactly `MAX_LEN` via `worst_case` (names like `cid_01_u8_worst_case_width`), every row of the I/O matrix, `Copy` static assertions for fixed-width types, `Cow::Borrowed` assertion, and `cid_07_…` tests that every built-in encoding of hostile strings is printable ASCII.
- `crates/noctis-codec/tests/round_trip.rs` -- proptest: strategies for every field type (`any::<u8..i64>`, `bool`, non-zero `u64` → `Id<GenericMarker>`, `"[ -~]{0,8}"` → `BoundedStr<8>`, `any::<String>()`, `option::of`, `vec(…, 0..8)` incl. `Vec<String>` and `Vec<Vec<u8>>`); `round_trip` holds for each; a composite list (`u8, i64, Option<Id<_>>, BoundedStr<8>, bool, String, Vec<String>`) joined with `,`, split with `str::split(',')`, yields exactly 7 parts that each decode to the original; "decode never panics and `Ok(v)` ⇒ `encode(v) == input`" over `".*"` and `"[ -~!,:~0-9a-zA-Z]{0,120}"` for every type. Commit any `proptest-regressions/` files.
- `xtask/rules-pending.toml` -- delete the CID-07 by-construction entry.
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` -- add `pub mod codec` (docs + one doctest showing `BoundedStr`, `<u8 as Field>::MAX_LEN`, an author newtype implementing `Field`/`FieldDecode` and `check::assert_round_trip`/`assert_worst_case`) re-exporting `BoundedStr, BoundedStrError, Field, FieldDecode, FieldError` and `check`; mention `codec` in the crate docs.
- `xtask/public-api.txt` -- regenerate with `cargo run --release -p xtask -- api-snapshot --bless`.
- `examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md` -- rewrite §2 tail: the two traits and their contract, escape table, `BoundedStr` API and errors, `FieldError` variants, `check` helpers with a custom-field example and a proptest usage snippet; mention every new public identifier. `SKILL.md` -- new `API hash`.
- `examples/project-template/AGENTS.md` -- convention line: custom Payload field types implement `noctis::codec::{Field, FieldDecode}` and are tested with `noctis::codec::check::{assert_round_trip, assert_worst_case}`. `examples/project-template/tests/payload_fields.rs` -- a `Percent(u8)` newtype field (0..=100, decode rejects >100) with round-trip and worst-case tests and a `BoundedStr<6>` round-trip.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already updated by planning (decision lines for this story); no implementer action.

**Acceptance Criteria:**
- Given the facade, when a bot author writes `use noctis::codec::{Field, FieldDecode, BoundedStr, check};`, then `<u8 as Field>::MAX_LEN == Some(2)` … the full AD-6 table holds, and each built-in worst-case value passes `check::worst_case` (encoded length exactly `MAX_LEN`).
- Given a `BoundedStr<N>` containing `,`, `!`, `:` or `~`, when encoded and decoded, then the value round-trips exactly, the encoding contains none of `,` `:` `~` raw and is printable ASCII, and non-ASCII input fails at `new` (runtime error) and at `from_static` in a `const` (compile error; `compile_fail` doctest tagged `// [CID-07]`).
- Given the proptest suite, when `cargo test --release -p noctis-codec` runs, then every field type and the composite list round-trip, and no decode panics on arbitrary input.
- Given an author type implementing `Field` + `FieldDecode`, when its tests call `noctis::codec::check::assert_round_trip` and `assert_worst_case`, then a wrong encoding (reserved char, non-ASCII, over `MAX_LEN`, lossy) fails with the matching `CheckError`, and the Usage Skill's `references/routes-payloads-refs.md` documents this.
- Given the change, when `rules-catalog`, `api-snapshot`, `skill-check` and `deps-check` run, then all pass with the CID-07 pending entry removed and the skill hash updated.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (logged to the memlog):
- **Two traits.** `Field` stays lifetime-free so the derive and `sum_fields` can write `<F as Field>::MAX_LEN` as the spine shows (a lifetime on `Field` would force `Field<'_>` under `rust_2018_idioms` + `-D warnings`). `FieldDecode<'de>` mirrors serde's `Deserialize<'de>` so `Cow<'de, str>` can borrow (AD-25) while owned types implement it for all `'de`.
- **Sink = `core::fmt::Write`.** `ArrayString<100>` (Story 2.2) and `String` both implement it; capacity overflow surfaces as `fmt::Error`. No custom writer type.
- **Canonical decoding.** Rejecting non-canonical forms makes `decode` injective, so later integrity tags and CID-02 uniqueness see one spelling per value, and gives the strong property test `Ok(v) ⇒ encode(v) == input`.
- **Escapes map to digits** (`!0..!3`) so encoded fields never contain a raw separator: the 2.2 layout decoder can split the body with plain `split(',')` and recognise a `~` body by its first char. `!u` + 6 hex keeps `String` ASCII for `spill` Routes that encode inline (CID-07); `BoundedStr` is printable ASCII only (controls rejected too — a stricter reading of "ASCII only" that keeps ids loggable).
- **`Vec<T>` = escaped terminator list**: composes for nested vectors and distinguishes `[]` from `[""]`; elements must be owned (`for<'x> FieldDecode<'x>`).
- **No-alloc proof in this story is by type** (`Copy` fixed-width types, borrowed `Cow`); the dhat guard is Story 2.6's.
- `twilight-model` enters `noctis-codec` for `Id<T>` (spine: twilight-model is used for ids); the facade does not re-export `Id` yet.

Golden example (author field):

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
struct Percent(u8);
impl Field for Percent {
    const MAX_LEN: Option<usize> = <u8 as Field>::MAX_LEN;
    fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result { self.0.encode(out) }
}
impl<'de> FieldDecode<'de> for Percent {
    fn decode(raw: &'de str) -> Result<Self, FieldError> {
        let v = u8::decode(raw)?;
        if v <= 100 { Ok(Self(v)) } else { Err(FieldError::Overflow) }
    }
}
```

## Verification

**Commands** (repo root; CI is paused, these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (codec unit, proptest and doctests incl. both `compile_fail`s; facade doctest).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- rules-catalog` -- expected: exit 0, `rules-catalog: ok (…)` with one fewer pending entry than before.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0 after `--bless`; snapshot contains `noctis::codec::` items and no `noctis_codec` path.
- `cargo run --release -p xtask -- skill-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0 (twilight-model and its deps' licences allowed).
- `cargo +1.91 check --release --workspace` -- expected: exit 0 (MSRV; `core::str::from_utf8` in const is stable since 1.87).
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.
