---
title: 'Story 12.4: Shared HTTP rate limiting through a proxy'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
baseline_revision: '0b920b924c7fd845eb2298a63d180cbf549a0691'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-9-warm-deadline-safe-discord-http-edge.md'
warnings: ['oversized']
deferred: []
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
