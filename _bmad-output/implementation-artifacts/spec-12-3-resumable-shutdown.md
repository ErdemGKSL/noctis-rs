---
title: 'Story 12.3: Resumable shutdown'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-12-2-session-resume-through-the-sessionstore.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 12.2 already closes every shard with 4000 and writes its session after the gateway acknowledges the close, but an operator has no way to shut down without leaving resumable sessions (or to start without resuming stale ones), `--no-resume` is still an unknown flag, and nothing proves at App level that the session write lands before `run`/`run_until` returns or that back-pressure drops are not replayed.

**Approach:** Add `--no-resume` to the Story 6.9 parser (`NoctisArgs`) and a matching builder knob `AppBuilder::resume_sessions(bool)` (default `true`; the flag sets it to `false` for that run). With resume off, core passes `GatewayOptions::resume = false` (each shard task deletes its stored entry at start instead of loading it, then identifies) and shuts down with `CloseMode::Fresh` (code 1000, entry deleted; 12.2 W5). Default mode is unchanged. App-level tests prove the AD-21 order (`shutdown started` -> 4000 close -> `put` with the last received sequence -> `run_until` returns) and the at-most-once semantics. FR-55, FR-54 (order), AD-21.

## Boundaries & Constraints

**Always:**
- Parser: `--no-resume` is a no-value flag, reserved by `NoctisArgs::split` like the others; `--no-resume=x` -> `CliError::UnexpectedValue { flag: "--no-resume" }`; given twice -> `CliError::DuplicateFlag { flag: "--no-resume" }`; with any publish mode (`--publish`, `--publish-guild`, `--publish-clear`, in either order) -> new `CliError::NoResumeWithPublish` ("`--no-resume` only applies to running the bot: drop it or the publish flag"), checked in `Parser::finish` after the existing `DryRunWithoutPublish` check. `NoctisArgs::no_resume() -> bool`; `IntoIterator` renders it last (after `--dry-run`, which cannot co-occur), so `parse(args.clone()) == args` still holds. `CliError::UnknownFlag`'s message and the module doc list `--no-resume`.
- `AppBuilder::resume_sessions(&mut self, bool) -> &mut Self` (default `true`, last call wins, mirrors `publisher`: builder field, config into `App`, `Debug` fields) and `App::resume_sessions(&self) -> bool`. `run_with_args` with `--no-resume` sets the App's value to `false` before `run_with` and logs `info` (target `noctis::app`) `session resume disabled (--no-resume)`.
- `GatewayOptions` gains `pub resume: bool` (doc: `true` loads the stored session at shard start; `false` deletes it instead and identifies). In `shard::run`, `resume == false` replaces `session::load` with `session::clear(&*store, &key, Instant::now() + STORE_TIMEOUT)` inside the same `select!` against `close_requested` (biased, close first), then builds the shard from the unmodified `config` (`ResumeState::new(false)`). The delete completes before the shard connects, so it precedes IDENTIFY.
- Core: `shut_down` calls `gateway.close_all(if self.resume_sessions { CloseMode::Resumable } else { CloseMode::Fresh })`; `GatewayOptions::resume = self.resume_sessions`. Nothing else in the shutdown sequence, `SHARD_CLOSE_TIMEOUT` or its 6 s wait changes. Core still never calls the SessionStore (deps-check `session-store` stays green).
- Stored sequence stays "last received" (12.2): an event the lanes dropped under back-pressure was received, so its seq is covered and Discord never replays it. Documented as at-most-once in the gateway crate doc (already), `run_until` rustdoc, the Skill §4/§8 and `README.md` of the template.
- Only this process's shards (`options.shards`, key `shard:{id}:{total}` with the current total) are cleared; other totals' or other ranges' entries are untouched.
- `rust-best-practices` applies (read `references/async-and-concurrency.md`, `error-handling.md`, `testing.md`, `api-design.md`).

**Never:**
- No `NOCTIS_NO_RESUME` env var, no new rule id, no `docs/discord-rules.md` change, no new crate or version, no change to the 12.2 load/validate/write logic, timeouts or log redaction. No SessionStore call from core (no clearing when the shutdown resolves before `GET /gateway/bot` finished: no shard plan exists, documented). No replay or dedup buffer of dropped events.

