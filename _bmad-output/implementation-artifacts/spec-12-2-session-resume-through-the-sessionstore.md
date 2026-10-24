---
title: 'Story 12.2: Session resume through the SessionStore'
type: 'feature'
created: '2026-10-24'
status: 'in-progress'
baseline_revision: 'a52191427b308cb41899738066968e35f3769e42'
review_loop_iteration: 0
followup_review_recommended: false
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
