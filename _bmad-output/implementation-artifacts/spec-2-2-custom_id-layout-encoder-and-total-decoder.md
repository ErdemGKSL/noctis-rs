---
title: 'Story 2.2: `custom_id` layout encoder and total decoder'
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

**Problem:** `noctis-codec` can encode single Payload fields (Story 2.1) but nothing builds or parses a whole `custom_id`. There is no `Route`/`Payload` contract, no layout constants, and no decoder that turns malformed, stale or mismatched ids into typed outcomes (FR-16, FR-19 decoding, FR-60; AD-5, AD-13, AD-19; CID-01, CID-03, CID-06, CID-07).

**Approach:** Add the AD-5 layout to `noctis-codec`:
- the 8 `pub const` layout constants;
- hand-implementable `Route`/`Payload`/`PayloadDecode` traits, which Story 2.5 will derive;
- an `encode::<R>` that writes inline ids into `ArrayString<100>`;
- a total, route-agnostic `decode(&str) -> Result<Decoded, CodecError>`, plus `Decoded::payload::<R>()`, which classifies the result as `Malformed`, `Stale` or `Forged`;
- trailing-`Option` compatibility through a new provided method, `FieldDecode::absent`.

The work also removes the `custom-id-constants` and CID-06 pending entries.

## Boundaries & Constraints

**Always:**
- **Layout:** `F route ":" [S] [E] [N] body [T]`. `F` = base62 digit of `flags` (generation 0). The flags are `S`=1, `E`=2, `N`=4 and `T`=8, so `F` ∈ `0-9A-F`.
  - `S` is one base62 digit, 1..=61, written only when schema > 0.
  - `E` is a `ViewEnvelope`: action (`_` or `[0-9A-Za-z]`), tab digit, page as 2 fixed digits (`hi = page / 62`), state-version digit.
  - `N` is a `SelectNonce` as 2 fixed digits.
  - The body is either inline fields joined by `,`, or `~` + 11 base62 characters.
  - `T` is a `Tag` of 6 fixed base62 digits, covering every preceding character.
- **Constants:** defined once each, one per line, exactly as `pub const FMT_LEN: usize = 1;` (and likewise `SEP_LEN` 1, `SCHEMA_LEN` 1, `VIEW_ENV_LEN` 5, `NONCE_LEN` 2, `REF_MARKER_LEN` 1, `REF_LEN` 11, `TAG_LEN` 6). Also add `pub const MAX_ID_LEN: usize = 100;` (CID-01). `xtask rules-catalog` parses these lines.
- **Phase 1:** `decode(raw)` is total and runs its checks in this order. The first failure wins.
  1. `Empty`
  2. `TooLong{len}` when the byte length is over 100
  3. `NotAscii{index}`
  4. `F`: not base62 → `InvalidChar{0}`; value ≥ 16 → `Generation{generation: v/16}`
  5. Route: scan `[a-z0-9._-]` from byte 1 up to `end`:
     - the byte at `end` exists and is not `:` → `InvalidChar{end}`
     - `end == len` → `MissingSeparator`
     - empty route → `EmptyRoute`
     - more than 24 characters → `RouteTooLong{len}`
  6. `S`: missing → `Truncated{Schema}`; `0` → `ZeroSchema`; not base62 → `InvalidChar`
  7. `E`: fewer than 5 bytes left → `Truncated{Envelope}`; a bad byte → `InvalidChar`
  8. `N`: same as `E`, with `Truncated{Nonce}`
  9. `T`: fewer than 6 bytes left → `Truncated{Tag}`; the last 6 bytes must be base62
  10. Body:
      - leading `~`: a `T` flag → `TagOnRef`; then the length after `~` must be 11 (`RefLength{len}`) and every character base62
      - otherwise every byte must be in `0x20..=0x7E` and not `:` or `~` (`InvalidChar{index}`)

  Every index is a byte index into `raw`. Every slice uses `.get()`; the module carries `#![deny(clippy::indexing_slicing, clippy::string_slice)]`.