## I/O & Edge-Case Matrix

Parser (`crates/noctis-core/src/cli/tests.rs`):

| # | Input | Expected | Error |
|---|-------|----------|-------|
| P1 | `[]` | `no_resume() == false`, mode `None` | — |
| P2 | `["--no-resume"]` | `no_resume() == true`, mode `None`; renders `["--no-resume"]` | — |
| P3 | `["--no-resume", "--no-resume"]` | — | `DuplicateFlag { flag: "--no-resume" }` |
| P4 | `["--no-resume="]`, `["--no-resume=1"]`, `["--no-resume=ü"]` | — | `UnexpectedValue { flag: "--no-resume" }` |
| P5 | `["--no-resume","--publish"]`, `["--publish","--no-resume"]`, `["--publish-guild","5","--no-resume"]`, `["--publish-clear","--guild","5","--no-resume"]` | — | `NoResumeWithPublish` |
| P6 | `["--dry-run","--no-resume"]` | — | `DryRunWithoutPublish` (checked first) |
| P7 | `["--no-resum"]`, `["--NO-RESUME"]`, `["-no-resume"]`, `["--no-resume-x"]` | — | `UnknownFlag` with that text |
| P8 | split `["bot","--no-resume","--x","--","--no-resume"]` | noctis `no_resume() == true`; rest `["bot","--x","--","--no-resume"]` | — |
| P9 | non-UTF-8 arg next to `--no-resume` (unix `OsString::from_vec([0xff])`) | split keeps it in rest; parse -> `UnknownFlag` | — |
| P10 | `NoctisArgs::parse(args.clone()) == args` for P1, P2 | round-trips | — |
| P11 | message of `NoResumeWithPublish` and of `UnknownFlag` | contain `--no-resume` | — |

Gateway start in fresh mode (`crates/noctis-testkit/tests/session_resume.rs`, its `Recording` store):

| # | Scenario | Input / State | Expected | Error handling |
|---|----------|---------------|----------|----------------|
| F1 | stored entry, resume off | valid entry under `shard:0:1` (fake knows it), `resume: false` | ops `[Delete]` before any `Get`/`Put`; zero op 6; exactly one op 2 through the queue; READY; entry gone | — |
| F2 | no entry | `resume: false`, empty store | ops `[Delete]`, identifies | delete of a missing key is `Ok` |
| F3 | delete fails / hangs | `delete` -> `PortError::Backend` / never resolves | fails: identifies at once, `warn` `session store delete failed`; hangs: op 2 arrives no earlier than 4.9 s and within 15 s after start (`STORE_TIMEOUT` = 5 s), `warn` `session store delete timed out` | never panics, no retry |
| F4 | close during start delete | `close_all(Fresh)` while `delete` hangs | `Closed` without connecting, no frame | — |
| F5 | other key | entry under `shard:0:2`, run total 1 | `shard:0:2` untouched | — |
| F6 | resume on (regression) | `resume: true` | 12.2 behaviour, ops start with `Get` | — |

App level (new `crates/noctis-testkit/tests/resumable_shutdown.rs`, one ordered journal shared by a capturing `tracing` layer and a recording store; tests serialized like `shutdown.rs::serial`):

| # | Scenario | Input / State | Expected | Error handling |
|---|----------|---------------|----------|----------------|
| S1 | default order | 1 shard, READY (s 1), 3 dispatches (s 4), `run_until` stop | journal order: `log:shutdown started` < `put(shard:0:1, seq 4)` < `log:shutdown complete`; at `put` time the fake already recorded the client close 4000 (store holds an `Arc<FakeGateway>` probe); the entry is readable right after `run_until` returns (no sleep) | — |
| S2 | `resume_sessions(false)` | same, stop | close frame 1000, no `put`, a `Delete` after `log:shutdown started`; store empty after return | — |
| S3 | restart after S2 | App 2 (default mode) with the same store and fake | IDENTIFY, no RESUME | — |
| S4 | start with `resume_sessions(false)` | entry seeded by a default-mode App 1 | App 2 sends no op 6; `Delete` precedes App 2's op 2 frame (probe counts op 2 at delete time = count before start) | — |
| S5 | at-most-once | `event_lane(1, 0)`, `handler_concurrency(1)`, a blocking droppable-event handler (events.rs `BlockedDelete` pattern), 20 dispatches -> `event lane is full` logged; release; stop | stored `seq` == the fake's last sent `s` (21 incl. READY); App 2 resumes with RESUME `d.seq` == that value; App 2's handlers see zero of App 1's events | — |
| S6 | `--no-resume` wiring | `App::run_with_args(["--no-resume", "--publish"])` | `Err(Error::Cli(NoResumeWithPublish))` before any HTTP request (fake HTTP records nothing) | — |
| S7 | builder | `App::builder().build()` / `.resume_sessions(false)` / two calls `false` then `true` | `resume_sessions()` = `true` / `false` / `true` | — |

