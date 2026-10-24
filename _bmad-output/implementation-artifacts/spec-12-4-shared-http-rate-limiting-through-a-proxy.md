---
title: 'Story 12.4: Shared HTTP rate limiting through a proxy'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: 'ed6784e5577185c1ce692e47011e60b2956a3c64'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-9-warm-deadline-safe-discord-http-edge.md'
warnings: ['oversized']
deferred:
  - summary: >-
      bg.sh lets the first attached caller consume the job, so a second caller of the same job spins forever and an empty status exits 0.
    evidence: |-
      tools/story-loop/bg.sh: after status appears, the first waiter runs mv "$J" "$J.done.$$" and rm -rf; a second waiter still in `while [ ! -f "$J/status" ]` never sees the file again; `exit "$st"` with an empty st exits 0. Not caused by Story 12.4: the file is identical to origin/main (commit 2defad7).
    location: >-
      tools/story-loop/bg.sh:24-29
    severity: medium
  - summary: >-
      bg.sh waits forever when the detached runner died before writing status (container restart, kill).
    evidence: |-
      tools/story-loop/bg.sh: the wait loop polls only for $J/status, with no runner pid or liveness check. Pre-existing tooling from origin/main (commit 2defad7), not this story.
    location: >-
      tools/story-loop/bg.sh:24
    severity: medium
  - summary: >-
      lane.sh lock spins without sleep and ignores its 9-minute deadline when move fails for a non-contention reason while the lock looks free.
    evidence: |-
      tools/story-loop/lane.sh lock: `move "$1" "$owner" && exit 0; continue` jumps back before the `$SECONDS -ge $end` check and the `sleep 20`. Pre-existing tooling from origin/main, not this story.
    location: >-
      tools/story-loop/lane.sh:86-89
    severity: medium
  - summary: >-
      story-loop.js resumed() ignores resumeAt 'done' outside the auto path, and a dead recovery agent in the auto path falls back to re-planning.
    evidence: |-
      Unverified reviewer claim on pre-existing workflow tooling (identical to origin/main). Settle by reading resumed()/retry in .claude/workflows/story-loop.js and tracing a recovery result with retry true and resumeAt 'done'.
    location: >-
      .claude/workflows/story-loop.js resumed()
    severity: medium (unverified)
  - summary: >-
      story-loop.js mergeIntoMain can leave the merge lock held for 4 h after a failed release or a crash between merge and push-main.
    evidence: |-
      Unverified reviewer claim on pre-existing workflow tooling. Settle by checking whether lane.sh push-main's release reports failure and whether mergeIntoMain unlocks on every exit path.
    location: >-
      .claude/workflows/story-loop.js mergeIntoMain
    severity: medium (unverified)
  - summary: >-
      epic-last.py may never run cargo hack for an epic when two of its stories are in progress at once across lanes, and silently ignores git fetch failures.
    evidence: |-
      Unverified reviewer claim on pre-existing tooling. Settle by running epic-last.py against a sprint-status with two in-progress stories of one epic and checking its fetch error handling.
    location: >-
      tools/story-loop/epic-last.py
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** Proxy mode exists since Stories 5.2/5.9 (`http_proxy`, `NOCTIS_HTTP_PROXY`, edge upstream, `ratelimiter(None)`), but nothing proves that no request bypasses the proxy, a Cluster without a proxy is silent (RL-02), invalid requests are not counted (RL-03), and 429s are not surfaced: twilight-http 0.17.1 `ResponseFuture` re-sends a 429 **immediately and forever** when the limiter is off (`future.rs:389-398`, `None => generate()`), and with the limiter on it cannot wait out a `global` or scope-less 429 either (`parse_ratelimit_headers` returns `None`). A 429 from the proxy therefore hot-loops, adding to the Cloudflare invalid-request count. The callback path ignores `http_timeout`.

**Approach:** The edge (the only place that sees every response) classifies every upstream 429 by `X-RateLimit-Scope`, counts it, and answers twilight with an edge rate-limit response (503 + noctis JSON body, Discord headers kept) unless twilight's own limiter can safely wait it out (direct mode, scope `user`, valid bucket headers). Noctis maps that response to a new typed `RateLimited` error on every Noctis surface and exposes `RateLimited::from_http_error` for raw twilight calls. Counters for 401/403/429 feed RL-03; `run` warns for a proxy-less Cluster; `http_timeout` also bounds callbacks and forwarded requests at the edge. Docs/template/Skill make proxy mode the Cluster setup. FR-52, AD-24, RL-01..04, INT-13.

## Boundaries & Constraints

