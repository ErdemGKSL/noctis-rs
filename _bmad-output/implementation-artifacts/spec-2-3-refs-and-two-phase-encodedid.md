---
title: 'Story 2.3: Refs and two-phase `EncodedId`'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: 'fe93a863cd1a2aad53e11b37c5a055a0cc8cc7f2'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred:
  - summary: >-
      The inline encoder never checks that a custom `Field` writes only inline-body bytes, so a fitting `spill` (or inline) Payload can emit an id that `decode` rejects, while the same Payload spilled gets `InvalidBody`.
    evidence: |-
      `Field` is an unsealed public trait (field.rs:50) and `encode` (layout.rs) pushes whatever `FieldWriter` writes into the id with no inline-grammar check; only the new Ref path validates via `RefRecord::new`. Pre-existing since Story 2.2's `encode`; encode_id's spill branch only forwards it.
    location: >-
      crates/noctis-codec/src/layout.rs encode; crates/noctis-codec/src/encoded.rs encode_id Spill branch
    severity: medium
  - summary: >-
      `RefPolicy` accepts any non-zero TTL, including sub-second and `Duration::MAX`, and any payload size; a store with coarser granularity or `Instant + ttl` arithmetic may expire immediately or overflow.
    evidence: |-
      Unverified: the codec has no clock, and the harm only occurs in core's RefStore or an adapter (Epic 7, Story 7.5). Settle it by fixing the RefStore TTL and record-size contract there (minimum/maximum TTL, checked `Instant` arithmetic, max record size).
    location: >-
      crates/noctis-codec/src/layout.rs check_parts; noctis-core RefStore (Epic 7)
    severity: medium (unverified)
  - summary: >-
      `PendingRef` equality lets core share one Ref between identical one-shot slots, so consuming the Ref on the first click may break every other identical component in the message.
    evidence: |-
      Unverified: AD-18 says identical slots in one message share one Ref and one-shot Refs are consumed with `take`; whether sharing a one-shot Ref is the intended behaviour is core's Responder decision (Epic 7). Settle it when the Responder dedups slots: either skip dedup for one-shot slots or document that one-shot means one use per message.
    location: >-
      crates/noctis-codec/src/encoded.rs PendingRef; noctis-core Responder (Epic 7)
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** `noctis-codec` (Story 2.2) can only write inline ids. `by_ref` Routes and `spill` Routes that do not fit stop at `EncodeError::NeedsRef`. Nothing mints Refs, finalizes a Ref id, or encodes and checks the `RefRecord` that core will store (FR-17 format, NFR-6; AD-5, AD-18; CID-01, CID-05).

**Approach:** Add the following to `noctis-codec`:
- a `Ref` newtype minted from the OS CSPRNG (`getrandom`) by unbiased rejection sampling;
- the two-phase `EncodedId = Inline(ArrayString<100>) | RefSlot(PendingRef)`, produced by a new `encode_id::<R>`;
- `finish(&PendingRef, Ref)`, which builds the final `~`-bodied id (at most 46 characters);
- a total `RefRecord` encoding and decoding;
- `decode_record::<R>`, which reports a record bound to another route as `Forged`.

The work also removes the two CID-05 pending entries.

## Boundaries & Constraints

**Always:**
- **`Ref`:** `pub struct Ref([u8; REF_LEN])` holding base62 ASCII digits. Its field is private. It derives `Clone, Copy, PartialEq, Eq, Hash`. It has `as_str()`, `Display`, and a `Debug` of the form `Ref("…")`.
  - Public constructors: only `Ref::mint() -> Result<Ref, RefError>`. It takes no arguments, so a Ref is never derived from interaction data.
  - `pub(crate)` constructors:
    - `from_digits([u8; 11])`, used by the phase-1 decoder;
    - `mint_from(fill: impl FnMut(&mut [u8]) -> Result<(), getrandom::Error>)`, whose `mint()` = `mint_from(getrandom::fill)`.
  - Sampling: fill a 16-byte buffer and walk it.
    - Accept a byte `b < 248` (= 4·62) as digit `b % 62`, and skip every other byte.
    - Refill when the buffer is used up. After 8 fills without 11 accepted bytes, return `RefError::Exhausted`.
    - A source error becomes `RefError::Rng(e)`.
- **`RefError`:** a `thiserror` enum, `#[non_exhaustive]`, deriving `Debug, Clone, Copy, PartialEq, Eq`, with variants `Rng(getrandom::Error)` and `Exhausted`.
- **`RefPolicy`:** derives `Debug, Clone, Copy, PartialEq, Eq, Hash, Default`. Its fields are private: `ttl: Option<Duration>` (`None` means the App default, FR-17) and `one_shot: bool`.
  - Const API: `new()`, `with_ttl(Duration)`, `one_shot()`, and the getters `ttl()` and `is_one_shot()`.
  - It is given to the encoder through `Parts::with_ref_policy(RefPolicy)`.