- **Phase 1 is injective:** for `Ok(d)`, the parts rebuild `raw` exactly.
- **Phase 2:** `Decoded::payload::<R>()` runs its checks in this order:
  1. Route ≠ `R::ID` → `Forged(RouteMismatch)`.
  2. Schema ≠ `R::SCHEMA` → `Stale(Schema{expected, found})`.
  3. Body kind not allowed by `R::SHAPE` → `Stale(Body{expected})`. `Spill` allows both kinds.
  4. E/N/T flags ≠ expected → `Stale(Flags{expected, found})`. Expected T means `signed` and an inline body.
  5. The inline body goes to `R::Payload::decode_fields` → `Malformed(..)`. A Ref body → `Ok(PayloadSource::Ref(&str))`.

  The tag is not verified here; that is Story 2.4.
- **`FieldReader`:**
  - Fields are split on `,`.
  - When the fields run out, it returns `F::absent()`, or `MissingField{field}` if that is `None`.
  - An empty body on the first read tries `decode("")` first and falls back to `absent()`.
  - `finish` (called by the codec) fails with `ExtraField{field}` if input is left over. A zero-field Payload accepts body `""`.
  - Field errors become `Field{field, error}`; the `error` indices are relative to that field.
- **Encoder errors**, checked in this order:
  1. `InvalidSchema{schema}` when the schema is over 61
  2. `MissingEnvelope`/`UnexpectedEnvelope`
  3. `MissingNonce`/`UnexpectedNonce`
  4. `MissingSigner`/`UnexpectedSigner`
  5. `NeedsRef` for `BY_REF`
  6. Writing: when the buffer would overflow (including the 6 tag characters), `Inline` gives `TooLong` and `Spill` gives `NeedsRef`.

  The signer receives exactly the characters before `T`.
- **Errors:** every error type is a `thiserror` enum marked `#[non_exhaustive]`, deriving `Debug, Clone, Copy, PartialEq, Eq`, with lower-case messages. Public items have rustdoc. There is no `unsafe`, and no `unwrap`/`expect` outside tests.

**Never:**
- No `EncodedId`, `PendingRef`, `finish`, `RefRecord`, Ref minting, `getrandom` (2.3); no `PayloadKey`, `blake3`, tag verification (2.4); no derive, `Payload::MAX_ENCODED_LEN`, `assert_fits`, length `const fn`s (2.5); no dhat (2.6).
- No facade re-export of the layout API, no `__private` module. The only facade-visible change is `FieldDecode::absent`.
- No test name containing `cid_04` or `cid_05`, and no `// [CID-xx]` doctest tags.
- No change to `docs/discord-rules.md` or `noctis-rules` (no limit changes).

## I/O & Edge-Case Matrix

**Fixtures** (in the tests; `R: Route` markers):

| Fixture | Route id | Shape | Schema | Payload |
|---|---|---|---|---|
| `Buy` | `shop.buy` | INLINE | 0 | `BuyV1{item:u64, qty:u8}` |
| `BuyV2` | `shop.buy` | INLINE | 0 | `{item, qty, note:Option<u8>, gift:Option<bool>}` |
| `BuyS2` | `shop.buy` | INLINE | 2 | `BuyV1` |
| `Text` | `t` | INLINE | 0 | `{s:String}` |
| `TextSpill` | `t` | SPILL | 0 | `{s:String}` |
| `TextSigned` | `t` | INLINE.signed() | 0 | `{s:String}` |
| `Ping` | `ping` | INLINE | 0 | zero fields |
| `Opt` | `o` | INLINE | 0 | `{v:Option<u8>}` |
| `View` | `v` | INLINE.view() | 0 | `{n:u8}` |
| `Sel` | `s` | INLINE.select() | 0 | `{n:u8}` |
| `Sig` | `sig` | INLINE.signed() | 0 | `{n:u8}` |
| `All` | `all` | INLINE.view().select().signed() | 61 | `{n:u8}` |
| `Stored` | `r` | BY_REF | 0 | `BuyV1` |
| `SpillSigned` | `ss` | SPILL.signed() | 0 | `{s:String}` |
| `BadSchema` | `x` | INLINE | 62 | `{n:u8}` |

`FakeSigner(u64)` returns `Tag::from_u64(n)` and records the input it was given.

Encode (`encode::<R>(&p, &parts)`):

