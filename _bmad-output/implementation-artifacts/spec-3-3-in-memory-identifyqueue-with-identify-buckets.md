---
title: 'Story 3.3: In-memory IdentifyQueue with identify buckets'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: 'bae92290d44eb553048f354b0f06d821a39afc0a'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-2-bounded-in-memory-store-defaults.md'
warnings: ['oversized']
deferred:
  - summary: >-
      PRD FR-2 says "Shards in the same bucket identify in parallel" and GW-01 says "buckets in order"; both contradict Discord's bucket semantics that Story 3.3 implements.
    evidence: |-
      prd.md:260-261 reads "With `max_concurrency = 16`, Shards in the same bucket identify in parallel"; docs/discord-rules.md:269 GW-01 reads "one per bucket per 5 s, buckets in order". The epics AC, the trait rustdoc and the Usage Skill (fixed in this story) say different buckets identify in parallel and each bucket once per 5 s. Pre-existing planning text; this story's spec forbids a discord-rules.md change. Risk: Epic 5's gateway work reads the wrong FR-2 consequence.
    location: >-
      _bmad-output/planning-artifacts/prds/prd-noctis-rs-2026-10-19/prd.md:260; docs/discord-rules.md:269
    severity: low
---

<intent-contract>

## Intent

**Problem:** `IdentifyQueue` (Story 3.1) is the only port without an in-memory default. Without one, shards cannot identify by Discord's buckets: `shard_id % max_concurrency`, one identify per bucket per 5 s, and different buckets in parallel (FR-2, FR-38; AD-14, AD-21; GW-01). Epic 5's gateway and Epic 12's Clusters consume it.

**Approach:** Add `MemoryIdentifyQueue` to `noctis_ports::memory`. It has one FIFO async mutex per bucket that holds the next allowed grant instant. The port docs gain the Cluster contract and the guide link. Re-export it through `noctis::ports::memory` and document it in the Usage Skill.

## Boundaries & Constraints

**Always:**

Dependencies:
- noctis-ports `[dependencies]` add `tokio = { workspace = true, features = ["sync", "time"] }`, written exactly so. That makes 8 deps: bytes, moka, postcard, serde, thiserror, tokio, twilight-cache-inmemory and twilight-model.
- The root entry `tokio = "1.53"` stays as is. It resolves to 1.54.0 in `Cargo.lock`, which crates.io shows as the latest, with MSRV 1.85. tokio's default features are empty.
- noctis-ports `[dev-dependencies]` add `tokio = { workspace = true, features = ["macros", "rt", "sync", "time", "test-util"] }`.

Purity (`crates/noctis-ports/tests/purity.rs`):
- `ALLOWED_DEPENDENCIES` gains `"tokio"`, kept sorted.
- `FORBIDDEN` loses the bare `"tokio"`.
- A new check: in every non-comment line of `src/`, each occurrence of `tokio` must be followed directly by `::sync::Mutex` or `::time::`. Any `tokio` at all in a file other than `src/memory/identify.rs` is a hit.
- A new manifest check: the `[dependencies]` tokio line must equal `tokio = { workspace = true, features = ["sync", "time"] }`.

`MemoryIdentifyQueue` (`crates/noctis-ports/src/memory/identify.rs`; `Send + Sync`, not `Clone`):
- `pub const IDENTIFY_INTERVAL: Duration = Duration::from_secs(5)`.
- `pub fn new(max_concurrency: NonZeroU16) -> Self`. The parameter type is `u16`, matching twilight `SessionStartLimit::max_concurrency`.
- `pub const fn max_concurrency(&self) -> NonZeroU16`.
- `Default` is `new(NonZeroU16::MIN)`, i.e. 1.
- A manual `Debug` prints `max_concurrency` and `interval` only.
- State: `buckets: Box<[tokio::sync::Mutex<Option<tokio::time::Instant>>]>` of length `max_concurrency`. Each slot is the earliest instant at which that bucket may grant next (`None` = now).
- `acquire(bucket)`:
  1. If `bucket >= max_concurrency`, return `Err(PortError::invalid_value(&bucket.to_string(), "bucket must be below max_concurrency"))` with no state change. Never fold the bucket with `%`.
  2. Otherwise lock the bucket's mutex.
  3. If the slot is `Some(t)`, `tokio::time::sleep_until(t).await`.
  4. Set the slot to `Some(Instant::now() + IDENTIFY_INTERVAL)`.
  5. Drop the guard and return `Ok(Lease::new(bucket))`.
