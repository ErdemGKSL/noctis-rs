# Epic 2 Context: Typed, length-proven `custom_id`s

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Give bot authors Routes and Payloads whose encoded `custom_id` is proven at compile time to fit Discord's 100-character limit. Refs, schema versions and integrity tags all go through one codec in `noctis-codec`, and that codec is the only code allowed to build or parse a `custom_id`. Length bugs then fail `cargo check` instead of breaking in production, and stale, malformed or forged ids become typed outcomes instead of panics.

## Stories

- Story 2.1: Payload field encodings
- Story 2.2: `custom_id` layout encoder and total decoder
- Story 2.3: Refs and two-phase `EncodedId`
- Story 2.4: Integrity tags for signed inline Payloads
- Story 2.5: `#[derive(Route)]`, `Payload` and `PayloadEnum` with compile-time length proof
- Story 2.6: Zero-allocation inline decode guard

## Requirements & Constraints

- Every field type round-trips exactly through encode and decode. Proptest covers every field type and composite field lists. Authors who implement `Field` for their own types get the same round-trip helper and width check.
- The decoder is total. Arbitrary input (non-ASCII, empty, 101+ chars, `F >= 16`) returns `Ok` or `CodecError` and never panics.
- Outcomes are distinct:
  - Malformed input is `CodecError`.
  - A flag or schema mismatch is `Stale`, which goes to the Route's stale handler and otherwise to the Expired Handler.
  - An unknown route, a bad tag or a Ref record whose route does not match is `Forged`, which takes the unknown-Route path.
- Schema evolution:
  - Appending a trailing `Option<T>` field is compatible: a missing trailing field decodes as `None` with no schema bump.
  - Any other Payload change, or a change to `view`/`select`/`signed`/`by_ref`, bumps `schema`.
- Refs are unguessable: 11 base62 characters from the OS CSPRNG (`getrandom`, at least 64 bits of entropy), never sequential and never derived from interaction data. A statistical test over 100k Refs checks that there are no duplicates and that characters are uniformly distributed.
- Fixed-width inline decode allocates nothing. A dhat test decodes 10k ids, asserts zero heap allocations, and runs in the required CI allocation job.
- The integrity key `PayloadKey` (32 bytes, from `NOCTIS_PAYLOAD_KEY`) is redacted in `Debug` and `Display`, never logged, and never passed to an adapter. One previous key still verifies during key rotation.
- Rule ids in scope: CID-01, CID-03, CID-05, CID-06, CID-07. Tests cite them.

## Technical Decisions

- Normative layout: `custom_id = F route ":" [S] [E] [N] body [T]`, ASCII only.
  - `F` is one base62 digit, `16*generation + flags` (S=1, E=2, N=4, T=8). Generation is 0 in v1.
  - `route` is `[a-z0-9._-]{1,24}`. Ids starting with `_` are reserved.
  - `S` is the schema version (1..=61). When `S` is absent, the schema is 0.
  - `E` is the 5-character View envelope. `N` is the 2-character select nonce.
  - The body is either inline (`field ("," field)*`) or by Ref (`"~"` followed by an 11-character Ref).
  - `T` is 6 base62 characters of truncated keyed BLAKE3 over all preceding characters. Only inline bodies of signed Routes carry it.
- `pub const` layout constants: `FMT_LEN=1`, `SEP_LEN=1`, `SCHEMA_LEN=1`, `VIEW_ENV_LEN=5`, `NONCE_LEN=2`, `REF_MARKER_LEN=1`, `REF_LEN=11`, `TAG_LEN=6`. `xtask rules-catalog` must assert that the catalog quotes the same values; remove the pending entry.
- Field widths (`Field { const MAX_LEN: Option<usize> }`):
  - `bool`: 1
  - `u8`/`i8`: 2
  - `u16`/`i16`: 3
  - `u32`/`i32`: 6
  - `u64`/`i64`/`Id<T>`: 11
  - `PayloadEnum`: 1 (at most 62 unit variants)
  - `Option<T>`: `T + 1`
  - `BoundedStr<N>`: `2N` (ASCII only, escape `!`)
  - `String`/`Vec<T>`: `None`

  Signed integers are zig-zag encoded in base62.
- Length bound: `FMT + route.len() + SEP + S + E + N + max(inline, 12)`, where `inline` is `Payload::MAX_ENCODED_LEN + T`. Codec `const fn`s compute it. The derive never inspects type names.
- `#[derive(Route)]` implements `Route { const ID; const SHAPE; const SCHEMA; type Payload }` and `Payload { MAX_ENCODED_LEN }`.
  - It emits `const _: () = ::noctis::__private::codec::assert_fits::<T>();` and a `#[test]` that prints the width breakdown.
  - Macro output names runtime items only through `::noctis::__private::<crate>` paths.
- These Routes are rejected at compile time:
  - generic Routes
  - Routes over 100 characters
  - unbounded fields without `by_ref`/`overflow = spill`
  - `by_ref` combined with `signed`
  - Routes with a bad route id
  - Routes with arguments outside AD-6's grammar
- Codec arguments in this epic: `by_ref`, `overflow = spill`, `schema = n`, `view`, `select`, `signed`. A `spill` Route encodes inline when the value fits and by Ref otherwise.
- The derive emits an empty `RouteRuntime` const. Later stories fill it: `ttl`/`one_shot` in 7.5, `owner`/`permissions`/`guard` in 7.7, `opens_modal` in 7.8.
- Two-phase Refs:
  - The codec yields `EncodedId::Inline(ArrayString<100>)` or `EncodedId::RefSlot(PendingRef { route, schema, payload: Bytes, ttl, one_shot })`.
  - `codec::finish(slot, ref)` builds the final id, which is at most 46 characters with a 24-character route.
  - `RefRecord { route, schema, payload }` is encoded with the same codec.
  - No crate below core touches `RefStore`.
- Decoding reads from `&str` slices. Strings decode to `Cow<'a, str>`, and encoding writes into `ArrayString<100>`.
- Compile errors use the format `[<RULE-ID>] <what is wrong>` plus a `help:` line.
  - trybuild fixtures live in `tests/ui/cid/`.
  - The exactly-100 pass case lives in `tests/boundary/`.

## Cross-Story Dependencies

- Depends on Epic 1: workspace, layer checks, `noctis-rules` ids and limits, the trybuild harness, rules-catalog and api-snapshot/skill-check gates.
- Stories 2.1 and 2.2 come first. Stories 2.3 and 2.4 extend the layout. Story 2.5 builds on all of them. Story 2.6 guards the decoder from 2.2.
- Story 2.5 changes the public API, so it must re-export the derives in the prelude and update the template and the Usage Skill's `references/routes-payloads-refs.md`.
- Later consumers:
  - `noctis-ui` builds components from routes (Epic 4/7).
  - Epic 7 handles Ref minting and resolution (7.5), stale and expired routing (7.3), and the Route lookup allocation test (7.1).