| Scenario | Input | Output |
|---|---|---|
| plain | `Buy{255,3}` | `"0shop.buy:47,3"` |
| schema | `BuyS2{255,3}` | `"1shop.buy:247,3"` |
| view | `View{1}`, env `('t',1,63,2)` | `"2v:t11121"` |
| select | `Sel{1}`, nonce 3843 | `"4s:zz1"` |
| signed | `Sig{1}`, `FakeSigner(0)` | `"8sig:1000000"`; signer saw `"8sig:1"` |
| all flags | `All{0}`, env `('_',61,3843,61)`, nonce 0, signer 1 | `"Fall:z_zzzz000000001"`; signer saw `"Fall:z_zzzz000"` |
| zero fields / empty / escaped / unicode | `Ping`; `Text{""}`; `Text{"a,b"}`; `Text{"é"}` | `"0ping:"`; `"0t:"`; `"0t:a!1b"`; `"0t:!u0000e9"` |
| exactly 100 / 101 | `Text{97×'x'}` / `Text{98×'x'}` | len 100 `Ok` / `Err(TooLong)` |
| spill fits / overflows | `TextSpill{97×'x'}` / `{98×'x'}` | `"0t:xxx…"` / `Err(NeedsRef)` |
| signed boundary | `TextSigned{91×'x'}` / `{92×'x'}` | len 100 `Ok` / `Err(TooLong)` |
| by ref | `Stored{1,1}` | `Err(NeedsRef)` |
| bad schema | `BadSchema{1}` | `Err(InvalidSchema{schema:62})` |
| parts mismatch | `View` without env / `Buy` + env / `Sel` without nonce / `Buy` + nonce / `Sig` without signer / `Buy` + signer | `MissingEnvelope` / `UnexpectedEnvelope` / `MissingNonce` / `UnexpectedNonce` / `MissingSigner` / `UnexpectedSigner` |

Decode, phase 1 (`decode(raw)`):

| Input | Result |
|---|---|
| `""` | `Empty` |
| 101×`a`; `"0"`+50×`é` (101 bytes) | `TooLong{len:101}` |
| `"0é:1"`; `"Gé"` | `NotAscii{index:1}` |
| `"!x:1"` | `InvalidChar{index:0}` |
| `"Ga:"`; `"za:"` | `Generation{generation:1}`; `Generation{generation:3}` |
| `"0"`; `"0abc"` | `MissingSeparator` |
| `"0:1"` | `EmptyRoute` |
| `"0Shop:1"`; `"0a b:1"`; `"0a,b:1"` | `InvalidChar{index:1}`; `{index:2}`; `{index:2}` |
| route of 24 chars; of 25 chars | `Ok`; `RouteTooLong{len:25}` |
| `"0_x:1"`; `"0a.b-c_9:"` | `Ok` (route `_x`); `Ok` (route `a.b-c_9`, empty inline body) |
| `"1a:"`; `"1a:0"`; `"1a:!"` | `Truncated{Schema}`; `ZeroSchema`; `InvalidChar{index:3}` |
| `"2a:_000"`; `"2a:!0000"`; `"2a:__000"`; `"2a:_0!00"` | `Truncated{Envelope}`; `InvalidChar{3}`; `{4}`; `{5}` |
| `"6a:_0000z"`; `"4a:!0"` | `Truncated{Nonce}`; `InvalidChar{3}` |
| `"8a:12345"`; `"8a:~12"`; `"8a:1234!6"` | `Truncated{Tag}`; `Truncated{Tag}`; `InvalidChar{index:7}` |
| `"8a:000000"` | `Ok`: body `Inline("")`, tag `"000000"`, `signed_part` `"8a:"` |
| `"8a:~0123456789A000000"` | `TagOnRef` |
| `"0a:~"`; `"0a:~123"`; `"0a:~0123456789AB"` | `RefLength{len:0}`; `{len:3}`; `{len:12}` |
| `"0a:~0123456789!"` | `InvalidChar{index:14}` |
| `"0a:~0123456789A"` | `Ok`: body `Ref("0123456789A")` |
| `"0a:1~2"`; `"0a:1:2"`; `"0a:\t"`; `"0a:\u{7f}"` | `InvalidChar{4}`; `{4}`; `{3}`; `{3}` |
| `"Fall:z_zzzz000000001"` | `Ok`: flags 15, schema 61, env `('_',61,3843,61)`, nonce 0, body `"0"`, tag `"000001"` |
| valid 100-char id; same plus 1 body char | `Ok`; `TooLong{len:101}` |
| arbitrary `&str` (proptest) | never panics; `Ok` ⇒ len ≤ 100, ASCII, rebuilt from parts == input |

