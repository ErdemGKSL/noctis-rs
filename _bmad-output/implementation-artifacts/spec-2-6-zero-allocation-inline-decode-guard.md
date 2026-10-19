---
title: 'Story 2.6: Zero-allocation inline decode guard'
type: 'chore'
created: '2026-10-19'
status: 'in-progress'
baseline_revision: '106abb935372faa6c9a8b739c45bcdb14c91132c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** AD-25 and NFR-9 promise that decoding a fixed-width inline `custom_id` allocates nothing, but no test checks it. A future change (a `String` in an error, a `format!` in a check, an owned `Cow`) could add per-interaction heap churn without anyone noticing (UX-DR8).

**Approach:** Add one dhat counting-allocator integration test to `noctis-codec`. It decodes 10,000 valid inline ids of fixed-width Routes, plus a set of rejected ids. It asserts that the decode loop makes zero heap allocations, and it runs a negative control that proves the counter is live. Add an `Allocation` job to the paused `ci.yml` that runs this test. The decoder does not change unless the test finds an allocation.

## Boundaries & Constraints

**Always:**
- The test is `crates/noctis-codec/tests/alloc_decode.rs`. It is its own integration-test binary (its own process) and holds **exactly one** `#[test]` fn, `inline_decode_allocates_nothing`, as dhat requires (one profiler per process, and no parallel libtest threads). It has `#[global_allocator] static ALLOC: dhat::Alloc = dhat::Alloc;`, a `//!` crate doc, and `mod common;` to reuse the `payload!`/`route!` macros.
- dhat comes from the existing workspace entry `dhat = "0.3"` (resolves to 0.3.3, latest on crates.io, MIT OR Apache-2.0). Add it as `dhat.workspace = true` under `noctis-codec` `[dev-dependencies]` only. Commit the `Cargo.lock` change.
- Start the profiler as `dhat::Profiler::builder().testing().file_name(concat!(env!("CARGO_TARGET_TMPDIR"), "/dhat-alloc-decode.json")).build()`, so a failure profile lands under `target/` and never in the crate dir.
- Measure deltas. Build every input (`Vec<ArrayString<100>>`, the expected payloads, the keys) **before** taking `let before = dhat::HeapStats::get();`. After the loops, take `after` and call `dhat::assert_eq!(after.total_blocks - before.total_blocks, 0, …)`; the message names the count. Also assert `total_bytes` delta == 0.
- Inside the measured region:
  - Pass each result through `std::hint::black_box`.
  - Compare each Ok payload with its expected value using `==` (no formatting on success).
  - Count the Ok decodes and assert the count is exactly 10,000 after the region.
  - Check each rejected id against its expected error with `matches!`.
- Every Route's payload is fixed-width. Each Route has `const _: () = noctis_codec::assert_fits::<R>();` and `const _: () = assert!(<P as Payload>::MAX_ENCODED_LEN.is_some());`.
- Use the decode APIs bot dispatch will use: `decode_route::<R>` for unsigned Routes, and `decode_route_verified::<R>(id, &keys)` with a `PayloadKeys` ring for the signed Route. Also call the two-phase form `decode(id)?.payload::<R>()` for the `f` Route.
- Encode the ids with the crate's own `encode::<R>(&payload, &parts)` (with `Parts::new()`, `.with_envelope`, `.with_nonce`, `.with_signer(&keys)`) **outside** the measured region.

**Never:**
- No changes to decoder code unless the test finds a real allocation. If it does, fix the allocation in `noctis-codec`, never the test, and record the fix in Design Notes.
- No `serial_test`, no `--test-threads=1`, no `harness = false`, no second `#[test]` in the file, and no custom allocator in place of dhat.
- No encode-path guard and no Route-lookup guard: the Route lookup part is Story 7.1, which extends this job with core's test.
- No public API change. No `Cargo.toml` profile changes (no `debug = 1`). `.cargo/config.toml` stays untouched.
- No new rule id, no catalog or pending change, no template or skill change.

## I/O & Edge-Case Matrix

Fixture Payload `FixedP` (field order matters, `opt` is last):
`flag: bool, a: u8, b: i16, c: u32, d: i64, user: Id<UserMarker>, s: BoundedStr<8>, kind: Kind, opt: Option<u64>`.

`Kind` is a hand unit enum of 3 variants (`Kind::{A, B, C}`) with `Field` (`MAX_LEN = Some(1)`, `encode_variant`) and `FieldDecode` (`decode_variant(raw, 3)`).

Routes over `FixedP`:
- `F` = `"f"`, `INLINE`, schema 0
- `FS` = `"f.s"`, `INLINE`, schema 61
- `FV` = `"f.v"`, `INLINE.view().select()`, schema 0
- `FT` = `"f.t"`, `INLINE.signed()`, schema 0

