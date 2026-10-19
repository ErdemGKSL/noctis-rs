---
title: 'Story 2.5: `#[derive(Route)]`, `Payload` and `PayloadEnum` with compile-time length proof'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: '46288835d01de9cffcc2b10e4eb5598477fa829e'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-codec` has hand-implementable `Route`/`Payload` traits but no derive. Nothing proves at compile time that a Route's `custom_id` fits in 100 characters, `noctis-macros` is empty, and the facade has no prelude or `__private`. A length bug therefore surfaces at run time (FR-16, FR-17, FR-60, FR-75, FR-76; AD-2, AD-6, AD-19; CID-01, CID-03, CID-05).

**Approach:**
- Add `Payload::MAX_ENCODED_LEN` and doc-hidden derive support to `noctis-codec`: `sum_fields`, `RouteWidth`, `assert_fits`, enum-digit helpers and an empty `RouteRuntime`.
- Move the route-id grammar and the codec's numeric caps into a new `noctis_rules::cid` module.
- Implement `#[derive(Route)]`, `#[derive(Payload)]` and `#[derive(PayloadEnum)]` in `noctis-macros`.
- Re-export the derives and their traits from `noctis` (`prelude`, `codec`, hidden `__private`), then update the template, the Usage Skill, the snapshot, the catalog and the pending list.

## Boundaries & Constraints

**Always:**
- **Codec (`crates/noctis-codec`):**
  - **`Payload` trait:** gains a required `const MAX_ENCODED_LEN: Option<usize>;`, the worst-case body width, where `None` means unbounded.
  - **`Field` trait:** gets `#[diagnostic::on_unimplemented(message = "[CID-01] `{Self}` is not a Payload field type", label = "not a Payload field", note = "implement `Field` and `FieldDecode` from `noctis::codec`, or derive `PayloadEnum` on a unit enum")]`.
  - **Constants:** `MAX_SCHEMA` and the route-byte and route-length checks come from `noctis_rules::cid`, which becomes a normal dependency. Add const asserts that `parts::DIGIT_MAX == SCHEMA_MAX` and that `base62::ALPHABET.len() == PAYLOAD_ENUM_MAX_VARIANTS`.
- **New `src/derive.rs`:** every item is `#[doc(hidden)] pub` and re-exported at the crate root.
  - **`sum_fields`:** `pub const fn sum_fields(widths: &[Option<usize>]) -> Option<usize>`. It returns `None` if any width is `None`. Otherwise it returns the saturating sum plus `len − 1` separators, and `Some(0)` for `[]`.
  - **`RouteWidth` / `route_width`:** `RouteWidth` is `Debug, Clone, Copy, PartialEq, Eq`, and `pub const fn route_width<R: Route>() -> RouteWidth` builds it.
    - Its getters are `route()`, `body_kind()`, `prefix()` (F + route + `:` + S + E + N), `schema()`, `envelope()`, `nonce()`, `body()` (`R::Payload::MAX_ENCODED_LEN`), `tag()`, `total() -> Option<usize>` and `fits() -> bool`.
    - **`total()`:** Inline gives `prefix + body + tag` (`None` if unbounded). ByRef gives `prefix + 12`. Spill gives `min(prefix + max(body + tag, 12), 100)`, where an unbounded body counts as more than 100.
    - **`fits()`:** Inline requires `total ≤ 100`. ByRef and Spill require `prefix + 12 ≤ 100`.
    - **`Display`:** exactly the matrix formats.
  - **`assert_fits`:** `pub const fn assert_fits<R: Route>()` panics only when `!fits()`. The panic message is built in const code (fixed byte buffer, `core::str::from_utf8`, `panic!("{}", msg)`) and names the route id and the length:
    - unbounded: `` [CID-01] route `<id>` has unbounded fields\n  = help: bound them (`BoundedStr<N>`), or add `by_ref` or `overflow = spill` to #[route] ``
    - too long: `` [CID-01] route `<id>` can exceed 100 characters: worst case <n>\n  = help: shorten the route id or the fields (`PayloadEnum`, smaller integers), or add `by_ref` or `overflow = spill`; run the generated noctis_route_width test for the breakdown ``
  - **Enum digits:** `encode_variant<W: fmt::Write + ?Sized>(index: u8, out: &mut W) -> fmt::Result` returns `Err(fmt::Error)` for `index ≥ 62`. `decode_variant(raw: &str, count: u8) -> Result<u8, FieldError>` checks, in order: empty → `Empty`, `len() > 1` bytes → `TooLong{max:1, actual}`, non-base62 → `InvalidChar{index:0}`, digit ≥ count → `Overflow`.
  - **Runtime declaration:** `RouteRuntime` (`#[non_exhaustive]`, `Debug, Clone, Copy, PartialEq, Eq`, with `pub const EMPTY`) and `trait DeclaredRuntime { const RUNTIME: RouteRuntime; }`.
- **New `noctis_rules::cid` (`src/cid.rs`, `pub mod cid`):**
  - **Constants:** `ROUTE_ID_MAX_LEN: usize = 24`, `SCHEMA_MAX: u8 = 61` and `PAYLOAD_ENUM_MAX_VARIANTS: usize = 62`.
  - **`is_route_id_byte`:** `pub const fn is_route_id_byte(b: u8) -> bool` accepts `[a-z0-9._-]`.
  - **`validate_route_id`:** `pub fn validate_route_id(id: &str) -> Vec<Violation>` reports CID-03 with path `route id`. Length is counted in chars. All applicable violations are returned, in this order:
    1. Length: `Between{1,24}`/`Count`.
    2. Bad character: `Describe("[a-z0-9._-]")`/`Text`, plus the note `` `<ch>` at byte <i> is not allowed `` for the first bad char.
    3. Reserved: `Describe("an id that does not start with `_`")`/`Text`.

    Each violation carries a `help`.
  - Not re-exported by the facade.
