# Research: keeping Discord HTTP "warm" without polling — root causes and a transport design for Noctis

- Date: 2026-10-20
- Scope: twilight-http 0.17.1 on hyper 1.12.0 / hyper-util 0.1.21 / hyper-rustls 0.27.10 / rustls 0.23.46 / h2 0.4.20 / tokio 1.53–1.54 (versions resolved in `~/.cargo/registry`, the workspace's lockfile set).
- Question: why does the first Discord request after a quiet period sometimes take ~7–10 s (interaction callbacks then miss the 3 s deadline), why does `GET /users/@me` every 5 s "fix" it, and what should Noctis do instead?
- Registry root below: `R = ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.
- Probe code (scratch, not in the repo): `/tmp/claude-0/-home-user/dd8af539-19a8-5b5d-b835-27c3a80d0c45/scratchpad/http-probe` (`src/main.rs`, `src/bin/serial.rs`, `src/bin/blackhole.rs`), all built with `--release`, User-Agent `noctis-research`.

## 1. TL;DR

1. **The 10 s number is twilight's own request timeout.** `ClientBuilder::default()` sets `timeout: Duration::from_secs(10)` (`R/twilight-http-0.17.1/src/client/builder.rs:161`). It wraps only the hyper response future (`src/response/future.rs:137-141`), so a request stuck on a dead socket is reported as `RequestTimedOut` after ~10 s. A "7–10 s" first request is most consistent with a request that **hung on a dead pooled connection until the timeout**, or with a 5 s glibc DNS retry plus a fresh TCP+TLS setup (section 3.4).
2. **Silent connection death poisons twilight's HTTP/2 connection.** twilight negotiates h2 via ALPN (`src/client/connector.rs:83-88`, `.enable_http1().enable_http2()`), and discord.com accepts h2 (measured). With h2, hyper-util shares one connection and refreshes its idle stamp at every checkout (`R/hyper-util-0.1.21/src/client/legacy/pool.rs:318-325`), and `is_open()` only checks local state (`client.rs:854-855`). twilight enables **no HTTP/2 PING, no TCP keepalive, no hyper timer** (`builder.rs:34-37`; hyper default `keep_alive_interval: None`, `R/hyper-1.12.0/src/proto/h2/client.rs:92`). When a NAT/conntrack/firewall drops the idle flow without FIN/RST, **every following request on that connection times out** until the kernel gives up retransmitting (Linux `tcp_retries2 = 15`, ≈ 15 min). Reproduced in `blackhole.rs` (section 4.4): 7 of 7 requests timed out on the twilight-like h2 client; an h2 client with PING recovered on the next request.
3. **Why the `/users/@me` every 5 s workaround works:** constant traffic keeps the NAT/conntrack entry alive, so the flow is never silently dropped, and keeps the pooled connection inside hyper's 90 s idle window. It is a keep-alive done with API calls (17 280 requests/day, each counted against the 50 req/s global limit and the `users/@me` bucket).
4. **twilight's default RateLimiter serializes every interaction callback bot-wide** (one in flight at a time), so one stuck callback delays all others. Root cause: callback paths (`interactions/{id}/{token}/callback`) have no "top-level resource", so they all hash into one queue (`R/twilight-http-ratelimiting-0.17.1/src/lib.rs:118-143`, `src/actor.rs:38-42, 300-335`), and a queue allows one in-flight request (`actor.rs:320-334`). Measured: four concurrent callbacks against a 1 s mock finished at 1, 2, 3, 4 s; with `ratelimiter(None)` all four finished at 1 s (section 4.3). This turns one 10 s hang into a burst of failed interactions.
5. **twilight-http 0.17.1 exposes no transport knobs** (no custom connector, hyper client, pool, PING, DNS or keepalive settings). Its one escape hatch is `ClientBuilder::proxy(host, use_http = true)` (`builder.rs:86-91`, `client/mod.rs:2915-2918`), which sends every request as plain HTTP/1.1 to a host we choose. **We can point it at an in-process loopback forwarder that owns a tuned hyper client** (verified: twilight sent `POST /api/v10/interactions/1/tok1/callback` to our loopback server, section 4.3). That gives us full transport control without forking twilight, and it uses the same override point AD-24 already uses for proxies and `FakeDiscordHttp`.

**Recommended:** an internal "Discord edge" transport in `noctis-core` (section 7): h2 with PING keep-alive (zero API calls), connection warm-up at startup, TCP keepalive/nodelay/connect timeout, async cached DNS, interaction callbacks sent directly on that transport outside twilight's RateLimiter with one hedged retry, plus per-phase metrics. Polling `/users/@me` is not needed.

## 2. What twilight-http 0.17.1 actually builds

| Aspect | twilight-http 0.17.1 behaviour | Evidence |
|---|---|---|
| hyper client | `hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector)`; no other builder call | `src/client/builder.rs:34-37` |
| Protocols | rustls connector with `.https_or_http().enable_http1().enable_http2()` → ALPN offers `h2, http/1.1` | `src/client/connector.rs:83-88` |
| TLS roots | `rustls-platform-verifier` (our features) → `try_with_platform_verifier()`; default rustls `ClientConfig` (session resumption on: `Resumption::default()`, no 0-RTT) | `connector.rs:70-74`; `R/rustls-0.23.46/src/client/builder.rs:172,180` |
| TCP connector | `HttpConnector::new()` + `enforce_http(false)`; nothing else | `connector.rs:46-52` |
| Connector defaults | `connect_timeout: None`, `happy_eyeballs_timeout: 300 ms`, TCP keepalive off, `nodelay: false` | `R/hyper-util-0.1.21/src/client/legacy/connect/http.rs:214-232` |
| DNS | `GaiResolver`: `getaddrinfo` on `spawn_blocking` for **every new connection**, no cache | `connect/dns.rs:108-120` |
| DNS alt. | feature `hickory` swaps in `hyper_hickory::TokioHickoryResolver::default()` (async, cached by hickory) | `connector.rs:21-26, 49-50`; `Cargo.toml` feature `hickory` |
| Pool | `idle_timeout: 90 s`, `max_idle_per_host: usize::MAX`, **no `pool_timer`** (expiry only checked at checkout, which is fine) | `client.rs:1043-1047`; `pool.rs:299-316, 424-440` |
| hyper timer | none set → HTTP/2 PING keep-alive impossible even if configured | `builder.rs:36-37`; `R/hyper-1.12.0/src/proto/h2/client.rs:71-92` |
| TCP keepalive | not set (hyper-util only sets it in `build_http()`, which twilight doesn't call) | `client.rs:1612-1624` |
| Stale reuse | `retry_canceled_requests: true`: retried only if the request was **not** written and the connection was reused | `client.rs:248-271, 368-389, 1033-1036` |
| Timeout | default 10 s, covers only "response headers", starts **after** the rate-limit permit | `builder.rs:107-114, 161`; `response/future.rs:125-143, 335-362` |
| 429 handling | re-acquires a permit and resends; without a RateLimiter it resends **immediately in a loop** | `response/future.rs:389-398` |
| Customisation | `proxy(String, use_http)`, `ratelimiter(Option<RateLimiter>)`, `timeout`, `default_headers`, `token`, `remember_invalid_token`, `default_allowed_mentions`. No connector/client injection | `builder.rs:15-24, 58-150` |
| `proxy` semantics | URL becomes `{http|https}://{proxy}/api/v10/{path}`; plain HTTP goes through `HttpConnector` (no ALPN → HTTP/1.1) | `client/mod.rs:2915-2918` |
| User-Agent | always `DiscordBot (<homepage>, <version>) Twilight-rs`, overridable through `default_headers` | `client/mod.rs:135-140, 2956, 2966-2970` |

