---
title: 'Story 5.9: Warm, deadline-safe Discord HTTP edge'
type: 'feature'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: '1984f25ae01463817040db869e5d6b822f29ff8b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/research/http-warm-transport.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-8-graceful-shutdown-and-fatal-close-reporting.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
  - '{project-root}/examples/project-template/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** twilight-http 0.17.1 gives no transport control. It sends no h2 PING, sets no TCP keepalive and no connect timeout, and resolves DNS with blocking `getaddrinfo`. A silently dropped pooled connection therefore hangs every request until twilight's 10 s timeout. twilight's RateLimiter also serializes every interaction callback bot-wide (research §1, §3; measured 1/2/3/4 s). Interactions miss Discord's 3 s deadline (INT-01) and nothing reports it.

**Approach:** The new crate-internal `noctis-core` module `edge` adds:
- A loopback HTTP/1.1 forwarder. The App's one twilight client points at it with `proxy("127.0.0.1:<port>", true)`.
- One tuned upstream: h2 with PING, TCP keepalive/nodelay, connect/TLS timeouts and a TTL-cached async DNS.
- A keeper task that warms a connection and redials it.
- A crate-internal interaction-callback path. It skips twilight, sends one hedged attempt, treats 40060 as success and returns `AckDeadlineMissed` past the deadline.
- Per-phase metrics.

The public surface is only `HttpConfig`, `HttpConfigError`, `AppBuilder::http`, `App::http_config`, `AckDeadlineMissed` and the new metric rows. `FakeDiscordHttp` gains h2c, a connection log with PINGs, stalls, black-holing and connection closing.

## Boundaries & Constraints

**Always:**
- **Public API (core, re-exported at the core and facade roots):**
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct HttpConfig` has pub fields and `Default`:

    | Field | Default | Range |
    | --- | --- | --- |
    | `ping_interval` | 20 s | 50 ms..=300 s |
    | `ping_timeout` | 5 s | 50 ms..=60 s |
    | `hedge_after` | 1000 ms | 300 ms..=2800 ms |
    | `connect_timeout` | 1500 ms | 100 ms..=30 s |
    | `dns_ttl_floor` | 30 s | 1 s..=86 400 s |
    | `dns_ttl_cap` | 300 s | 1 s..=86 400 s |
    | `upstream_h2c` | `false` | — |

    Rustdoc shows the mutate-a-default pattern, because `non_exhaustive` forbids struct literals.
  - `AppBuilder::http(&mut self, HttpConfig) -> &mut Self`: the last call wins and `build()` resets it. `App::http_config(&self) -> HttpConfig`. Both `Debug` impls show it. There is no env variable.
  - `build()` validates the fields in the order above. Each out-of-range field is its own `BuildViolation::InvalidHttpConfig { reason: HttpConfigError }`.
    - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)] pub enum HttpConfigError` has `PingInterval(Duration)`, `PingTimeout`, `HedgeAfter`, `ConnectTimeout`, `DnsTtlFloor`, `DnsTtlCap` and `DnsTtlOrder { floor, cap }`.
    - Message: `"HttpConfig.<field> {actual:?} is out of range: use <lo>..=<hi>"`. For the order error: `"HttpConfig.dns_ttl_floor {floor:?} is above dns_ttl_cap {cap:?}"`, checked only when both are in range.
    - `BuildViolation`'s message is `"invalid HTTP config: {reason}"`.
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)] pub struct AckDeadlineMissed { pub elapsed: Duration }`.
    - Message: `"interaction not acknowledged in time: {elapsed:?} after receipt, past the 2.9 s ack deadline (Discord allows 3 s; INT-01): defer slow work"`.
    - Story 6.5 wraps it into its Responder error. `noctis::Error` does not change.
- **Edge server (`build()`):**
  - Bind a `std::net::TcpListener` on `127.0.0.1:0`; `build()` stays sync. A bind error is `BuildViolation::HttpEdge { kind: std::io::ErrorKind }` with the message `"the Discord HTTP edge could not bind 127.0.0.1:0 ({kind:?})"`.
  - `tokio::spawn` the accept task on the current runtime. The task converts the listener with `TcpListener::from_std` inside `catch_unwind`, because that panics when the runtime has no IO driver. On failure it logs `error!(target: "noctis::http", "the Discord HTTP edge could not start: build the tokio runtime with enable_all()")` and drops the listener, so clients are refused instead of hanging.
  - The twilight client is built with `.proxy(edge_authority, true)`, plus `.ratelimiter(None)` only when `.http_proxy` is set (5.2 behaviour).
  - The `Edge` handle lives in `AppShared`. Dropping the last `AppShared` aborts the accept task, every connection task and the keeper. A twilight `Arc<Client>` that outlives the App then gets connection errors; document this.
  - It serves HTTP/1.1 with keep-alive. It forwards only an origin-form target whose raw path starts with `/api/v10/` and has no `.`/`..` segment (`%2e`/`%2E` included) and whose method is not `CONNECT`. Anything else gets 403 with `{"message":"noctis edge: forbidden path","code":0}` and is never forwarded.
  - The request body is buffered, capped at 512 MiB. A larger declared or streamed body gets 413 with `{"message":"Request entity too large","code":40005}`.
  - **Request:** method, query, headers (order and duplicates kept) and body are byte-identical, except:
    - The hop-by-hop headers are dropped: `connection` and the names it lists, `keep-alive`, `proxy-connection`, `te`, `trailer`, `transfer-encoding`, `upgrade`.
    - `host` is rewritten to the upstream authority over h1. Over h2 it is removed and the absolute URI carries `:authority`.
  - **Response:** status, headers (minus hop-by-hop) and the body (streamed, never buffered) pass unchanged, so twilight keeps learning buckets (RL-01).
  - The edge never adds `authorization`. It never logs or records a path, query, header value or body (paths carry interaction/webhook tokens).
- **Upstream (`edge/connect.rs`, `edge/pool.rs`):**
  - The default upstream is `https://discord.com:443` with TLS server name `discord.com`. With `.http_proxy` it is `http://<normalized authority>`. A crate-internal `Upstream::Tls { host, port, server_name, config: Arc<rustls::ClientConfig> }` variant exists for tests.
  - TLS: `rustls::ClientConfig::with_platform_verifier()` (`rustls_platform_verifier::ConfigVerifierExt`, the process-default `ring` provider). ALPN is `["h2", "http/1.1"]` and the server's choice decides the protocol.
  - Over `http://`, `upstream_h2c` selects h2 prior-knowledge, otherwise HTTP/1.1.
  - **Dial phases (each timed):**
    1. DNS through the cache; an IP literal skips the resolver, but the `dns` sample is still recorded.
    2. TCP, trying up to 3 addresses in order, each under `connect_timeout`. Then, through `socket2::SockRef`: `TCP_NODELAY`, keepalive 30 s / 10 s / 3 probes and, on Linux, `TCP_USER_TIMEOUT` 10 s. An option error is logged at `debug` and ignored.
    3. TLS, bounded by 3 s.
    4. The h1/h2 handshake, bounded by 3 s.
  - **h2 setup:** `hyper::client::conn::http2::Builder` with `TokioExecutor`, `timer(TokioTimer)`, `keep_alive_interval(ping_interval)`, `keep_alive_timeout(ping_timeout)` and `keep_alive_while_idle(true)`. Its IO is wrapped in a `Sniff` adapter that records h2 PING round trips.
  - **h1 setup:** `hyper::client::conn::http1`. Idle connections sit in a LIFO stack (max 32 idle), and closed ones are discarded on checkout.
  - **Connections:** there is one shared primary for h2. An on-demand dial counts as reason `demand` when no ready connection exists; an h2 demand dial becomes the primary.