- The guard is held across the sleep on purpose: tokio's `Mutex` is FIFO, so waiters in a bucket are granted in arrival order.
- A future dropped before step 4 changes nothing (cancel-safe).
- Use only `tokio::time::Instant`, never `std::time::Instant`, so a paused tokio clock drives it.
- Rustdoc includes a `# Panics` section: waiting needs a tokio runtime with the time driver. The first acquire in a bucket never waits. The rustdoc also states the Cluster contract.

Docs:
- The `IdentifyQueue` trait rustdoc (`src/identify.rs`) states: "A Cluster implementation must serialize per bucket across processes: at most one Lease per bucket per 5 s across the whole Cluster, different buckets in parallel. See the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md) (written with Story 14.6)."
- The rustdoc also names `memory::MemoryIdentifyQueue` as the in-process default and says it does not cover several processes.
- `docs/guides/README.md` names the future file `cluster.md`.
- Public API changes bless `xtask/public-api.txt` and update the template skill, its `SKILL.md` API hash and the template `AGENTS.md` (l.41 lists `MemoryIdentifyQueue` among the `noctis::ports::memory` defaults), all in the same commit.

**Never:**
- No App builder setter (`.identify_queue(..)` stays [planned]), no gateway wiring, no `max_concurrency` fetch (Epic 5), no Cluster or shared queue (Story 12.1).
- No `rt`, `net`, `fs`, `io-util`, `process`, `signal` or `macros` feature of tokio in `[dependencies]`.
- No `tokio::spawn`, task, thread, `std::time::Instant` or `unsafe` in `src/`.
- No change to `Lease` or to the `IdentifyQueue` signature. No new rule id, no `docs/discord-rules.md` change, no CI job.

## I/O & Edge-Case Matrix

These rows live in `crates/noctis-ports/tests/identify_queue.rs`. Every test is `#[tokio::test(start_paused = true)]` (current-thread, auto-advancing clock) unless marked. Times are `Instant::now() - start`, compared exactly. `Q(n)` means `MemoryIdentifyQueue::new(NonZeroU16::new(n).unwrap())`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Buckets of 16 (AC1) | `Q(16)` as `Arc<dyn IdentifyQueue>`; 32 spawned tasks, shard `s` acquires `s % 16` | shards 0..16 granted at 0 s; shards 16..32 at exactly 5 s; each `lease.bucket() == s % 16` | — |
| Serial (AC2) | `Q(1)`; 4 tasks spawned in order with `yield_now().await` between spawns, each acquires 0 | task i granted at `5*i` s (0, 5, 10, 15): FIFO, never two within 5 s | — |
| Sequential same task | `Q(1)`: acquire, acquire, acquire | grants at 0, 5, 10 s | — |
| Idle bucket | `Q(1)`: acquire, `sleep(10s)`, acquire | the second is granted at 10 s (no extra wait) | — |
| Exact boundary | `Q(1)`: acquire, `sleep(5s)`, acquire | the second is granted at 5 s (no wait) | — |
| Partial wait | `Q(1)`: acquire, `sleep(3s)`, acquire | the second is granted at 5 s (2 s wait) | — |
| Independent buckets | `Q(2)`: acquire 0, acquire 1, acquire 1 | 0 s, 0 s, 5 s; bucket 0 is not delayed by bucket 1 | — |
| Interval from grant | `Q(1)`: acquire, `sleep(7s)`, acquire, acquire | 0 s, 7 s, 12 s | — |
| Cancelled waiter | `Q(1)`: acquire; `timeout(1s, acquire(0))`; acquire | the timeout returns `Err(Elapsed)`; the third is granted at 5 s (the dropped waiter took nothing) | — |
| Highest bucket | `Q(16)` bucket 15; `Q(u16::MAX)` bucket 65534 | `Ok`, granted at 0 s | — |
| Bucket = max | `Q(16)` bucket 16; `Q(1)` bucket 1; `Q(u16::MAX)` bucket 65535 | `Err(PortError::InvalidValue { key, .. })` with `key` = the bucket in decimal ("16", "1", "65535") | no state change |
| Bucket u32::MAX | `Q(16)` bucket `u32::MAX` | `InvalidValue`, key `"4294967295"` | no state change |
| Error leaves state | `Q(1)`: acquire 7 (err), then acquire 0 | the acquire of 0 is granted at 0 s | — |
| Getters and default | `Q(16).max_concurrency()`; `MemoryIdentifyQueue::default().max_concurrency()`; `IDENTIFY_INTERVAL` | 16; 1; 5 s | — |
| Debug | `format!("{:?}", Q(16))` | contains `max_concurrency` and `16` | — |
| Swap via dyn | `Arc<dyn IdentifyQueue>` passed to a caller fn | compiles and behaves as above | — |
| Multi-thread (not paused) | `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`, `Q(64)`, 64 spawned tasks on distinct buckets | all `Ok` within 1 s of real time | — |