### 2.1 RateLimiter details that matter for interactions

- Interaction endpoints skip the global limit: `Endpoint::is_interaction()` is true for paths starting with `interactions` or `webhooks` (`R/twilight-http-ratelimiting-0.17.1/src/lib.rs:103-107`); they never decrement the global counter (`actor.rs:142-151, 323-331`).
- Queues are keyed by the hash of an endpoint's **top-level resources** (`channels/<id>`, `guilds/<id>`, `webhooks/<id>[/<token>]`) plus the learned bucket (`lib.rs:109-144`, `actor.rs:26-42`). `interactions/...` has no top-level resource, so before a bucket is known **all** such endpoints share the queue with the hash of nothing. The key in `buckets` is the full `Endpoint` (method + path with the unique token), so a callback never finds a learned bucket; and even if Discord returns a bucket header, `hasher.bucket(bucket, endpoint)` hashes only the bucket for these paths, so they still share one queue.
- A queue lets one request through and keeps the rest pending until that request's response **headers** arrive (`queue.in_flight`, `actor.rs:320-334`, released at `actor.rs:207-290`; the permit is completed in `response/future.rs:370-379`). If the in-flight request times out, the dropped permit releases the queue only after the 10 s timeout (`actor.rs:276-290`).
- Follow-ups (`webhooks/{app_id}/{token}`) get one queue per interaction token, so they don't serialize across interactions.