- **Encoder parts check, shared by `encode` and `encode_id`**, in this order:
  1. `InvalidSchema`
  2. envelope
  3. nonce
  4. signer (unchanged from 2.2)
  5. `UnexpectedRefPolicy`, when the policy is not the default and `R::SHAPE.body() == Inline`
  6. `ZeroTtl`, when `ttl == Some(Duration::ZERO)`
- **`encode_id::<R>(&R::Payload, &Parts) -> Result<EncodedId, EncodeError>`**, where `EncodedId { Inline(ArrayString<100>), RefSlot(PendingRef) }` derives `Debug, Clone, PartialEq, Eq, Hash`:
  - **`BY_REF`:** returns a RefSlot.
  - **`INLINE`:** returns `encode`'s result.
  - **`SPILL`:** returns `Inline` when `encode` succeeds. When `encode` gives `NeedsRef`, it returns a RefSlot. `encode_id` never returns `NeedsRef`.
  - **RefSlot payload:** the fields are written through `FieldWriter` into an unbounded `String`, then turned into `Bytes`. A field `Err` is `EncodeError::Field`.
- **`PendingRef`:** derives `Debug, Clone, PartialEq, Eq, Hash`, so core can deduplicate identical slots. Its fields are private and it is built only by the codec.
  - Getters: `route() -> RouteId`, `schema() -> u8`, `payload() -> &Bytes` (the inline field body, ASCII), `ttl() -> Option<Duration>`, `is_one_shot() -> bool`, `envelope() -> Option<ViewEnvelope>`, `nonce() -> Option<SelectNonce>` (AD-18's five fields, plus the `E`/`N` sections that `finish` needs).
  - `record() -> Bytes` returns the encoded `RefRecord`.
- **`finish(slot: &PendingRef, r: Ref) -> ArrayString<MAX_ID_LEN>`** is infallible.
  - It writes `F route ":" [S] [E] [N] "~" ref`, where `F` is the digit of `S|E|N` as present. It never writes `T`: a Ref body is never tagged, even for `SPILL.signed()`.
  - Add `pub const MAX_REF_ID_LEN: usize = FMT_LEN + MAX_ROUTE_LEN + SEP_LEN + SCHEMA_LEN + VIEW_ENV_LEN + NONCE_LEN + REF_MARKER_LEN + REF_LEN;` and `const _: () = assert!(MAX_REF_ID_LEN == 46 && MAX_REF_ID_LEN <= MAX_ID_LEN);`. This assertion is what lets the pushes be infallible.
- **`RefRecord<'a>` wire format:** `F route ":" [S] body`, using the same grammar and helpers as the id (no duplicated parser), with no length cap. Its private fields are `route: &'a str`, `schema: u8` and `payload: &'a str`. It derives `Debug, Clone, Copy, PartialEq, Eq, Hash`.
  - **`new(route, schema, payload) -> Result<Self, EncodeError>`** checks, in this order:
    1. route grammar `[a-z0-9._-]{1,24}` → `InvalidRoute`
    2. schema over 61 → `InvalidSchema{schema}`
    3. payload byte not in `0x20..=0x7E`, or equal to `:` or `~` → `InvalidBody{index}` (index into the payload)
  - **`encode(&self) -> Bytes`.**
  - **`decode(&'a [u8]) -> Result<Self, CodecError>`** is total and injective. Its checks run in this order:
    1. `Empty`
    2. `NotAscii{index}`
    3. `F`: `InvalidChar{0}` / `Generation`
    4. a flag other than `S` → `UnexpectedSection{section}`, naming the first of Envelope, Nonce, Tag
    5. route, exactly as in the id (`InvalidChar` / `MissingSeparator` / `EmptyRoute` / `RouteTooLong`)
    6. `S` (`Truncated{Schema}` / `ZeroSchema` / `InvalidChar`)
    7. body, inline grammar only (`InvalidChar{index}`)

    Indices are byte indices into the input.
  - **Getters:** `route() -> &'a str`, `schema()`, `payload() -> &'a str`.
- **`decode_record::<'r, R: Route>(record: &'r [u8]) -> Result<R::Payload, DecodeError>`**, where `R::Payload: PayloadDecode<'r>`, checks in this order:
  1. a record `CodecError` → `Malformed`
  2. `record.route() != R::ID` → `Forged(ForgedReason::RecordRouteMismatch)`
  3. a schema different from `R::SCHEMA` → `Stale(Schema{expected, found})`
  4. fields through `FieldReader` (with `finish`) → `Malformed`

  It is documented as being called only after `Decoded::payload::<R>()` returned `PayloadSource::Ref`, which already proved the id's route is `R::ID`.
- **The phase-1 Ref body is typed:** `RawBody::Ref(Ref)`, `PayloadSource::Ref(Ref)`, and `PayloadSource<P>` loses its lifetime.
- **New error variants** (all enums stay `#[non_exhaustive]` with lower-case messages):
  - `EncodeError::{UnexpectedRefPolicy, ZeroTtl, InvalidRoute, InvalidBody{index}}`
  - `CodecError::UnexpectedSection{section: Section}`
  - `ForgedReason::RecordRouteMismatch`
- **Code rules:**
  - New modules carry `#![deny(clippy::indexing_slicing, clippy::string_slice)]`.
  - Every public item has rustdoc.
  - No `unsafe`.
  - No `unwrap`/`expect` outside tests.

**Never:**
- No `RefStore`, port, TTL clock, App namespacing or collision retry. Those are core's (AD-18, Epic 7).
- No `PayloadKey`/`blake3`/tag verification (2.4).
- No derive, `MAX_ENCODED_LEN` or `assert_fits` (2.5).
- No dhat (2.6).
- No facade (`crates/noctis`) change, so no template, skill or snapshot change. No `__private`.
- No change to `docs/discord-rules.md` or `noctis-rules`.
- No test name containing `cid_01`. CID-01 stays pending for 2.5.
- No sequential counter, timestamp or hash of input anywhere in Ref generation.
- Do not remove or change `encode`'s `NeedsRef` behaviour.

## I/O & Edge-Case Matrix

**Fixtures** (shared from 2.2, plus new ones):

| Fixture | Route | Shape | Schema | Payload |
|---|---|---|---|---|
| `Stored` | `r` | BY_REF | 0 | `BuyV1` |
| `StoredV2` | `r` | BY_REF | 0 | `BuyV2Payload` |
| `StoredS2` | `r` | BY_REF | 2 | `BuyV1` |
| `StoredText` | `rt` | BY_REF | 0 | `TextP` |
| `StoredAll` | 24×`a` | BY_REF.view().select() | 61 | `NumP` |
| `TextSpill` | `t` | SPILL | 0 | `TextP` |
| `SpillSigned` | `ss` | SPILL.signed() | 0 | `TextP` |

`R0` is the decoded `Ref` from `decode("0a:~0123456789A")`.

**`Ref`:**

| Input | Result |
|---|---|
| `Ref::mint()` | `Ok`; 11 chars, all `[0-9A-Za-z]` |
| 100 000 mints | no duplicates; overall char χ² (61 dof) < 130; χ² at each of the 11 positions < 140; no mint is the base62 successor of the previous one |
| `mint_from` bytes `0..=10` | `"0123456789A"` |
| byte 247; byte 61; byte 62 | `'z'`; `'z'`; `'0'` |
| bytes `[248, 255, 0, 1, 2, …]` | starts with `"012"` (248..=255 skipped) |
| first 16 bytes all 255, then `0..` | `Ok` (refill works) |
| always 255 | `Err(Exhausted)` after exactly 8 fills |
| source returns `Error::UNSUPPORTED` | `Err(Rng(Error::UNSUPPORTED))` |
| `format!("{R0:?}")`; `R0.to_string()` | `Ref("0123456789A")`; `"0123456789A"` |

**`encode_id`** (`Parts::new()` unless noted):

| Input | Result |
|---|---|
| `Buy{255,3}` | `Inline("0shop.buy:47,3")` |
| `Stored{255,3}` | `RefSlot`: route `r`, schema 0, payload `b"47,3"`, ttl `None`, not one-shot, no env/nonce; `record()` = `b"0r:47,3"` |
| `StoredS2{255,3}` | `record()` = `b"1r:247,3"` |
| `StoredText{"é,~"}` | payload `b"!u0000e9!1!3"` |
| `StoredText{10 000×'x'}` | payload of 10 000 bytes; `finish` length 16 |
| `TextSpill{97×x}` / `{98×x}` | `Inline` (len 100) / `RefSlot` (payload 98×`x`) |
| `SpillSigned{91×x}` + signer / `{92×x}` + signer | `Inline` len 100 / `RefSlot`; `finish(…,R0)` = `"0ss:~0123456789A"` (no `T`) |
| `Text{98×x}` | `Err(TooLong)` |
| `Stored` + `RefPolicy::new().with_ttl(60s).one_shot()` | slot ttl `Some(60s)`, `is_one_shot()` |
| `Stored` + `with_ttl(ZERO)` | `Err(ZeroTtl)` |
| `Buy` + any non-default policy; `Buy` + `RefPolicy::new()` | `Err(UnexpectedRefPolicy)`; `Ok(Inline)` |
| `TextSpill{"a"}` + a ttl policy | `Inline("0t:a")` (the policy only applies when spilled) |
| `encode::<Buy>` + a non-default policy | `Err(UnexpectedRefPolicy)` (shared check) |
| `Stored` + signer; `Stored` + env | `Err(UnexpectedSigner)`; `Err(UnexpectedEnvelope)` |
| order: `BadSchema`-like BY_REF route with schema 62 + policy `ZERO` | `Err(InvalidSchema{62})` |
| a BY_REF route whose field `encode` fails | `Err(Field)` |

**`finish`:**

| Input | Result |
|---|---|
| `Stored` slot, `R0` | `"0r:~0123456789A"` (len 15) |
| `StoredS2` slot, `R0` | `"1r:2~0123456789A"` |
| `StoredAll` slot, env `('_',61,3843,61)`, nonce 3843, ref `zzzzzzzzzzz` | `"7"+24×a+":z_zzzzzz~zzzzzzzzzzz"`, len 46 = `MAX_REF_ID_LEN` |
| any finish output → `decode_route::<R>` for the slot's Route | `Ok(PayloadSource::Ref(r))` with the same `r` |

**`RefRecord::new` / `encode`:**

| Input | Result |
|---|---|
| `("r",0,"47,3")` / `("r",2,"47,3")` / `("r",61,"")` | `b"0r:47,3"` / `b"1r:247,3"` / `b"1r:z"` |
| `("_x",0,"")`; 24-char route | `Ok` (`b"0_x:"`); `Ok` |
| route `""`, 25×`a`, `"R"`, `"a:b"`, `"é"` | `Err(InvalidRoute)` |
| schema 62 / 255 | `Err(InvalidSchema{62})` / `{255}` |
| payload `"a~"`, `"a:b"`, `"\t"`, `"é"`, `"\u{7f}"` | `InvalidBody{index:1}`, `{1}`, `{0}`, `{0}`, `{0}` |
| route `""` + schema 62 | `InvalidRoute` (route first) |

**`RefRecord::decode`** (bytes):

| Input | Result |
|---|---|
| `b""` | `Empty` |
| `b"0r:\xc3\xa9"`; `b"\xff"` | `NotAscii{3}`; `NotAscii{0}` |
| `b"!r:"`; `b"Gr:"` | `InvalidChar{0}`; `Generation{1}` |
| `b"2r:_0000"`, `b"4r:00"`, `b"8r:000000"`, `b"Fr:z"` | `UnexpectedSection{Envelope}`, `{Nonce}`, `{Tag}`, `{Envelope}` |
| `b"0r"`; `b"0:1"`; `"0"+25×a+":"`; `b"0R:1"` | `MissingSeparator`; `EmptyRoute`; `RouteTooLong{25}`; `InvalidChar{1}` |
| `b"1r:"`; `b"1r:0"`; `b"1r:!"` | `Truncated{Schema}`; `ZeroSchema`; `InvalidChar{3}` |
| `b"0r:~0123456789A"`; `b"0r:a:b"`; `b"0r:\t"` | `InvalidChar{3}`; `{4}`; `{3}` |
| `b"0r:"`; `"0r:"+10 000×x` | `Ok` (empty payload); `Ok` (no length cap) |
| proptest over arbitrary `Vec<u8>` (0..300) | never panics; `Ok(r)` ⇒ `r.encode() == input` |
| proptest over valid `(route, 0..=61, "[ -9;-}]{0,300}")` | `decode(new(..).encode()) == new(..)` |

**`decode_record`:**

| R | Record | Result |
|---|---|---|
| Stored | `b"0r:47,3"` | `Ok(BuyV1{255,3})` |
| Stored | `b"0x:47,3"`; `b"0x:junk"` | `Forged(RecordRouteMismatch)` (route before fields) |
| Stored | `b"1r:247,3"` | `Stale(Schema{expected:0,found:2})` |
| StoredS2 | `b"0r:47,3"` | `Stale(Schema{expected:2,found:0})` |
| Stored | `b"0r:47"` / `b"0r:47,3,1"` / `b"0r:47,-"` | `Malformed(MissingField{1})` / `Malformed(ExtraField{2})` / `Malformed(Field{1, InvalidChar{0}})` |
| Stored | `b""`; `b"2r:_0000"` | `Malformed(Empty)`; `Malformed(UnexpectedSection{Envelope})` |
| StoredV2 | `b"0r:47,3"` | `{255,3,None,None}` (trailing `Option` compatibility) |
| proptest: `MixP` on BY_REF and SPILL fixtures | `encode_id`, then either `decode_route` of `Inline`, or `finish(mint())` + `decode_route` → `Ref(r)` + `decode_record(&slot.record())` | equals the input |

</intent-contract>

## Code Map

- `crates/noctis-codec/Cargo.toml` -- add `bytes.workspace = true` and `getrandom.workspace = true`. The workspace already pins `bytes = "1.12"` and `getrandom = "0.4"`, and `Cargo.lock` already has bytes 1.12.1 (MIT, MSRV 1.57) and getrandom 0.4.3 (MIT OR Apache-2.0, MSRV 1.85), both verified on crates.io on 2026-10-19 and covered by `deny.toml`. getrandom 0.4's API is `fill(&mut [u8]) -> Result<(), Error>`; `Error` is `Copy + Eq`, implements `core::error::Error`, and has the const `Error::UNSUPPORTED`. Use no features.
- `crates/noctis-codec/src/layout.rs`:
  - l.40-79 `Parts`: add the `ref_policy` field and `with_ref_policy`, and update the `Debug` impl.
  - l.81-95 `section`, plus the parts checks inside `encode` (l.135-165): factor them into one `pub(crate)` fn that both `encode` and `encode_id` call.
  - l.226-241 `RawBody`/`PayloadSource`: switch to `Ref`.
  - l.286-292 `body()`.
  - l.367 `PayloadSource::Ref`.
  - l.374-425 `digit_at`, `fixed` and `check_body` (inline branch), plus the route scan and `S` parse inside `decode` (l.473-512): extract them as `pub(crate)` helpers that `RefRecord::decode` reuses.
  - l.689 unit test: update it to `Ref`.
- `crates/noctis-codec/src/route.rs` -- l.9 `MAX_ROUTE_LEN` and l.12 `is_route_byte` (pub(crate)), reused for `RefRecord::new`. l.277 `FieldWriter::new(&mut dyn fmt::Write)` works with a `String` sink. l.316/l.355 `FieldReader::new`/`finish` are reused by `decode_record`.
- `crates/noctis-codec/src/base62.rs` -- `ALPHABET` (l.6), `digit` (l.29) and `digit_value` (l.46), for minting and the successor check.
- `crates/noctis-codec/src/parts.rs` l.248-283 -- `Tag` is the template for `Ref` (fixed bytes, an `as_str` via `from_utf8(..).unwrap_or_default()`, `Debug`/`Display`).
- `crates/noctis-codec/src/error.rs` -- l.116-195 `CodecError`, l.229-233 `ForgedReason`, l.253-288 `EncodeError`. Add the variants. Put `RefError` here.
- `crates/noctis-codec/src/lib.rs` -- declare the new modules, re-export them at the root, and add a "Refs" crate-doc section.
- `crates/noctis-codec/tests/layout.rs` l.1-110 -- the `payload!`/`route!` macros, the fixtures and `FakeSigner`. Move them into `tests/common/mod.rs` (`mod common;` in both test files). Update l.269 (`inline` helper), l.397-411 and l.451 for `PayloadSource<P>` / `RawBody::Ref(Ref)`.
- `xtask/rules-pending.toml` l.306-314 -- the two CID-05 entries (`by-construction` and `unit`, story 2.3). Delete both. `xtask/src/rules_catalog/mod.rs` l.20-28: by-construction evidence is a `compile_fail` doctest in `crates/*/src` with `// [CID-05]` after code on a line. Unit evidence is a `#[test]` whose name contains the token `cid_05`.
- `.github/workflows-disabled/ci.yml` -- no new gate; the existing `cargo test` job covers this story. Read-only.
- Previous story 2.2 (done): `encode`'s `NeedsRef` is the hook; `RawBody::Ref` becomes the newtype as its Design Notes foresaw. Its residual risk (tags are unverified) stays for 2.4.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-codec/Cargo.toml` -- add `bytes` and `getrandom` -- needed for the Ref payload and the CSPRNG.
- `crates/noctis-codec/src/error.rs` -- add `RefError` and the new `EncodeError`/`CodecError`/`ForgedReason` variants, each with a doc line -- typed outcomes.
- `crates/noctis-codec/src/refs.rs` (new) -- add `Ref`, `mint`, `mint_from` and `from_digits`. Add a `compile_fail` doctest `let r = noctis_codec::Ref([b'0'; 11]); // [CID-05]`, whose rustdoc says Refs come only from the CSPRNG or a decoded id. Add unit tests `cid_05_mint_rejection_sampling` (every `mint_from` row) and `ref_display_and_debug` -- CID-05 by construction.
- `crates/noctis-codec/src/layout.rs` -- add `RefPolicy`, `Parts::with_ref_policy`, the shared parts check, the extracted helpers, `RawBody::Ref(Ref)` and `PayloadSource<P>`, and update the unit tests -- one parser, one parts check.
- `crates/noctis-codec/src/encoded.rs` (new) -- add `EncodedId`, `PendingRef`, `encode_id`, `finish` and `MAX_REF_ID_LEN` with its const assertion, plus rustdoc that shows the two phases (encode, mint, finish) -- AD-18.
- `crates/noctis-codec/src/record.rs` (new) -- add `RefRecord` (`new`, `encode`, `decode`, getters) and `decode_record`, with unit tests for every `RefRecord` matrix row -- AD-5 record binding.
- `crates/noctis-codec/src/lib.rs` -- add `mod encoded; mod record; mod refs;` and re-export `EncodedId, PendingRef, RefPolicy, encode_id, finish, MAX_REF_ID_LEN, Ref, RefError, RefRecord, decode_record`. Add a crate-docs "Refs" section covering minting, the two phases, the record format and the Forged/Stale mapping.
- `crates/noctis-codec/tests/common/mod.rs` (new) and `tests/layout.rs` -- move the shared fixtures out and update the tests to `Ref`. Keep every 2.2 assertion.
- `crates/noctis-codec/tests/refs.rs` (new) -- add:
  - `cid_05_refs_unique_and_uniform`: 100k mints, plus the χ² and successor checks.
  - `encode_id_matrix` and `finish_matrix`.
  - `cid_05_record_route_mismatch_is_forged` and `decode_record_outcomes`.
  - proptests `record_round_trip`, `record_decode_never_panics`, `finish_decodes_as_ref` (random route, schema 0..=61, optional env and nonce, minted ref: `decode` is `Ok`, the length is ≤ 46 and equals the predicted length, and the body is the same `Ref`), and `encode_id_round_trip`.

  Commit any `proptest-regressions/`.
- `xtask/rules-pending.toml` -- delete both CID-05 entries.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already appended by planning. No implementer action.

**Acceptance Criteria:**
- Given the Ref generator, when 100 000 Refs are minted in `cid_05_refs_unique_and_uniform`, then every Ref is 11 base62 characters, no two are equal, the χ² bounds hold overall and per position, and no Ref is its predecessor's successor.
- Given a `by_ref` Route, or a `spill` Route whose value does not fit, when `encode_id` runs, then it returns `EncodedId::RefSlot(PendingRef)` carrying the route, schema, payload `Bytes`, ttl and one-shot flag. A fitting inline value returns `EncodedId::Inline`.
- Given any `PendingRef` and `Ref`, when `finish` runs, then the id ends in `"~" + ref`, is at most 46 characters (exactly 46 for a 24-character route with S+E+N), and `decode_route` for its Route yields `PayloadSource::Ref` of the same Ref.
- Given a `RefRecord`, when it is encoded and decoded, then it round-trips (proptest). `decode_record` of a record whose route differs from the id's Route returns `DecodeError::Forged(RecordRouteMismatch)`, and a schema mismatch returns `Stale`.
- Given the two CID-05 pending entries are removed, when `cargo run --release -p xtask -- rules-catalog` runs, then it prints `ok`, counting CID-05 as covered (its doctest tag and `cid_05` tests are found).
- Given the facade is untouched, when `api-snapshot`, `skill-check` and `deps-check` run, then they pass with no re-bless.

## Spec Change Log

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 26 findings — high 0, medium 2, low 9, false 12, maybe-false 3
- findings:
  - `[false]` `[reject]` By-Ref ids leave `E`/`N` unbound and untagged — AD-5 limits `T` to inline bodies of signed Routes and the spec settles that a Ref body is never tagged; the record binds route and schema only by design.
  - `[false]` `[reject]` Signer required then dropped on the Ref path; `BY_REF.signed()` not refused — `RouteShape::signed` asserts against `ByRef` (route.rs:155-159, compile error in const); dropping `T` on a spilled body is AD-5's rule, and `Decoded::payload` already reports a tagged Ref body as `Stale(Flags)`.
  - `[low]` `[reject]` `spill` overflow encodes the Payload twice and runs `check_parts` twice — real but only on the overflow path, the spec prescribes this flow, and a single-pass rewrite adds restructuring.
  - `[maybe-false]` `[defer]` No upper bound on stored payload or TTL — grouped with the edge-case TTL finding; needs core's RefStore TTL/size contract (Epic 7) to settle; deferred as medium (unverified).
  - `[low]` `[reject]` `getrandom::Error` in `RefError`'s public API — the spec mandates `Rng(getrandom::Error)` and the facade does not re-export it; the fix would edit the spec.
  - `[false]` `[reject]` Silent fallbacks in `Decoded::body`, `PendingRef::record` and `finish` — every fallback is unreachable: `decode` checks the 11 base62 digits, `slot` validates the payload through `RefRecord::new`, and the const assertion bounds `finish` (the proptest pins exact length); the repo forbids `unwrap`/`expect`.
  - `[false]` `[reject]` Project template and rule bookkeeping not updated — the `noctis` facade (crates/noctis/src/lib.rs:72) re-exports none of the new items, so noctis's public API and snapshot are unchanged; CID-05 already has its row in docs/discord-rules.md:178.
  - `[low]` `[reject]` `PendingRef::record()` re-encodes and allocates per call — one small allocation per stored slot; caching changes the struct for negligible gain.
  - `[low]` `[patch]` Missing tests for `ZeroTtl`/`UnexpectedRefPolicy` order and edge cases — grouped with the gap below; add the rows to `encode_id_matrix`.
  - `[low]` `[reject]` Counter/clock detection in the 100k test is weak — the thresholds and successor check are the spec's; `cid_05_mint_rejection_sampling` pins the mapping deterministically; the fix would edit the spec.
  - `[false]` `[reject]` No checked `Ref::parse` from text — not a defect; the spec allows only `mint` as a public constructor, and core's store keying is Epic 7.
  - `[medium]` `[defer]` A fitting `spill` Payload whose custom `Field` writes `:`/`~`/non-printable bytes yields an id `decode` rejects — real (`Field` is unsealed, `encode` has no inline-grammar check) but pre-existing in Story 2.2's `encode`; deferred.
  - `[maybe-false]` `[defer]` Identical one-shot slots sharing one Ref — whether sharing is wrong is core's Responder decision (AD-18, Epic 7); deferred as medium (unverified).
  - `[maybe-false]` `[defer]` Sub-second or huge TTL breaks a second-granular store — the codec has no clock; settle in Story 7.5's RefStore contract; grouped with the payload/TTL bound finding.
  - `[false]` `[reject]` `decode_record` accepts a record for an inline Route — `Decoded::payload` never returns `PayloadSource::Ref` for an inline Route (layout.rs:464-473), and records are only written by core under minted Refs, so the documented precondition is never met for one.
  - `[false]` `[reject]` `finish` write failure is only `debug_assert!`ed — the failure is unreachable (`MAX_REF_ID_LEN == 46 <= 100` is const-asserted, the proptest pins exact length); the dead assertion itself is the low row below.
  - `[low]` `[reject]` `finish_decodes_as_ref` lives in src/encoded.rs, not tests/refs.rs — it builds `PendingRef` with arbitrary routes, which needs crate-private access; placement is cosmetic.
  - `[low]` `[patch]` (gap) Zero-TTL and inline-route policy checks pinned only for a `by_ref` Route via `encode_id` — add `TextSpill`+zero → `ZeroTtl`, `encode::<Stored>`+zero → `ZeroTtl`, `Buy`+zero → `UnexpectedRefPolicy`, `encode::<Stored>`+ttl policy → `NeedsRef`.
  - `[medium]` `[patch]` (gap) CID-05 by-construction doctest pins only the tuple constructor, not `Ref::from_digits` visibility — add a second `compile_fail` doctest calling `Ref::from_digits`, tagged `// [CID-05]`.
  - `[low]` `[patch]` `finish`'s `debug_assert!` never runs in this release-only repo (`.cargo/config.toml` disables debug assertions) — make it a plain `assert!`.
  - `[false]` `[reject]` (intent) Record route check is Route-relative, not id-relative — the spec settles it: `decode_record` is documented to follow `decode_route::<R>` returning `Ref`, which already proved the id's route is `R::ID`.
  - `[false]` `[reject]` (intent) `Ref` is also the type of parsed Refs — CID-05 constrains generation; a parsed Ref is the lookup key and cannot mint one; the spec types the phase-1 body as `Ref` deliberately. The doctest-coverage part is the medium gap above.
  - `[false]` `[reject]` (intent) `PendingRef` has seven private fields, not AD-18's literal five — the spec's Design Notes settle it: `finish` needs `E`/`N`, and getters expose AD-18's five.
  - `[false]` `[reject]` (intent) `ttl`/`one_shot` come from a runtime `RefPolicy` on `Parts` — AD-6 keeps `ttl`/`one_shot` out of the codec's `Route` trait (core reads `RouteRuntime`, Story 7.5), so core passing them per call is the supported bridge the spec chose.
  - `[low]` `[reject]` (intent) χ² bounds are a gross-bias check, not a conventional significance test — the bounds are the spec's and rejection sampling is pinned deterministically; the fix would edit the spec.
  - `[false]` `[reject]` (intent) `PayloadSource`/`RawBody` public shape changed — the spec mandates `RawBody::Ref(Ref)` and `PayloadSource<P>`, and the 2.2 tests were updated.

## Design Notes

Decisions (logged to the memlog):
- **Unbiased sampling.** 62^11 is larger than `u64::MAX`, so a Ref cannot come from one `u64`. Bytes below 248 map uniformly by `% 62` (≈ 5.95 bits each, 65.5 bits in total). The 8-fill cap keeps the function total against a broken source; with a real CSPRNG it is never reached.
- **The record carries no `E`/`N`/`T`.** These are per render and live in the id. The record binds only route and schema (CID-05). It reuses the id's grammar with no 100-character cap, because by-ref Payloads are large.
- **`decode_record` compares against `R::ID`.** Phase 2 has already proved that the id's route is `R::ID`, so this is the same as comparing with the id's route, and core needs no extra handle.
- **`RefPolicy` rides `Parts`.** The ttl and one-shot flag reach `PendingRef` now. Story 7.5 may source them from `RouteRuntime`, keeping `PendingRef` immutable after the encoder.
- **`PendingRef` holds `E`/`N` beyond AD-18's five fields.** The final id needs them. `Eq + Hash` over all fields lets core share one Ref between identical slots.

```rust
let id = match encode_id::<Stored>(&p, &Parts::new())? {
    EncodedId::Inline(id) => id,
    EncodedId::RefSlot(slot) => { let r = Ref::mint()?; /* core: put_if_absent(r, slot.record()) */ finish(&slot, r) }
};
```

## Verification

**Commands** (run from the repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0, including the 100k-Ref statistical test, the proptests and the `compile_fail` doctest.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- rules-catalog` -- expected: `ok`, with 2 fewer pending entries than before and CID-05 covered.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0 with no diff (do not bless).
- `cargo run --release -p xtask -- skill-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.

## Auto Run Result

- **Summary:** `noctis-codec` gains CSPRNG-minted 11-digit base62 `Ref`s (CID-05, rejection-sampled, only `Ref::mint` constructs one), a two-phase `EncodedId` (`Inline` id or `RefSlot(PendingRef)` -> `finish(&slot, ref)`), `RefPolicy` (ttl, one-shot) on `Parts`, a `RefRecord` codec (`record()` / `decode_record::<R>`) binding route and schema only, `RefError`, and `RawBody::Ref` / `PayloadSource<P>` on decode.
- **Files changed:**
  - `crates/noctis-codec/src/refs.rs` - `Ref` type, unbiased minting, CID-05 `compile_fail` doctests.
  - `crates/noctis-codec/src/encoded.rs` - `EncodedId`, `PendingRef`, `encode_id`, `finish` (now a hard `assert!`).
  - `crates/noctis-codec/src/record.rs` - stored-record encode/decode with route/schema check.
  - `crates/noctis-codec/src/layout.rs`, `lib.rs`, `error.rs` - by-ref decode path, `PayloadSource`, `RefError`, exports.
  - `crates/noctis-codec/tests/refs.rs`, `tests/common/mod.rs`, `tests/layout.rs` - 100k-Ref statistical test, proptests, policy matrix; shared fixtures.
  - `crates/noctis-codec/Cargo.toml`, `Cargo.lock` - `getrandom` dependency.
  - `xtask/rules-pending.toml` - CID-05 pending entries removed (now covered).
- **Review findings:** 26 (high 0, medium 2, low 9, false 12, maybe-false 3).
  - Patched (3 entries; medium 1, low 2): second CID-05 `compile_fail` doctest for `Ref::from_digits`; `ZeroTtl`/`UnexpectedRefPolicy`/`NeedsRef` rows added to `encode_id_matrix`; `finish`'s `debug_assert!` made `assert!` (release-only repo disables debug assertions).
  - Deferred (4): `spill` with a custom `Field` writing `:`/`~`/non-printable bytes yields an undecodable id (pre-existing from 2.2); no upper bound on stored payload size or TTL and sub-second/huge TTL vs a second-granular store (Story 7.5 RefStore contract); identical one-shot slots sharing one Ref (core Responder, AD-18, Epic 7).
  - Rejected (19): untagged By-Ref ids and dropped signer (AD-5 by design); double encode on spill overflow (spec flow, overflow-only); `getrandom::Error` in `RefError` (spec-mandated, not re-exported); silent fallbacks (unreachable); template/rules not updated (facade re-exports nothing new, CID-05 row exists); per-call allocation in `record()` (negligible); weak counter/clock detection and chi-square bounds (spec's thresholds); no `Ref::parse` (spec allows only `mint`); `decode_record` on inline Route (precondition never met); `finish_decodes_as_ref` placement (needs crate-private access); route-relative record check, `Ref` as parsed type, seven `PendingRef` fields, runtime `RefPolicy`, `PayloadSource`/`RawBody` shape change (all settled by spec/ADs).
- **Follow-up review recommended:** false - first pass patched 0 high and 1 medium.
- **Verification:** local gates via `gates.sh` all green (fmt, clippy -D warnings, release tests incl. 100k-Ref statistical test, proptests and doctests, cargo-hack each-feature, rules-catalog, deps-check, api-snapshot no diff, skill-check, cargo deny, MSRV 1.91 check, build-config check, no `target/debug`). CI paused by maintainer.
- **Residual risks:** unsealed `Field` can still produce ids `decode` rejects; Ref store TTL/size bounds and one-shot Ref sharing remain undefined until Epic 7 (Story 7.5); statistical mint test is a gross-bias check only.
