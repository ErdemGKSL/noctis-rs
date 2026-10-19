---
title: 'Story 2.4: Integrity tags for signed inline Payloads'
type: 'feature'
created: '2026-10-19'
status: 'in-progress'
baseline_revision: '3e7ac597d1f6e62ed6752c531e9699a077fc8dcf'
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

**Problem:** `noctis-codec` writes the `T` section through a `Sign` trait (Story 2.2), but nothing implements it with a real key, and decoding never verifies `T`: a forged or tampered id of a `signed` Route decodes as a valid Payload, and a signed Route's id without `T` is only `Stale` (FR-19, NFR-6; AD-5, AD-29; CID-06).

**Approach:** Add to `noctis-codec` a redacted 32-byte `PayloadKey` (keyed BLAKE3, truncated to a 6-digit base62 `Tag`), a `PayloadKeys` ring (current key signs, current or one previous key verifies), a `Verify` trait, constant-time tag comparison, and a verifying phase-2 decode that reports missing and bad tags as `Forged`.

## Boundaries & Constraints

**Always:**
- **Tag formula:** `tag(key, signed) = Tag::from_u64(u64::from_le_bytes(first 8 bytes of blake3::keyed_hash(&key, signed.as_bytes())))`. `signed` is every character before `T` (exactly what `encode` already passes to `Sign::sign`, and `Decoded::signed_part()`). No prefix, no domain string.
- **`PayloadKey`** (new `src/key.rs`): `pub struct PayloadKey([u8; 32])`, private field, derives `Clone` only (no `Copy`, `PartialEq`, `Eq`, `Hash`, no byte getter, no `AsRef`, no serde).
  - `pub const fn from_bytes(bytes: [u8; 32]) -> Self`.
  - `pub fn from_hex(s: &str) -> Result<Self, KeyError>` and `impl FromStr` (same function) for the `NOCTIS_PAYLOAD_KEY` format: exactly 64 ASCII hex digits, upper or lower case, no trimming, no `0x`. Checks in order: `s.len() != 64` (bytes) → `KeyError::Length { len }`; first non-hex byte → `KeyError::InvalidChar { index }` (byte index). An all-zero key is accepted.
  - `Debug` prints exactly `PayloadKey(<redacted>)`; `Display` prints exactly `<redacted>` (AD-29).
  - Implements `Sign` (the formula) and `Verify` (constant-time compare against its own tag).
- **`PayloadKeys`**: derives `Clone, Debug` (the derived Debug shows only redacted keys); private fields `current: PayloadKey`, `previous: Option<PayloadKey>`. `new(current) -> Self`, `#[must_use] with_previous(self, previous: PayloadKey) -> Self`. `Sign` uses `current` only. `Verify` computes the current-key comparison and, when set, the previous-key comparison, and combines them with non-short-circuit `|`.
- **`Verify` trait** (in `src/parts.rs` beside `Sign`): `pub trait Verify { fn verify(&self, signed: &str, tag: Tag) -> bool; }`.
- **Constant time:** `pub(crate) fn ct_eq(&self, other: &Tag) -> bool` on `Tag`: XOR-OR fold over all 6 bytes, no early exit, result passed through `core::hint::black_box`. No new crate for it.
- **`KeyError`** in `error.rs`: `thiserror`, `#[non_exhaustive]`, derives `Debug, Clone, Copy, PartialEq, Eq`; variants `Length { len: usize }` ("payload key is {len} bytes, 64 hex digits expected") and `InvalidChar { index: usize }` ("payload key has a non-hex character at {index}"). It never carries or prints key characters.
- **`ForgedReason`** gains `MissingTag` ("integrity tag is missing") and `BadTag` ("integrity tag does not verify").
- **Verifying phase 2:** `Decoded::verified_payload::<R>(&self, keys: &dyn Verify) -> Result<PayloadSource<R::Payload>, DecodeError>` and `decode_route_verified::<'a, R>(raw: &'a str, keys: &dyn Verify)` (`decode` then `verified_payload`). Checks in this order:
  1. route ≠ `R::ID` → `Forged(RouteMismatch)`
  2. schema → `Stale(Schema)`
  3. body kind → `Stale(Body)`
  4. Route is signed, body inline, `T` absent → `Forged(MissingTag)`
  5. `E`/`N`/`T` flags ≠ expected → `Stale(Flags)` (unchanged; covers `T` on an unsigned Route)
  6. Route is signed, body inline: `!keys.verify(signed_part, tag)` → `Forged(BadTag)`
  7. fields → `Malformed`