</intent-contract>

## Code Map

- `crates/noctis-core/src/cli.rs` -- add `const NO_RESUME: &str = "--no-resume"`; `NoctisArgs { .., no_resume: bool }`, `Parser { .., no_resume: bool }`; arm in `Parser::flag` (mirror `DRY_RUN`); `finish`: after `DryRunWithoutPublish`, `if self.no_resume && self.mode.is_some() { Err(NoResumeWithPublish) }`; `no_resume()` accessor; `IntoIterator` pushes it last; `split` doc lists the reserved name; module doc line 1-3; `UnknownFlag` message.
- `crates/noctis-core/src/cli/tests.rs` -- P1-P11.
- `crates/noctis-core/src/app.rs` -- `App` field `resume_sessions: bool` beside `publisher` (`:150`), getter beside `is_publisher` (`:360`), `Debug` (`:1317`, `:2540`), builder field (`:1383`), default `true` (`:1477`), setter after `publisher` (`:2025`, rustdoc: default `true`, `false` = fresh shutdown + start-time clearing, `--no-resume` sets it), into App (`:2440`). `run_with_args` (`:646`): `let mut app = self;` set `false` + info log when `args.no_resume()`; update its rustdoc. `run_with` `GatewayOptions` (`:818`): `resume: self.resume_sessions`. `shut_down` (`:897`): mode from `self.resume_sessions`. `run_until` rustdoc (`:696-760`): shutdown step 2 names both modes and the at-most-once note.
- `crates/noctis-core/src/app/tests.rs:2549` -- add `resume: true` to the literal; unit test: `resume_sessions` default/setter (S7).
- `crates/noctis-gateway/src/gateway.rs` -- `GatewayOptions` (`:66-120`): `pub resume: bool` after `session_store`, `Debug` field; pass it into `shard::run` from `Gateway::start`; add `resume: true` to the doctests (`:60`, `:150`, `:203`, `:282`, `:311`, `:350`, `:393`) and `tests::options` (`:442`).
- `crates/noctis-gateway/src/shard.rs:192` `run` -- new `resume: bool` parameter; the `loaded = session::load(..)` select arm becomes `match resume { true => load, false => clear then None }`; doc comment updated.
- `crates/noctis-gateway/src/lib.rs:25-52` -- `# Sessions`: add "**Cleared** at start, instead of read, when `GatewayOptions::resume` is `false` (`--no-resume`)".
- `crates/noctis-testkit/tests/{lanes.rs:60,shard_log_redaction.rs:93,sharding.rs:538,sharding.rs:625,gateway_close.rs:57,session_resume.rs:219}` -- add `resume: true` (session_resume `options` gets a `resume` parameter for F1-F6).
- `crates/noctis-testkit/tests/session_resume.rs` -- F1-F6 (reuse `Recording`, add a delete fail/hang mode like `PutMode`).
- `crates/noctis-testkit/tests/resumable_shutdown.rs` (new binary) -- S1-S7; helpers copied from `session_resume.rs` (`builder`, `run`, `RunningApp`, `wait_sessions`, `settle`, `http`) and `events.rs` (`BlockedDelete`, gate), capturing layer like `session_log_redaction.rs`.
- `examples/project-template/src/main.rs:40` -- drop `--no-resume` from the planned comment, list it as supported.
- `examples/project-template/README.md:110` -- `--no-resume` row without `[planned]`: "Close shards fresh (1000) and clear this process's stored sessions at start and at shutdown; the next start identifies."
- `examples/project-template/AGENTS.md:33` -- mention `--no-resume` for full resets.
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- builder table row `.resume_sessions(bool)` (near `:115`), `App::resume_sessions()` in the getters list (`:89`), CLI table `:311` without `[planned]` and with the `NoResumeWithPublish` error, §4 Session resume (`:529-559`) "Cleared" bullet + at-most-once paragraph, §8 step 2 (`:658-661`) both modes. `SKILL.md:470` CLI row adds `--no-resume`; `SKILL.md:23` API hash.
- `xtask/public-api.txt` -- re-bless.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions appended; add implementation ones.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/cli.rs`, `cli/tests.rs` -- `--no-resume` flag, error, accessor, rendering; P1-P11.
- `crates/noctis-gateway/src/{gateway.rs,shard.rs,lib.rs}` -- `GatewayOptions::resume`, start-time clear branch, docs/doctests.
- `crates/noctis-core/src/{app.rs,app/tests.rs}` -- knob, getter, `run_with_args` wiring, close mode, `GatewayOptions::resume`, rustdoc; S7 unit test.
- `crates/noctis-testkit/tests/*` -- literal fixes; F1-F6 in `session_resume.rs`; `resumable_shutdown.rs` S1-S6.
- `examples/project-template/{src/main.rs,README.md,AGENTS.md,.claude/skills/noctis/SKILL.md,.claude/skills/noctis/references/app-and-sharding.md}` -- docs; then `cargo run --release -p xtask -- api-snapshot --bless` and put the printed hash in `SKILL.md`'s `API hash` line.

**Acceptance Criteria:**
- Given a graceful shutdown in default mode, when `run_until` returns, then every shard closed with 4000 and its session id, last received sequence and resume URL are in the SessionStore, written after the gateway's close was observed and after `shutdown started` (S1).
- Given `--no-resume` (or `resume_sessions(false)`), when the process starts and shuts down, then no shard sends RESUME, each deletes its stored entry before it identifies, and the shutdown closes with 1000 and leaves no entry (F1, S2-S4).
- Given events dropped under back-pressure before a default shutdown, when the next App resumes, then RESUME carries the last received sequence and none of the dropped events reaches a handler (S5); the Skill and template README document at-most-once.
- Given the story is done, when all local gates run, then they pass, `deps-check` still reports no SessionStore call outside `noctis-gateway`, and the Skill's API hash equals the one `api-snapshot --bless` printed.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Builder knob plus flag** (decision): `run_until` takes no args and the lifecycle API is crate-private, so a builder knob (`resume_sessions`) is the code-level switch embedders and tests use; `--no-resume` only flips it for one run. No env var (FR-55 names a mode, the CLI covers deploy scripts; can be added later without breaking).
- **`--no-resume` with a publish mode is an error** (decision): publish modes never start shards, so the flag would silently do nothing.
- **Start-time clear replaces the load** (decision): reading then deleting would be two store ops for the same outcome; deleting first also guarantees no RESUME and the delete-before-IDENTIFY order 12.2 left unproven.
- **No clearing before the shard plan** (decision): a shutdown resolved during `GET /gateway/bot` has no total, so no keys; the next `--no-resume` start clears them.
- Shape of the start branch in `shard::run`:

```rust
started = async {
    if resume { session::load(&*stored.store, &stored.key).await }
    else { session::clear(&*stored.store, &stored.key, Instant::now() + STORE_TIMEOUT).await; None }
} => started,
```

## Verification

Run from `/home/claude/lanes/c/noctis-rs`, release only:

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis-core` cli tests, testkit `session_resume`, `resumable_shutdown`, `shutdown`, `gateway_close`
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0
- `cargo run --release -p xtask -- api-snapshot --bless` then `-- api-snapshot` -- expected: exit 0, hash matches `SKILL.md`
- `cargo run --release -p xtask -- deps-check` / `-- rules-catalog` / `-- skill-check` -- expected: exit 0
- `cargo deny check` -- expected: exit 0
- `cargo +1.91 check --release --workspace` -- expected: exit 0