Each Route gets 2,500 valid ids, 10,000 in all. Values come from a deterministic splitmix64 over the index, and the rows below are always included.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Mid values | generated `FixedP` on each Route | `Ok(PayloadSource::Inline(p))` equal to the source | 0 allocations |
| Min boundary | `flag=false, a=0, b=0, c=0, d=0, user=1, s="", kind=A, opt=None` | Ok, equal | 0 allocations |
| Max boundary | `true, u8::MAX, i16::MIN, u32::MAX, i64::MIN, user=u64::MAX, s="!,:~!,:~" (8 escape chars), kind=C, opt=Some(u64::MAX)`; on `FT` (longest) | Ok, equal; its body is 71 chars = `MAX_ENCODED_LEN` (asserted once before the region; `FT` id = 82 chars) | 0 allocations |
| Signed max | `i64::MAX`, `i16::MAX` | Ok, equal | 0 allocations |
| Trailing Option absent | `F` id with `opt=None` and the trailing `,0` stripped (every 10th `F` id) | Ok, `opt == None` (the `FieldDecode::absent` path) | 0 allocations |
| Schema section | `FS` ids (S digit `z`) | Ok | 0 allocations |
| E/N sections | `FV` ids with `ViewEnvelope::new('a', 61, 3843, 61)` / `SelectNonce::new(3843)`, plus mid values | Ok, and `decode(id)` exposes the same envelope and nonce | 0 allocations |
| Signed, current key | `FT` id signed by `PayloadKeys::new(k1)`, verified by `PayloadKeys::new(k1).with_previous(k0)` | Ok | 0 allocations |
| Signed, previous key | `FT` id signed by `k0` (every 5th `FT` id), same ring | Ok | 0 allocations |
| Route mismatch | an `F` id decoded as `FS` | `Err(Forged(RouteMismatch))` | 0 allocations |
| Schema mismatch | an `FS` id with its S digit removed and the `F` char set to `0` (S flag cleared), decoded as `FS` | `Err(Stale(Schema{expected:61,found:0}))` | 0 allocations |
| Missing tag | an `FT` id with the T section and flag removed | `Err(Forged(MissingTag))` | 0 allocations |
| Bad tag | an `FT` id with its last tag char changed | `Err(Forged(BadTag))` | 0 allocations |
| Wrong key ring | a valid `FT` id verified by `PayloadKeys::new(k2)` | `Err(Forged(BadTag))` | 0 allocations |
| Ref body on an inline Route | `"0f:~"` + 11 base62 chars, as `F` | `Err(Stale(Body{..}))` | 0 allocations |
| Field overflow | `F` id with `a` = `"4I"` (62*4+18 = 266 > 255) | `Err(Malformed(Field{..}))` | 0 allocations |
| Extra field | a valid `F` id + `",1"` | `Err(Malformed(ExtraField{..}))` | 0 allocations |
| Unknown enum index | `kind` digit `3` | `Err(Malformed(Field{ error: Overflow, .. }))` | 0 allocations |
| BoundedStr unicode escape | `s` = `"!u0000e9"` | `Err(Malformed(Field{..}))` (`BadEscape`) | 0 allocations |
| Empty | `""` | `Err(Malformed(CodecError::Empty))` | 0 allocations |
| Max+1 length | the max-boundary `FT` id padded to 101 bytes | `Err(Malformed(TooLong{len:101}))` | 0 allocations |
| Exactly 100 bytes, junk | `"0f:"` + 97 × `x` | `Err(Malformed(_))` (field error) | 0 allocations |
| Non-ASCII | `"0f:é"` | `Err(Malformed(NotAscii{index:3}))` | 0 allocations |
| Unknown generation | `F` char `'G'` (16) | `Err(Malformed(Generation{..}))` | 0 allocations |
| No separator | `"0f"` | `Err(Malformed(MissingSeparator))` | 0 allocations |

Each rejected row decodes 100 times inside the measured region.

**Negative control:** after the zero assertions, take a fresh `HeapStats` snapshot. Decode `common::Text` (`String` field) with the body `"a!0b"` once (an escape forces `Cow::Owned`), then `dhat::assert!(delta.total_blocks >= 1)`. This proves the allocator is installed and counting, so the guard cannot pass vacuously.

Pin every expected error variant against `src/error.rs` and adjust a row's input when the decoder legitimately reports an earlier check. The order is the one documented on `decode` and `verified_payload`.

</intent-contract>

## Code Map

- `crates/noctis-codec/Cargo.toml` -- `[dev-dependencies]` has only `proptest`. Add `dhat.workspace = true`.
- `Cargo.toml` l.82 -- `dhat = "0.3"` already exists in `[workspace.dependencies]` but is unused, so it is not in `Cargo.lock` yet.
- `crates/noctis-codec/tests/common/mod.rs` -- the `payload!` macro (l.9-38; derives `Debug, Clone, PartialEq, Eq`), the `route!` macro (l.41-52), the `TextP`/`Text` String Route for the negative control, and `FakeSigner` (unused here). It has `#![allow(dead_code)]`.
- `crates/noctis-codec/src/layout.rs`:
  - `encode` l.269 (writes `ArrayString<100>`);
  - `decode` l.684 (total phase 1);
  - `Decoded::payload`/`verified_payload` l.455/481, where the check order is documented;
  - `decode_route`/`decode_route_verified` l.762/776.