- **Fail-closed plain decode:** `Decoded::payload::<R>()` and `decode_route::<R>()` keep their signatures and become `verified_payload::<R>(&NeverVerifies)` with a private `NeverVerifies` (always `false`). Unsigned Routes and Ref bodies are unaffected; a signed Route's inline id is always `Forged` there. Their rustdoc says so and points to the verified variants.
- `lib.rs` re-exports `PayloadKey, PayloadKeys, Verify, KeyError, decode_route_verified` and adds an "Integrity tags" crate-doc section (formula, rotation, outcomes, redaction).
- Code rules: `src/key.rs` carries `#![deny(clippy::indexing_slicing, clippy::string_slice)]`; every public item has rustdoc; no `unsafe`; no `unwrap`/`expect` outside tests.

**Never:**
- No reading of `NOCTIS_PAYLOAD_KEY` from the environment, no `App::from_env`, no logging, no metrics: core's (Story 7.2). No facade (`crates/noctis`) change, so no template, skill or snapshot change; no `__private`.
- No tag on Ref bodies or Ref records; `finish` and `RefRecord` unchanged.
- No new crate other than `blake3` (no `subtle`, `zeroize`, `hex`, `constant_time_eq`). No key generation API.
- No change to `docs/discord-rules.md`, `noctis-rules` or `xtask/rules-pending.toml` (CID-06 is already covered).
- Do not change `encode`/`encode_id` behaviour or the `Sign` trait.

## I/O & Edge-Case Matrix

Keys: `K1 = PayloadKey::from_bytes([1; 32])`, `K2 = [2; 32]`, `K3 = [3; 32]`. Fixtures from `tests/common/mod.rs` (`Sig` = `"sig"`, INLINE.signed, `NumP`; `TextSigned` = `"t"`; `SpillSigned` = `"ss"`; `All` = `"all"`, view+select+signed, schema 61). Golden tags were computed with the reference BLAKE3 implementation using the formula above; tests assert these literals **and** recompute them with `blake3::keyed_hash` in the test.

**Signing (`encode` with `Parts::with_signer(&key)`):**

| Input | Result |
|---|---|
| `Sig{1}`, K1 / K2 / K3 | `"8sig:1UU2sw1"` / `"8sig:1a0cQ9C"` / `"8sig:1DMzSul"` |
| `Sig{2}`, K1 | `"8sig:2ktZWEa"` |
| `SpillSigned{"a"}`, K1 | `"8ss:aOlrJxp"` |
| `TextSigned{""}`, K1 | `"8t:R0qOBY"` |
| `TextSigned{91×x}`, K1 | len 100, ends `"LIqs04"`; `{92×x}` → `Err(TooLong)` |
| `All{0}`, env `('_',61,3843,61)`, nonce 0, K1 | `"Fall:z_zzzz000ymc2iM"` |
| `Sig{1}`, `PayloadKeys::new(K1).with_previous(K2)` | `"8sig:1UU2sw1"` (current signs) |
| `SpillSigned{90×x}`, K1 / `{91×x}`, K1 via `encode_id` + `finish(R0)` | `Inline`, len 100 / `"0ss:~0123456789A"` (no tag) |

**`decode_route_verified`** (keys argument in column 2):