Non-numeric inputs (empty, unicode) do not apply: the only inputs are `NonZeroU16` and `u32`. 0 is unrepresentable for `max_concurrency`, and the type system covers it.

</intent-contract>

## Code Map

- `crates/noctis-ports/src/identify.rs` -- the trait and `Lease` (`new`, `bucket()`, `#[must_use]`, not `Clone`). Change only the docs.
- `crates/noctis-ports/src/lib.rs` -- `BoxFuture` (l.91-92) is `Pin<Box<dyn Future + Send + 'a>>`, so the async block must be `Send`; tokio's `MutexGuard` and `Sleep` are `Send`. The l.55-61 "# Defaults" paragraph says "every port except `IdentifyQueue`"; change it to every port.
- `crates/noctis-ports/src/error.rs` l.46 -- `PortError::invalid_value(key: &str, why)`.
- `crates/noctis-ports/src/memory/mod.rs` -- module doc l.1 ("except `IdentifyQueue`") and the `pub use` list. Add `mod identify; pub use identify::MemoryIdentifyQueue;` and a doc paragraph.
- `crates/noctis-ports/src/memory/trivial.rs` -- the style to copy for a small default (a `Box::pin(async move { … })` impl).
- `crates/noctis-ports/tests/purity.rs` -- `FORBIDDEN` l.6-16, `ALLOWED_DEPENDENCIES` l.21-29, the line scanner, and the `[dependencies]` parser (line form `name = …`).
- `crates/noctis-ports/tests/ports.rs` l.272-275, 395-397, 444-447 -- the existing stub and `Lease` tests. Leave them unchanged.
- `crates/noctis-ports/Cargo.toml` -- add deps as above.
- `crates/noctis/src/lib.rs` l.151-240 -- `pub mod ports`. Its doc line "[`memory`]: … every port except [`IdentifyQueue`]" becomes every port. Add `MemoryIdentifyQueue` to the `memory` re-export list and docs. Extend the doctest with `let q: Arc<dyn IdentifyQueue> = Arc::new(MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap()));` and assert `max_concurrency().get() == 16`. Do not await it, since the facade doctest has no runtime.
- `examples/project-template/.claude/skills/noctis/references/adapters.md`:
  - l.15: Default becomes `MemoryIdentifyQueue` (built from `max_concurrency`).
  - l.28: drop the "except `IdentifyQueue` … [planned]" clause. Add a table row and a short `MemoryIdentifyQueue` subsection: `new(NonZeroU16)`, `Default` = 1, `max_concurrency()`, `IDENTIFY_INTERVAL` 5 s, FIFO per bucket, an out-of-range bucket is `PortError::InvalidValue`, in-process only.
  - l.200-201 and §7 (l.410+): add the Cluster guide link sentence.
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` l.118-119 -- fix the wrong sentence "Shards with the same bucket identify in parallel" to "Shards in different buckets identify in parallel; each bucket identifies once per 5 s".
- `examples/project-template/.claude/skills/noctis/SKILL.md` l.21 -- the API hash.
- `xtask/src/skill_check/mention.rs` -- every new snapshot item's last segment (`MemoryIdentifyQueue`, `IDENTIFY_INTERVAL`, `max_concurrency`) must appear in the skill `*.md`.
- `docs/guides/README.md` -- name `cluster.md` as the Cluster guide's file.
- `examples/project-template/AGENTS.md` l.41 -- add `MemoryIdentifyQueue` to the listed defaults.
- `.github/workflows-disabled/ci.yml` -- unchanged; the existing jobs cover this story.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ports/Cargo.toml` -- tokio dep and dev-dep -- needed for the timer and the paused-clock tests.
- `crates/noctis-ports/tests/purity.rs` -- allow-list, scoped tokio-path rule, exact manifest line -- keeps layer 1 I/O-free.
- `crates/noctis-ports/src/memory/identify.rs` -- `MemoryIdentifyQueue` as specified.
- `crates/noctis-ports/src/memory/mod.rs`, `src/lib.rs`, `src/identify.rs` -- wiring and docs (the Cluster contract and the guide link).
- `crates/noctis-ports/tests/identify_queue.rs` -- the full matrix.
- `crates/noctis/src/lib.rs` -- re-export, docs and doctest.
- `docs/guides/README.md` and the template skill files listed in the Code Map -- docs and the hash.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`.

**Acceptance Criteria:**
- Given `MemoryIdentifyQueue::new(16)`, when shards `0..32` acquire with `bucket = shard_id % 16` on a paused tokio clock, then all 16 buckets grant concurrently at 0 s and each bucket's second lease comes exactly 5 s later.
- Given `max_concurrency = 1`, when shards acquire, then leases are granted 5 s apart in arrival order.
- Given the rustdoc of `IdentifyQueue` and `MemoryIdentifyQueue`, and the Usage Skill, when a Cluster implementer reads them, then they state that a Cluster implementation must serialize per bucket across processes, and they link the Cluster deployment guide (`docs/guides/cluster.md`).
- Given `noctis::ports::memory`, when a bot author builds `Arc<dyn IdentifyQueue>` from `MemoryIdentifyQueue`, then it compiles, and `api-snapshot`, `skill-check` and `purity` pass.

## Spec Change Log

## Design Notes

These decisions are logged in the memlog.
- **tokio in noctis-ports.** The wait needs a timer, and the AC demands a paused tokio clock, which only `tokio::time` honours. `sync` and `time` do no I/O, so layer 1 keeps "no I/O". The purity test narrows the allowance to those paths in one file. The alternative, putting the default in `noctis-gateway`, would break the epic's "every default lives in `noctis_ports::memory`".
- **`NonZeroU16`.** It matches Discord and twilight (`u16`) and makes `max_concurrency = 0` unrepresentable. The caller converts once at the boundary.
- **Out-of-range bucket is an error, not `%`.** The caller owns the bucket formula (GW-01). Silently folding would hide a wrong shard total.
- **Lock held across the sleep.** This gives FIFO order and per-bucket serialization for free. Memory is bounded at `max_concurrency` slots (at most 65535).

```rust
let slot = usize::try_from(bucket).ok().and_then(|i| self.buckets.get(i))
    .ok_or_else(|| PortError::invalid_value(&bucket.to_string(), "bucket must be below max_concurrency"))?;