**Always:**
- New crate-private module `crates/noctis-core/src/edge/rate_limit.rs` with pure functions (no I/O):
  - `scope(headers) -> RateLimitScope`: header `x-ratelimit-scope` bytes exactly `user`/`global`/`shared` → that scope; missing or anything else → `Unknown`.
  - `classify_429(headers, body: &[u8]) -> RateLimited`: scope as above, but `Unknown` with body JSON `"global": true` → `Global`; `retry_after` = body `retry_after` (JSON number, finite, `>= 0`, `Duration::try_from_secs_f64` ok) else header `retry-after` (same checks, trimmed) else `None`; `discord_code` = body `code` as `u32` when present and in range.
  - `relay_429(headers, proxied: bool) -> bool` (true = forward the 429 unchanged to twilight): only when `!proxied`, scope bytes exactly `user`, `x-ratelimit-bucket` present, `x-ratelimit-limit` and `x-ratelimit-remaining` parse as `u16` (`str::parse`, no trimming), `x-ratelimit-reset-after` parses as `f32`, is finite and `0.0..=86_400.0` (twilight's `Duration::from_secs_f32` panics on negative/NaN/overflow). Everything else is surfaced.
  - `edge_body(&RateLimited) -> Bytes`: `{"message":"noctis edge: rate limited","global":<scope==Global>,"scope":"<user|global|shared|unknown>"}` plus `"retry_after":<f64 secs>` when known and `"discord_code":<n>` when known; never a `code` field (so twilight parses it as `ApiError::Ratelimited` when `retry_after` is present). Const `RATE_LIMIT_BODY_PREFIX = r#"{"message":"noctis edge: rate limited""#` (`message` is always the first key: write the JSON with a fixed field order, e.g. `format!` over `serde_json::to_string` of each value, not a `serde_json::Map`, whose order is not guaranteed without `preserve_order`).
  - `parse_edge_body(status: u16, body: &[u8]) -> Option<RateLimited>`: `Some` only for status 503 and a body starting with `RATE_LIMIT_BODY_PREFIX` that parses back (round-trip of `edge_body`).
- Edge forward path (`edge/mod.rs::upstream`): for an upstream 429, read the body capped at 64 KiB (reuse `callback::read_capped`, make it `pub(crate)`), record metrics, then either relay (rebuilt response with the same status/headers and the read bytes) or answer **503** with `edge_body` and the upstream's end-to-end headers (so twilight's limiter still learns the bucket; `content-type: application/json`, `content-length` replaced). Log `debug` target `noctis::http` with scope only (never path, token, header values).
- Metrics (both forward and callback attempts, upstream responses only, never edge-synthesized ones), added to `EdgeMetrics`, `observe::METRICS` (after `noctis_http_requests_total`) and `describe_metrics`:
  - `noctis_http_rate_limited_total{app, scope}` counter, `scope` ∈ `user|global|shared|unknown`, every upstream 429 (relayed or surfaced);
  - `noctis_http_invalid_requests_total{app, status}` counter, `status` ∈ `401|403|429`, every upstream 401/403 and every 429 **except** scope `shared` (Discord: "429 errors returned with `X-RateLimit-Scope: shared` are not counted against you", verified 2026-10-24 at docs.discord.com/developers/topics/rate-limits; the limit is 10,000 per 10 minutes per IP).