Decode, phase 2 (`decode(raw)?.payload::<R>()`; `decode_route::<R>(raw)` is the same thing in one call):

| R | Input | Result |
|---|---|---|
| Buy | `"0shop.buy:47,3"` | `Inline(BuyV1{255,3})` |
| BuyV2 | `"0shop.buy:47,3"` / `"…:47,3,15"` / `"…:47,3,15,11"` | `{255,3,None,None}` / `{…,Some(5),None}` / `{…,Some(5),Some(true)}` |
| Buy | `"0shop.buy:47,3,15"` / `"0shop.buy:47"` | `Malformed(ExtraField{field:2})` / `Malformed(MissingField{field:1})` |
| BuyV2 | `"0shop.buy:47,3,"` | `Malformed(Field{field:2, error:Empty})` (no fallback inside a body) |
| Buy | `"0shop.buy:"` / `"0shop.buy:47,-"` | `Malformed(Field{field:0,error:Empty})` / `Malformed(Field{field:1,error:InvalidChar{index:0}})` |
| Opt | `"0o:"` / `"0o:0"` / `"0o:15"` | `{None}` / `{None}` / `{Some(5)}` |
| Text | `"0t:"` | `{s:""}` |
| Ping | `"0ping:"` / `"0ping:x"` | `Ok` / `Malformed(ExtraField{field:0})` |
| Buy | `"0shop.buz:47,3"`; `"1x:247,3"` | `Forged(RouteMismatch)` (route is checked first) |
| BuyS2 / Buy | `"0shop.buy:47,3"` / `"1shop.buy:247,3"` | `Stale(Schema{expected:2,found:0})` / `Stale(Schema{expected:0,found:2})` |
| Buy | `"3shop.buy:2_000047,3"` | `Stale(Schema{expected:0,found:2})` (schema before flags) |
| Buy / View / Sig | `"2shop.buy:_000047,3"` / `"0v:1"` / `"0sig:1"` | `Stale(Flags{expected:∅,found:E})` / `Stale(Flags{expected:E,found:∅})` / `Stale(Flags{expected:T,found:∅})` |
| Buy / Stored | `"0shop.buy:~0123456789A"` / `"0r:47,3"` | `Stale(Body{expected:Inline})` / `Stale(Body{expected:ByRef})` |
| Stored / TextSpill | `"0r:~0123456789A"` / `"0t:~0123456789A"`, `"0t:ab"` | `Ref("0123456789A")` / `Ref(..)`, `Inline({s:"ab"})` |
| SpillSigned | `"8ss:a000000"` / `"0ss:~0123456789A"` / `"0ss:a"` | `Inline` / `Ref` / `Stale(Flags{expected:T,found:∅})` |
| Sig | `"8sig:1000000"` | `Inline({n:1})` (tag not verified in 2.2) |
| any | phase-1 error | `Malformed(that CodecError)` |
| all fixtures | proptest: `encode` then `decode_route` | `Inline(p)` equal to input `p` |

Value types:

| Call | Result |
|---|---|
| `RouteId::new("a")`, a 24-char id, `"_x"`, `"a.b-c_9"` | `Ok`; `"_x"`'s `is_reserved()` is `true`, the others' `false` |
| `RouteId::new` with `""`, 25 chars, `"Shop"`, `"a:b"`, `"é"` | panics (`#[should_panic]`); in a `const` it fails to compile (`compile_fail` doctest, no rule tag) |
| `RouteShape::BY_REF.signed()` | panics (`#[should_panic]`); `compile_fail` doctest in a `const` |
| `RouteShape::SPILL.signed()`, `INLINE.view().select()` | `Ok`; the getters reflect the flags |
| `ViewEnvelope::new('t',1,63,2)` / `('_',61,3843,61)` | `Display` gives `"t1112"` / `"_zzzz"` |
| `ViewEnvelope::new` with `'-'`, `'é'`; tab 62; page 3844; version 62 | `InvalidAction('-')`, `InvalidAction('é')`; `TabOutOfRange(62)`; `PageOutOfRange(3844)`; `VersionOutOfRange(62)`. Checked in this order. |
| `SelectNonce::new(0)` / `(3843)` / `(3844)` | `"00"` / `"zz"` / `None` |
| `Tag::from_u64(0)`, `1`, `56800235583`, `56800235584`, `u64::MAX` | `"000000"`, `"000001"`, `"zzzzzz"`, `"000000"`, `"16AHYF"` |
| `<Option<u8>>::absent()`, `<Option<Option<u8>>>::absent()`, `<u8>::absent()`, `<String>::absent()` | `Some(None)`, `Some(None)`, `None`, `None` |
| `MAX_ID_LEN` | equals `noctis_rules::Limits::default().custom_id_len` |