- **`crates/noctis-macros`:**
  - **Dependencies:** `syn`, `quote`, `proc-macro2` (workspace: syn 3.0.9, quote 1.0.47, proc-macro2 1.0.107, verified on crates.io on 2026-10-19 with MSRV 1.71 and MIT/Apache) and `noctis-rules`.
  - **Entry points:** `#[proc_macro_derive(Route, attributes(route))]`, `#[proc_macro_derive(Payload)]` and `#[proc_macro_derive(PayloadEnum)]`.
  - **Errors:** errors are `syn::Error`s spanned on the offending token. The text is `[<RULE-ID>] <what is wrong>\n  = help: <fix>`, or a `noctis_rules::Violation`'s `Display`. Several errors are combined.
  - **Generated paths:** generated code names runtime items only through `::noctis::__private::codec::…` and std items through `::core::…`/`::std::…`.
- **`#[route("id", args…)]` grammar:** exactly one `#[route(...)]` list attribute. The first element is a string literal (any `LitStr`, value-checked by `validate_route_id`). It is followed by comma-separated arguments in any order, and a trailing comma is allowed.
  - **Codec arguments:**
    - `by_ref`
    - `overflow = spill`
    - `schema = <int literal 1..=61>`
    - `view`
    - `select`
    - `signed`
  - **Shape:** the base is `INLINE`, `BY_REF` or `SPILL`, then `.view()`, `.select()` and `.signed()` are applied when declared. `SCHEMA` is 0 when `schema` is absent.
  - **Rejected with CID-03:**
    - a missing, duplicate or non-list `#[route]`
    - a missing or non-string id
    - an unknown argument, which the error names
    - an AD-6 runtime argument (`ttl`, `one_shot`, `owner`, `permissions`, `guard`, `opens_modal`). Message: `` [CID-03] `<arg>` is a runtime argument of #[route] that this Noctis release does not support yet ``.
    - a duplicate argument
    - a value on a flag
    - `overflow` with any value other than `spill`. For `overflow = error`, the help says the plain inline Route is the error policy.
    - a `schema` that is not an int literal or is outside 1..=61 (`schema = 0` → help "leave it out for schema 0")
    - `by_ref` combined with `overflow = spill`
    - `by_ref` combined with `signed`
- **Structure rules (CID-01):**
  - All three derives reject any generics (type, lifetime, const or `where`).
  - `Route`/`Payload` accept structs only (named, tuple or unit); enums and unions get an error.
  - `PayloadEnum` accepts enums only, with 1..=62 unit variants. The error spans the 63rd variant or the first non-unit variant. Explicit discriminants are allowed and ignored.
- **`#[derive(Route)]` output:**
  1. `impl Route for T`: `ID = RouteId::new("<id>")`, `SHAPE`, `SCHEMA`, `type Payload = Self`.
  2. The `Payload` and `PayloadDecode` impls, exactly as `#[derive(Payload)]` emits them.
  3. `impl DeclaredRuntime for T { const RUNTIME: RouteRuntime = RouteRuntime::EMPTY; }`.
  4. `const _: () = ::noctis::__private::codec::assert_fits::<T>();`, `quote_spanned!` at the struct ident.
  5. `#[cfg(test)] #[test] #[allow(non_snake_case, clippy::print_stdout)] fn __noctis_route_width_<Ident>()`, which prints `route_width::<T>()` and asserts `fits()`.
- **`#[derive(Payload)]` output:**
  - `MAX_ENCODED_LEN = sum_fields(&[<F as Field>::MAX_LEN, …])`, with fields in declaration order.
  - `encode_fields` calls `out.field(&self.f)?` per field and `Ok(())` at the end.
  - `impl<'de> PayloadDecode<'de>` builds `Self{f: f.field()?,…}`, `Self(f.field()?,…)` or `Self`.
  - A zero-field struct encodes `""` and gives `Some(0)`.
- **`#[derive(PayloadEnum)]` output:**
  - `Field` with `MAX_LEN = Some(1)`; `encode` writes the declaration index as one base62 digit.
  - `impl<'de> FieldDecode<'de>` matches the result of `decode_variant` to each variant and falls back to `Err(FieldError::Overflow)` (never panics).
  - `absent()` stays at its default.
- **Lint-clean output:** generated items carry `#[automatically_derived]` where valid, and generated code must be warning-free under the workspace lints (`clippy::pedantic`, `unused_qualifications`, `missing_docs`) in crates that use the derives (the clippy gate covers `crates/noctis/tests`).
- **Facade (`crates/noctis`):**
  - **Dependencies:** `noctis-macros` (normal); dev-dependencies `twilight-model` and `proptest`.
  - **`__private`:** `#[doc(hidden)] pub mod __private { pub use noctis_codec as codec; }`.
  - **`pub mod prelude`:** `pub use noctis_codec::{Payload, Route}; pub use noctis_macros::{Payload, PayloadEnum, Route};`.
  - **`noctis::codec`:** additionally re-exports `Route, RouteId, RouteShape, BodyKind, Payload, PayloadDecode, FieldWriter, FieldReader, CodecError, Section`.
  - **Docs:** a crate-doc section and a doctest deriving `BuyButton`.