let mut next = slot.lock().await;
if let Some(at) = *next { tokio::time::sleep_until(at).await; }
*next = Some(tokio::time::Instant::now() + Self::IDENTIFY_INTERVAL);
Ok(Lease::new(bucket))
```

## Verification

**Commands** (run from the repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0, including `identify_queue`, `purity`, `ports` and the facade doctests.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- expected: exit 0.
- `cargo tree --release -p noctis-ports -e normal --depth 1` -- expected: exactly the 8 deps.
- `cargo tree --release -p noctis-ports -e features -i tokio` -- expected: no `rt`, `net`, `fs`, `io-util` or `process` feature enabled from noctis-ports' normal edge.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot` and `-- skill-check` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.

## Review Triage Log

### 2026-10-19 — Review pass
- note: the staged review diff was the reverse of planning commit `bae9229` (`git diff bae9229 5311006`, `_bmad-output/` only); it contained no Story 3.3 code. Cause: the spec frontmatter carried two `baseline_revision` keys (`bae9229…` added by the start commit, `5311006…` from planning). Triage removed the duplicate `5311006…` key (frontmatter only) so the next staging diffs `bae9229..worktree`. The four layers therefore reviewed no Rust code; every finding about deleted files is refuted by the working tree.
- verdicts: 19 findings — high 1, medium 0, low 1, false 17, maybe-false 0
- findings:
  - `false` `reject` (blind) Patch deletes the Story 3.3 spec — the spec exists at HEAD `e97e347`; the real change since baseline adds it, the reversed diff only made it look deleted.
  - `false` `reject` (blind) epic-3-context loses the Story 3.2 moka / `MemoryDiscordCache` bullets — both bullets are present at `epic-3-context.md:41-42`.
  - `false` `reject` (blind) Story 3.3 decision erased from the append-only memlog — the decision line is present at `.memlog.md:127`.
  - `false` `reject` (blind) No record of why Story 3.3 is withdrawn — nothing is withdrawn; the story is implemented in `e97e347`.
  - `false` `reject` (blind) epic-3-context inconsistent about memory defaults — the bullets it says are gone are still there.
  - `false` `reject` (blind) Wrong "same bucket identify in parallel" sentence left untracked — the implementation fixed it (`app-and-sharding.md:119` now says "Shards in different buckets identify in parallel").
  - `high` `patch` (edge) Review diff is the reverse of the planning commit, so identify.rs, purity.rs, tests and docs went unreviewed — confirmed (`git diff --stat 5311006 bae9229` matches the reversed hunks); root cause the duplicate `baseline_revision` key, repaired by triage; action: re-stage the diff from `bae9229` to the working tree and run a follow-up review on the real code.
  - `false` `reject` (edge) Applying the diff deletes the spec — the diff is not the change; nothing applies it, and the spec is present.
  - `false` `reject` (edge) Removed epic-3-context lines 41-42 — present at HEAD.
  - `false` `reject` (edge) Removed memlog decision line — present at HEAD (`.memlog.md:127`).
  - `high` `patch` (edge) Diff adds no code, so the story would be accepted without implementation evidence — same root cause as the mis-staged diff; same action (re-stage and follow-up review).
  - `low` `reject` (edge) Dev-dep tokio enables `rt-multi-thread`, which the spec's exact feature list omits — real text divergence, but the spec's own multi-thread matrix row needs `rt-multi-thread`, so the code is right; the only fix is editing this build's spec.
  - `high` `patch` (vgap other) Diff is the reversed planning commit; Story 3.3 code not reviewed for verification gaps — same root cause as the mis-staged diff; same action.
  - `false` `reject` (intent 1) AC1 bucket concurrency not exercised — `crates/noctis-ports/tests/identify_queue.rs` (223 lines, `e97e347`) holds the matrix; absent only from the mis-staged diff.
  - `false` `reject` (intent 2) AC2 serialization not exercised — same evidence: covered by `identify_queue.rs` at HEAD.
  - `false` `reject` (intent 3) No Cluster-contract rustdoc / guide link — `crates/noctis-ports/src/identify.rs` and `memory/identify.rs` changed in `e97e347`; absent only from the mis-staged diff.
  - `false` `reject` (intent 4) No facade re-export or template update — `crates/noctis/src/lib.rs`, template skill files, `AGENTS.md` and `xtask/public-api.txt` change in `e97e347`.
  - `false` `reject` (intent 5) Change is purely subtractive — the real change is +641/-23 and additive; subtraction is an artifact of the reversed diff.
  - `false` `reject` (intent 6) No tests — `identify_queue.rs` and `purity.rs` change in `e97e347`.