</intent-contract>

## Code Map

- `crates/noctis-codec/src/lib.rs` -- crate docs list the module set (`base62`, `bounded_str`, `check`, `error`, `escape`, `field`). Re-exports sit at the root. The crate docs gain a "Layout" section.
- `crates/noctis-codec/src/base62.rs` -- `ALPHABET`, `digit_value` (pub(crate) const fn) and `decode_u64`. Add `pub(crate) const fn digit(v: u8) -> u8` and a fixed-width writer, both reused for E, N and T.
- `crates/noctis-codec/src/field.rs` l.66-75 -- `FieldDecode<'de>`, which gets the provided `fn absent() -> Option<Self> { None }`. l.175-186 -- the `Option<T>` impl, which overrides it to `Some(None)`. `Field::encode` takes `W: Write + ?Sized`, so `&mut dyn fmt::Write` works.
- `crates/noctis-codec/src/error.rs` -- `FieldError`; copy its style (doc per variant, `#[non_exhaustive]`, `Copy`) for the new errors.
- `crates/noctis-codec/Cargo.toml` -- add `arrayvec.workspace = true`. The workspace pins `arrayvec = "0.7"` and `Cargo.lock` has 0.7.8 (verified on crates.io: MIT OR Apache-2.0, MSRV 1.51). Add `[dev-dependencies] noctis-rules.workspace = true`; the codec→rules edge is allowed in `xtask/src/deps_check.rs` l.63.
- `xtask/src/rules_catalog/mod.rs` l.555-625 -- scans `crates/noctis-codec/src` for `pub const NAME: <int-type> = <int>;` lines. Each catalog name must be defined exactly once (a second definition fails as "defined N times"). l.17-28: unit evidence is a `#[test]` fn in `crates/*/src` or `crates/*/tests` whose name has the snake token `cid_06`; a `#[test]` line inside `proptest!` counts too.
- `xtask/rules-pending.toml` l.16-18 (`[[check]] custom-id-constants`, story 2.2) and l.320-323 (`CID-06` unit, story 2.2) -- delete both. Keep CID-01/03/05.
- `docs/discord-rules.md` l.283-330 -- the normative layout and constant table. Read-only.
- `crates/noctis/src/lib.rs` l.27+ (`pub mod codec`) -- facade docs. Add one sentence on `FieldDecode::absent` (trailing `Option` compatibility). Nothing else is re-exported.
- `xtask/public-api.txt`, `examples/project-template/.claude/skills/noctis/SKILL.md` l.21 (`API hash`), `references/routes-payloads-refs.md` l.122-160 (custom field types) and l.249-260 (§5 Payload evolution), `examples/project-template/AGENTS.md` l.39, `examples/project-template/tests/payload_fields.rs` -- the facade change (`absent`) needs a re-bless, a new hash, the identifier `absent` in the skill, a clause in AGENTS.md and a template test.
- Previous story 2.1 (done): reuse its gate list. Its deferred items (duplicate BoundedStr impl blocks in the snapshot; template test never compiled) are not this story's to fix. Index offsets: `FieldError::offset` exists if you need it.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-codec/Cargo.toml` -- add `arrayvec` and the `noctis-rules` dev-dependency -- `ArrayString<100>`, CID-01 cross-check.
- `crates/noctis-codec/src/base62.rs` -- add `digit(v)` and `write_fixed(n: u64, width, out)` (zero-padded; the caller guarantees range), with unit tests -- shared by E, N and T.
- `crates/noctis-codec/src/field.rs` -- add `FieldDecode::absent` (rustdoc: it is used when trailing fields are missing from an older id; only `Option<T>` overrides it; FR-60) and the `Option` override, with tests -- compatibility without inspecting type names.
- `crates/noctis-codec/src/parts.rs` -- add `ViewEnvelope` (private fields; `new` → `Result<_, EnvelopeError>`; getters `action() -> char`, `tab`, `page`, `state_version`; `Display`), `SelectNonce` (`new(u16) -> Option`, `get`, `MAX`, `Display`), `Tag` (`[u8; 6]`, `const fn from_u64`, `as_str`, `Display`), `Sign` (`fn sign(&self, signed: &str) -> Tag`), and `Flags` (`S`, `E`, `N`, `T` consts; `bits`, `contains`; `Debug` lists the letters) -- the typed sections of AD-5 and AD-20.
- `crates/noctis-codec/src/route.rs` -- add:
  - `RouteId` (`&'static str`; `const fn new` panics on a grammar violation; `as_str`, `is_reserved`, `Display`)
  - `BodyKind {Inline, ByRef, Spill}`
  - `RouteShape` (consts `INLINE`, `BY_REF`, `SPILL`; const builders `view`, `select`, `signed` — the last panics on `ByRef`; getters `body`, `has_view`, `has_select`, `is_signed`)
  - the traits `Route`, `Payload` and `PayloadDecode<'de>`
  - `FieldWriter<'w>` (`field(&impl Field)`; writes `,` before every field but the first) and `FieldReader<'de>` (`field::<F>()` as in Boundaries; `pub(crate)` `new` and `finish`)
  - rustdoc showing a hand implementation