- **Template, skill and snapshot:**
  - Bless `xtask/public-api.txt`.
  - The skill must mention every new item and carry the new API hash.
  - Remove `CID-01` and `CID-03` from `xtask/rules-pending.toml`.
  - Every test that cites a rule has `cid_01`/`cid_03` in its name.

**Never:**
- No runtime arguments, no `#[handler]`, no `Handled`, no component builders, no `#[noctis(crate = …)]` and no `Id` re-export (Epics 4 and 7).
- No change to the `encode`/`decode` behaviour, the wire format or the `Field` widths.
- No `const { assert!() }` inside generic code as an enforcement point.
- No type-name inspection in macros.
- No `unwrap`/`expect` outside tests, no `unsafe`.
- No new crates beyond those listed; no CI workflow change (CI is paused and no new gate is added).
- Never weaken `.cargo/config.toml`.

## I/O & Edge-Case Matrix

**`noctis_rules::cid::validate_route_id`** (rule column = violations in order):

| Input | Result |
|---|---|
| `"shop.buy"`, `"a"`, 24×`a`, `"a.b-c_9"`, `"0"` | `[]` |
| `""` | `[CID-03 Between{1,24} Count(0)]` |
| 25×`a` | `[CID-03 Between{1,24} Count(25)]` |
| `"Shop"`, `"a b"`, `"a:b"`, `"a,b"`, `"a~b"`, `"a!b"`, `"é"` | `[CID-03 Describe Text]`, note names `S`@0, ` `@1, `:`@1, `,`@1, `~`@1, `!`@1, `é`@0 |
| `"_x"`, `"_"` | `[CID-03 reserved]` |
| 25×`A` | length then bad char (2 violations) |
| `"_X"` | bad char then reserved (2) |
| `is_route_id_byte` for `a z 0 9 . _ -` / `A : , ~ ! space 0x80` | `true` / `false` |

**`sum_fields`:** `[]`→`Some(0)`; `[Some(1)]`→`Some(1)`; `[Some(11),Some(2)]`→`Some(14)`; `[Some(1);3]`→`Some(5)`; `[Some(1),None]`, `[None]`→`None`; `[Some(usize::MAX),Some(1)]`→`Some(usize::MAX)`.

**Derived Routes** (`route_width` total; `Display`; encode/decode via `__private::codec`):

| Declaration | Result |
|---|---|
| `#[route("shop.buy")] struct BuyButton{item: Id<ItemMarker>, qty: u8}` | ID `shop.buy`, `SHAPE == INLINE`, `SCHEMA 0`, `MAX_ENCODED_LEN Some(14)`, total 24; Display `shop.buy: F 1 + route 8 + sep 1 + S 0 + E 0 + N 0 + body 14 + T 0 = 24 (limit 100)`; `{255,3}` encodes `"0shop.buy:47,3"` and `decode_route` gives it back |
| `#[route("ping")] struct Ping;` | `Some(0)`, total 6, encodes `"0ping:"`, round-trips |
| `#[route("t")] struct T2(u8, bool);` | `Some(4)`, `T2(7,true)` → `"0t:7,1"` |
| `#[route("a", view)]` / `select` / `signed` / `schema = 3` | `INLINE.view()` / `.select()` / `.signed()` / SCHEMA 3; width +5 / +2 / +6 / +1 |
| `#[route("a", signed, schema = 61, select, view, overflow = spill)]` | `SPILL.view().select().signed()`, SCHEMA 61 (argument order irrelevant) |
| `#[route("wizard.step", by_ref)] {s: String}` | `BY_REF`, body `None`, total 25; Display `wizard.step: F 1 + route 11 + sep 1 + S 0 + E 0 + N 0 + ref 12 = 25 (limit 100)` |
| 24-char id, `by_ref, view, select, schema = 61` | total 46, compiles |
| `#[route("s", overflow = spill)] {s: String}` | total 100; Display `s: F 1 + route 1 + sep 1 + S 0 + E 0 + N 0 + body unbounded + T 0 inline, else ref 12 = 100 (limit 100)`; `encode_id` of `"x"` → `Inline("0s:x")`, of 98×`x` → `RefSlot` |
| `#[route("ss", overflow = spill, signed)] {n: u8}` | total 16 |
| `#[route("a")] {t: BoundedStr<47>, n: u8}` | **compiles**, total exactly 100; worst value (47×`!`, 255) encodes to 100 chars (boundary) |
| `#[route("r", schema = 61, view, select, signed)] {t: BoundedStr<40>, n: u8}` | **compiles**, total exactly 100 (boundary) |
| `#[route("a")] {t: BoundedStr<47>, n: u16}` | compile error `` [CID-01] route `a` can exceed 100 characters: worst case 101 `` |
| `#[route("report.reason")] {r: String}` | compile error `` [CID-01] route `report.reason` has unbounded fields `` |
| field type without `Field` (e.g. `f32`) | `[CID-01] `f32` is not a Payload field type` |
| `struct R<T>`, `struct R<'a>`, `struct R<const N: usize>`, `where` clause | `[CID-01]` generic Route rejected (span: generics) |
| `#[derive(Route)]` on enum / union | `[CID-01]` structs only |