### 2026-10-19 — Review pass
- verdicts: 19 findings — high 0, medium 2, low 8, false 9, maybe-false 0
- findings:
  - `false` `reject` (blind 1) Interval counts from the grant, not from the IDENTIFY sent; an unused Lease blocks its bucket 5 s — the spec settled grant-anchored timing (matrix row "Interval from grant"), the `Lease` doc says "one Lease, one identify", the skill says "counted from the previous grant"; no caller in the repo delays its identify (gateway wiring is Epic 5), and an unused Lease only makes the queue more conservative.
  - `false` `reject` (blind 2) Cluster guide URL is a dead link until Story 14.6 — the epics AC requires "linking the (future) Cluster guide" and the spec fixes the URL and the "(written with Story 14.6)" note; `docs/guides/README.md` names the file.
  - `medium` `patch` (blind 3) `distinct_buckets_on_many_threads` cannot fail under a global lock (each bucket acquired once, first acquire never waits) — confirmed in `tests/identify_queue.rs`; same root cause as the vgap finding (no test makes one bucket wait while another is acquired); action: add the staggered-deadline paused-clock test. The real-time 1 s flake claim is not established (64 uncontended first acquires).
  - `low` `reject` (blind 4) `buckets_of_16_...` relies on spawn-order polling without `yield_now` — tokio's current-thread scheduler (the `start_paused` flavor) polls spawned tasks FIFO, so the order is deterministic today, and a scheduler change fails the test loudly rather than hiding a bug; not met in everyday use, and the fix adds test machinery.
  - `low` `reject` (blind 5) No test cancels a waiter queued behind another, and `# Panics` is untested — queued-waiter cancellation is tokio `Mutex`'s own guarantee with no noctis code on that path; a no-runtime panic test needs a separate harness; unlikely to bite and the fix adds tests, not a correction.
  - `low` `patch` (blind 6) Skill snippet computes `shard_id % 16` with a literal and cannot call `max_concurrency()` after erasing to `Arc<dyn IdentifyQueue>` — confirmed at `references/adapters.md:108-112`; agents copy skill snippets; action: bind the `NonZeroU16` once and compute `shard_id % u32::from(max_concurrency.get())`.
  - `false` `reject` (blind 7) Spec frontmatter (`review_loop_iteration: 0`, `followup_review_recommended: false`) contradicts the triage log — this pass is the follow-up review the log asked for; the loop counter counts bad_spec loopbacks only and the follow-up flag is set at finalize; the fix would edit this build's spec.
  - `low` `reject` (blind 8) carried — dev-dep tokio enables `rt-multi-thread`, which the spec's exact list omits; the spec's own multi-thread row needs it, so the code is right and the only fix edits this build's spec.
  - `false` `reject` (blind 9) Purity checks are brittle (trailing comments, grouped tokio import, exact manifest line) — the spec mandates both rules exactly ("any `tokio` at all in another file is a hit", "the tokio line must equal …"); every mismatch fails loudly and names the file and line, so no violation passes silently.
  - `low` `patch` (blind 10) `docs/guides/README.md:4` runs to 121 columns while the file is wrapped — confirmed; direct correction; action: rewrap the edited paragraph to the file's width.
  - `low` `reject` (edge 1) One queue shared by runtimes with different clocks stores an Instant from another clock base — every caller builds one queue per App runtime; no path in the repo shares a queue across runtimes; the guard adds clamping logic for a state not shown reachable.
  - `false` `reject` (edge 2) Purity test does not forbid `std::time::Instant`, so a real-clock Instant could slip in silently — the slot type `Mutex<Option<tokio::time::Instant>>` rejects a std Instant at compile time, and a wholesale switch to std time breaks the exact paused-clock grant times in `identify_queue.rs`, so it cannot pass silently.
  - `low` `reject` (edge 3) carried — dev-dep tokio enables `rt-multi-thread` beyond the spec's list; same evidence as the earlier logged row.
  - `medium` `patch` (vgap) "Different buckets grant in parallel" is unverified while a bucket waits; a cross-bucket lock passes every test — pre-verified; grouped with blind 3; action: add `waiting_bucket_does_not_block_another` (paused clock, `queue(2)`: bucket 0 at 0 s, sleep 3 s, bucket 1 at 3 s, spawn bucket 1 + `yield_now` (granted 8 s), bucket 0 granted at exactly 5 s).
  - `false` `reject` (intent 1) App/gateway wiring (R2) not implemented or tested — the epics ACs are port-level, and the spec's Never list defers wiring and the `max_concurrency` fetch to Epic 5.
  - `false` `reject` (intent 2) `Default` (1 bucket) conflicts with AD-21/GW-01 "never `InMemoryQueue::default()`" — the spec settled `Default` = 1; one bucket is the most conservative queue and can never exceed Discord's limit; GW-01's rule is enforced at App-build (Epic 5), which builds from the fetched `max_concurrency`.
  - `low` `defer` (intent 3) PRD FR-2 "same bucket … in parallel" and GW-01 "buckets in order" still contradict the shipped semantics — pre-existing planning text (prd.md:260, discord-rules.md:269); the spec forbids a discord-rules.md change; deferred to the frontmatter list.
  - `false` `reject` (intent 4) Cluster guide link resolves to a missing file — same refutation as blind 2: the AC asks for a link to the future guide.
  - `false` `reject` (intent 5) Layer-1 purity relaxed by the same diff; spine does not allow tokio in layer 1 — the spine's layer-1 I/O column is "none (memory defaults only)" and tokio `sync`/`time` do no I/O; the AC demands a paused tokio clock, which the spec's design note resolves by allowing only those paths in one file and an exact manifest line.