- **Keeper (started by `run_with` before `gateway_info`, guard dropped after `shut_down` returns):**
  - It dials one connection (reason `warmup`) and sends no request. `run_with` awaits the first warm-up for at most `connect_timeout + 6 s`, and a failure is a `warn!` (the following `GET /gateway/bot` reports the error).
  - It redials (reason `redial`) when:
    - The connection task ends (GOAWAY, PING timeout, IO error, peer close). A hyper `is_timeout()` error increments `noctis_http_ping_timeouts_total`.
    - `on_shard_event` sees `ShardEvent::Disconnected { close_code: None, .. }` (network loss).
    - A callback hedge won after attempt 1 got no headers.
  - A redial replaces the primary; the old connection finishes its streams and drops.
  - Back-off: the delays after consecutive failed dials are 0, 0.5, 1, 2, 5, 5 … s, each × a uniform jitter in [0.8, 1.2] (`getrandom`). The sequence resets after a successful handshake.
- **Retry rule (forwarded requests):** exactly one retry, on a fresh or other ready connection, only when the failure is retry-safe:
  - a dial error (DNS, connect, TLS or handshake);
  - `TrySendError::into_message()` returned `Some` (hyper did not send it);
  - an `h2::Error` in the source chain with `reason() == Some(Reason::REFUSED_STREAM)`, or `is_go_away() && is_remote()` (h2 0.4.20 fails streams above GOAWAY's `last_stream_id` that way).

  Any other failure, or a failed retry, answers 502 with `{"message":"noctis edge: upstream unavailable","code":0}` (the constant `EDGE_ERROR_BODY`) and is never retried. `App::gateway_info` maps a `Response { status: 502 }` whose body starts with `{"message":"noctis edge:` to `GatewayInfo { status: None }`. twilight's own timeout still applies to every request.
- **Callback path (`edge/callback.rs`, crate-internal):**
  - `Edge::interaction_callback(CallbackRequest) -> Result<CallbackAck, CallbackError>`.
    - `CallbackRequest` has `interaction_id: Id<InteractionMarker>`, `token: String` (redacted `Debug`), `body: CallbackBody`, `with_response: bool` and `received_at: tokio::time::Instant`.
    - `CallbackBody` is `Json(Bytes)`, built by `CallbackBody::json(&InteractionResponse)` with serde_json, or `Raw { content_type: HeaderValue, body: Bytes }`.
  - Validation before any send: the token is 1..=512 bytes of `[A-Za-z0-9._-]`, else `CallbackError::InvalidToken`.
  - Request: `POST /api/v10/interactions/{id}/{token}/callback`, plus `?with_response=true` when asked. Headers: `content-type`, `content-length` and `user-agent: DiscordBot (https://github.com/ErdemGKSL/noctis-rs, <CARGO_PKG_VERSION>)`. No `authorization`. It goes straight to the upstream pool and never through twilight or the loopback listener.
  - `deadline = received_at + 2900 ms`.
    - If `now >= deadline` ⇒ `AckDeadlineMissed`, nothing is sent.
    - `hedge_at = now + max(300 ms, min(hedge_after, deadline − now − 1200 ms))` (saturating). There is no hedge when `hedge_at >= deadline`.
  - At most 2 attempts:
    - Attempt 1 uses the pool.
    - If no response headers have arrived by `hedge_at`, attempt 2 (the hedge) goes out on a freshly dialed, never-pooled connection (reason `hedge`).
    - If attempt 1 fails with a transport error or 5xx while attempt 2 is unused, attempt 2 goes out at once on a fresh connection.
    - A 429 with `retry_after` (the body's JSON number of seconds first, else the `retry-after` header in seconds) is resent after that wait when `now + retry_after < deadline` and an attempt is left. Otherwise (missing or unparseable values count as not fitting) it is `AckDeadlineMissed`.
  - **Outcomes:**
    - The first 2xx wins; the other attempt's future is dropped (h2 RST).
    - 400 with JSON `code == 40060` (body read up to 64 KiB) is success with `already_acknowledged = true`; it increments `outcome="40060"` and logs at `debug` only.
    - Any other 4xx is `CallbackError::Rejected { status, code: Option<u32> }`, without retry.
    - With both attempts failed or 5xx before the deadline: `Transport` or `Rejected` from the last one.
    - Everything runs under `timeout_at(deadline)` ⇒ `AckDeadlineMissed { elapsed: now − received_at }`.
  - **Hedge metric:** `outcome="won"` when the hedge's 2xx won, `"lost"` when a hedge was sent but attempt 1 won.
  - `CallbackAck` holds `status: u16`, `body: Bytes` (the 2xx body, capped at 1 MiB) and `already_acknowledged: bool`.
  - `#[doc(hidden)] pub mod __testing` in core exposes `interaction_callback(app: &App, id: u64, token: &str, body: Bytes, with_response: bool, received_at: Instant) -> Result<CallbackAck, CallbackError>` (both types are doc-hidden pub there) for the testkit E2E tests. Story 6.5 calls `Edge::interaction_callback` directly.
- **DNS (`edge/dns.rs`):**
  - A crate-internal `trait Resolve: Send + Sync + 'static { fn resolve(&self, host: &str) -> BoxFuture<'_, io::Result<Resolved>> }`, where `Resolved` is `{ addrs: Vec<IpAddr>, ttl: Duration }`.
  - Production uses `hickory_resolver::Resolver::builder_tokio()?.build()`, with `ttl = valid_until − now`. If the builder fails, it falls back to `tokio::net::lookup_host` with `ttl = dns_ttl_floor`.
  - `DnsCache` clamps the TTL to `[floor, cap]` and stores `{ addrs, fetched_at, ttl, last_success }`.
    - At age ≥ 75 % of the TTL, a lookup returns the cached answer at once and starts one background refresh (single-flight).
    - At age ≥ the TTL, callers await one shared lookup.
    - On a resolver error or an empty answer, a cached answer whose `last_success` is ≤ 1 h old is served (stale). Without one, the result is a dial error.
- **Metrics (add every row to `observe::METRICS`; the label values are fixed sets):**
  - `noctis_http_requests_total{app, status_class}`: `1xx`..`5xx` or `error`, one per upstream attempt.
  - `noctis_http_phase_seconds{app, phase}` (histogram):
    - `dns`, `connect` and `tls` on new connections (`tls` only for TLS);
    - `ttfb` and `total` per attempt.
  - `noctis_http_connections_total{app, reason}`: `warmup`, `redial`, `hedge` or `demand`.
  - `noctis_http_conn_reused_total{app, reused}`: `true` or `false`.
  - `noctis_http_h2_ping_rtt_seconds{app}` (histogram).
  - `noctis_http_ping_timeouts_total{app}`.
  - `noctis_interaction_ack_hedges_total{app, outcome}`: `won`, `lost` or `40060`.
  - Span `noctis.http{app, kind = forward|callback|warmup, method, attempt, reused, status_class}`.
  - The span fields and labels never carry a path, a token, an id or a header value.
- **Shutdown:** the edge and the keeper stay up through the whole AD-21 sequence and stop only after `gateway.closed()` (the keeper guard is dropped after `shut_down`; the edge drops with `self`).
- **Dependencies:**
  - Root workspace: `h2 = "0.4.20"`, `socket2 = "0.6.5"`, `tokio-rustls = { version = "0.26.6", default-features = false }`, `rustls-platform-verifier = "0.7.1"`, `hickory-resolver = "0.26.3"`, `rcgen = { version = "0.14.10", default-features = false }`.
  - Core: `bytes`, `h2`, `http-body-util`, `hyper` (`client`, `server`, `http1`, `http2`), `hyper-util` (`tokio`), `socket2` (`all`), `tokio-rustls` (`logging`, `tls12`, `ring`), `rustls-platform-verifier`, `hickory-resolver` (default features), `serde_json`, `getrandom`, plus tokio `net`.
  - Core dev-dependencies: `rcgen` (`crypto`, `ring`).
  - Testkit: hyper `http2`, hyper-util `server-auto`, and a normal `noctis-core` dependency (an allowed graph edge).
  - `cargo deny check` must stay green; no `aws-lc` may enter the graph.
- No `unwrap`/`expect` outside tests. Never name a test after `int_01`, `int_13` or `rl_01`: those rules stay pending in `xtask/rules-pending.toml` for 6.6 and 12.4. Build everything `--release`.

**Never:**
- No API polling or `HEAD` liveness requests. The warm-up sends zero HTTP requests.
- No twilight fork, no second twilight client, and `ratelimiter(None)` only with `.http_proxy`.
- No retry of a non-callback request after bytes were written.
- No Responder, auto-defer or Story 6.5/6.6 wiring.
- No change to `docs/discord-rules.md`: the INT-01, INT-13 and RL-01 rows already name the edge
- No env variables for `HttpConfig`, no HTTPS proxy support, no Cluster warning (12.4).
- No change to `noctis::Error`.

## I/O & Edge-Case Matrix

Abbreviations:
- "F" is `FakeDiscordHttp`, attached with `F::attach` (h2c on).
- "F1" is F attached through `.http_proxy(url)` only (HTTP/1.1).
- "cb(d)" is a `__testing::interaction_callback` whose `received_at` lies `d` in the past.
- "E2E" tests are in `noctis-testkit/tests/http_edge.rs`; "unit" tests are in `noctis-core` `edge/**/tests.rs`.
- Every wait is bounded at 15 s.

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Config default | no `.http` | `app.http_config() == HttpConfig::default()` (values in the table above) |
| 2 | Config bounds ok | each field at its low and high end; floor = cap = 1 s | `build()` ok |
| 3 | Config bounds bad | `ping_interval` 49 ms, `ZERO`, 300 s + 1 ns, `Duration::MAX`; `ping_timeout` 49 ms / 60 s + 1 ns; `hedge_after` 299 ms / 2800 ms + 1 ns; `connect_timeout` 99 ms / 30 s + 1 ns; floor 999 ms; cap 86 400 s + 1 ns | one `InvalidHttpConfig` per bad field, in field order, among the other violations; exact messages |
| 4 | TTL order | floor 60 s, cap 30 s; floor 0 + cap 0 | `DnsTtlOrder{60s,30s}`; with both out of range only `DnsTtlFloor` and `DnsTtlCap` |
| 5 | Last wins / reset | `.http(a).http(b)`; build twice | `b`; the second App is back to the default |
| 6 | Loopback bind | any build | the edge's local address is `127.0.0.1:p` with `p != 0`; twilight's requests reach F with `host` = F's authority |
| 7 | No IO driver | `new_current_thread().build()` (no IO), `build()` | `Ok(App)`, with no panic escaping `build()` or the runtime; the edge logs "could not start" (no request is sent: twilight itself needs an IO driver) |
| 8 | Path filter | raw requests to the edge: `/api/v10/gateway/bot` (ok); `/api/v9/x`, `/api/v10`, `/`, `/api/v100/x`, `/API/V10/x`, `/api/v10/../x`, `/api/v10/./x`, `/api/v10/%2e%2e/x`, `/api/v10/%2E/x`, absolute-form `http://evil/api/v10/x`, `CONNECT discord.com:443` | the first is forwarded; every other one gets 403 with the edge body, and F logs nothing |
| 9 | Query raw | `/api/v10/channels/1/messages?limit=5&after=%31` | F gets the same path and query bytes |
| 10 | Request bytes | twilight `create_message` with `X-Audit-Log-Reason` "çay ☕" (percent-encoded by twilight), a multipart upload (2 files, 1 MiB + 0 B), duplicate custom headers in order | the same calls go through a control client: `noctis::twilight::http::Client::builder().token(..).proxy(F2 authority, true).ratelimiter(None)` straight to a second fake F2 (test code only; the `deps-check` scan covers `src/` only). F's and F2's records match on method, path, query, body bytes and every header except `host` and hop-by-hop. Before comparing, each multipart boundary (from `content-type`) is replaced by a fixed token, because twilight randomizes it |
| 11 | Hop-by-hop | raw edge request with `connection: x-foo, keep-alive`, `x-foo: 1`, `te: trailers`, `upgrade: h2c` | F sees none of them; other headers are unchanged |
| 12 | Response bytes | F answers 200 with `x-ratelimit-*` headers and a 10 MiB body; 204 empty; 404 Discord JSON | twilight sees the identical status, headers and body; the proxied no-local-limiter test (5.2) still passes |
| 13 | Body size | 0 B; 64 MiB; declared `content-length` 512 MiB + 1 | forwarded; forwarded; 413 `40005`, F logs nothing |
| 14 | Upstream refused | F dropped, then `app.http().gateway().authed()` | twilight `Response{502}` within 1 s (2 dial attempts); `run` → `GatewayInfo{status: None}`; `requests_total{status_class="error"}` +1 |
| 15 | Post-write failure | stalled route; F logged the request; F dropped | twilight gets 502 ≤ 1 s after the drop; F's log had exactly 1 such request (no retry) |
| 16 | Retry-safe classes | unit, raw h2 script server: (a) RST_STREAM REFUSED_STREAM on stream 1; (b) GOAWAY last_stream_id 0 after HEADERS; (c) an RST with `INTERNAL_ERROR`; (d) a connection closed after HEADERS+DATA; (e) a dial to a refused port | (a) and (b): exactly 1 retry, sent to a normal server on a new connection, which succeeds; (e): exactly 2 dial attempts, then 502; (c) and (d): no retry, 502 |
| 17 | Warm-up | `App::run` against F + FakeGateway | `F.connections()[0]` (h2) opened before `GET /gateway/bot`, which is served on connection 0; the warm-up made 0 requests; `connections_total{reason="warmup"}` = 1 |
| 18 | Warm-up failure | `http_proxy("127.0.0.1:1")`, `run` | `Err(Start(GatewayInfo{status: None}))` within `connect_timeout + 7 s`; one warm-up `warn` |
| 19 | Redial on close | running App, `F.close_connections()` | a new connection is opened within 1 s, `reason="redial"` +1; the next request is on it |
| 20 | Back-off | unit, a dial that keeps failing (paused clock, pure scheduler) | delays fall in [0,0], [0.4,0.6], [0.8,1.2], [1.6,2.4], [4,6], [4,6] s; reset after a success |
| 21 | Idle keep-warm | `HttpConfig{ping_interval 100 ms, ping_timeout 1 s}`, App idle 3 s after startup | ≥ 20 PINGs logged on connection 0; F's request log unchanged; then a request lands on connection 0 with `conn_reused{reused="true"}` +1; the `h2_ping_rtt` histogram has samples |
| 22 | Black hole, then callback | config as in 21; `F.black_hole()` | a redial ≤ 100 ms + 1 s + 1 s; `ping_timeouts_total` +1; `cb(0)` afterwards returns `Ok` on the new connection, with no hedge |
| 23 | Black hole during a callback | `cb(0)` sent right after `F.black_hole()` (before detection), default `hedge_after` | the hedge at 1000 ms ± 100 ms on a new connection succeeds; total < 2.9 s; `outcome="won"` +1 |
| 24 | Shard network loss | `supervise` fed `Disconnected{0, None}`, then `Disconnected{0, Some(4000)}` | exactly 1 keeper redial request (`KeeperHandle::redial_requests()`, a crate-internal counter) |
| 25 | h1 upstream | F1, `run` | the warm connection is `Http1`, no PINGs, and `GET /gateway/bot` reuses it |
| 26 | Callback basic | route `POST /interactions/1/tok-1.x_Y/callback` → 204, `cb(0)`, JSON of `InteractionResponse { kind: DeferredChannelMessageWithSource, data: None }` | `Ok{204, already_acknowledged: false}`; F: no `authorization`, `content-type: application/json`, body = serde_json bytes, `user-agent` starts with `DiscordBot (`; no other request |
| 27 | With response | `with_response = true`, the route answers 200 JSON | query `with_response=true`; `body` = F's body |
| 28 | No serialization | 4 concurrent `cb(0)`, the route delays 1 s | all `Ok` within 1.2 s of the start |
| 29 | Hedge wins | `F.stall(POST, path, 1)`, the route then 204, `cb(0)` | 2 requests on 2 connections (the 2nd new); the 2nd sent at 1000 ± 100 ms; `Ok`; `outcome="won"` +1; `reason="hedge"` +1 |
| 30 | Hedge loses | the 1st attempt delayed 1300 ms → 204, the 2nd delayed 1000 ms | `Ok`, `outcome="lost"` +1 |
| 31 | 40060 | the 2nd attempt answers 400 `{"code":40060}`, the 1st stalled; separately one attempt only → 40060 | `Ok{already_acknowledged: true}`, `outcome="40060"` +1 each; no log at `warn` or `error` |
| 32 | Hedge timing (pure) | remaining 2900 / 2000 / 1500 / 1400 / 1200 / 300 / 200 / 0 ms, `hedge_after` 1000; and remaining 2900 with `hedge_after` 300 | 1000 / 800 / 300 / 300 / 300 / none (300 ≥ remaining) / none / missed; 300 |
| 33 | Deadline passed | `cb(2900 ms)`, `cb(5 s)` | `AckDeadlineMissed{elapsed ≥ 2900 ms}`, F logs 0 requests |
| 34 | Both stall | `F.stall(.., 2)`, `cb(0)` | `AckDeadlineMissed` at 2.9 s ± 100 ms; exactly 2 requests |
| 35 | 429 fits | 1st → 429 `{"retry_after":0.5}`, then 204 | the 2nd sent ≥ 500 ms after the 1st reply; `Ok` |
| 36 | 429 too long / unknown | `retry_after` 5; no `retry_after` and no header; header `retry-after: 1` only (body lacks it) | `AckDeadlineMissed` with 1 request; same; `Ok` after ≥ 1 s |
| 37 | Other 4xx | 400 `{"code":50035}`; 404 `{"code":10062}`; 401 empty body | `Rejected{400, Some(50035)}`, `Rejected{404, Some(10062)}`, `Rejected{401, None}`; 1 request each |
| 38 | 5xx / transport | 1st → 503, 2nd → 204; both 503; both dials refused | `Ok` (2 requests); `Rejected{503, None}`; `Transport` |
| 39 | Token check | `""`, `"a/b"`, `"a?b"`, `"a#b"`, `"a b"`, `"é"`, 513 × `a`; 512 × `a`; `"A-z_0.9"` | `InvalidToken` and 0 requests for the first 7; the last 2 are sent |
| 40 | 40060 body cap | 400 with a 1 MiB body that does not start with JSON | `Rejected{400, None}`; at most 64 KiB read |
| 41 | DNS fresh / refresh | unit, scripted resolver, paused clock: TTL 60 s; lookups at 0, 44, 46 (refresh slow 10 s), 50 and 61 s | resolver calls at 0 and 46 s only (1 refresh); the 50 s lookup returns at once from the cache; the 61 s lookup returns the refreshed answer |
| 42 | TTL clamp | TTL 5 s, 0 s, 3600 s; floor 30 s, cap 300 s | effective 30, 30, 300 s |
| 43 | Stale on failure | TTL 60 s; resolver fails from 70 s on; lookups at 70 s and 3600 + 59 s; one at 3600 + 61 s | the cached addresses, the cached addresses, then a dial error |
| 44 | Single-flight | 10 concurrent lookups of an expired entry | 1 resolver call; all 10 get the answer |
| 45 | IP literal / empty | `127.0.0.1`, `[::1]`; resolver returns `[]` | resolver never called (the `dns` sample is still recorded); `[]` counts as a failure (stale rule applies) |
| 46 | PING sniffer | unit: client preface + SETTINGS + PING(p1), split at every byte offset; inbound PING ACK(p1) split; ACK(p9) unknown; 16 KiB DATA; 0-length frames; 5 outstanding pings | 1 RTT sample for p1; none for p9; DATA skipped; at most 4 outstanding kept (oldest evicted); never panics |
| 47 | TLS upstream | unit, rcgen CA + tokio-rustls server, ALPN `h2` / only `http/1.1` | the edge negotiates h2 / h1; a `tls` phase sample; a request round-trips |
| 48 | Phases & reuse | 1st request (new connection) and 2nd request through F | 1st: `dns`, `connect`, `ttfb`, `total` samples, `conn_reused{false}`; 2nd: `ttfb`, `total`, `conn_reused{true}`; no `tls` for http |
| 49 | Label policy | the recorded set after rows 14, 17, 21, 23, 26 and 31 (5.7 harness) | every key is in `METRICS` with exactly its labels; values are in the sets above; no label or `noctis.http` field contains the token, `tok`, an interaction id or a path; `METRICS.len() == 13` |
| 50 | Shutdown | the 5.8 E2E `B` handler calls `ctx.http()` after close 4000 | it still succeeds; after `run_until` returns, every F connection is closed within 2 s, and none closed before the last shard's close frame |
| 51 | Smoke unchanged | the 5.2 `http_fake.rs` and 5.3 `gateway_fake.rs` suites | pass with no source change |
| 52 | Facade | `noctis::{HttpConfig, HttpConfigError, AckDeadlineMissed}` | they resolve; the `AckDeadlineMissed` message is exact |

</intent-contract>

## Code Map

- `crates/noctis-core/src/app.rs`:
  - `App` fields L96-115: add `http_config`.
  - `run_with` L441: start the keeper before `gateway_info` and await the warm-up; the keeper guard is dropped after `shut_down`.
  - `shut_down` L538.
  - `gateway_info` L572-595: the edge-body mapping.
  - `supervise` L703 / `on_shard_event` L737: the `Disconnected{None}` redial; pass the keeper handle.
  - `Debug` L852 and L1482.
  - Builder fields L897, `new` L948, the setter beside `http_timeout` L1057, `build` L1362-1460 (validation, bind, twilight `.proxy`).
  - Tests in `app/tests.rs`: L515 `run_multi_thread` must keep `GatewayInfo{None}`; L1184 `run_without_io_driver` must stay `SignalHandler` (the edge must not fail `build()` there).
- `crates/noctis-core/src/handler.rs` L96 `AppShared`: add `edge: Edge`.
- `crates/noctis-core/src/error.rs`: `BuildViolation` L105ff (`InvalidHttpConfig`, `HttpEdge`), plus `AckDeadlineMissed`. `GatewayInfo` L302.
- `crates/noctis-core/src/observe.rs`: `METRICS` L76 (7 rows), docs table, module docs (`noctis.http`). `observe/tests.rs` L48 (`len` 13, label whitelists).
- New module tree `crates/noctis-core/src/edge/`, each file with a sibling `tests.rs`:
  - `mod.rs`: `Edge`, the accept loop, forward and retry.
  - `config.rs`: `HttpConfig`, `HttpConfigError`.
  - `connect.rs`: the dial phases and `Upstream`.
  - `pool.rs`: the primary, the h1 stack and the keeper.
  - `dns.rs`.
  - `sniff.rs`.
  - `callback.rs`.
  - `testing.rs`: the `__testing` module.
- `crates/noctis-core/src/lib.rs`: `mod edge;`, `#[doc(hidden)] pub mod __testing`, re-exports, the crate docs (an HTTP section).
- `crates/noctis-core/src/worker.rs`: `AbortOnDrop` L40 (reuse it for the edge tasks).
- `crates/noctis-gateway/src/inbound.rs` L172: `Disconnected{close_code: None}` means no close frame (read-only).
- `crates/noctis-testkit/src/fake.rs`:
  - Add `connection` to `RecordedRequest` L264 (fix the 2 doctest literals).
  - Add `RecordedHttpConnection { id, protocol: HttpProtocol, opened_at: tokio::time::Instant, closed_at: Option<Instant>, requests: usize, pings: Vec<Instant>, black_holed: bool }`.
  - Add `connections()`, `stall(method, path, count)`, `black_hole() -> usize` (affects the open connections only), `close_connections() -> usize` and `attach(&self, &mut AppBuilder) -> &mut AppBuilder` (`http_proxy(url)` + `HttpConfig` with `upstream_h2c = true`). Update the module docs.
- `crates/noctis-testkit/src/fake/http.rs`:
  - `serve` L38: switch to `hyper_util::server::conn::auto::Builder` (h1 + h2c prior knowledge).
  - Add a per-connection IO wrapper: a client-to-server frame scanner for PINGs, a black-hole flag (reads drained and discarded, writes swallowed, the socket kept open) and close-on-command.
  - Stalled routes await `pending()`.
- Tests that must stay green: `noctis-testkit/tests/http_fake.rs` (`dropped_fake_*` L519-536 now get edge 502s), `sharding.rs` L300-325 (`status: None` rows), `shutdown.rs`, `observability.rs` (the global recorder and serial lock pattern; extend its whitelist).
- `crates/noctis/src/lib.rs`: the root re-export L267-272, the `observe` module docs L274, the crate docs.
- `xtask/src/deps_check.rs`: read-only (no new rule); `xtask/rules-pending.toml`: read-only.
- Skill:
  - `references/app-and-sharding.md`: §1 L40 (`build` spawns the edge on the current runtime), the AppBuilder table L95 (`.http(HttpConfig)`), the Errors § (`InvalidHttpConfig`, `HttpEdge`, `AckDeadlineMissed`), §5 L367-378 (the edge, `HttpConfig`, no polling, the proxy composition), §8 (the edge lives through shutdown), §11 L538 (the 7 new metrics, `noctis.http`; drop "[planned, Story 5.9]").
  - `troubleshooting.md`: rows for "interaction failed / AckDeadlineMissed" and "first request after idle slow".
  - `SKILL.md` L21: the API hash.
- Template: `examples/project-template/src/lib.rs` `configure` (`.http(HttpConfig::default())` with a comment that the defaults are tuned and optional) and `AGENTS.md` (one HTTP-edge bullet).
- Architecture: `ARCHITECTURE-SPINE.md` AD-24 already describes the edge (read-only); add the `.memlog.md` decisions.

## Tasks & Acceptance

**Execution:**
1. Root `Cargo.toml`, `crates/noctis-core/Cargo.toml`, `crates/noctis-testkit/Cargo.toml`: the dependencies listed above. Run `cargo deny check` first.
2. `crates/noctis-core/src/edge/{config,dns,sniff}.rs` (+ tests), with `HttpConfigError`, `BuildViolation`s and `AckDeadlineMissed` in `error.rs`. Tests: Matrix 1-5 (in `app/tests.rs`), 20, 32, 41-46.
3. `edge/{connect,pool}.rs` and `edge/mod.rs` (+ tests: the raw h2 script server; the rcgen TLS server). Tests: Matrix 6-9, 11, 13, 16, 24 (the keeper handle), 47.
4. `edge/callback.rs` and `testing.rs`.
5. `app.rs`/`handler.rs`/`lib.rs`: wiring as in the Code Map.
6. `observe.rs`: the rows and the label tests.
7. Testkit fake and `tests/http_edge.rs`: Matrix 10, 12, 14-15, 17-19, 21-23, 25-31, 33-40, 48. Extend `observability.rs` (49) and `shutdown.rs` (50). Run `http_fake.rs` and `gateway_fake.rs` untouched (51).
8. Facade re-exports and docs, with a `crates/noctis/tests/` assertion (52).
9. Skill, template, `AGENTS.md`, `api-snapshot --bless`, the `SKILL.md` hash, and the memlog implementation decisions.

**Acceptance Criteria:**
- Given any built App, when twilight sends a request, then it passes the loopback edge, which forwards only `/api/v10/` targets byte-for-byte (except `host` and hop-by-hop headers) to discord.com, or to the `.http_proxy` upstream, with twilight's limiter unchanged, and never logs a path, header value or token (Matrix 6-15).
- Given `App::run`, when it starts, then one upstream connection is warm before `GET /gateway/bot`. It stays warm through h2 PINGs only (zero API requests), is redialed with back-off after a close, a PING timeout or a shard network loss, and stays up until after `gateway.closed()` (Matrix 17-25, 50).
- Given an interaction callback, when it is sent through the crate-internal path, then it bypasses twilight, runs concurrently, is hedged once on a fresh connection, counts 40060 as success, and returns `AckDeadlineMissed` past `received_at + 2.9 s` (Matrix 26-40).
- Given the new public API, when the gates run, then the skill, template, snapshot and hash are updated and every local gate passes.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why a loopback forwarder:** `proxy(.., true)` is twilight's only transport hook (research §1.5). It gives full control without a fork, and the fakes keep using the same production path (AD-24).
- **Build-time spawn:** `app.http()` must work without `run` (5.2 smoke tests), so the edge serves from `build()`. Only the keeper (warm-up) waits for `run`. `catch_unwind` around `from_std` keeps `build()` panic-free on IO-less runtimes (5.8 used the same pattern for signals).
- **hyper `conn` instead of the legacy `Client`:** the legacy pool cannot be pre-filled without a request and hides connection closure. A small own pool gives warm-up, the hedge's fresh connection and the `reused` metric.
- **PING RTT through a sniffer:** hyper keeps PING handling internal. A passive frame scanner on the plaintext IO measures RTT without replacing hyper's h2 client. The fake uses its own copy, because testkit cannot reach core internals.
- **hickory-resolver:** `getaddrinfo` reports no TTL, and the story requires TTL-honouring refresh. hickory 0.26.3 (MSRV 1.88, MIT/Apache) reads the system config. The `getaddrinfo` fallback assumes the floor TTL.
- **Edge 502 marker:** tests and users read "upstream unreachable" as `GatewayInfo{status: None}`, as before the edge. A real Discord 502 has another body and keeps `Some(502)`.
- **`demand` reason:** a connection opened for a request when none is ready is neither warm-up, redial nor hedge. A fourth fixed value keeps the counter honest.
- **`__testing` seam:** the callback path stays crate-internal until 6.5. Doc-hidden items are absent from rustdoc JSON, so the API snapshot ignores them.
- Golden sketch (hedge):
  ```rust
  let first = attempt(pool.checkout().await?, &req);
  tokio::pin!(first);
  let ack = tokio::select! {
      biased;
      r = &mut first => r,
      () = sleep_until(hedge_at), if hedge_at < deadline => race(first, attempt(dial(Hedge).await?, &req)).await,
  };
  ```

## Verification

**Commands** (repo root; local gates only; never a debug build):
- `cargo fmt --all --check`: expected exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: expected exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace`: expected exit 0. Run `-p noctis-testkit --test http_edge --test http_fake --test shutdown --test observability` 3 times; there must be no flakes.
- `cargo hack test --release --workspace --each-feature`: expected exit 0.
- `cargo +1.91 check --release --workspace --all-targets` and `cargo deny check`: expected exit 0, and `cargo tree -i aws-lc-rs` must find nothing.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog` and `-- skill-check`: each must exit 0.
- `test ! -d target/debug`: expected true.