| Id → Route | Keys | Result |
|---|---|---|
| `"8sig:1UU2sw1"` → Sig | K1; `PayloadKeys::new(K1)`; `new(K2).with_previous(K1)`; `new(K1).with_previous(K1)` | `Ok(Inline(NumP{1}))` |
| `"8sig:1UU2sw1"` → Sig | K2; `PayloadKeys::new(K2)`; `new(K2).with_previous(K3)` | `Forged(BadTag)` |
| `"8sig:2UU2sw1"` (tampered body) → Sig | K1 | `Forged(BadTag)` |
| `"8sig:1UU2sw2"`, `"8sig:1uU2sw1"` (tampered tag) → Sig | K1 | `Forged(BadTag)` |
| `"8sig:1000000"` → Sig | K1 | `Forged(BadTag)` |
| `"0sig:1"` (missing tag) → Sig | K1 | `Forged(MissingTag)` |
| `"2sig:_00001"` (E flag, no `T`) → Sig | K1 | `Forged(MissingTag)` (step 4 before 5) |
| `"0ss:a"` → SpillSigned | K1 | `Forged(MissingTag)` |
| `"8ss:aOlrJxp"` → SpillSigned | K1 | `Ok(Inline(text("a")))` |
| `"0ss:~0123456789A"` → SpillSigned | K1 | `Ok(Ref(R0))` (Ref bodies are never tagged) |
| `"Fall:z_zzzz000ymc2iM"` → All | K1 | `Ok(Inline(NumP{0}))`; same id with `_` → `a` in the envelope → `Forged(BadTag)` |
| `"8sig:1UU2sw1"` → Buy | K1 | `Forged(RouteMismatch)` (route first) |
| `"9sig:21UU2sw1"` (S=2) → Sig | K1 | `Stale(Schema{expected:0,found:2})` (schema before tag) |
| `"8shop.buy:47,3000000"` → Buy (T on unsigned) | K1 | `Stale(Flags{expected:EMPTY,found:T})` |
| `"0shop.buy:47,3"` → Buy | K1 | `Ok(Inline(BuyV1{255,3}))` (keys unused) |
| `"8sig:1UU2sw"` (tag = last 6 chars `1UU2sw`, empty body) → Sig | K1 | `Forged(BadTag)` |
| `"8sig:UU2sw"` (5 chars after `:`) → Sig | K1 | `Malformed(Truncated{section: Tag})` (phase 1, unchanged) |
| `"8sig:-UU2sw1"` (undecodable body) → Sig | K1 | `Forged(BadTag)` (tag before fields) |

**Fail-closed `decode_route` (no keys):** `"8sig:1UU2sw1"` → Sig = `Forged(BadTag)`; `"0sig:1"` → Sig = `Forged(MissingTag)`; `"0shop.buy:47,3"` → Buy = `Ok`; `"0ss:~0123456789A"` → SpillSigned = `Ok(Ref(R0))`.

**`PayloadKey::from_hex` / `FromStr`:**

| Input | Result |
|---|---|
| `"01"×32` | `Ok`; signs `Sig{1}` like `from_bytes([1;32])` (`"…UU2sw1"`) |
| `"AB"×32` and `"ab"×32` | both `Ok`, identical tags (case-insensitive) |
| `"00"×32` | `Ok` (all-zero accepted) |
| `""` / 63 hex / 65 hex / 128 hex | `Length{0}` / `{63}` / `{65}` / `{128}` |
| 62 hex + `"é"` (64 bytes) | `InvalidChar{62}` |
| `"0x"` + 62 hex | `InvalidChar{1}` |
| 63 hex + `"g"`; `" "` + 63 hex; 63 hex + `"\n"` | `InvalidChar{63}`; `{0}`; `{63}` |
| 32 `"é"` (64 bytes) | `InvalidChar{0}` |

**Redaction:**

| Input | Result |
|---|---|
| `format!("{:?}", K)`, `format!("{K}")`, `format!("{K:#?}")` for `K = from_hex("ab"×32)` | `PayloadKey(<redacted>)`, `<redacted>`, `PayloadKey(<redacted>)` |
| `format!("{:?}", PayloadKeys::new(K).with_previous(K))` | contains `<redacted>` twice and neither `ab`, `AB`, `171` nor `[` |
| `KeyError` Debug/Display for every error row above | contains no input character other than digits of `len`/`index` |

**Tag internals:**