- `crates/noctis-codec/src/layout.rs` -- add:
  - the constants and `MAX_ID_LEN`
  - `Parts<'s>` (`new`, `with_envelope`, `with_nonce`, `with_signer(&'s dyn Sign)`; `Default`, `Copy`)
  - `encode::<R>`
  - `decode`
  - `Decoded<'a>` (getters `flags`, `route`, `schema`, `envelope`, `nonce`, `body() -> RawBody<'a>`, `tag() -> Option<Tag>`, `signed_part`, `as_str`; `payload::<R>()` where `R::Payload: PayloadDecode<'a>`)
  - `RawBody {Inline(&str), Ref(&str)}` and `PayloadSource<'a, P> {Inline(P), Ref(&'a str)}`
  - `decode_route::<R>(raw)`
  - the deny lints from Boundaries
- `crates/noctis-codec/src/error.rs` -- add `CodecError` (variants as named in Boundaries/Matrix, plus `Section {Schema, Envelope, Nonce, Tag}`), `DecodeError {Malformed(CodecError), Stale(StaleReason), Forged(ForgedReason)}` (with `From<CodecError>`), `StaleReason {Schema{expected,found}, Body{expected: BodyKind}, Flags{expected: Flags, found: Flags}}`, `ForgedReason {RouteMismatch}`, `EncodeError` (variants per Matrix), and `EnvelopeError`.
- `crates/noctis-codec/src/lib.rs` -- declare `mod layout; mod parts; mod route;` and re-export every public item at the root. Update the crate docs with the layout, the two-phase decode, the outcome mapping and the trailing-`Option` exception to canonicity.
- `crates/noctis-codec/src/{layout,parts,route}.rs` unit tests -- cover every Matrix row:
  - `cid_06_decode_matrix` (phase 1)
  - `cid_06_payload_outcomes` (phase 2)
  - `#[should_panic]` tests for `RouteId` and `RouteShape`
  - two `compile_fail` doctests (untagged)
  - `max_id_len_matches_rules`