**Attribute errors (all CID-03, span on the offending token):** a missing `#[route]`, a second `#[route]`, `#[route]`, `#[route = "a"]`, `#[route()]`, `#[route(a)]`, `#[route(b"a")]`; ids `"Shop"`, `""`, 25 chars, `"_x"`, `"é"`; `colour`, `ttl = "1m"`, `one_shot`, `owner = user`, `permissions = X`, `guard = f`, `opens_modal`; `view, view`, `view = true`, `by_ref()`, `overflow = error`, `overflow = inline`, `overflow`, `schema = 0`, `schema = 62`, `schema = -1`, `schema = "3"`, `schema`, `schema = 3, schema = 4`, `by_ref, overflow = spill`, `by_ref, signed`. Accepted: `"a",` (trailing comma), `r"a"`, `schema = 61u8`, `schema = 1`, `"shop\x2Ebuy"` (value `shop.buy`).

**`#[derive(PayloadEnum)] enum K { A, B, C }`:**

| Input | Result |
|---|---|
| encode A / B / C | `"0"` / `"1"` / `"2"`; `MAX_LEN Some(1)`; `check::worst_case` passes for each |
| decode `"2"` / `"3"` / `""` / `"00"` / `"-"` / `"é"` / `"z"` | `C` / `Overflow` / `Empty` / `TooLong{1,2}` / `InvalidChar{0}` / `TooLong{1,2}` / `Overflow` |
| 62 variants | last encodes `"z"` and decodes back; compiles |
| 63 variants / `A(u8)` / `A{x:u8}` / no variants / generic / on a struct | compile error `[CID-01]` (span: 63rd variant / that variant / enum / generics / item) |
| `enum E { A = 5, B }` | A → `"0"`, B → `"1"` |
| `encode_variant(61)` / `(62)` | `"z"` / `Err(fmt::Error)` |
| `decode_variant("1", 1)` / `("0", 1)` | `Overflow` / `Ok(0)` |

**Hygiene:** a module that defines its own `Option`, `Some`, `None`, `Ok`, `Result`, `Route`, `Payload`, `Field`, `FieldDecode`, `noctis_codec` and `std` (as a module) still compiles all three derives.

</intent-contract>

## Code Map