| Input | Result |
|---|---|
| `ct_eq` on equal tags; tags differing only at byte 0; only at byte 5 | `true`; `false`; `false` |
| proptest: random key bytes, random printable-ASCII `signed` (0..=94 chars) | `key.verify(s, key.sign(s))`; a tag with any one digit changed fails; `PayloadKeys::new(k2).with_previous(k)` verifies, `PayloadKeys::new(k2)` fails when `k ≠ k2` |
| proptest: `MixP` on signed fixtures with a random key | `decode_route_verified(encode(..))` round-trips; flipping any one char of the id gives `Err` (never `Ok` with a different Payload) |

</intent-contract>

## Code Map

- `Cargo.toml` (workspace) l.64 -- `blake3 = "1.8"` already pinned; `Cargo.lock` has blake3 1.8.7 (verified on crates.io 2026-10-19: latest stable, license `CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception`, all allowed by `deny.toml` l.21-32; already built for `xtask`). Use default features.
- `crates/noctis-codec/Cargo.toml` -- add `blake3.workspace = true` under `[dependencies]`.
- `crates/noctis-codec/src/parts.rs` -- l.247-279 `Tag` (private `[u8; 6]`, `from_u64` reduces mod 62^6 big-endian digits, `as_str`): add `ct_eq`. l.281-288 `Sign`: add `Verify` beside it; update `Sign`'s "Story 2.4 implements…" doc line to point at `PayloadKey`.
- `crates/noctis-codec/src/layout.rs`:
  - l.329-333: `encode` already passes `out.as_str()` (exactly the pre-`T` chars) to `sign`. Unchanged.
  - l.422-431 `tag()` / `signed_part()`: inputs of step 6.
  - l.438-497 `Decoded::payload`: move the body into `verified_payload` with the new steps 4 and 6; `payload` delegates with `NeverVerifies`. Update its `# Errors` docs.
  - l.704-717 `decode_route`: add `decode_route_verified`.