- Public typed error in new `crates/noctis-core/src/rate_limit.rs`, re-exported from `noctis_core` root and `noctis::http`:
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum RateLimitScope { User, Global, Shared, Unknown }` with `as_str()` (lowercase) and `Display`;
  - `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)] pub struct RateLimited { pub scope: RateLimitScope, pub retry_after: Option<Duration>, pub discord_code: Option<u32> }`, message `Discord rate limited the request ({scope} scope)` + `, retry after {secs:.3} s` when known;
  - `RateLimited::from_http_error(&twilight_http::Error) -> Option<RateLimited>` (`ErrorType::Response { status, body, .. }` through `parse_edge_body`).
- New variants, mapped wherever the twilight error is mapped today:
  - `ResponderError::RateLimited(#[from] RateLimited)` in `responder/transport.rs::http_error` (checked before `Rejected`);
  - `WebhookError::RateLimited(RateLimited)` plus its arm in `From<ResponderError>`;
  - `RawHttpError::RateLimited(RateLimited)` in `raw_http.rs::from_twilight` (before `Status`);
  - `PublishError::Rejected { status: 429, code: discord_code }` and `StartError::GatewayInfo { status: Some(429) }` for an edge rate-limit body (no new variants there).
  - `CallbackError` and its 429 handling (INT-01 fit rule) stay unchanged; only the metrics are added.
- `http_timeout` applies to every Discord request: twilight already applies it per attempt; `Edge::start`/`start_with` gain `request_timeout: Duration` (from `config.http_timeout`) stored on `Pool`; the forward path wraps both attempts of `upstream()` in `timeout(request_timeout)` → 504 with `EDGE_TIMEOUT_BODY = {"message":"noctis edge: upstream timed out","code":0}`, counted `status_class="error"`; `callback::interaction_callback` uses `deadline = min(received_at + ACK_DEADLINE, Instant::now() + request_timeout)` (result stays `DeadlineMissed`). `gateway_info` and `publish::http_error` treat 504 + `EDGE_BODY_PREFIX` exactly like their 502 case (one helper `edge::is_edge_failure(status, body)` = status 502 or 504 and prefix).
- RL-02 warning: crate-private `fn cluster_without_proxy(range: Option<&Range<u32>>, total: Option<u32>, proxy: Option<&str>) -> bool` (true iff range and total are set, `range.len() < total` and no proxy); `run_with` logs once, right after the token check and before `start_keeper`, `warn!(target: "noctis::app", start, end, total, "this process runs a Shard Range of a Cluster without an HTTP proxy: processes sharing one bot token can exceed Discord's global limit of 50 requests/s per token (RL-02); point every process at one shared rate-limit proxy with NOCTIS_HTTP_PROXY or AppBuilder::http_proxy")`. Not in publish-only CLI modes (they never call `run_with`).
- `rules-catalog`: remove the `INT-13`, `RL-01`, `RL-02`, `RL-03`, `RL-04` entries from `xtask/rules-pending.toml`; each gets a test whose name contains `int_13`, `rl_01`, `rl_02`, `rl_03`, `rl_04`.
- `docs/discord-rules.md` rows RL-01..RL-04 and INT-13 keep their ids and tiers; update their Notes cells to name where each is enforced now (edge relay rule, `noctis_http_invalid_requests_total`, `RateLimited`, startup warning). No noctis-rules constant changes (no new limit value).
- AD-24 in the spine: amend the Discord edge bullet with "an upstream 429 twilight's limiter cannot wait out is answered as an edge rate-limit response (503, noctis body, Discord headers kept) and surfaced as `RateLimited`", in the same change; log decisions in the memlog.
- `rust-best-practices` applies (read `references/error-handling.md`, `async-and-concurrency.md`, `testing.md`, `api-design.md`).

**Never:**
- No hard-coded per-route or per-resource limits, no local retry/wait of a surfaced 429 (neither in the edge nor in Noctis mappers), no second twilight client, no request path that skips the edge, no new env var or builder setter (proxy and timeout setters exist), no change to the callback 429 fit rule, no metric label carrying a path, id or header value, no logging of header values or bodies.

## I/O & Edge-Case Matrix

Pure functions (`edge/rate_limit.rs` unit tests; names cite `rl_01`/`rl_04` where marked):

| # | Input | Expected |
|---|-------|----------|
| R1 | scope header `user` / `global` / `shared` | `User` / `Global` / `Shared` |
| R2 | scope header missing, `""`, `User`, ` shared`, `shared ` , `sharéd` (UTF-8), non-UTF-8 byte `0xff` | `Unknown` |
| R3 | `classify_429`, no scope, body `{"global":true,"retry_after":1.5}` | `Global`, `Some(1.5 s)` |
| R4 | body `retry_after` `0`, `0.001`, `64.57` | `Some(0)`, `Some(1 ms)`, `Some(64.57 s)` |
| R5 | body `retry_after` `-1`, `"1"` (string), `1e400`/huge, missing; header `retry-after: 2` | `Some(2 s)` from the header |
| R6 | body invalid JSON / empty / 64 KiB+ truncated; header `retry-after: abc`, `-3`, `NaN`, missing | `retry_after = None`, scope from header |
| R7 | body `code: 20028` / `code: 2^32` / `code: -1` | `discord_code` `Some(20028)` / `None` / `None` |
| R8 | `relay_429` direct, `user`, bucket `b`, limit `5`, remaining `0`, reset-after `0.5` (rl_01) | `true` |
| R9 | same but proxied | `false` |
| R10 | direct, scope `shared` / `global` / missing (rl_04 for shared) | `false` |
| R11 | direct, `user`, each one of bucket/limit/remaining/reset-after missing | `false` |
| R12 | direct, `user`, limit `65536`, remaining `-1`, limit ` 5`, reset-after `-0.1`, `NaN`, `inf`, `86400.5`, `abc` | `false` each |
| R13 | direct, `user`, reset-after `0`, `86400` | `true` (boundaries) |
| R14 | `edge_body` / `parse_edge_body(503, ..)` round-trip for every scope, with and without `retry_after`/`discord_code` | equal value; body starts with `RATE_LIMIT_BODY_PREFIX`; has no `code` key; with `retry_after` it parses as twilight `ApiError::Ratelimited` |
| R15 | `parse_edge_body(429 or 502, edge body)`, `(503, Discord body)`, `(503, prefix + garbage)`, `(503, "")` | `None` |
| R16 | `cluster_without_proxy`: `(0..2, 4, None)`, `(2..4, 4, None)`, `(0..1, 2, None)` (rl_02) | `true` |
| R17 | `(0..4, 4, None)`, `(None, 4, None)`, `(None, None, None)`, `(0..2, 4, Some("p:1"))` | `false` |
| R18 | `RateLimited` `Display` with/without `retry_after`; `RateLimitScope::as_str` for all | `"Discord rate limited the request (shared scope), retry after 1.500 s"`; lowercase names |

End to end through `FakeDiscordHttp` as the proxy (new testkit binary `crates/noctis-testkit/tests/http_rate_limit.rs`, global `DebuggingRecorder` + capturing subscriber like `http_edge.rs`, tests serialized):

| # | Scenario | Input / State | Expected | Error handling |
|---|----------|---------------|----------|----------------|
| E1 | no bypass (int_13) | proxied App + `FakeGateway`, `publisher(true)`: startup (`GET /gateway/bot`, Publish `PUT …/commands`), `app.http().channel_messages`, raw `GET channels/1/pins`, owned webhook `send`, a slash command whose handler replies then `follow_up`s, `fetch_reply` | every request (each method+path) is in the fake's log; the callback and interaction-webhook requests carry no `authorization` and no `with_components`; `app.http().ratelimiter().is_none()` | — |
| E2 | proxied 429 surfaced, no hot loop (rl_04) | route answers 429 scope `shared`, `retry-after: 600`, body `{"message":"…","retry_after":600.0,"global":false,"code":20028}` | `app.http().update_channel(..)` → error; `RateLimited::from_http_error` = `{Shared, Some(600 s), Some(20028)}`; fake logged **exactly 1** request after 300 ms; `noctis_http_rate_limited_total{scope="shared"}` +1, invalid counter unchanged | — |
| E3 | proxied 429 `user` | 429 scope `user` with valid bucket headers | surfaced as `RateLimited{User}`, exactly 1 request; invalid `{status="429"}` +1 | proxied never relays |
| E4 | proxied 429 no scope (Cloudflare-like), body not JSON | `429`, body `error code: 1015` | `RateLimited{Unknown, None, None}`, 1 request; invalid 429 +1 | — |
| E5 | typed on every surface | 429 shared on raw path / webhook execute / follow-up / edit_reply / Publish / `GET /gateway/bot` | `RawHttpError::RateLimited`, `WebhookError::RateLimited`, `ResponderError::RateLimited` (×2), `PublishError::Rejected{status:429, code:Some(20028)}`, `StartError::GatewayInfo{status:Some(429)}` | — |
| E6 | 401 / 403 counted (rl_03) | routes answering 401 and 403 (forward) and 403 on the callback route | invalid `{status="401"}` +1, `{status="403"}` +2; `requests_total` unchanged semantics | — |
| E7 | callback 429 | callback route 429 shared `retry_after 5` | `AckDeadlineMissed` as before (fit rule); rate_limited `shared` +1, invalid unchanged | — |
| E8 | 2xx/404/500 | normal responses | no rate_limited/invalid samples | — |
| E9 | timeout on callbacks | `http_timeout(500 ms)`, callback route delayed 2 s, received now | `CallbackError::DeadlineMissed` within 0.5..1.5 s (not 2.9 s) | — |
| E10 | timeout on a forwarded request | `http_timeout(300 ms)`, route delayed 2 s, raw request | `RawHttpError::Http` (twilight timeout) within 1 s | — |
| E11 | default timeout | `App::builder().build()` | `http_timeout() == 10 s` (regression) | — |
| E12 | Cluster with proxy, no warning | `shard_total(2).shard_range(0..1)` + proxy, `run_until` after READY | no `RL-02` warning line | — |
| E13 | RL-02 log (core unit test, `with_default` capturing subscriber, calls the crate-private warn helper with `0..2`/4) | — | exactly one `warn` with `noctis::app` target containing `RL-02`, `50 requests/s`, `NOCTIS_HTTP_PROXY`, fields start/end/total | — |

Direct-mode relay through the edge (core `edge/tests.rs`, extend `TestServer`'s `Answer` to return headers too, upstream `Upstream::Tls` cannot be faked, so pass `proxied` explicitly: `Pool` stores `proxied = matches!(upstream, Upstream::Http { .. })`, and a crate-private `Edge::start_with` parameter `proxied: bool` overrides it in tests):

| # | Scenario | Expected |
|---|----------|----------|
| D1 | `proxied = false`, upstream 429 `user` valid headers, body `B` | loopback gets 429, same headers, body `B` byte-for-byte (rl_01) |
| D2 | `proxied = false`, upstream 429 `shared` with bucket + `retry-after: 600` | loopback gets 503, `x-ratelimit-*` and `retry-after` headers kept, edge body |
| D3 | `proxied = true`, 429 `user` valid | 503 edge body |
| D4 | upstream 429 body larger than 64 KiB | surfaced, `retry_after` from header only, no hang |
| D5 | `request_timeout` 300 ms, upstream answers after 2 s, raw loopback request to the edge (no twilight) | 504 + `EDGE_TIMEOUT_BODY` after 300..1000 ms; `noctis_http_requests_total{status_class="error"}` +1 |
| D6 | upstream 401 / 403 / 429 `shared` / 200 | invalid counter `401` +1 / `403` +1 / unchanged / unchanged (rl_03) |

</intent-contract>

## Code Map

- `crates/noctis-core/src/edge/mod.rs` -- `Edge::start(..)`/`start_with(..)` (`:100-135`) gain `request_timeout` (+ `proxied` in `start_with`); `forward` (`:390`) wraps `upstream` in the timeout; `upstream` (`:431-470`) handles a 429 per the rule; new `EDGE_TIMEOUT_BODY`, `is_edge_failure`; declare `pub(crate) mod rate_limit;`. Callers: `app.rs:2474`, edge tests.
- `crates/noctis-core/src/edge/pool.rs` -- `Pool` fields: `request_timeout: Duration`, `proxied: bool` (`Pool::new` callers).
- `crates/noctis-core/src/edge/callback.rs` -- deadline `min(..)` (`:182`); in `attempt` (`:350`) record the new metrics from status + headers before `classify`; `read_capped` -> `pub(crate)`.
- `crates/noctis-core/src/edge/metrics.rs` -- `EdgeMetrics::rate_limited(scope)`, `invalid_request(status)`, or one `upstream_status(status, scope)` helper used by both paths.
- `crates/noctis-core/src/edge/rate_limit.rs` (new) -- pure functions + R1-R15 tests (`rate_limit/tests.rs` if > ~150 lines).
- `crates/noctis-core/src/rate_limit.rs` (new) -- `RateLimited`, `RateLimitScope`, `from_http_error`; `lib.rs:228-275` re-export; module docs list it.
- `crates/noctis-core/src/responder.rs:357` `ResponderError` + `responder/transport.rs:472` `http_error`; `webhook.rs:104` + `From<ResponderError>` (`:150`); `raw_http.rs:194` `from_twilight`, `:297` `RawHttpError`; `publish.rs:719` and `app.rs:992` `gateway_info` (429 and 504 cases).
- `crates/noctis-core/src/app.rs` -- `run_with` (`:810`) RL-02 warn after token check; `cluster_without_proxy` near `log_plan` (`:1021`); `http_client` (`:2665`) unchanged; pass `config.http_timeout` into `Edge::start`. `app/tests.rs` -- R16, R17, E13, E11.
- `crates/noctis-core/src/observe.rs:75-215` -- two METRICS rows + table lines.
- `crates/noctis-core/src/edge/test_support.rs` -- `Answer` returns `(StatusCode, HeaderMap, Bytes)` (update existing users) for D1-D6 in `edge/tests.rs`.
- `crates/noctis/src/lib.rs:1603` -- `noctis::http` re-exports `RateLimited, RateLimitScope`; doc paragraph on 429s and `from_http_error`.
- `crates/noctis-testkit/tests/http_rate_limit.rs` (new) -- E1-E12; helpers from `http_edge.rs` (`globals`, counter delta), `raw_http.rs`, `responder_ergonomics.rs:385` (INT-13 header checks), `cluster.rs` (Shard Range App), `testapp.rs`/`commands.rs` for a command interaction through `FakeGateway`.
- `xtask/rules-pending.toml:16-45` -- drop INT-13, RL-01..04.
- `docs/discord-rules.md:168,283-286` -- Notes cells.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md:1140` -- AD-24 edge bullet amendment.
- `examples/project-template/.env.example` -- `NOCTIS_HTTP_PROXY` comment: "Cluster setup: every process points at one shared rate-limit proxy (twilight-http-proxy, nirn-proxy); without it a Cluster logs an RL-02 warning."
- `examples/project-template/README.md:139-143` -- "Scale out later": replace stale `NOCTIS_SHARDS` with `NOCTIS_SHARD_RANGE`/`NOCTIS_SHARD_TOTAL`, proxy mode as the Cluster setup, 429s surface as `RateLimited`.
- `examples/project-template/AGENTS.md:63,65` -- proxy in the Cluster line; "handle `RateLimited` (never retry in a loop; `ctx.http()` errors via `RateLimited::from_http_error`)".
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- §4 Clusters checklist item 2 (warning text), §5 proxy paragraph (`:616-624`: 429 handling, relay rule, timeout on every request incl. callbacks), §11 metrics table (two rows); `limits-cheatsheet.md:218,322-325` notes; `responder.md`/`components-v2.md` §11b error lists gain `RateLimited` where `Rejected` is listed; `SKILL.md:23` API hash.
- `xtask/public-api.txt` -- re-bless.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/rate_limit.rs`, `edge/rate_limit.rs`, `lib.rs` -- public type + pure rules + R1-R15, R18.
- `crates/noctis-core/src/edge/{mod.rs,pool.rs,callback.rs,metrics.rs,test_support.rs,tests.rs}`, `observe.rs` -- 429 relay/surface, metrics, timeouts; D1-D6.
- `crates/noctis-core/src/{responder.rs,responder/transport.rs,webhook.rs,raw_http.rs,publish.rs,app.rs,app/tests.rs}` -- typed mapping, 504 handling, RL-02 warning, `Edge::start` timeout; R16-R17, E11, E13.
- `crates/noctis/src/lib.rs` -- re-exports and docs.
- `crates/noctis-testkit/tests/http_rate_limit.rs` -- E1-E10, E12.
- `xtask/rules-pending.toml`, `docs/discord-rules.md`, spine AD-24, memlog -- catalog and decision records.
- `examples/project-template/{.env.example,README.md,AGENTS.md,.claude/skills/noctis/SKILL.md,.claude/skills/noctis/references/*.md}` -- docs; then `cargo run --release -p xtask -- api-snapshot --bless` and put the printed hash in `SKILL.md`'s `API hash` line.

**Acceptance Criteria:**
- Given a proxy configured by builder or `NOCTIS_HTTP_PROXY`, when the App runs and sends twilight, raw REST, webhook, interaction-callback and interaction-webhook requests, then each reaches the proxy through the edge, the twilight client has no rate limiter, and interaction requests carry no bot token (E1).
- Given an upstream 429 with `X-RateLimit-Scope: shared`, when any Noctis surface or `ctx.http()` sends the request, then it fails once (no retry) with a typed `RateLimited { scope: Shared, .. }` (or the mapped Publish/Startup status 429), and `noctis_http_rate_limited_total{scope="shared"}` grows (E2, E5).
- Given upstream 401/403/429 responses, when they pass the edge, then `noctis_http_invalid_requests_total{status}` counts each except shared-scope 429s (E3, E4, E6, E7).
- Given a Shard Range smaller than the total and no proxy, when `run` starts, then one RL-02 warning is logged; with a proxy or without a Cluster, none (R16, R17, E12, E13).
- Given a configured `http_timeout` (default 10 s), when any Discord request (twilight, raw, callback, edge forward) exceeds it, then it fails within that time (E9-E11).
- Given the story is done, when all local gates run, then they pass, `rules-catalog` reports INT-13 and RL-01..04 covered (none pending), and the Skill's API hash equals the one `api-snapshot --bless` printed.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 38 findings — high 0, medium 9, low 15, false 11, maybe-false 3
- findings:
  - `[low]` `[patch]` (blind) Callback 429s keep the INT-01 fit rule (one local retry, else `AckDeadlineMissed`), yet docs say every 429 is never retried and typed on every surface — code follows the spec's Never rule; docs overclaim. Patch: name the exception in `noctis-core` crate docs, `noctis::http` docs, template AGENTS.md and `app-and-sharding.md` §5.
  - `[low]` `[reject]` (blind) Timeout firing during the 429 body read double-counts `requests_total` and skips the rate-limit counters — real, but needs a 429 body that stalls after headers until `http_timeout`; the fix adds a recorded-status flag (new branch), not worth it.
  - `[low]` `[reject]` (blind) Edge timeout does not bound a streamed non-429 body after headers — matches the spec's shape (timeout around `upstream()`); a stalled body after timely headers is unlikely and bounding `Timed` adds complexity.
  - `[low]` `[reject]` (blind) Relayed direct `user` 429 body capped at 64 KiB / empty on read error — twilight's limiter path re-sends a 429 without reading its body and Discord 429 bodies are ~100 bytes; fix adds a fallback branch.
  - `[medium]` `[patch]` (blind) `classify_429` parses a content-encoded 429 body as raw bytes — verified: twilight-http 0.17.1 with `decompression` sends `Accept-Encoding: br` (`client/mod.rs:2952`) and `end_to_end` forwards it, so a Brotli 429 body from Discord loses body `retry_after`, `global` and `discord_code`. Patch: decode a `content-encoding: br` body (capped) before classifying.
  - `[low]` `[reject]` (blind) `valid_reset_after` guard only on surfaced 429s, not on other relayed direct-mode responses — `relay()` behaviour is pre-existing and Discord never sends such headers; a guard on every response adds a branch on the hot path.
  - `[low]` `[reject]` (blind) A proxy-synthesized scope-less 429 counts as an invalid request — the spec counts scope-less 429s on purpose (Cloudflare's own 429s carry no scope and are indistinguishable at the edge); overcounting only makes alerts conservative, and separating them needs heuristics.
  - `[false]` `[reject]` (blind) Callback hitting `http_timeout` reported as `AckDeadlineMissed`, hedge window shortened — the spec settles it (`deadline = min(..)`, "result stays `DeadlineMissed`"), and `responder.md` documents both causes.
  - `[low]` `[reject]` (blind) Edge 504 timeout body not mapped on raw/responder surfaces — twilight's per-attempt timeout starts before the edge's (which starts after the body is collected) with the same value, so the edge 504 practically never reaches an App caller; extra mappings add branches.
  - `[low]` `[reject]` (blind) `proxied` derived in `Pool::new` and `Edge::start`, then overwritten in `start_with` — the spec prescribes this shape; production always overwrites through `EdgeOptions` and no caller diverges.
  - `[low]` `[reject]` (blind) `RawHttpError::RateLimited` Display omits scope/retry_after (data in `source()`) — follows `RawHttpError`'s own source-chain convention; cosmetic.
  - `[false]` `[reject]` (blind) E10 accepts twilight's own timeout, not the edge 504 — the spec's E10 expects `RawHttpError::Http` (twilight timeout); the edge bound is proven by D5 `forward_times_out` as the spec assigns.
  - `[medium]` `[patch]` (blind) RL-02 warning has no positive test through `run`/the App method — same root as the vgap gap below. Patch: test `App::warn_cluster_without_proxy` on a built App with and without proxy.
  - `[false]` `[reject]` (blind) Add `+5`/leading-zero/non-UTF-8 cases to R12 — `relay_429` and twilight's `parse_ratelimit_headers` use the same `to_str().parse::<u16>()` and bucket `as_bytes()`; no disagreement or bad outcome exists.
  - `[maybe-false]` `[defer]` (blind) `story-loop.js` resume of `'done'` and dead recovery agent — pre-existing workflow tooling identical to origin/main (commit 2defad7), not this story; settle by tracing `resumed()`.
  - `[maybe-false]` `[defer]` (blind) `mergeIntoMain` can leave the merge lock held — pre-existing tooling; settle by checking release failure handling.
  - `[medium]` `[defer]` (blind) `bg.sh` first caller consumes the job, second caller spins; empty status exits 0 — verified in `bg.sh:24-29`; pre-existing tooling from origin/main, not this story.
  - `[maybe-false]` `[defer]` (blind) `epic-last.py` two in-progress stories block `cargo hack`; silent fetch fallback — pre-existing tooling; settle by running it on such a status.
  - `[false]` `[reject]` (blind) Story diff bundles unrelated tooling/AGENTS.md policy changes — those files are byte-identical to origin/main (`git diff origin/main HEAD -- tools .claude/workflows AGENTS.md` is empty; commit 2defad7 and siblings); the lane synced them, the story did not author them.
  - `[low]` `[patch]` (blind) `.env.example` new Cluster comment repeats the line above it — direct correction. Patch: merge the two comments into one.
  - `[low]` `[reject]` (edge) Timeout after a recorded 429 double-counts — same root and rejection as the blind timeout/429-body row.
  - `[low]` `[reject]` (edge) Truncated/failed 429 body relayed as complete — same root and rejection as the blind relay-body row.
  - `[medium]` `[defer]` (edge) `bg.sh` waits forever on a dead runner — verified (no liveness check); pre-existing tooling, not this story.
  - `[medium]` `[defer]` (edge) `bg.sh` second caller gets an empty status — same root as the blind `bg.sh` row; deferred with it.
  - `[medium]` `[defer]` (edge) `lane.sh lock` spins without sleep past its deadline on a failed move — verified (`continue` skips the deadline check and `sleep`); pre-existing tooling.
  - `[low]` `[patch]` (edge) Callback callers never see `ResponderError::RateLimited` and may get one retry — same root as the blind callback-docs row (spec keeps the fit rule); fixed by the same doc correction.
  - `[low]` `[reject]` (edge) Streamed body after timely headers outlasts `request_timeout` — same root and rejection as the blind streamed-body row.
  - `[medium]` `[patch]` (vgap) RL-02 warning never verified at its `run` call site (pre-verified gap) — grouped with the blind RL-02 row. Patch: `app/tests.rs` test on a built App (`shard_total(4).shard_range(0..2)`, no proxy → one RL-02 warn; with `http_proxy` → none).
  - `[medium]` `[patch]` (vgap) `Edge::start`'s `proxied` derivation never exercised (pre-verified gap) — Patch: `edge/tests.rs` asserts `!pool.proxied` for `Upstream::discord()` and `pool.proxied` for `Upstream::http(..)` via `Edge::start`.
  - `[low]` `[reject]` (vgap other) `proxied` computed twice — same root and rejection as the blind duplication row.
  - `[false]` `[reject]` (intent 1) Direct-mode `global`/`shared`/scope-less 429s surfaced too — the spec deliberately settles it (Design Notes: twilight 0.17.1 hot-loops/cannot wait these out; D2).
  - `[false]` `[reject]` (intent 2) `ctx.http()` callers see 503 and need `from_http_error` — spec-settled decision ("Typed error everywhere"), documented in the Skill.
  - `[medium]` `[patch]` (intent 3) RL-02 positive case untested at startup — grouped with the vgap RL-02 gap.
  - `[false]` `[reject]` (intent 4) No-bypass shown as reachability, not absence — the spec's E1 defines the check; the single-client structure (no second client, `Never` rule) gives the absence.
  - `[false]` `[reject]` (intent 5) Edge forward bound not distinguished end to end; callback timeout is `DeadlineMissed` — both settled by the spec (E10 expectation, D5, "result stays `DeadlineMissed`").
  - `[false]` `[reject]` (intent 6) Unrelated repository changes in the diff — identical to origin/main; synced lane tooling, not authored by the story.
  - `[false]` `[reject]` (intent 7) Template code not changed — the new API is additive (new type, new variants); template code matches only `UpdateUnavailable` and `WebhookError::Rejected{404}`, so no template code is affected; AGENTS.md and Skill were updated.
  - `[false]` `[reject]` (intent 8) RL-03 excludes shared 429s and counts proxy 429s — spec-settled per Discord's docs (Design Notes, E3/E4/D6).

## Design Notes

- **Why the edge rewrites some 429s** (decision): twilight-http 0.17.1 with `ratelimiter(None)` re-sends a 429 at once in an unbounded loop, and its limiter returns `None` for `global`/scope-less 429s (also an immediate retry) and panics on a negative/NaN `x-ratelimit-reset-after`. A response twilight treats as a plain error (503, chosen as "temporarily unavailable, see Retry-After") stops the loop; keeping Discord's headers lets the local limiter (direct mode) still learn the bucket. Byte-for-byte forwarding stays the rule for everything else, including the one 429 twilight handles well (direct, `user`, valid bucket headers: RL-01 per-route waiting).
- **Typed error everywhere** (decision): Noctis-owned surfaces get a `RateLimited` variant; `ctx.http()` users get `RateLimited::from_http_error`. Publish/Startup keep their status-carrying errors and report the original 429.
- **Invalid-request metric excludes shared 429s** (decision), per Discord's docs; counters only (alert on `increase(noctis_http_invalid_requests_total[10m])` summed per egress IP), no in-process window.
- **Timeout everywhere** (decision): the callback deadline is the earlier of the ack deadline and `http_timeout`; the edge bounds forwarded requests by the same value, so a request stays bounded even when the twilight caller is gone.
- Forward-path 429 shape:

```rust
if status == StatusCode::TOO_MANY_REQUESTS {
    let headers = end_to_end(response.headers());
    let body = read_capped(response.into_body(), 64 * 1024).await.unwrap_or_default();
    let info = rate_limit::classify_429(&headers, &body);
    metrics.upstream_status(429, Some(info.scope));
    return if rate_limit::relay_429(&headers, pool.proxied) { rebuilt(429, headers, body) }
           else { edge_rate_limited(headers, rate_limit::edge_body(&info)) };
}
```

## Verification

Run from `/home/claude/lanes/c/noctis-rs`, release only:

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis-core` edge/rate_limit/app tests, testkit `http_rate_limit`, `http_edge`, `http_fake`, `raw_http`
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0 (12.4 is not the last story of Epic 12; the story loop may skip it per the 2026-10-23 decision)
- `cargo run --release -p xtask -- api-snapshot --bless` then `-- api-snapshot` -- expected: exit 0, hash matches `SKILL.md`
- `cargo run --release -p xtask -- deps-check` / `-- rules-catalog` / `-- skill-check` / `-- tw-grammar --check` -- expected: exit 0; rules-catalog lists no pending INT-13/RL-*
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` -- expected: exit 0
- `cargo +1.91 check --release --workspace` -- expected: exit 0

## Auto Run Result

- **Summary:** Shared HTTP rate limiting through the HTTP edge/proxy: the edge classifies every upstream 429 (`classify_429`), relays only the direct-mode `user` 429 with valid bucket headers (RL-01 per-route waiting) and rewrites the rest to a 503 so twilight-http 0.17.1 stops hot-looping; typed `RateLimited` errors on Noctis surfaces (`RawHttpError`, `ResponderError`, `WebhookError`) plus `RateLimited::from_http_error` for `ctx.http()` users; RL-02 startup warning for a cluster without a proxy; RL-03 invalid-request counters (shared 429s excluded); edge forward timeout = `http_timeout`, callback deadline = min(ack deadline, `http_timeout`).
- **Files changed:** `crates/noctis-core/src/edge/{mod,rate_limit,metrics,pool,callback,test_support}.rs` (429 classification, rewrite, timeout, metrics); `crates/noctis-core/src/rate_limit.rs` (public `RateLimited` type); `app.rs`, `raw_http.rs`, `responder*.rs`, `webhook.rs`, `publish.rs`, `observe.rs`, `lib.rs` (typed variants, RL-02 warning, metrics); tests in `edge/tests.rs`, `edge/rate_limit/tests.rs`, `app/tests.rs`, testkit `http_rate_limit.rs`, `observability.rs`; `Cargo.toml`/`noctis-core/Cargo.toml` (`brotli-decompressor` 5.0.3, already in graph); `noctis/src/lib.rs` docs; `docs/discord-rules.md`, `xtask/rules-pending.toml`, `xtask/public-api.txt`; ARCHITECTURE-SPINE + memlog; project template AGENTS.md, README, `.env.example`, Skill (`SKILL.md` API hash, `app-and-sharding.md`, `responder.md`, `components-v2.md`, `limits-cheatsheet.md`).
- **Review findings:** 38 total (high 0, medium 9, low 15, false 11, maybe-false 3).
  - Patched (6 entries; medium 4, low 3 counted by verdict incl. grouped): Brotli-encoded 429 body decoded before classification; RL-02 warning tested on a built App with/without proxy; `Edge::start` `proxied` derivation tested; callback-429 fit-rule exception documented in core/noctis docs, template AGENTS.md and `app-and-sharding.md`; `.env.example` duplicate comment merged.
  - Deferred (6): pre-existing story-loop tooling issues (`bg.sh` single-consumer/empty status/no liveness check, `lane.sh lock` spin, `story-loop.js` resume, `mergeIntoMain` lock, `epic-last.py`) - not this story's code.
  - Rejected (26): timeout during 429 body read double-count, streamed body after headers unbounded, 64 KiB relay cap, guard on non-429 relays, scope-less proxy 429 counted, edge 504 unmapped, duplicate `proxied` derivation, Display omits scope - all low-value/new-branch costs per triage; 11 false findings spec-settled (direct-mode global/shared surfaced, typed 503 for `ctx.http()`, E10/D5 timeout assignment, unrelated synced tooling diff, template code unaffected, RL-03 semantics).
- **Follow-up review recommended: true** - four medium entries patched on a first pass; named unverified risk: the new Brotli decode path for `content-encoding: br` 429 bodies (capped decompression, fallback on decode error) has not had an independent review.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, deps-check, tw-grammar, tw_crosscheck, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91); `cargo hack` skipped (not the epic's last story). CI paused by the maintainer.
- **Residual risks:** Brotli decode path reviewed only by its author; streamed non-429 bodies after timely headers are not bounded by the edge timeout; a 429 body stalling until `http_timeout` double-counts `requests_total`.