- `crates/noctis-macros/src/lib.rs` -- empty crate (doc line only), already `proc-macro = true` with workspace lints. Add deps in `Cargo.toml`.
- `crates/noctis-codec/src/route.rs` l.8-14 `MAX_ROUTE_LEN`/`is_route_byte` (used by `encoded.rs` l.20-26, `record.rs` l.11/51, `layout.rs` l.16/583): replace them with `noctis_rules::cid` re-aliases. l.221-229 `Route` (rustdoc example at l.181-220 needs `MAX_ENCODED_LEN`). l.237-245 `Payload`: add the const.
- `crates/noctis-codec/src/layout.rs` l.13, l.198 and `record.rs` l.10/54: `DIGIT_MAX as MAX_SCHEMA`. Add the const assert against `SCHEMA_MAX`. l.21-37: layout constants and `MAX_ID_LEN` used by `RouteWidth`.
- `crates/noctis-codec/src/base62.rs` l.6 `ALPHABET`, l.28 `digit()`: reuse in `encode_variant`/`decode_variant`.
- `crates/noctis-codec/src/field.rs` l.50: `Field` (add `on_unimplemented`). l.156 `Id<T>` works for any marker (user-defined `ItemMarker` is fine).
- `crates/noctis-codec/src/encoded.rs` l.158 `encode_id`: spill → `RefSlot` already (used by spill tests).
- `crates/noctis-codec/tests/common/mod.rs` l.9-35 `payload!` macro, plus the other hand `impl Payload` blocks (5 in tests/src): add `MAX_ENCODED_LEN` via `sum_fields`.
- `crates/noctis-codec/Cargo.toml`: move `noctis-rules` into `[dependencies]`. The edge codec→rules is allowed by `deps-check`; macros→rules is the macros' only allowed internal edge, so the macros never depend on codec.
- `crates/noctis-rules/src/lib.rs` l.30-45: module list and crate docs. Add `pub mod cid`. `rule.rs` `Violation` (`Display` = `[ID] path: limit …, actual …` + `= note`/`= help` lines), `RuleId::CID_03`.
- `crates/noctis/src/lib.rs`: the facade. Currently `codec` and `rules` modules only. `crates/noctis/Cargo.toml`: deps and dev-deps.
- `crates/noctis/tests/ui.rs`: trybuild harness, `compile_fail("tests/ui/**/*.rs")`, pinned toolchain. Bless with `TRYBUILD=overwrite`. `tests/boundary.rs` + `tests/boundary/msg.rs`: the pattern for `boundary/cid.rs`.
- `xtask/src/rules_catalog/mod.rs` l.335-412: fixture rules (`ui/<area>/<ID>_<slug>.rs`, `.stderr` contains `[ID]`). `xtask/rules-pending.toml` l.286-299: the CID-01 and CID-03 entries to delete.
- `xtask/src/api_snapshot.rs`: assembles re-exports by path. Run `cargo run --release -p xtask -- api-snapshot --bless`. Proc-macro re-exports and a trait and derive sharing the path `noctis::prelude::Route` have never been assembled before. If assembly fails or drops items, fix `api_snapshot.rs`/`public_api.rs` minimally and add a unit test beside the existing ones (l.700-845). Hidden items (`__private`) are absent from rustdoc JSON.
- `examples/project-template/src/modules/{setup,dashboard,profile}.rs`: Route and PayloadEnum declarations. They use runtime arguments (`owner`, `permissions`, `opens_modal`) that 2.5 rejects; the template is tracking-only and does not build yet.
- `examples/project-template/.claude/skills/noctis/{SKILL.md,references/routes-payloads-refs.md,references/troubleshooting.md}`: the API hash at SKILL.md l.21. routes-payloads-refs.md l.95 lists a "`#[derive(Payload)]` struct" as a field type, which is wrong.
- `docs/discord-rules.md` l.360-405: §2 expansion example and the "Const panic messages cannot format values" note, both now outdated. l.505-515: the fixture list example.
- Toolchain: `nightly-2026-10-15`, `cargo-public-api 0.52.0`, `cargo-hack`, `cargo-deny` and `1.91` are installed locally.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/cid.rs`, `lib.rs` -- `cid` module with rustdoc and unit tests `cid_03_route_ids` (every `validate_route_id` row) -- one source for the macro and the codec (AD-3).
- `crates/noctis-codec/{Cargo.toml,src/route.rs,src/layout.rs,src/record.rs,src/encoded.rs,src/parts.rs,src/field.rs}` -- rules constants, `MAX_ENCODED_LEN`, `on_unimplemented`, const asserts; update the doc example -- AD-6.
- `crates/noctis-codec/src/derive.rs` (new), `lib.rs` -- the hidden support items, with unit tests `sum_fields_matrix`, `cid_01_route_width_matrix` (hand Routes for every width row incl. Display strings), `cid_01_assert_fits_panics` (`should_panic` with both message texts), `variant_digits`; add a crate-doc "Derive support" paragraph -- length proof.
- `crates/noctis-codec/tests/common/mod.rs` and every other hand `impl Payload` -- add `MAX_ENCODED_LEN` -- keeps 2.1-2.4 tests green.
- `crates/noctis-macros/{Cargo.toml,src/lib.rs,src/route.rs,src/payload.rs,src/payload_enum.rs}` -- the three derives. Attribute parsing and validation are pure functions over `syn` types, with unit tests `cid_03_route_attribute_matrix` (every attribute row) and `cid_01_structure_errors` -- AD-6, AD-2.
- `crates/noctis/{Cargo.toml,src/lib.rs}` -- deps, `__private`, `prelude`, `codec` re-exports, crate docs and a `BuyButton` doctest -- AD-2.
- `crates/noctis/tests/derive.rs` (new) -- `cid_01_buy_button` (first AC), `cid_01_codec_argument_matrix`, `cid_01_spill_inline_or_ref`, `payload_enum_matrix`, `derive_payload_standalone` (a hand `Route` with `type Payload = DerivedPayload`), and a proptest `cid_01_derived_round_trip` over `{a: u8, b: i64, c: Option<u64>, d: BoundedStr<4>, e: bool, k: K}` (encode → `decode_route` equal, len ≤ `route_width().total()`) -- FR-16.
- `crates/noctis/tests/derive_hygiene.rs` (new) -- the hygiene row -- AD-2.
- `crates/noctis/tests/boundary.rs`, `tests/boundary/cid.rs` (new) -- `cid_01_exactly_100_inline`, `cid_01_exactly_100_all_sections`, `cid_01_by_ref_24_char_id_46`, `payload_enum_62_variants` -- FR-76.
- `crates/noctis/tests/ui/cid/*.rs` + blessed `.stderr` -- add these fixtures, each `fn main() {}`, and inspect that every `.stderr` has `[CID-0x]` and a `help:` line (except rustc's `on_unimplemented` one):
  - `CID-01_worst_case_101`
  - `CID-01_unbounded_string`
  - `CID-01_not_a_field_type`
  - `CID-01_generic_route`
  - `CID-01_payload_enum_63_variants`
  - `CID-01_payload_enum_tuple_variant`
  - `CID-03_uppercase_route_id`
  - `CID-03_reserved_route_id`
  - `CID-03_route_id_too_long`
  - `CID-03_missing_route_attribute`
  - `CID-03_unknown_argument`
  - `CID-03_runtime_argument`
  - `CID-03_by_ref_signed`
  - `CID-03_schema_62`
  - `CID-03_overflow_error`
- `xtask/rules-pending.toml` -- delete the CID-01 and CID-03 entries.
- `xtask/public-api.txt` (bless), `xtask/src/{api_snapshot,public_api}.rs` only if needed -- snapshot.
- `examples/project-template/.claude/skills/noctis/`:
  - **`references/routes-payloads-refs.md`:** attribute grammar and the CID-03 error list. Mark the runtime arguments **[planned]** (rejected until their release). Document `#[derive(Payload)]` (standalone Payload for a hand `Route`; not a field type, so remove the table row), `PayloadEnum` (unit variants only, at most 62, declaration index, appending a variant is compatible, reordering or removing bumps `schema`, unknown index = `FieldError::Overflow`), the width test name `noctis_route_width` and the `Display` format.
  - **Mentions:** every new snapshot item (traits, consts, methods, variants and fields of `CodecError`/`Section`/`BodyKind`).
  - **`troubleshooting.md`:** the exact CID-01 and CID-03 texts.
  - **`SKILL.md`:** the API hash.
- `examples/project-template/src/modules/{setup,dashboard,profile}.rs`, `AGENTS.md` -- add the derive's width line as a doc comment on each Route (e.g. `` dash.mod: F 1 + route 8 + sep 1 + S 0 + E 0 + N 0 + body 15 + T 0 = 25 (limit 100) ``, `` setup: … + E 5 + … + body 37 + T 0 = 49 ``), and keep the runtime arguments with a `// [planned]` note. Add an AGENTS.md convention bullet on `#[route]` codec arguments, `PayloadEnum` and the width test -- AGENTS.md policy.
- `docs/discord-rules.md` -- update the §2 expansion (`type Payload = Self`, `MAX_ENCODED_LEN`) and the note (the message now carries the route id and worst case); fixture example names -- catalog accuracy.

**Acceptance Criteria:**
- **BuyButton derive:** Given `#[derive(Route)] #[route("shop.buy")] struct BuyButton { item: Id<ItemMarker>, qty: u8 }`, when `cargo test --release -p noctis` runs, then:
  - it implements `Route` and `Payload` with the matrix values;
  - its generated `__noctis_route_width_BuyButton` test passes;
  - `cargo test --release -p noctis -- --nocapture noctis_route_width` prints the breakdown line.
- **Trybuild suite:** Given the `tests/ui/cid/` fixtures, when `cargo test --release -p noctis --test ui` runs, then each fails to compile with a blessed `.stderr` containing its `[CID-0x]`, and the exactly-100 boundary tests compile and pass.
- **Facade and gates:** Given the facade, when the gates below run, then:
  - `use noctis::prelude::*;` brings `Route`, `Payload` and `PayloadEnum` into scope;
  - `api-snapshot` (after the bless), `skill-check`, `rules-catalog` (CID-01 and CID-03 covered, pending count down by 2), `deps-check` and `cargo deny check` pass.

## Spec Change Log

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 35 findings — high 0, medium 7, low 21, false 7, maybe-false 0
- findings:
  - `[false]` `[reject]` Blind: template Routes keep `permissions`/`owner`/`opens_modal`, which the derive rejects — the template is excluded from the workspace (`Cargo.toml` `exclude`), every module file is headed "written against the planned Noctis API", and the Design Notes deliberately keep runtime arguments as `[planned]`; nothing that is supposed to build breaks.
  - `[low]` `[patch]` Blind: no public encode/decode path, yet the skill says "Today Routes encode and decode" — `noctis::codec` re-exports no `encode`/`decode_route` (only `__private` does), so the skill sentence in `routes-payloads-refs.md` l.38 misleads bot authors' agents; patch: reword the sentence (encoding/decoding is internal, done by the [planned] component builders and dispatcher).
  - `[low]` `[patch]` Blind: hand-written Payloads/Routes get no length proof and the docs never say so — `assert_fits` is emitted only by `#[derive(Route)]` and is doc-hidden; skill §3 shows the hand `PickRoute` without saying it is unproven (grouped with Edge hand-Route row); patch: one sentence in skill §3/§5.
  - `[low]` `[reject]` Blind: spill total overstated when the inline form is shorter than a Ref (`ss` → 16) — real but conservative; the intent-contract matrix row pins total 16 and `total()` formula, so the fix edits this build's spec.
  - `[low]` `[reject]` Blind: generated code hard-codes `::noctis::__private::codec`, breaking a renamed dependency — the intent itself requires `::noctis::__private::<crate>` paths; renaming the dependency is uncommon and the escape hatch adds an argument (Epic 7 per Never list).
  - `[medium]` `[patch]` Blind: generated `__noctis_route_width_<Name>` test is fragile (inner items, `allow` without reason, redundant assert) — reproduced: a `#[derive(Route)]` struct inside a `#[test] fn` fails `#![deny(warnings)]` with `cannot test inner items` (`unnameable_test_items`); the `allow` without reason is not flagged (clippy skips external-macro expansions, checked with `allow_attributes_without_reason` denied); the redundant assert is spec-mandated and harmless; patch: add `unnameable_test_items` to the generated `allow`, plus a function-local Route test.
  - `[low]` `[patch]` Blind: PayloadEnum variant-count error hidden when the 63rd variant has fields — `payload_enum.rs` uses `else if i == PAYLOAD_ENUM_MAX_VARIANTS` after the fields check (grouped with Edge row); patch: make the count check an independent `if`.
  - `[low]` `[reject]` Blind: `schema = 3usize` reported as out of range — real (`parse_schema` maps a non-`u8` suffix to `None`), but a suffixed schema literal is rare and the fix adds a branch.
  - `[medium]` `[patch]` Blind: `FieldDecode` has no `#[diagnostic::on_unimplemented]` — `CID-01_not_a_field_type.stderr` ends with an untagged `f32: FieldDecode<'_>` error, contradicting the intent's "every `.stderr` follows `[<RULE-ID>]`" (grouped with Edge FieldDecode row and Intent D2); patch: annotate `FieldDecode` and re-bless.
  - `[low]` `[reject]` Blind: trybuild misses several documented diagnostics — they are covered by `cid_03_route_attribute_matrix`/`cid_01_structure_errors` unit tests; no wrong span was shown, and adding 8+ fixtures is more than a direct correction.
  - `[low]` `[reject]` Blind: unbounded-fields message does not name the field — the message format is pinned by the spec; per-field const checks add complexity for a rare diagnostic nicety.
  - `[low]` `[reject]` Blind: lifetimes rejected, blocking borrowed fields — the intent contract rejects all generics including lifetimes on purpose; only the help wording ("width must be known") is imprecise, a cosmetic point.
  - `[false]` `[reject]` Blind: `decode_variant("é")` reports `TooLong` — the intent-contract matrix explicitly specifies `"é"` → `TooLong{1,2}` and the check order.
  - `[low]` `[reject]` Blind: xtask proc-macro spelling rewrite has no path boundary — no crate whose name ends in `noctis_macros` appears in the snapshot, so the mis-rewrite is unreachable in practice; fix adds a boundary branch.
  - `[medium]` `[patch]` Edge: generated test on a function-local Route triggers `unnameable_test_items` — reproduced (see Blind row); same patch.
  - `[medium]` `[patch]` Edge: bare `#[test]` resolves to an imported `test` attribute — reproduced: `use std::prelude::v1::derive as test;` in the Route's module makes the derive fail with E0774, so `use tokio::test;` breaks every derived Route in test builds (AD-2 hygiene); patch: emit `#[::core::prelude::v1::test]`.
  - `[low]` `[reject]` Edge: `#![forbid(clippy::print_stdout)]`/`forbid(non_snake_case)` makes the generated `allow` an error — real but rare in bot crates; the fix needs a renamed test and a non-`println!` writer.
  - `[low]` `[reject]` Edge: `#[cfg]` on PayloadEnum variants shifts indexes per feature set — real but rare, same class as reordering variants (documented to bump `schema`); the fix adds a rejection branch.
  - `[low]` `[reject]` Edge: `#[cfg]` on Payload/Route fields changes the layout per build — same reasoning as the variant row.
  - `[low]` `[patch]` Edge: hand Route with `#[derive(Payload)]` gets no CID-01 proof — grouped with the Blind hand-Payload row; doc sentence patch.
  - `[medium]` `[patch]` Edge: `FieldDecode` lacks `on_unimplemented` — grouped with Blind `FieldDecode` row.
  - `[low]` `[reject]` Edge: renamed `noctis` dependency breaks every expansion — same as Blind crate-path row.
  - `[low]` `[reject]` Edge: suffixed route id literal `"a"x` accepted — real (`parse_id` ignores the suffix) but nobody writes it; fix adds a guard.
  - `[low]` `[reject]` Edge: `schema = 3usize` misleading error — same as Blind schema row.
  - `[low]` `[patch]` Edge: 63rd variant with fields hides the count error — grouped with Blind PayloadEnum row.
  - `[low]` `[reject]` Edge: xtask substring rewrite — same as Blind xtask row.
  - `[low]` `[patch]` Vgap: nothing checks that `#[derive(Route)]` emits `__noctis_route_width_<Name>` (pre-verified, disposition patch) — patch: a `route.rs` unit test asserting the expansion contains `#[cfg(test)]`, the test attribute and `fn __noctis_route_width_R`.
  - `[medium]` `[patch]` Vgap other: width test not collected for function-local Routes — reproduced through the derive: it is a hard error under `-D warnings`; grouped with the Blind generated-test row.
  - `[false]` `[reject]` Intent D1: runtime arguments rejected instead of accepted — the spec deliberately and correctly settled it (Design Notes: ignoring `owner`/`permissions` would silently drop a security guard); the intent's "not yet in AD-6's grammar" supports "not yet in this release".
  - `[medium]` `[patch]` Intent D2: `.stderr` format not met by `CID-01_not_a_field_type.stderr` — grouped with Blind `FieldDecode` row; the embedded `= help:` text is the spec's chosen format.
  - `[false]` `[reject]` Intent D3: template not reconciled to compilable syntax — same refutation as Blind template row (planned-API template, excluded from workspace).
  - `[low]` `[patch]` Intent D4: no test checks that the generated width test exists — grouped with the Vgap row (the inner-item half is the medium generated-test group).
  - `[false]` `[reject]` Intent D5: spill/encode verified only through `__private` — bot authors never call `encode` directly (component builders do, Epic 4); the intent-contract matrix names `__private::codec` as the test surface on purpose.
  - `[false]` `[reject]` Intent D6: by_ref/spill proof covers only prefix + Ref — the runtime `encode_id` picks the Ref form whenever the inline form exceeds 100, so no derived id can exceed 100; the spec settled it.
  - `[false]` `[reject]` Intent D7: alias `__private::codec` vs `__private::noctis_codec` — functionally identical; the spec pins the alias, and rustc printing the real path harms nobody.

## Design Notes

Decisions (also in the memlog):
- **Rule ids.** The length proof and the item structure (unbounded, over 100, generics, item kind, `PayloadEnum` shape, non-field type) cite CID-01. The `#[route]` declaration (id grammar, argument grammar and conflicts, including `by_ref`+`signed`) cites CID-03, the "Route format" row validated in the derive. CID-05 stays covered by its 2.3 tests. Its compile-time part is the fixed `REF_LEN` that `RouteWidth` uses.
- **Runtime arguments are rejected, not ignored.** Silently ignoring `owner`/`permissions` would drop a security guard. Each argument enters with its story (AD-6), and the template keeps them as `[planned]`.
- **Spill total.** A spill id never exceeds 100: its inline form is used only when it fits. `total()` is therefore capped at 100, and the proof only needs `prefix + 12 ≤ 100`.
- **Formatted const panic.** Building the message in const code puts the route id and the worst case into the error, as FR-75 wants. This supersedes the catalog note that said it was impossible. `core::str::from_utf8` has been const since Rust 1.87, below the MSRV of 1.91.
- **`RouteRuntime` lives in codec for now** as a hidden empty struct plus the `DeclaredRuntime` trait. Stories 7.5/7.7/7.8 may move it to core, since its later fields need core types. Its paths are private macro output.
- **Shared rule constants.** Route-grammar and cap constants move to `noctis_rules::cid` because macros may depend only on rules (AD-3: "never copy a constant"). The codec const-asserts its own copies against them.
- **`#[derive(Payload)]`** is the Payload half of `#[derive(Route)]`, for a hand-written `Route`. Deriving both on one struct is a conflicting-impl error.
- **PayloadEnum decode errors.** An unknown variant index reuses `FieldError::Overflow` (value out of range), so no new error variant is added.

```rust
#[derive(Route, Debug, PartialEq)]
#[route("shop.buy")]
struct BuyButton { item: Id<ItemMarker>, qty: u8 }
// impl Route (ID, SHAPE = INLINE, SCHEMA = 0, Payload = Self), impl Payload (MAX_ENCODED_LEN = Some(14)),
// impl PayloadDecode, impl DeclaredRuntime, const _: () = assert_fits::<BuyButton>(), #[test] width print
```

## Verification

**Commands** (repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `TRYBUILD=overwrite cargo test --release -p noctis --test ui`, then review every `tests/ui/cid/*.stderr` -- expected: each contains its `[CID-0x]`; `help:` present where the macro or `assert_fits` emits it.
- `cargo run --release -p xtask -- api-snapshot --bless`, then update the SKILL.md hash and mentions.
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (incl. ui, boundary, derive and proptests, doctests).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- rules-catalog` -- expected: `ok`, CID-01 and CID-03 covered.
- `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0.
- `cargo run --release -p xtask -- skill-check` -- expected: exit 0.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.

## Auto Run Result

- **Summary:** Added `#[derive(Route)]`, `#[derive(Payload)]` and `#[derive(PayloadEnum)]` in `noctis-macros`, with a compile-time length proof (`assert_fits`, formatted const panic naming route id and worst case, CID-01) and `#[route("id", schema = N, by_ref | signed)]` grammar validation (CID-03); runtime arguments (`owner`, `permissions`, `opens_modal`) are rejected until their stories land; derived Routes emit a `#[cfg(test)]` width test.
- **Files changed:**
  - `crates/noctis-macros/src/{lib,common,route,payload,payload_enum}.rs` - derive implementations and diagnostics.
  - `crates/noctis-codec/src/{derive,field,lib,route,...}.rs` - `RouteWidth`, `assert_fits`, `RouteRuntime`/`DeclaredRuntime`, `FieldDecode` `on_unimplemented`.
  - `crates/noctis-rules/src/cid.rs` - route-grammar and cap constants moved to `noctis_rules::cid`.
  - `crates/noctis/src/lib.rs`, `crates/noctis/tests/{derive.rs,derive_hygiene.rs,boundary/cid.rs,ui/cid/*}` - derive, hygiene, proptest and trybuild compile_fail suites.
  - `docs/discord-rules.md`, `xtask/{rules-pending.toml,public-api.txt,src/*}` - rule coverage, API snapshot, xtask updates.
  - `examples/project-template/` - noctis skill (routes-payloads-refs, troubleshooting, SKILL.md), AGENTS.md, module syntax.
- **Review findings:** 35 total (high 0, medium 7, low 21, false 7).
  - Patched (5 groups; mediums patched: 7 entries, highs: 0): generated width test now allows `unnameable_test_items` and uses `#[::core::prelude::v1::test]` (fixes function-local Routes under `-D warnings` and imported `test` attributes); `FieldDecode` gets `#[diagnostic::on_unimplemented]` tagged CID-01 and `.stderr` re-blessed; PayloadEnum variant-count check made independent of the fields check; route.rs unit test asserts the width test is emitted; skill reworded (encode/decode is internal; hand-written Payloads/Routes get no length proof).
  - Deferred: none.
  - Rejected: template keeps planned runtime args (template is planned-API, excluded from workspace); spill total conservative for short inline (pinned by spec); hard-coded `::noctis::__private` path / renamed dependency (intent-mandated); `schema = 3usize` misleading error (rare); missing extra trybuild fixtures (unit-test covered); unbounded-field message not naming field (pinned format); lifetimes rejected (intent contract); `"é"` -> TooLong (spec matrix); xtask substring rewrite (unreachable); `forbid(...)` vs generated allow (rare); `#[cfg]` on variants/fields (rare, same class as reordering); suffixed id literal (nobody writes it); D1 runtime args rejected (spec decision); D5 `__private` test surface (intended); D6 by_ref proof (runtime picks Ref above 100); D7 alias naming (equivalent).
- **Follow-up review recommended: true** - first pass patched 2+ medium groups (0 high). Unverified risk: the changed generated test attribute (`#[::core::prelude::v1::test]` + `allow(unnameable_test_items)`) inside user crates with unusual lint/prelude setups, and the new `on_unimplemented` text on `FieldDecode`, have only been checked by the added hygiene test and one re-blessed `.stderr`.
- **Verification:** local gates via `gates.sh` all green (fmt, clippy `--release -D warnings`, `cargo test --release --workspace` incl. ui/trybuild, derive, hygiene and proptests, cargo-hack each-feature, xtask rules-catalog/api-snapshot/skill-check/deps-check, cargo deny, MSRV 1.91 check, build-config check, no `target/debug`). CI paused by maintainer, so no CI run.
- **Residual risks:** renamed `noctis` dependency breaks expansions; `#[cfg]` on fields/variants changes layout per build without a schema bump; spill total overstated for very short inline forms; `RouteRuntime` may move to core in Stories 7.5/7.7/7.8.