- `crates/noctis-codec/tests/layout.rs` -- add the fixtures, written as hand impls (one `macro_rules!` for the marker routes). Add proptests:
  - `cid_06_decode_never_panics` over `".*"`, `"\\PC{0,130}"` and `"[0-9A-Za-z][a-z0-9._:~!,_ -]{0,110}"`, also calling `payload::<R>()` for several fixtures. For `Ok`, it checks len ≤ 100, ASCII, and that the id rebuilt from the getters (F digit, route, `:`, S digit, envelope/nonce `Display`, `~`+ref or inline, tag) equals the input.
  - `encode_decode_round_trip` for all 16 view/select/signed × schema {0, 61} combinations over `{a:u8, b:i64, c:Option<u64>, d:String}`. Every `Ok` encoding has len ≤ 100 and decodes to the input; strings over budget give `TooLong`.
  - `trailing_option_compat`: a `BuyV1` encoding decodes as `BuyV2` with `None`s.

  Commit any `proptest-regressions/`.
- `xtask/rules-pending.toml` -- delete the `custom-id-constants` check and the CID-06 entry.
- `crates/noctis/src/lib.rs`; `xtask/public-api.txt` (`cargo run --release -p xtask -- api-snapshot --bless`); the skill (`API hash`, plus `absent` in §2 custom fields and §5); the template `AGENTS.md` (extend l.39: only `Option` fields may be appended without a schema bump, since they decode as `None` through `FieldDecode::absent`); `examples/project-template/tests/payload_fields.rs` (`option_field_is_absent_compatible`) -- AGENTS.md public-API policy.

**Acceptance Criteria:**
- Given any Route with a combination of S/E/N/T, when `encode` runs, then `F` is the base62 digit of the flag bits (generation 0), only the sections whose flags are set appear, and the result is an `ArrayString<100>`.
- Given the 8 layout constants are now defined in `noctis-codec` and the pending check is removed, when `cargo run --release -p xtask -- rules-catalog` runs, then it reports ok and compares the catalog table with the constants (a wrong value would fail it).
- Given arbitrary strings (non-ASCII, empty, 101+ characters, `F >= 16`), when `decode` and `payload::<R>()` run under proptest, then they return `Ok` or a typed error and never panic (CID-06; the CID-06 pending entry is removed and rules-catalog passes).
- Given an id whose flags or schema do not match the Route, when it is decoded against that Route, then the result is `DecodeError::Stale(_)`, which is distinct from `Malformed` and `Forged`.
- Given a Payload that gains trailing `Option` fields, when an id encoded by the earlier version is decoded, then the new fields are `None` and no schema bump is needed.
- Given the facade change, when `api-snapshot`, `skill-check` and `deps-check` run, then all three pass with the re-blessed snapshot and the new skill hash.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (logged to the memlog):
- **Traits now, derive later.** 2.2's ACs speak of "a Route value" and of decoding "against that Route", so `noctis-codec` defines `Route`, `Payload` and `PayloadDecode<'de>` now (AD-19). Story 2.5 derives them and adds `MAX_ENCODED_LEN`. Story 2.5 adding a required const later only breaks hand impls, which exist only in tests.
- **Two-phase decode.** Core looks up the Registry by `Decoded::route()` before it knows `R`. Phase 1 therefore cannot report Stale or Forged, and phase 2 classifies (AD-5). Phase 1 is injective, so 2.4's tag covers one spelling.
- **Seams for 2.3 and 2.4.** `EncodeError::NeedsRef` is the hook that 2.3 replaces with `EncodedId::RefSlot`. `Sign` is the hook that 2.4 implements for `PayloadKey`, which must compare tags in constant time. `RawBody::Ref(&str)` may become a `Ref` newtype in 2.3.
- **Reserved routes parse.** `_`-prefixed ids are grammar-valid. Core's lookup misses them (Forged), and the 2.5 derive rejects them for authors (CID-03).
- **Absent fields.** An empty body is ambiguous between "zero fields" and "one empty field". The codec tries the real decode first, so `String` `""` round-trips while `Option` falls back to `None`.

```rust
impl Payload for BuyV1 {
    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result { out.field(&self.item)?; out.field(&self.qty) }
}
impl<'de> PayloadDecode<'de> for BuyV1 {
    fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError> { Ok(Self { item: f.field()?, qty: f.field()? }) }
}
```

## Verification

**Commands** (run from the repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (unit tests, proptests, compile_fail doctests).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- rules-catalog` -- expected: `ok`, with one fewer pending rule and no pending check.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0 after `--bless`; the only diff is `absent`.
- `cargo run --release -p xtask -- skill-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.