- `crates/noctis-codec/src/error.rs` -- l.229-240 `ForgedReason`: add `MissingTag`, `BadTag`. Add `KeyError` after `RefError` (l.302-312 pattern).
- `crates/noctis-codec/src/lib.rs` -- add `mod key;`, re-exports, "Integrity tags" doc section; update the "Two-phase decode" bullets (Forged now also covers missing/bad tags).
- `crates/noctis-codec/tests/common/mod.rs` l.92-110 `FakeSigner` -- also implement `Verify` (`tag.as_str() == Tag::from_u64(self.0).as_str()`) so 2.2/2.3 signed tests keep using it.
- `crates/noctis-codec/tests/layout.rs` -- existing expectations that change because `decode_route` is now fail-closed for signed Routes: l.275-278 (`"0sig:1"` → `Forged(MissingTag)`), l.311-322 (`"8ss:a000000"` and `"8sig:1000000"` → use `decode_route_verified(.., &FakeSigner::new(0))` to keep `Ok`; `"0ss:a"` → `Forged(MissingTag)`), l.368-378 `rt` helper and l.408-420 `round_trip` (use `decode_route_verified` with the same signer). `decode_total` (l.381-403) keeps calling `payload` (totality) and adds `verified_payload::<Sig>(&K1)`.
- `crates/noctis-codec/tests/refs.rs` l.190-196, l.390-400 -- signed spill tests; switch any `decode_route` of a signed inline id to the verified variant.
- `xtask/rules-pending.toml` -- no CID-06 entry exists (CID-06 is already covered by `cid_06_*` tests). Read-only.
- `.github/workflows-disabled/ci.yml` -- no new gate; existing `cargo test` covers this story. Read-only.
- Previous story 2.3 (done): its residual "tags are unverified" (from 2.2) is closed here; its deferred items (unsealed `Field`, Ref TTL/size, one-shot sharing) stay deferred and untouched.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-codec/Cargo.toml` -- add `blake3` -- keyed hash.
- `crates/noctis-codec/src/error.rs` -- add `KeyError`, `ForgedReason::{MissingTag, BadTag}` with doc lines and lower-case messages -- typed outcomes.
- `crates/noctis-codec/src/parts.rs` -- add `Verify` and `Tag::ct_eq` with unit test `tag_ct_eq` -- constant-time comparison.
- `crates/noctis-codec/src/key.rs` (new) -- `PayloadKey`, `PayloadKeys`, the tag formula (one private fn), `Sign`/`Verify` impls, `from_hex`/`FromStr`, redacted `Debug`/`Display`; rustdoc example signing and verifying an id; unit tests `payload_key_is_redacted`, `from_hex_matrix`, `golden_tags` -- AD-5 tag, AD-29 redaction.
- `crates/noctis-codec/src/layout.rs` -- `verified_payload`, `decode_route_verified`, private `NeverVerifies`, fail-closed `payload`/`decode_route`, updated docs -- FR-19.
- `crates/noctis-codec/src/lib.rs` -- `mod key;`, re-exports, crate docs.
- `crates/noctis-codec/tests/common/mod.rs`, `tests/layout.rs`, `tests/refs.rs` -- `FakeSigner: Verify`; update the changed expectations listed in the Code Map; keep every other 2.2/2.3 assertion.
- `crates/noctis-codec/tests/tags.rs` (new) -- `cid_06_signing_matrix`, `cid_06_tampered_ids_are_forged` (every `decode_route_verified` and fail-closed row), `cid_06_key_rotation`, proptests `cid_06_sign_verify`, `cid_06_signed_round_trip_and_single_char_tamper`. Commit any `proptest-regressions/`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already appended by planning. No implementer action.

**Acceptance Criteria:**
- Given a `PayloadKey` or `PayloadKeys`, when formatted with `{:?}`, `{:#?}` or `{}`, then the output is the redacted placeholder and contains no key material.
- Given a signed Route with an inline body and `Parts::with_signer(&key)`, when `encode` runs, then the id ends in 6 base62 characters equal to the formula's tag over every preceding character, and `F` has the `T` flag (the golden ids above).
- Given a tampered body, envelope or tag, a tag from an unknown key, or a missing tag on a signed Route's inline id, when `decode_route_verified` runs, then it returns `DecodeError::Forged(BadTag)` or `Forged(MissingTag)`; an id signed with the configured previous key returns `Ok`.
- Given no keys (`decode_route` / `Decoded::payload`), when a signed Route's inline id is decoded, then it is `Forged`, never `Ok`.
- Given the facade is untouched, when `api-snapshot`, `skill-check`, `deps-check` and `rules-catalog` run, then they pass with no re-bless and no pending-file change.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (logged to the memlog):
- **Truncation:** first 8 little-endian bytes of the keyed hash, reduced mod 62^6 by the existing `Tag::from_u64` (≈ 35.7 bits; bias ≈ 2^-28; forgery chance per guess ≈ 1.8·10^-11). Hashing exactly the pre-`T` characters binds `F`, route, `S`, `E`, `N` and body.
- **Missing `T` is `Forged`, a stray `T` stays `Stale`.** AD-5 bumps `schema` whenever `signed` changes, so a schema-matching untagged id of a signed Route cannot be legitimate. A tag on an unsigned Route grants nothing, so the 2.2 `Stale(Flags)` stays. A tampered `S` digit gives `Stale(Schema)`: no handler runs with the Payload.
- **Fail-closed instead of a new parameter.** `payload`/`decode_route` keep their signatures (≈130 call sites) and can never accept a signed inline id; core (Story 7.2) calls the `_verified` variants with the App's `PayloadKeys`. An App with signed Routes and no key is core's App-build error, not the codec's.
- **Key format:** `NOCTIS_PAYLOAD_KEY` is 64 hex digits; strict (no trimming) so a stray newline is reported, not silently accepted; errors carry only lengths and indices. No raw-byte accessor keeps the key from reaching adapters (NFR-6) by construction.
- **Constant time without a crate:** a 6-byte XOR-OR fold with `black_box`; the ring checks both keys without short-circuit.

```rust
let keys = PayloadKeys::new(current).with_previous(previous);
let id = encode::<Sig>(&p, &Parts::new().with_signer(&keys))?;
let got = decode_route_verified::<Sig>(&id, &keys)?; // Forged(BadTag) when tampered
```

## Verification

**Commands** (repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0, including `tests/tags.rs`, the proptests and doctests.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- rules-catalog` -- expected: `ok`, pending count unchanged.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, no diff (do not bless).
- `cargo run --release -p xtask -- skill-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.