- `crates/noctis-codec/src/error.rs` -- every error is `Copy`, with no heap. `DecodeError::{Malformed(CodecError), Stale(StaleReason), Forged(ForgedReason)}`; the `CodecError` variants are at l.116+.
- `crates/noctis-codec/src/bounded_str.rs` l.218-237 -- decodes into `[u8; N]` with no allocation. `src/escape.rs` l.77 `unescape` allocates only on `!` for `String`/`Cow`, which the negative control uses.
- `crates/noctis-codec/src/derive.rs` -- `encode_variant` l.298, `decode_variant` l.313, `assert_fits` l.271, `sum_fields` l.32.
- `crates/noctis-codec/src/parts.rs` -- `ViewEnvelope::new` l.119, `SelectNonce::new` l.218. `src/key.rs` -- `PayloadKey::from_bytes` l.54, `PayloadKeys::new`/`with_previous` l.140/149.
- `crates/noctis-codec/src/lib.rs` l.1-142 -- the crate docs. Add an "Allocation" paragraph.
- `.github/workflows-disabled/ci.yml` -- the paused CI. Each job installs the pinned toolchain plus `Swatinem/rust-cache@v2`. Add the job after `test`.
- `clippy.toml` -- `allow-unwrap-in-tests`/`allow-expect-in-tests = true`. The workspace lints deny `unsafe_code` (`#[global_allocator]` needs no `unsafe`) and warn `missing_docs`.
- dhat 0.3.3 source (checked):
  - in testing mode, the drop writes no file;
  - a failing `dhat::assert*` saves the profile to `file_name` and panics;
  - `HeapStats::get()` excludes dhat's own allocations;
  - `HeapStats { total_blocks: u64, total_bytes: u64, … }`.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-codec/Cargo.toml`, `Cargo.lock` -- add the dhat dev-dependency, then run `cargo update -p dhat --precise 0.3.3` if needed -- the counting allocator.
- `crates/noctis-codec/tests/alloc_decode.rs` (new) -- the single test covering every matrix row and the negative control, with `Kind` and `FixedP`/Routes declared via `common` macros -- the AC1 guard.
- `crates/noctis-codec/src/lib.rs` -- crate-doc paragraph "# Allocation": fixed-width inline decode (`decode`, `decode_route`, `decode_route_verified`, the fields with `MAX_LEN = Some`) allocates nothing; `String`, `Cow` with escapes and `Vec` fields may; this is guarded by `tests/alloc_decode.rs` (AD-25, NFR-9) -- docs. The doc text changes no API.
- `.github/workflows-disabled/ci.yml` -- new job `allocation` (`name: Allocation`), using the same setup steps as `test`, that runs `cargo test --release -p noctis-codec --test alloc_decode`. Add a comment: "Story 7.1 adds the Route lookup test to this job." -- AC2 (CI stays paused).
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already has the planning decision line. Add an implementation decision only if one arises.

**Acceptance Criteria:**
- Given `crates/noctis-codec/tests/alloc_decode.rs`, when `cargo test --release -p noctis-codec --test alloc_decode` runs, then it passes:
  - it decoded exactly 10,000 valid inline ids of the fixed-width Routes, plus the rejected rows;
  - the heap-block and byte deltas over the decode region are 0;
  - the negative control saw at least 1 allocation.
- Given a temporary allocation added to the decode path (e.g. `let _ = raw.to_owned();` in `decode`), when the same command runs, then it fails with the dhat assertion naming the nonzero block count. Revert the probe afterwards; this check is manual and is recorded in the Auto Run Result.
- Given `.github/workflows-disabled/ci.yml`, when it is read, then a job named `Allocation` runs exactly that command with the pinned toolchain. `cargo test --release --workspace` also runs the test, and it passes under `cargo hack test --release --workspace --each-feature`.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Location (decision, memlog).** The epics place the test in `noctis-codec`, and AD-25 lists a core bench that covers lookup plus decode. The codec test guards the decoder now. Story 7.1 adds core's Route-lookup test to the same `Allocation` job, and its 7.1 AC wording "extended from Story 2.6" refers to that job.
- **Why one `#[test]` and a delta.** dhat panics on a second profiler, and libtest only runs single-test binaries without threads. The before/after delta also excludes the harness's setup allocations. The negative control guards against a silently missing `#[global_allocator]`.
- **Why hand-written Routes, not the derive.** Derive output names `::noctis::__private`, and codec must not depend on the facade. The `payload!` macro generates the same `Payload`/`PayloadDecode` shape the derive emits (`f.field()?` per field).

## Verification

**Commands** (repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo test --release -p noctis-codec --test alloc_decode -- --nocapture` -- expected: 1 passed.
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0 (dhat's tree is MIT/Apache/Zlib-compatible; `multiple-versions` only warns).
- `cargo +1.91 check --release --workspace --all-targets` -- expected: exit 0 (MSRV with dhat's tree).
- `cargo run --release -p xtask -- deps-check` and `-- rules-catalog` and `-- api-snapshot` and `-- skill-check` -- expected: exit 0, snapshot unchanged.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.

**Manual checks:**
- The probe AC above: the test fails with a probe in place and passes once the probe is reverted.
