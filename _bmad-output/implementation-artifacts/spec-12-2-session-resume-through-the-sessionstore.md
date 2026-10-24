---
title: 'Story 12.2: Session resume through the SessionStore'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: 'a52191427b308cb41899738066968e35f3769e42'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The `SessionStore` port exists (memory default, namespaced `<app>:session:`) but nothing reads or writes it: every process start identifies every shard, spending the daily identify budget (GW-02) and losing the resumable session a graceful (code 4000) close left behind.

**Approach:** Make `noctis-gateway` the sole reader and writer of the SessionStore (AD-21): each shard task loads its stored `(session_id, seq, resume_url)` before connecting and, if valid, starts twilight with that session (RESUME, no IDENTIFY, no `IdentifyQueue` lease); a rejected stored session is deleted and twilight falls back to IDENTIFY; a requested resumable close persists the session after the gateway's close frame is observed, a fresh close deletes it. Core passes the App's namespaced port through `GatewayOptions`, counts RESUMED as startup-ready, and a new `deps-check` rule proves no other crate calls the port. FR-4, AD-21, AD-14, GW-02.

## Boundaries & Constraints

**Always:**
- Key (before the `Namespaced` prefix): `shard:{id}:{total}` (decimal, global id, e.g. `shard:3:4`; the store sees `default:session:shard:3:4`). Value: `noctis_ports::encode_value(1, &StoredSession { session_id: String, sequence: u64, resume_url: String })` (postcard, field order as written; private serde struct in the gateway). TTL `None` (a stale entry costs one rejected RESUME and is then deleted).
- A loaded entry is valid only if: it decodes with schema 1 (no trailing bytes), `session_id` is 1..=256 bytes, `resume_url` is 1..=2048 bytes and passes the same check as `AppBuilder::gateway_url` (`shard_config::check_gateway_url`, made `pub(crate)`). twilight `expect`s a valid URL at connect, so an unchecked stored URL could panic the shard task: never hand twilight an unchecked URL.
- Resume uses twilight's own path: `ConfigBuilder::from(config).session(Session::new(seq, id)).resume_url(url).build()` then `Shard::with_config`. twilight sends RESUME at HELLO and only enqueues on the `IdentifyQueue` when it has no session, so a resumed shard takes no lease and sends no IDENTIFY.
- Persist only on `CloseMode::Resumable`, only for an identified shard, only after `Message::Close` from the gateway was observed (spine AD-21), using `shard.session()` (sequence = last received) and `shard.resume_url()`; skip when either is `None`. The ack wait and the `put` share one deadline (`close request + close_timeout`), so `ShardEvent::Closed` still comes within `close_timeout` and core's 6 s wait is unchanged. `put`/`delete` finish before `Closed` is sent.
- `get` and the reject-time `delete` are bounded by a private `STORE_TIMEOUT = 5 s` each; any store error or timeout only logs and the shard identifies; never a panic, never a retry loop.
- Logs (target `noctis::gateway`, inside the existing `noctis.shard` span) never contain the session id, the resume URL, the stored bytes or the token; they may name the shard, the sequence and the `PortError` Display.
- `rust-best-practices` applies (read `references/async-and-concurrency.md`, `error-handling.md`, `serde.md`, `testing.md`).