## Auto Run Result

- **Summary:** Added `noctis_ports::memory::InMemoryQueue`, the default `IdentifyQueue`: `max_concurrency: NonZeroU16` buckets, each a `tokio::sync::Mutex<Option<tokio::time::Instant>>` held across the wait, granting one `Lease` per bucket per 5 s (interval counted from the previous grant), FIFO within a bucket, buckets in parallel; out-of-range bucket returns `PortError::invalid_value` (caller owns `shard_id % max_concurrency`, GW-01). `Default` = 1 bucket. Re-exported through the `noctis` facade; Cluster-contract rustdoc links the future Cluster guide (Story 14.6).
- **Files changed:**
  - `crates/noctis-ports/src/memory/identify.rs` - new `InMemoryQueue` implementation.
  - `crates/noctis-ports/src/memory/mod.rs`, `src/lib.rs`, `src/identify.rs` - module wiring, re-exports, Cluster-contract docs.
  - `crates/noctis-ports/Cargo.toml`, `Cargo.lock` - tokio (`sync`, `time` only) for layer 1.
  - `crates/noctis-ports/tests/identify_queue.rs` - paused-clock matrix incl. new `waiting_bucket_does_not_block_another`.
  - `crates/noctis-ports/tests/purity.rs` - allows tokio sync/time in one file with an exact manifest line.
  - `crates/noctis/src/lib.rs`, `xtask/public-api.txt` - facade re-export and API snapshot.
  - `examples/project-template/` (`AGENTS.md`, skill `SKILL.md`, `adapters.md`, `app-and-sharding.md`) - usage docs; bucket snippet fixed.
  - `docs/guides/README.md` - guide index entry, rewrapped.