## 3. Root-cause candidates, ranked by evidence

### 3.1 Silent drop of an idle pooled connection (most likely; explains 7–10 s and the 5 s-poll fix)

- Mechanism: NATs, cloud SNAT/LBs and stateful firewalls evict idle TCP flows without telling either side. Then the client writes the request into a dead socket; Linux retransmits (RTO back-off, `tcp_retries2 = 15` ≈ 924 s) and nothing comes back; twilight reports `RequestTimedOut` at 10 s.
- Cloudflare itself closes idle client connections only after **400 s** for both HTTP/1.1 keep-alive and HTTP/2 ("Connection Keep-Alive HTTP/1.1: 400 s, TCP connection closed"; "Connection Idle HTTP/2: 400 s") — [Cloudflare connection limits](https://developers.cloudflare.com/fundamentals/reference/connection-limits/). hyper's 90 s checkout expiry is below that, and a clean FIN is noticed by hyper's connection task. So a **clean** server close is not the problem; a **silent** middlebox drop below 90 s, or below the time since the last request in the h2 case, is.
- HTTP/2 makes it worse: the shared h2 connection's `idle_at` is refreshed on each checkout (`pool.rs:318-325`), a timed-out stream doesn't close the connection, and `is_open()` sees nothing wrong (`client.rs:854-855`). Every request keeps failing until the kernel kills the socket. With HTTP/1.1 only the first request is lost: the timed-out connection is dropped and the next request dials fresh (section 4.4).
- Why the poll fixes it: traffic every 5 s is below every common NAT idle timer, so the flow stays mapped.

### 3.2 twilight RateLimiter head-of-line blocking of interaction callbacks (certain; amplifier)

Section 2.1 and measurement 4.3. Effect: one slow or hung callback (3.1) delays every concurrent callback by up to 10 s, so a single stale connection fails a burst of interactions, and even healthy bursts pay N × RTT.

### 3.3 Cold connection after > 90 s idle (certain; cost ≈ 1 RTT DNS + 1 RTT TCP + 1 RTT TLS 1.3)

hyper drops pooled connections idle for more than 90 s at checkout (`pool.rs:313-316`), so a bot quiet for 90 s pays a full connection setup on the next request (measured: requests after 100 s and 200 s idle opened a new connection, section 4.2). From this container that costs ~50–80 ms extra; from a far region (Istanbul ↔ nearest Cloudflare PoP ≈ 30–60 ms RTT) still well under 1 s. **Alone, this does not explain 7–10 s**, but it multiplies with 3.4.

### 3.4 Blocking, uncached DNS with glibc 5 s retries (likely contributor on some hosts)

Every new connection calls `getaddrinfo` (`dns.rs:108-120`). glibc's defaults are `timeout:5 attempts:2`; one lost UDP datagram (typical: A and AAAA sent in parallel from one socket and conntrack races on Docker/Kubernetes, or an overloaded local stub) costs exactly 5 s. 5 s DNS + new TLS + request ≈ 5.3–7 s, the bottom of the reported range. This container overrides it with `options timeout:2 attempts:3` and 8.8.8.8/8.8.4.4 (`/etc/resolv.conf`); first `getaddrinfo` took 17 ms, cached ones 0.3–0.8 ms. twilight's `hickory` feature replaces this with an async, caching resolver.

### 3.5 Lesser candidates

- **IPv6 black hole:** Happy Eyeballs falls back after 300 ms (`http.rs:228, 732-753`), so it adds at most 0.3 s. discord.com returned no AAAA here (`curl -6`: could not resolve).
- **No connect timeout:** `connect_timeout: None` means one unreachable address waits for the OS SYN timeout (~127 s); with 5 IPv4 addresses this is capped only by twilight's 10 s timeout.
- **Nagle:** `nodelay: false`; hyper writes headers and body in one buffer, so it rarely matters for small JSON, but costs nothing to fix.
- **Rate limiter/global limit:** interaction endpoints bypass the global limit (2.1), so the global limiter is not the cause.
- **Not a cause:** tokio's blocking pool spin-up (µs), rustls handshake CPU (sub-ms with `ring`).

## 4. Measurements (this container, 2026-10-20)

**Caveat:** all outbound traffic from this container, including `curl --noproxy`, terminates at a transparent egress gateway (TLS issuer `O=Anthropic; CN=Egress Gateway SDS Issuing CA`; TCP connect 0.1–0.3 ms). TCP/TLS times and idle-close behaviour therefore describe **the gateway**, not Cloudflare. HTTP response headers (`server: cloudflare`, `cf-ray: …-ATL`, `alt-svc: h3=":443"; ma=86400`) do come from Discord's Cloudflare edge. Treat absolute numbers as a lower bound; the mechanisms (4.3, 4.4) were tested against local servers and don't depend on the network.

### 4.1 curl, `GET https://discord.com/api/v10/gateway` (no token)

| Run | DNS | TCP | TLS done | TTFB | Proto |
|---|---|---|---|---|---|
| h2 cold ×5 | 0.8–1.1 ms | 1.1–1.4 ms | 21–78 ms | 67–158 ms | HTTP/2 |
| h1.1 cold ×3 | 0.9–1.0 ms | 1.1–1.3 ms | 21–24 ms | 68–77 ms | HTTP/1.1 |
| IPv4 (`-4`) | 1.0 ms | 1.3 ms | 21 ms | 73 ms | HTTP/2 |
| IPv6 (`-6`) | — | — | — | — | no AAAA (curl exit 6) |
| h2 warm (same connection, 2nd/3rd) | — | — | — | **19–20 ms** | HTTP/2 |
| h1.1 warm (same connection) | — | — | — | **23–24 ms** | HTTP/1.1 |

ALPN: `curl offers h2,http/1.1` → `server accepted h2`; TLS 1.3, `TLS_AES_256_GCM_SHA384`, X25519. discord.com resolves to 5 IPv4 addresses (162.159.128.233, 162.159.135–138.232). A cold request costs roughly 3–5× a warm one.

### 4.2 hyper probe built like twilight (`http-probe pool-reuse <idle> default|tuned`)

`default` = twilight's construction; `tuned` = `pool_idle_timeout(50 s)`, h2 PING 15 s/5 s while idle, nodelay, TCP keepalive 30 s, connect timeout 2 s.

| Idle gap | default: connects / latency after gap | tuned: connects / latency after gap |
|---|---|---|
| 2 s | 1 / 21–22 ms | — |
| 30 s | 1 / 47–53 ms (reused) | 1 / 30–72 ms (reused) |
| 65 s | 1 / 52–56 ms (reused) | 2–3 / 57–77 ms (50 s pool timeout → redial, as configured) |
| 100 s | **2** / 102 ms (90 s expiry → new connection) | 2 / 51 ms |
| 200 s | **2** / 113 ms | 2 / 90 ms |

hyper's 90 s checkout expiry works as read in the source: after > 90 s every quiet bot pays a new handshake. Note: `pool_idle_timeout` closes idle connections even while PING keeps them healthy, so with PING the idle timeout should be long or off (section 7).

### 4.3 twilight RateLimiter vs concurrent interaction callbacks (`serial.rs`)

twilight `Client` with `proxy("127.0.0.1:<port>", true)` pointed at a local HTTP/1.1 mock that answers each callback after 1 s; four `create_response` calls (different interaction ids/tokens) spawned at once:

| Configuration | Completion times |
|---|---|
| default RateLimiter, no rate-limit headers | 1.0 s, 2.0 s, 3.0 s, 4.0 s |
| default RateLimiter, mock returns `x-ratelimit-bucket` headers | 1.0 s, 2.0 s, 3.0 s, 4.0 s |
| `ratelimiter(None)` | 1.0 s, 1.0 s, 1.0 s, 1.0 s |

The mock received `POST /api/v10/interactions/<id>/<token>/callback` with `Host: 127.0.0.1:<port>`: the proxy hook routes every request to an address we choose, and a forwarder must rewrite `Host`/`:authority` to `discord.com`.

### 4.4 Silent connection drop (`blackhole.rs`)

A local origin behind a TCP relay that, at a chosen moment, keeps existing connections open but discards their bytes (what an evicting NAT looks like to the endpoints). Requests every ~1 s with a 3 s timeout (standing in for twilight's 10 s):

| Client | After the drop |
|---|---|
| twilight-like, h2 (no PING) | **7/7 requests TIMEOUT**, still 1 connection: the dead h2 connection is never evicted |
| twilight-like, HTTP/1.1 | 1st request TIMEOUT, then a new connection and all OK |
| h2 + PING (1 s interval, 1 s timeout, while idle) | the in-flight request failed after 1 s (`SendRequest`), the next one redialed; OK from then on. With an idle gap longer than interval + timeout, the dead connection is gone before the next request. |

### 4.5 Idle close by the server (`http-probe idle-close`)

One request on a raw TLS connection, then waiting for the peer to close it (cap 1 200 s):

| Connection | Closed by peer? |
|---|---|
| HTTP/1.1 keep-alive | not within 1 200 s |
| HTTP/2, no PING | not within 1 200 s |
| HTTP/2, PING every 20 s | not within 1 200 s |

The peer here is the egress gateway, which holds client connections longer than Cloudflare's documented 400 s, so this run **cannot** measure Discord's idle close; the documented 400 s ([Cloudflare connection limits](https://developers.cloudflare.com/fundamentals/reference/connection-limits/)) is the design input. Whether Cloudflare counts h2 PING as activity is not documented; the design doesn't depend on it, because the keeper redials on close (section 7). Re-run `http-probe idle-close h1|h2 [ping]` from a production-like host without an egress proxy to get the real number.

### 4.6 Resolver

`/etc/resolv.conf`: `nameserver 8.8.8.8`, `nameserver 8.8.4.4`, `options timeout:2 attempts:3`. `tokio::net::lookup_host("discord.com:443")`: 17.2 ms first, then 0.31–0.78 ms. Kernel: `tcp_retries2 = 15`, `tcp_keepalive_time = 7200` (so OS keepalive is useless unless the socket overrides it).

## 5. Prior reports

- twilight issue tracker: no issue on idle slowness, h2 keep-alive, connector customisation or callback serialization was found (searched `twilight-rs/twilight` for timeout/idle/pool/hyper/hickory/http2/ratelimiter interaction). The changelog shows the relevant history: interaction endpoints were removed from the global limit in 0.12.2/0.13.0; the RateLimiter was rewritten and `ResponseFuture` retries 429s since 0.17.0; the `trust-dns`/`hickory` feature exists since 0.8.0; nothing on transport tuning ([CHANGELOG](https://github.com/twilight-rs/twilight/blob/main/twilight-http/CHANGELOG.md)).
- hyper ecosystem: the reuse-while-server-closes race (`IncompleteMessage` / "connection closed before message completed") is a well-known hyper pool issue; the usual mitigation is a client idle timeout below the server's, plus a retry where safe ([juspay/hyperswitch#3426](https://github.com/juspay/hyperswitch/pull/3426)). Cloudflare's 400 s idle close is far above hyper's 90 s, so the race is rare for Discord; silent drops (3.1) are the dangerous case.

## 6. Strategy options, ranked

| # | Strategy | Fixes | API calls | Cost / risk | Possible without forking twilight? |
|---|---|---|---|---|---|
| 1 | **Send interaction callbacks outside twilight's RateLimiter** | 3.2 (HoL blocking) | 0 | Small: build `POST /interactions/{id}/{token}/callback` with the already-serialized `InteractionResponse`; no auth header; handle 429 ourselves (rare: callbacks are one-shot per token) | Yes: our own hyper request, or a second twilight `Client` with `ratelimiter(None)` (but its 429 path loops without delay, `future.rs:389-398`, so we'd still need our own 429 guard) |
| 2 | **HTTP/2 PING keep-alive** (interval 20 s, timeout 5 s, while idle) on a pinned connection | 3.1 (detects dead flows in ≤ 25 s, and the PING traffic itself keeps NAT entries alive), 3.3 (connection stays warm) | 0 (PING frames are not API requests) | 3 tiny frames/min per connection. Cloudflare may still close after its idle limit (4.5); the client then redials proactively | **No** via twilight's client (no timer, no knob). Yes through the edge transport (#4) |
| 3 | **TCP tuning**: `TCP_KEEPIDLE` 30 s / interval 10 s / 3 probes, `TCP_NODELAY`, `TCP_USER_TIMEOUT` ≈ 10 s, connect timeout 1.5 s per address | 3.1 for HTTP/1.1 and as a backstop, 3.5 | 0 | none | Only through #4 |
| 4 | **In-process "Discord edge" forwarder** behind twilight's `proxy("127.0.0.1:<port>", true)` | everything above, for **all** twilight traffic | 0 | One loopback HTTP/1.1 hop (~50–150 µs), one more component; must bind loopback only and forward only to the configured upstream; must rewrite `Host`; must preserve headers and bodies exactly (multipart, `X-Audit-Log-Reason`) | Yes, documented twilight API; same hook AD-24 already uses for proxies and fakes |
| 5 | **Warm-up**: open and TLS-handshake the h2 connection at startup and after a reconnect, then once more after any connection error | 3.3 | 0 (handshake only; an optional `HEAD /api/v10/gateway`, unauthenticated and outside the bot's rate limits, verifies end-to-end) | negligible | Only through #4 (twilight's pool can't be pre-filled without a real request) |
| 6 | **Async cached DNS** with TTL-respecting background refresh (hickory resolver, or a small cache over `getaddrinfo` that refreshes before TTL) | 3.4 | 0 | hickory adds a dependency (twilight already offers it as a feature); a cache can serve stale addresses for one refresh interval | Partly: twilight feature `hickory` (async + cached, but no prefetch and no metrics). Full control only via #4 |
| 7 | **Hedged callback**: if the callback hasn't answered within a budget (e.g. 1 000 ms, or `deadline − 1 200 ms`), send the same callback on a **fresh** connection; first success wins; Discord error 40060 "Interaction has already been acknowledged" on either attempt counts as success | residual tail from 3.1/3.3 | at most 1 extra callback per stalled interaction | Duplicate-safe because Discord accepts only one callback per interaction | Only through our own transport (#1/#4) |
| 8 | **Safe retry rule**: retry automatically only when the request provably didn't leave (connect/DNS/TLS error, h2 `REFUSED_STREAM`, GOAWAY above our stream id, hyper "canceled before sent"); never blindly retry non-idempotent requests after bytes were written, except callbacks (#7) | race in 3.1 | 0 | none | hyper-util already does the "not sent + reused" case (`client.rs:248-271`); the rest only via #4 |
| 9 | **Per-phase metrics/tracing** (dns, connect, tls, ttfb, total, reused vs new, h2 PING RTT, hedges fired, 40060 counted as success) | diagnosis | 0 | none | Only via #4 (twilight exposes one `req` span and nothing else) |
| 10 | Deadline-aware auto-defer (Story 6.6) | user-visible failures | 0 | already planned | n/a: it depends on the transport being able to send the type-5/6 ack in time; with #1 + #2 the ack costs ~1 warm RTT |
| ✗ | Poll `GET /users/@me` every 5 s | 3.1, 3.3 by brute force | 17 280/day | spends global and bucket budget, shows up in Discord's request logs, still doesn't fix 3.2 | — |

Forking twilight-http is not needed: #4 reaches every knob through a public API.

## 7. Recommended design for Noctis

### 7.1 Shape

A crate-internal **`DiscordEdge`** in `noctis-core` (no new public types except config and metrics), owned by the App alongside its `Arc<twilight_http::Client>` (AD-24):

1. **Edge listener.** On `App::run`, core binds `127.0.0.1:0` (loopback only), starts a hyper HTTP/1.1 server and builds the twilight client with `proxy("127.0.0.1:<port>", true)`. When the user configures an external twilight http-proxy, the edge forwards to it instead of `discord.com` (the external proxy's behaviour, including its rate limiting, is preserved). The forwarder:
   - accepts only paths starting with `/api/v10/`, rewrites the authority to `discord.com` (or the proxy), and forwards method, headers and body byte-for-byte; it never adds or logs `Authorization`;
   - streams the response back unchanged (headers included, so twilight's RateLimiter keeps learning buckets).
2. **Upstream client** (one per App, shared by the edge and the direct callback path):
   - `hyper_util::client::legacy::Client` with `TokioTimer` for both `timer` and `pool_timer`; ALPN `h2, http/1.1` (h2 preferred);
   - h2: `http2_keep_alive_interval(20 s)`, `http2_keep_alive_timeout(5 s)`, `http2_keep_alive_while_idle(true)`; `pool_idle_timeout(None)` while PING is active (Cloudflare closes at its own idle limit; we redial on close, see warm-up);
   - connector: `set_nodelay(true)`, `set_connect_timeout(1.5 s)`, `set_happy_eyeballs_timeout(250 ms)`, TCP keepalive 30 s / 10 s / 3, `TCP_USER_TIMEOUT` 10 s on Linux (through a `socket2` hook in a small custom connector);
   - DNS: async resolver with an in-memory cache honouring TTL (floor 30 s, cap 5 min) and refresh at 75 % of TTL in the background; on resolver failure the last good answer is served (stale-if-error, max 1 h);
   - TLS: the process-default rustls `ring` provider (AD-21) and `rustls-platform-verifier`, session resumption on (default).
3. **Warm-up / keeper task.** At startup (before the gateway identifies) and whenever the h2 connection closes (GOAWAY, PING timeout, error), the keeper opens a connection and completes TLS + the h2 handshake. Connection warming needs no API call; if the transport can only verify a connection with a request, it uses `HEAD /api/v10/gateway` (unauthenticated, so it spends no bot or global rate-limit budget; only Cloudflare per-IP limits apply). Redial back-off: 0, 0.5, 1, 2, 5 s (capped), jittered.
4. **Direct interaction callback path.** The Responder sends `POST /api/v10/interactions/{id}/{token}/callback` (and `?with_response=true` when needed) **directly on the upstream client**, outside twilight's RateLimiter, with the JSON/multipart body built from the same twilight-model types (AD-11's single Responder path is unchanged). Follow-ups and edits keep going through twilight (per-token queues are fine).
   - Hedging: if no response headers within `hedge_after` (default `min(1 000 ms, deadline − now − 1 200 ms)`, never below 300 ms), send an identical second attempt on a **new** connection (bypassing the pool); first 2xx wins; the loser is cancelled. A 400 with Discord code **40060** on either attempt counts as acknowledged. Max 2 attempts per callback.
   - Deadline: callback total budget = `received_at + 3 s − 100 ms` safety margin; past it the call returns a typed `AckDeadlineMissed` error (no further attempts; the interaction token is dead for callbacks).
   - 429 on a callback: honour `retry_after` if it fits the deadline, otherwise `AckDeadlineMissed`.
5. **Story 6.6 interplay.** Auto-defer fires at 2 500 ms from `received_at` by default; with a warm h2 connection the ack costs one RTT (~20–60 ms) and a hedge still fits inside the remaining ~500 ms. Recommend lowering the auto-defer default to **2 000 ms** so a hedged ack keeps ≥ 1 s of margin even from distant regions.
6. **twilight timeout.** Keep the configured twilight `timeout` (default 10 s) for normal requests; the edge enforces its own per-phase timeouts (connect 1.5 s per address, TLS 3 s), so a dead upstream turns into a fast 502 from the edge instead of a 10 s hang.
7. **Observability** (Prometheus names follow the existing `noctis_` prefix): `noctis_http_phase_seconds{phase="dns|connect|tls|ttfb|total"}`, `noctis_http_connections_total{reason="warmup|redial|hedge"}`, `noctis_http_conn_reused_total{reused="true|false"}`, `noctis_http_h2_ping_rtt_seconds`, `noctis_http_ping_timeouts_total`, `noctis_interaction_ack_hedges_total{outcome="won|lost|40060"}`; tracing spans `discord.http` with the same fields. Tokens and personal data never appear in spans, labels or headers we add; the edge sends only the User-Agent twilight sets (or the App's configured one).

### 7.2 Architecture impact

- AD-24 says "no other crate constructs a Discord HTTP client" and core builds the twilight client with `proxy` when configured. The edge lives **in core** and is internal, so the rule stands; AD-24 needs one amendment: "Core always routes twilight-http through its internal Discord edge (`proxy(loopback, true)`); the edge forwards to discord.com or to the configured proxy. Interaction callbacks use the edge's upstream client directly."
- `FakeDiscordHttp` keeps working: in tests the edge's upstream is the fake's address (plain HTTP), so the same code path runs in tests and in production.
- A new rule id for `docs/discord-rules.md` / `noctis-rules` is not needed (no new Discord limit), but INT-01 (3 s ack) gains enforcement in the transport (hedge + deadline).

### 7.3 Acceptance criteria (story-ready)

1. **Given** a running App **When** it starts **Then** the twilight client is built with `proxy("127.0.0.1:<port>", true)`, the edge listens on loopback only, and one h2 connection to the upstream is established and TLS-complete before the first shard identifies (asserted with `FakeDiscordHttp` connection logs; no API request is made for warm-up).
2. **Given** an idle App with no requests for 10 minutes against a fake upstream **When** the clock advances (paused tokio clock) **Then** the fake observes h2 PING frames every 20 s and zero HTTP requests, and the next callback reuses the warm connection (`noctis_http_conn_reused_total{reused="true"}` increments).
3. **Given** the upstream connection is silently blackholed (testkit relay that discards bytes without closing) **When** 25 s pass **Then** the keeper detects the PING timeout, redials, and a callback sent afterwards succeeds on the new connection within one RTT; no request waits for the 10 s twilight timeout.
4. **Given** four concurrent interactions whose fake callback endpoint answers after 1 s **When** each handler acks **Then** all four acks complete within 1.2 s (no serialization), unlike the 1/2/3/4 s twilight baseline.
5. **Given** a fake upstream that stalls the first callback attempt indefinitely **When** `hedge_after` elapses **Then** a second attempt goes out on a new connection, the ack succeeds before the 3 s deadline, `noctis_interaction_ack_hedges_total{outcome="won"}` increments, and exactly one success reaches the handler.
6. **Given** both hedged attempts reach Discord **When** the second returns HTTP 400 with code 40060 **Then** the ack is reported as successful and nothing is logged as an error.
7. **Given** DNS answers with TTL 60 s **When** 50 s pass **Then** the resolver refreshes in the background, and no request in the window waits on DNS (`noctis_http_phase_seconds{phase="dns"}` stays at cache-hit latency); **Given** the resolver then fails **Then** the cached address is still used.
8. **Given** a request that failed before any byte was written (connect refused, `REFUSED_STREAM`, GOAWAY with `last_stream_id` below ours) **When** it is retried **Then** exactly one retry goes out; **Given** a non-callback request that failed after bytes were written **Then** it is not retried and the error surfaces.
9. **Given** an App configured with an external twilight http-proxy **When** requests are made **Then** the edge forwards to that proxy, and requests carry the same method, path, headers (except `Host`) and body as twilight produced (byte-equality test, including multipart uploads and `X-Audit-Log-Reason`).
10. **Given** any request **When** it completes **Then** `noctis_http_phase_seconds` has dns/connect/tls (new connections only), ttfb and total samples, and no metric label, span field or header we add contains a token or personal data.
11. **Given** the edge listener **When** a request arrives for a path outside `/api/v10/` **Then** it is rejected with 403 and never forwarded.
12. **Given** the change to AD-24 and the new config (`HttpConfig { ping_interval, ping_timeout, hedge_after, connect_timeout, dns_ttl_floor/cap }` with defaults) **When** the story is done **Then** the architecture spine, `examples/project-template/` (code, `AGENTS.md`, Usage Skill), `xtask/public-api.txt` and the Usage Skill's `API hash` line are updated in the same commit, and every local gate in AGENTS.md passes.

### 7.4 What not to do

- Don't poll API endpoints for liveness; HTTP/2 PING is the protocol's own mechanism and is free.
- Don't disable twilight's RateLimiter globally; only interaction callbacks leave it.
- Don't retry non-idempotent requests after bytes were written (double posts), except the callback hedge, which Discord deduplicates (40060).
- Don't fork twilight-http; `proxy(.., true)` gives the same control and keeps upgrades cheap. If twilight later exposes a connector/client hook, the edge can shrink to the direct callback path plus a twilight client built on our upstream client.