**Never:**
- No `--no-resume` flag, no start-time clearing, no change to core's shutdown order or `SHARD_CLOSE_TIMEOUT` (Story 12.3). No writes on READY, on in-process reconnects, on `Ended`, or periodically (a crash leaves no entry; documented).
- No change to the GW-02 boot check: it still counts every shard of the range, because a rejected resume falls back to IDENTIFY (documented in GW-02's How text).
- No new public item in the facade `noctis` (public-api.txt and the Skill's `API hash` stay unchanged; `api-snapshot` must still pass). `GatewayOptions` (internal crate) gains one required field. No new crates or versions (serde derive, postcard via `noctis-ports`, twilight 0.17.1 are already in the workspace). No new rule ids, no `Enforcement` column change in `docs/discord-rules.md`.
- No `SessionStore` call in any crate other than `noctis-gateway` (and `noctis-ports`, which defines it); core only passes the `Arc`.

## I/O & Edge-Case Matrix

Load at shard start (`session::load`, unit tests with a stub `SessionStore`; paused clock for timeouts):

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| L1 | miss | `get` → `None` | identify as today; no `delete` | none |
| L2 | valid hit | `{ "s1", 7, "ws://127.0.0.1:9" }` | shard starts with session (seq 7) + resume URL; `info` `resuming a stored session` (fields `shard`, `sequence`) | none |
| L3 | key | shard 0 of 1; shard 4294967294 of 4294967295 | keys `shard:0:1`, `shard:4294967294:4294967295` | — |
| L4 | empty bytes / other schema byte (2, 0, 255) / malformed / trailing byte | — | entry deleted, identify; `warn` `stored session unreadable; identifying` | delete failure only warns |
| L5 | session_id bounds | `""`; 1 byte; 256 bytes; 257 bytes; `"セッション"` (15 bytes) | `""` and 257 → invalid (deleted, identify); 1, 256, unicode → accepted | — |
| L6 | resume_url accepted | `wss://gateway-us-east1-b.discord.gg`, `WSS://X.Y`, `ws://127.0.0.1:3000`, exactly 2048 bytes (`wss://h/` + path) | accepted | — |
| L7 | resume_url rejected | `""`, `http://x`, `gateway.discord.gg`, `ws://a b`, `ws:/x`, `wss://dïscord.gg`, 2049 bytes | invalid → deleted, identify, no panic | — |
| L8 | sequence bounds | 0, `u64::MAX` | accepted verbatim (RESUME `d.seq`) | — |
| L9 | store error | `get` → `PortError::Backend` | identify; `warn` `session store read failed; identifying` with the error; no delete | — |
| L10 | store hangs | `get` never resolves | after exactly 5 s: identify, `warn` (timeout) | — |
| L11 | close during load | `close_all` while `get` pending | task reports `Closed`, opens no connection, sends no frame | — |
| L12 | other total | entry under `shard:0:2`, now total 4 | miss for `shard:0:4`; old entry untouched | — |

Resume outcome (`ResumeState` unit tests plus Gateway-level tests against `FakeGateway`):

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| R1 | resumed | fake knows the session (`ResumeReply::Auto`) | op 6 RESUME (`d.session_id`, `d.seq` = stored), no op 2 and no `IdentifyQueue::acquire`; `ShardEvent::Resumed`; entry kept | — |
| R2 | rejected | `ResumeReply::InvalidSession { resumable: false }` or an unknown session on `Auto` | `delete(key)` once, then IDENTIFY through the queue, READY → `ShardEvent::Ready`; `info` `stored session rejected; identifying` | delete failure only warns |
| R3 | op 9 resumable | op 9 `d: true` while pending | no delete (twilight resumes again) | — |
| R4 | READY while pending | session superseded without op 9 | `delete(key)` once | — |
| R5 | later frames | a second op 9 `false` or READY after R2/R4, or any after R1 | no further store op | — |
| R6 | op 9 parse | `{"op":9,"d":false}`, `{"op":9,"d":true}`, `{"op":9,"d":null}`, `{"op":9}` | `Classified.invalid_session` = `Some(false)`, `Some(true)`, `None`, `None` | — |

Write at close (Gateway-level tests, `gateway_close.rs`-style helpers):

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| W1 | resumable, acked | READY (s 1) + 2 dispatches (s 3), `close_all(Resumable)` | `put(shard:0:1, {fake-session-1, 3, fake url}, None)` before `Closed`; frame 4000 | — |
| W2 | after a resume | R1 then 1 dispatch, resumable close | `put` with the new sequence | — |
| W3 | ack timeout | silent server, identified, resumable | no `put`; `debug` `close not acknowledged; session not stored` | — |
| W4 | not identified | close before READY | no `put`, entry untouched | — |
| W5 | fresh | `close_all(Fresh)`, identified or not | `delete(key)`, no `put`; frame 1000 when identified | failure only warns |
| W6 | put fails / hangs | `put` → error, or never resolves | `warn`; `Closed` still within `close_timeout` + 100 ms | — |
| W7 | no request | fatal close (`Ended`), or in-process reconnect | no store op | — |

App level (testkit, `FakeDiscordHttp` + `FakeGateway`, one `Arc` store shared by two sequential Apps):

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| A1 | restart resumes | App 1 runs (1 shard), dispatch 1 event, `run_until` returns; App 2 same fakes and store | App 2: new connection sends op 6 with `fake-session-1`, `seq` 2; zero op 2 after the restart; recording `IdentifyQueue` 0 acquires; `run_until` startup completes (lifecycle `ShardResumed { shard: 0 }`) | — |
| A2 | restart, rejected | App 2 against a new `FakeGateway` | RESUME, op 9 false, exactly 1 IDENTIFY, READY; App 2 store ops `get, delete, …, put` (delete before put) | — |
| A3 | two shards | total 2, both stored | both resume, 0 identifies | — |
| A4 | memory default | App 2 without the shared store | identifies (memory store is per App) | — |
| A5 | only the gateway | recording store over A1 | ops exactly `get` then `put` per run, keys only `default:session:shard:0:1` | — |
| A6 | redaction | A1 with a capturing subscriber (`noctis::*` targets, debug) | no `fake-session-`, no fake URL, no token in `noctis::` lines | — |
| A7 | deps-check | a `noctis-core` source line `self.ports().session_store.get(`; a `src/**/tests.rs` file with `SessionStore::get(` | first reported as `session-store`, second exempt; the real workspace passes | — |

</intent-contract>

## Code Map

- `crates/noctis-gateway/src/session.rs` (new, private) -- `StoredSession` (serde), `SCHEMA = 1`, `MAX_SESSION_ID = 256`, `MAX_RESUME_URL = 2048`, `STORE_TIMEOUT = 5 s`, `key(ShardId) -> String`, `load(&dyn SessionStore, ShardId) -> Option<(Session, String)>` (L1-L12), `save(.., &Session, &str, deadline)`, `clear(.., deadline)`, `ResumeState { pending: bool }` with `fn on_frame(..) -> bool /* delete now */` (R3-R5). Unit tests here.
- `crates/noctis-gateway/src/gateway.rs` -- `GatewayOptions` (`:66-100`): add `pub session_store: Arc<dyn SessionStore>` (doc: read at shard start, written on resumable close, deleted on rejection/fresh close; core passes the App's namespaced port); `Debug` prints `session_store: ..` without content. `Gateway::start` (`:180-224`): pass `id`, `config.clone()`, `Arc::clone(&options.session_store)` to `shard::run` instead of a built `Shard`. Add the field to the 6 doctests and `tests::options`.
- `crates/noctis-gateway/src/shard.rs` -- `run` (`:160`): load (select against `close_requested`, L11 → `Closed`) then build the shard; `Classified` (`:27`) gains `pub(crate) invalid_session: Option<bool>` (R6) set for op 9 in `classify`; `drive` (`:196`) feeds READY / RESUMED / op 9 to `ResumeState` and awaits `clear` when it says so; `close_shard` (`:248`) gets store + key: deadline, ack, then `save` (Resumable+acked) or `clear` (Fresh, also for a non-identified shard), per W1-W6.
- `crates/noctis-gateway/src/shard_config.rs:87` -- `check_gateway_url` → `pub(crate)`.
- `crates/noctis-gateway/src/lib.rs` -- crate doc: new `# Sessions` section (key, when read/written/deleted, at-most-once note); module `session`.
- `crates/noctis-core/src/app.rs` -- `run_with` `GatewayOptions` (`:814`): `session_store: Arc::clone(&self.ports().session_store) as Arc<dyn SessionStore>`; `await_startup` (`:1086`): `Ready | Resumed` both mark the shard ready (debug `shard ready` / `shard resumed`); `run_until` rustdoc mentions resume. Builder `session_store` setter doc (macro row `:2186`): "must persist across restarts to resume".
- `crates/noctis-core/src/app/tests.rs:2513` -- add field; add a unit test that `await_startup` completes on `Resumed`.
- `crates/noctis-testkit/tests/{lanes.rs:49,shard_log_redaction.rs:82,sharding.rs:527,sharding.rs:613,gateway_close.rs:58}` -- add `session_store: Arc::new(MemorySessionStore::new())`.
- `crates/noctis-testkit/tests/session_resume.rs` (new) -- R1, R2, W1-W7 against `Gateway::start` (copy `gateway_close.rs` helpers `Counting`, `install_provider`, `options`, `all_ready`, `silent_server`) and A1-A5 with sharding.rs-style `Fakes`/`builder()` plus a `Recording` store (`Mutex<Vec<(Op, String)>>` over `MemorySessionStore`) and a recording `IdentifyQueue` (`sharding.rs:180`). Lifecycle via `app.subscribe_lifecycle()`.
- `crates/noctis-testkit/tests/session_log_redaction.rs` (new binary, global capturing subscriber like `shard_log_redaction.rs`) -- A6.
- `xtask/src/deps_check.rs` -- rule `Rule::SessionStore` id `session-store`, mirroring `http-client` (`:56`, `:252`, `:483`): scan `src/**/*.rs` of every workspace member except `noctis-gateway`/`noctis-ports`, skipping files named `tests.rs` and paths with a `tests` directory component; patterns `session_store.get(`, `session_store.put(`, `session_store.delete(`, `SessionStore::get(`, `SessionStore::put(`, `SessionStore::delete(`; message "`<file:line>` calls the SessionStore; only noctis-gateway reads and writes sessions (AD-21)". Module doc paragraph + tests beside `scan_finds_every_constructor_line` (`:1083`) (A7).
- `docs/discord-rules.md:293` -- GW-02 How text: append "a shard with a stored session resumes without an IDENTIFY (FR-4, `SessionStore`); the check still counts it, because a rejected resume falls back to identify."
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- §4 `### Session resume` (`:529-539`): key `<app>:session:shard:<id>:<total>`, read at shard start, written after the 4000 close is acknowledged, deleted on rejection or fresh close, crash leaves nothing, GW-02 still counts resumed shards, memory default only resumes when the same store reaches the next App; drop the stale `[planned, Story 8.6]` (also `:112`); §8 step 2 (`:637-639`): the session write happens now, only `--no-resume` stays planned (12.3). `references/adapters.md:14` and `:522`: same facts, stored bytes are opaque and must be returned verbatim.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions appended; add implementation ones.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-gateway/src/{session.rs,shard_config.rs}` -- store codec, validation, timeouts, `ResumeState` + unit tests (L1-L12, R3-R6).
- `crates/noctis-gateway/src/{shard.rs,gateway.rs,lib.rs}` -- load before connect, reject handling, persist/clear at close, `GatewayOptions::session_store`, docs/doctests.
- `crates/noctis-core/src/{app.rs,app/tests.rs}` -- pass the port, Resumed counts as ready, docs, literal fix.
- `crates/noctis-testkit/tests/*` -- literal fixes; `session_resume.rs` (R1, R2, W1-W7, A1-A5); `session_log_redaction.rs` (A6).
- `xtask/src/deps_check.rs` -- `session-store` rule + tests (A7).
- `docs/discord-rules.md`, Skill `app-and-sharding.md`, `adapters.md` -- behaviour docs.

**Acceptance Criteria:**
- Given an App that shut down gracefully with a SessionStore shared with the next App, when the next App starts within the resume window (`FakeGateway`), then each shard sends RESUME instead of IDENTIFY, takes no `IdentifyQueue` lease, and startup completes on RESUMED (GW-02 budget untouched).
- Given a stored session the gateway rejects, when the shard starts, then it identifies with no operator action and the entry is deleted before any later write.
- Given the workspace, when `cargo run --release -p xtask -- deps-check` runs, then it passes, and it reports any `SessionStore` call outside `noctis-gateway`; a whole run's store ops are exactly the gateway's `get`/`put`/`delete` on `shard:` keys.
- Given the story is done, then the Skill documents when sessions are read, written and deleted, and `skill-check`, `api-snapshot` (unchanged hash) and `rules-catalog` pass.

## Spec Change Log

## Review Triage Log

### 2026-10-23 — Review pass
- verdicts: 33 findings — high 0, medium 5, low 19, false 8, maybe-false 1
- findings:
  - `[medium]` `[patch]` (blind) A resumed entry is kept, so "a crash leaves no entry" is false and a crash can replay handled events — verified: `ResumeState::on_frame` keeps the entry on RESUMED (R1, intent), lib.rs/app-and-sharding.md/adapters.md claim a crash leaves nothing; a crash after a resume leaves the old-seq entry and the next start resumes from it. R1 settles keeping, so the fix corrects the three doc claims.
  - `[false]` `[reject]` (blind) An unreachable stored `resume_url` has no fallback and startup hangs — twilight 0.17.1 `poll_connect` sets `self.resume_url = None` on a failed connect and reconnects to `proxy_url`/default, so the shard resumes (or is rejected) on the configured URL.
  - `[low]` `[reject]` (blind) The inline reject-time `clear` blocks the shard loop up to 5 s, delaying a close — real only with a hanging store at the moment of a rejection plus a concurrent shutdown; the fix adds a select branch and a cancelled-delete path; rare, not worth the complexity.
  - `[medium]` `[patch]` (blind) The early-close branch in `run` (close while `load` pending) has no test — same gap as the vgap L11 finding; L11 is in the matrix and no test has a `get` outstanding at close. Patch: Gateway-level test with a hanging `get`, both modes.
  - `[low]` `[reject]` (blind) The `session-store` deps-check rule is lexical and easy to bypass — true (bindings, wrapped chains), but the spec fixed exactly these patterns and AD-21 holds in today's code; making it token-aware is new machinery for a lint developers rarely trip.
  - `[false]` `[reject]` (blind) A new pub field on `GatewayOptions` breaks `noctis-gateway`'s public API — AD-2: only the facade `noctis` is public API; the spec states "`GatewayOptions` (internal crate) gains one required field"; `public-api.txt` does not cover it.
  - `[low]` `[patch]` (blind) No TTL, so a reshard orphans old-total entries forever — real (nothing ever reads or deletes `shard:{id}:{old}`), TTL `None` is intent; the fix documents the orphaned keys in adapters.md.
  - `[false]` `[reject]` (blind) A6 checks only `noctis::` lines — the spec's guarantee is scoped to noctis's own targets ("Logs (target `noctis::gateway` ...)"), and A6 asserts exactly that; twilight's own debug logs are outside the claim.
  - `[maybe-false]` `[reject]` (blind) Timing assertions in integration tests are likely flaky — the negative checks run after a concrete `wait_for_frame`/`wait_sessions`, and W6's bound is spec-mandated; only repeated runs under load would settle it; if true it is a low test flake.
  - `[low]` `[reject]` (blind) No test shows the reject-time delete finishes before IDENTIFY — grouped with the vgap ordering finding; the inline await is correct today and pinning the order needs delay plumbing in the recording store (fits Story 12.3's shutdown-order proofs).
  - `[low]` `[reject]` (blind) The Superseded path has no test above unit level — `ResumeState` unit tests cover R4; an integration test needs a FakeGateway mode answering RESUME with READY; little risk for new test machinery.
  - `[low]` `[patch]` (blind) GW-02 How text cites "(FR-4)" twice; `noctis-rules` not updated — the duplicate is real (cosmetic); no limit or id changed, so `noctis-rules` needs no edit (rules-catalog is a verification gate). Patch: drop the second `FR-4`.
  - `[low]` `[patch]` (blind) app-and-sharding.md names `LifecycleEvent::ShardResumed`, a crate-private type — `noctis-core/src/lifecycle.rs` declares `pub(crate) enum LifecycleEvent`; the Skill must not name it. Patch: name the event only.
  - `[low]` `[patch]` (blind) `await_startup` matches `Ready | Resumed` and then `matches!` again — real double match at app.rs:1091-1096; direct restructuring into two arms.
  - `[low]` `[patch]` (blind) `startup_completes_on_resumed` ends with a stray `drop(tx)` — dead statement; delete it.
  - `[false]` `[reject]` (blind) Spec frontmatter still says `status: 'in-progress'` — correct mid-review; the finalize step sets `done`; fixing would edit this build's spec.
  - `[low]` `[reject]` (edge) Close requested during the inline reject/supersede clear is delayed up to 5 s — same root cause as the blind inline-clear finding; rejected with it.
  - `[low]` `[reject]` (edge) Fresh close whose ack wait used the whole `close_timeout` polls the delete at an expired deadline — real for a non-instant store, but the cost is one rejected RESUME at the next start (self-heals) and a minimum budget would break the spec's single-deadline bound; Fresh is not used by core until 12.3.
  - `[low]` `[reject]` (edge) Any gateway Close counts as the ack, so a racing 4009/4007 stores an invalidated session — twilight skips `disconnect` once we initiated, so the session is stored; needs a gateway close racing our 4000 and costs one rejected RESUME; a code comparison depends on Discord's unverified echo code and could drop every real ack.
  - `[low]` `[reject]` (edge) Close during `load`'s delete of an unreadable entry cancels it — real; the entry is still unreadable next start and is deleted then; a guard is not worth it.
  - `[low]` `[reject]` (edge) deps-check misses bindings, accessors, UFCS and wrapped chains — same root cause as the blind deps-check finding; rejected with it.
  - `[medium]` `[patch]` (edge) Claim "a crash leaves no entry" is false after a resumed run — same root cause as the blind crash-claim finding; fixed by the same doc correction.
  - `[low]` `[reject]` (edge) Claim "`Closed` still comes within `close_timeout`" is broken by the inline reject-time clear — same root cause as the blind inline-clear finding; the close path itself honours the bound.
  - `[medium]` `[patch]` (vgap) The stored `resume_url` being applied is never observed by a test — pre-verified: every seeded URL equals `gateway_url`; removing `.resume_url(..)` passes all tests. Patch: two-FakeGateway test asserting the RESUME lands on the stored URL.
  - `[medium]` `[patch]` (vgap) A close requested while the start-time `get` is pending is never exercised — pre-verified (L11); grouped with the blind early-close finding. Patch: `get` hang mode in `Recording` plus a both-modes Gateway-level test.
  - `[low]` `[reject]` (vgap) The inline reject-time delete ordering is not observable by R2/A2 — pre-verified gap, filed `defer`; caused by this story, so not deferrable; low (code correct) and needs new delay plumbing; grouped with the blind ordering finding; candidate for 12.3's shutdown-order proofs.
  - `[low]` `[reject]` (vgap) The `session-store` rule is never run against real workspace metadata — pre-verified gap, filed `defer`; caused by this story; low (the gate passes and the sibling `http-client` wiring has the same shape) and the fix is a new wiring test, better done for both scanners in one xtask change.
  - `[false]` `[reject]` (intent) Scope taken from Story 12.3 (the write ships in 12.2) — the spec and memlog deliberately settled it ("Write in 12.2, not 12.3" decision); 12.3 keeps `--no-resume`, start-time clearing and the order proof.
  - `[false]` `[reject]` (intent) App-level tests run two Apps in one process, not a real restart — matches the AC's "(`FakeGateway`)" qualifier and the spec's A1 design with one shared `Arc` store.
  - `[low]` `[patch]` (intent) GW-02: R-C vs R-D and the How-text wording mismatch — R-C is deliberately settled ("GW-02 unchanged" decision); the wording is real: "below the number of shards this process identifies" contradicts "the check still counts it". Patch: "shards this process runs".
  - `[low]` `[reject]` (intent) "Only reader and writer" is enforced by a lexical lint, not an invariant — same root cause as the blind deps-check finding; rejected with it.
  - `[false]` `[reject]` (intent) "Within the resume window" is delegated to Discord, no local TTL/timestamp — the intent fixes TTL `None` ("a stale entry costs one rejected RESUME and is then deleted"); deliberate and correct.
  - `[false]` `[reject]` (intent) The delete trigger is wider than the AC's (READY-supersede, unreadable entry, Fresh close) — all three are intent rows (R4, L4, W5), settled deliberately.

## Design Notes

- **Write in 12.2, not 12.3** (decision): the restart AC needs a session written by the first process, and core already closes with `CloseMode::Resumable`; 12.3 keeps `--no-resume`, start-time clearing and the shutdown-order proof. Fresh close deleting the entry is the inverse of the write and needs no flag.
- **Ack-only write** (decision): the spine writes after observing the gateway's close; on an ack timeout nothing is written (an older entry may remain; a rejected resume self-heals).
- **Total in the key** (decision): a session belongs to `[id, total]`; changing the total must never resume a session of another shard layout.
- **GW-02 unchanged** (decision): counting stored sessions would make core read the port and could refuse too late when resumes fail.
- Shape of the reject check in `drive`:

```rust
if let Some(status) = classified.status { /* Ready => resume.superseded(), Resumed => resume.done() */ }
if resume.rejected(classified.invalid_session) {
    session::clear(&*store, &key, Instant::now() + STORE_TIMEOUT).await; // shard is disconnected here
}
```

## Verification

Run from `/home/claude/lanes/c/noctis-rs`, release only:

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis-gateway` session/shard tests, testkit `session_resume`, `session_log_redaction`, `gateway_close`, `sharding`, xtask deps-check tests
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0
- `cargo run --release -p xtask -- deps-check` / `-- rules-catalog` / `-- api-snapshot` / `-- skill-check` -- expected: exit 0, API hash unchanged
- `cargo deny check` -- expected: exit 0
- `cargo +1.91 check --release --workspace` -- expected: exit 0

## Auto Run Result

- **Summary:** shards now resume through the `SessionStore` port. At start each shard loads `shard:{id}:{total}` and RESUMEs with the stored session id, sequence and `resume_url`. A resumable close writes the session only once the gateway has acked the close. A rejected resume (INVALID_SESSION), a READY that supersedes the resume, an unreadable entry and a Fresh close each delete the entry. Core completes startup on READY or RESUMED. The session id and resume URL never show up in logs.
- **Files changed:**
  - `crates/noctis-gateway/src/session.rs`: new; session key, codec, load/store/clear with timeouts, `ResumeState`.
  - `crates/noctis-gateway/src/shard.rs`, `gateway.rs`, `lib.rs`, `shard_config.rs`: resume wiring, reject/supersede clears, ack-only write on close, `GatewayOptions` store field, crate docs.
  - `crates/noctis-core/src/app.rs`, `app/tests.rs`: passes the store to the gateway; startup completes on READY or RESUMED.
  - `crates/noctis-testkit/tests/session_resume.rs`, `session_log_redaction.rs` (new) and the updated `gateway_close`/`lanes`/`sharding`/`shard_log_redaction` tests: restart, reject, supersede, stored-URL, early-close and redaction tests.
  - `xtask/src/deps_check.rs`: `session-store` rule (only the gateway reads or writes the port).
  - `docs/discord-rules.md`: GW-02 wording.
  - `examples/project-template/.claude/skills/noctis/references/adapters.md`, `app-and-sharding.md`: Usage Skill docs for resume.
- **Review findings:** 33 in total: 0 high, 5 medium, 19 low, 8 false, 1 maybe-false.
  - Patched, 5 medium entries (3 root causes) and 6 low:
    - "a crash leaves no entry" was false after a resumed run; the doc claims are corrected.
    - Gateway-level test for a close while the start-time `get` hangs, in both modes.
    - Two-FakeGateway test that proves the stored `resume_url` is used.
    - Orphaned old-total keys after a reshard are documented.
    - Duplicate FR-4 and the wording in GW-02 are fixed.
    - The private `LifecycleEvent` name is removed from the Skill.
    - The double match in `await_startup` is gone.
    - A stray `drop(tx)` is removed.
  - Deferred: none.
  - Rejected, with reasons:
    - The inline reject-time clear can delay a close by up to 5 s (rare, needs a hanging store; not worth the complexity).
    - The deps-check lint is lexical (it covers the patterns the spec named, and AD-21 holds).
    - Delete-before-IDENTIFY ordering and the Superseded path at integration level have no test (the code is correct; candidates for the 12.3 order proofs).
    - A Fresh close at an expired deadline, and a racing 4009 treated as the ack (each costs at most one rejected RESUME, which self-heals).
    - A close can cancel the delete of an unreadable entry (deleted next start).
    - The `session-store` rule is never run against real metadata (low; better as one xtask change).
    - False: the unreachable `resume_url` hang (twilight falls back), the GatewayOptions API break (internal crate, AD-2), A6 scope, the spec status, the scope taken from 12.3 (a decision), the two Apps in one process (matches the AC), no TTL (intent), the wider delete trigger (intent rows).
    - Maybe-false: timing flakes in the tests.
- **Follow-up review recommended: true.** Patched counts: high 0, medium 5, low 6. The named risk is that the new early-close test (hanging `get`) and the stored-`resume_url` test went in after review and no independent pass has checked that they observe the behaviour they claim.
- **Verification:** all local gates in `tools/story-loop/gates.sh` passed (fmt, clippy -D warnings, test, hack each-feature, deps-check, rules-catalog, api-snapshot with the API hash unchanged, skill-check, cargo deny, MSRV 1.91). CI is paused by the maintainer.
- **Residual risks:**
  - A hanging store at a rejection can delay a concurrent close by up to 5 s.
  - Entries keyed to an old shard total are never deleted after a reshard.
  - The deps-check rule is lexical.
  - Story 12.3 must add `--no-resume`, start-time clearing and the shutdown-order proof.