- **Review findings:** pass 1 (diff mis-staged as reversed planning commit due to duplicate `baseline_revision` key; key removed): 3 high patched (same root cause, resolved by re-staging and a follow-up pass), 1 low rejected, 15 false rejected. Pass 2 (follow-up, real code): patched medium 2 (cross-bucket parallelism untested while a bucket waits -> added staggered paused-clock test), low 2 (skill snippet `% 16` literal -> `% u32::from(max_concurrency.get())`; README line rewrap); deferred low 1 (PRD FR-2 / GW-01 wording contradicts shipped bucket semantics; discord-rules.md change forbidden by spec); rejected low 5 (spawn-order polling, untested queued-waiter cancellation/panic, dev-dep `rt-multi-thread` x2, cross-runtime Instant) and false 9, each with reason in the triage log.
- **Follow-up review recommended:** false - follow-up pass patched high 0 (medium 2, low 2), so the work has converged.
- **Verification:** local gates via `gates.sh` all green (fmt, clippy --release -D warnings, test --release, cargo hack each-feature, deny, MSRV 1.91 check, dep tree checks, xtask deps-check/rules-catalog/api-snapshot/skill-check, build-config check, no `target/debug`). CI paused by maintainer.
- **Residual risks:** grant-anchored interval means an unused or delayed Lease holds its bucket conservatively; planning text (PRD FR-2, GW-01) still describes old bucket semantics until the deferred fix; gateway wiring and `max_concurrency` fetch land in Epic 5; Cluster guide link dead until Story 14.6.
