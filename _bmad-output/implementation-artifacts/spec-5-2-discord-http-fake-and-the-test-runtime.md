---
title: 'Story 5.2: Discord HTTP fake and the test Runtime'
type: 'feature'
created: '2026-10-20'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** No later story can test REST behavior end to end: there is no local Discord HTTP fake, and the App's one twilight client cannot be pointed anywhere but `discord.com` (FR-51, AD-24, AD-14; Conventions Tests). Story 5.1 also deferred "configured HTTP timeout never observed on the client" for lack of a stalled local server.

**Approach:** Add `AppBuilder::http_proxy(..)` to `noctis-core` (validated at `build()` into twilight's `proxy(authority, true)` + `ratelimiter(None)`, also read from `NOCTIS_HTTP_PROXY`), and `noctis_testkit::fake::FakeDiscordHttp`, a local hyper HTTP/1.1 server with per-test routes, a canned `GET /gateway/bot`, per-route delays and a request log. A smoke test in `crates/noctis-testkit/tests/` drives a real App built on `Runtime::for_test()` against it.

## Boundaries & Constraints

**Always:**
- **http_proxy (core):** `AppBuilder::http_proxy(&mut self, proxy: impl Into<String>) -> &mut Self` stores the raw string (last call wins). `build()` normalizes it with `pub(crate) fn normalize_http_proxy(&str) -> Result<String, HttpProxyError>` (new `crates/noctis-core/src/proxy.rs`) as a check placed after the timeout check and before the crypto/runtime checks; failure records `BuildViolation::InvalidHttpProxy { reason: HttpProxyError }`. On success the client is `twilight_http::Client::builder().timeout(t).proxy(authority, true).ratelimiter(None)` (+ token); without a proxy the builder is unchanged from 5.1 (local rate limiter kept). New getter `App::http_proxy(&self) -> Option<&str>` returns the normalized authority. No test-only branch anywhere in core.
- **Normalization, in this order:** (1) empty input ⇒ `Empty`; (2) if it contains `"://"`: the part before must equal `http` ASCII-case-insensitively, else `UnsupportedScheme`; drop it; (3) drop one trailing `/`; (4) empty ⇒ `Empty`; (5) any char outside `0x21..=0x7E` ⇒ `InvalidChar { index }` (char index in the step-4 string); (6) any of `/ ? # @ \` ⇒ `NotAnAuthority`; (7) byte length > 255 ⇒ `TooLong { len }`; (8) host/port: a leading `[` needs a `]`, a non-empty bracket body of only hex digits, `:` and `.`, and after `]` either nothing or `:port`; otherwise at most one `:`, a non-empty host with no `[`/`]`; any violation ⇒ `InvalidHost`; (9) a port must be 1-5 ASCII digits with value 1..=65535, else `InvalidPort`. Returns the step-4 string unchanged.
- **HttpProxyError:** `thiserror`, `Debug, Clone, Copy, PartialEq, Eq`, `#[non_exhaustive]`: `Empty`, `UnsupportedScheme`, `InvalidChar { index: usize }`, `NotAnAuthority`, `TooLong { len: usize }`, `InvalidHost`, `InvalidPort`. Every message names the fix (e.g. UnsupportedScheme: "use `host:port` or `http://host:port`; HTTPS proxies are not supported") and never contains the value. `BuildViolation::InvalidHttpProxy` message: "invalid HTTP proxy: {reason}". Re-exported from `noctis_core` and the `noctis` facade root.
- **from_env:** also reads `NOCTIS_HTTP_PROXY` with the token's rules (trim ASCII whitespace; empty ⇒ unset; non-UTF-8 ⇒ `EnvError::NotUnicode { var: "NOCTIS_HTTP_PROXY" }`), then calls `http_proxy(value)`; content is validated only at `build()`. Token is read first.
- **Debug:** `AppBuilder` prints `http_proxy` as a bool (`is_some`) because the raw value is unvalidated; `App` prints the normalized authority.
- **FakeDiscordHttp (`crates/noctis-testkit/src/fake.rs`, `pub mod fake`; `fake/http.rs` allowed):** `start() -> io::Result<Self>` and `start_with(GatewayBot) -> io::Result<Self>` (async, need a current tokio runtime, bind `127.0.0.1:0`, spawn one accept task owning a `JoinSet` of `hyper::server::conn::http1` connections via `hyper_util::rt::TokioIo` + `hyper::service::service_fn`); `url() -> String` = `"http://127.0.0.1:{port}"`; `addr() -> SocketAddr`; `set_gateway_bot(&self, GatewayBot)`; `route(&self, Method, path: impl Into<String>, FakeResponse) -> &Self`; `route_with(&self, Method, path, F) -> &Self` where `F: Fn(&RecordedRequest) -> FakeResponse + Send + Sync + 'static`; `requests(&self) -> Vec<RecordedRequest>` (arrival order); `clear_requests(&self)`. `Drop` aborts the accept task (dropping the `JoinSet` aborts connections). State in `Arc<Mutex<..>>` (std), poison recovered with `PoisonError::into_inner`, lock never held across `.await`.
- **Routing:** every request is logged once its body is read (or rejected), before any route lookup or delay. Paths are compared raw (no percent-decoding), query ignored. Request path must start with `/api/v10`; the remainder is the route path. Route paths given to `route*` get a leading `/` when missing. Order: per-test route for exact (method, route path) — last registration wins — then the canned `GET /gateway/bot`, else 404 with body `{"message":"404: Not Found","code":0}`. `pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024`: a `content-length` header above it ⇒ 413 `{"message":"Request entity too large","code":40005}` without polling the body (so a client sending `expect: 100-continue` never uploads it); otherwise the body is read through `http_body_util::Limited` and exceeding it (chunked bodies) gives the same 413. Rejected requests are logged with an empty body, no route consulted. A route's `delay` sleeps (`tokio::time::sleep`) after logging, before responding.
- **Types:** re-export `pub use hyper::{HeaderMap, Method, StatusCode}` and `pub use hyper::header::{HeaderName, HeaderValue}` in `fake`. `GatewayBot { pub url: String, pub shards: u32, pub session_start_limit: SessionStartLimit }`, `SessionStartLimit { pub total: u32, pub remaining: u32, pub reset_after_ms: u64, pub max_concurrency: u32 }` (u32 on purpose: twilight's model is `u16`, so tests can script values twilight rejects), both `Debug, Clone, PartialEq, Eq` + `Default` (`"wss://gateway.discord.gg"`, 1 / 1000, 1000, 14_400_000, 1) + `to_json(&self) -> serde_json::Value` in Discord's shape (`reset_after` key). No value validation (tests may script 0 or `u32::MAX`). `FakeResponse`: `new(StatusCode, impl Into<Bytes>)`, `json(StatusCode, &serde_json::Value)` (sets `content-type: application/json`), `empty(StatusCode)`, `header(self, HeaderName, HeaderValue) -> Self`, `delay(self, Duration) -> Self`; `Clone, Debug`. `RecordedRequest { pub method: Method, pub path: String /* full, e.g. /api/v10/gateway/bot */, pub query: Option<String>, pub headers: HeaderMap, pub body: Bytes }` + `route_path(&self) -> Option<&str>` + `json(&self) -> Option<serde_json::Value>`; `Clone, Debug`.
- **Dependencies:** `crates/noctis-testkit/Cargo.toml` `[dependencies]`: `bytes.workspace = true`, `http-body-util.workspace = true`, `hyper = { workspace = true, features = ["http1", "server"] }`, `hyper-util = { workspace = true, features = ["tokio"] }`, `serde_json.workspace = true`, `tokio = { workspace = true, features = ["net", "rt", "sync", "time"] }`; `[dev-dependencies]`: `noctis-core.workspace = true`, `tokio = { workspace = true, features = ["macros", "rt", "rt-multi-thread"] }`. Root pins already exist (hyper 1.12 → lock 1.12.0, hyper-util 0.1.21, http-body-util 0.1.5; all MSRV ≤ 1.85): leave them. testkit must NOT depend on `twilight-http` (deps-check `http-client`); tests reach the client only through `app.http()` and assert errors by `Display` (twilight 0.17.1: `"request timed out"`).
- Every new public item has rustdoc + a doctest (`no_run` when it needs a runtime or network). No `unwrap`/`expect` outside tests. Release-only cargo commands. Crate docs of `noctis-testkit` state that E2E tests for Epics 5-7 live in `crates/noctis-testkit/tests/`.

**Never:**
- No facade `testkit` wiring or `noctis::testkit` re-export (Story 8.6), no `TestApp`, no testkit App-builder helper, no gateway fake or `gateway_url` (5.3), no `GET /gateway/bot` call from core (5.4), no Cluster proxy warning (12.4), no HTTPS/HTTP2 in the fake, no path templates.
- No `cfg(test)`/test branch in core; no network access in tests; no `std::env::set_var`.
- Never echo the proxy value in an error message or `BuildError`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Proxy accepted | `127.0.0.1:3000`, `proxy`, `http://proxy:3000`, `HTTP://proxy:3000/`, `Http://proxy`, `[::1]`, `http://[::1]:8080`, `proxy:65535`, `proxy:1`, `proxy:08080`, 255×`a` | Ok: `127.0.0.1:3000`, `proxy`, `proxy:3000`, `proxy:3000`, `proxy`, `[::1]`, `[::1]:8080`, `proxy:65535`, `proxy:1`, `proxy:08080`, same 255 chars |
| 2 | Empty | `""`, `"http://"`, `"/"`, `"http:///"` | `Empty` ×4 |
| 3 | Scheme | `https://proxy`, `HTTPS://proxy`, `ws://x`, `://x`, `ftp://x:21` | `UnsupportedScheme` ×5 |
| 4 | Bad char | `" proxy"`, `"a b"`, `"proxy\n"`, `"a\tb"`, `"pröxy:3000"`, `"🙂"`, `"a\u{7f}"` | `InvalidChar{index}` = 0, 1, 5, 1, 2, 0, 1 |
| 5 | Not authority | `proxy:3000/api`, `proxy//`, `proxy?x`, `proxy#x`, `user:pw@proxy`, `a\b`, `http://http://x` | `NotAnAuthority` ×7 |
| 6 | Length | 256×`a`; `http://` + 255×`a` | `TooLong{len:256}`; Ok |
| 7 | Host | `:3000`, `::1`, `[::1`, `[]`, `[::1]x`, `[zz]`, `a]b`, `a[b` | `InvalidHost` ×8 |
| 8 | Port | `proxy:`, `proxy:0`, `proxy:65536`, `proxy:123456`, `proxy:+80`, `proxy:8a`, `[::1]:0` | `InvalidPort` ×7 |
| 9 | Build wiring | `#[tokio::test]`: `.http_proxy("http://p:1")`; twice (`a:1` then `b:2`); unset; invalid `"https://x"` + timeout 0 | `app.http_proxy()==Some("p:1")`; `Some("b:2")`; `None`; one `BuildError` [InvalidHttpTimeout, InvalidHttpProxy{UnsupportedScheme}] in that order |
| 10 | Redaction | `.http_proxy("user:SECRET@h")` build; builder `Debug` before build | `BuildError` `Display`/`Debug` lack `SECRET`; builder Debug shows `http_proxy: true`, lacks `SECRET` |
| 11 | from_env proxy | lookup per var: proxy absent / `""` / `"  \n"` / `" http://p:1 \n"` / non-UTF-8 / `"https://x"` | unset / unset / unset / builds with `Some("p:1")` / `EnvError::NotUnicode{var:"NOCTIS_HTTP_PROXY"}` / from_env Ok, build `InvalidHttpProxy{UnsupportedScheme}` |
| 12 | Fake start | `FakeDiscordHttp::start()` twice | two distinct non-zero ports; `url()` = `http://127.0.0.1:{port}`; separate logs |
| 13 | Canned gateway/bot | default; `start_with` shards 4, max_concurrency 16, remaining 0, reset 1000; `set_gateway_bot` shards 7 after start; shards 0 / `u32::MAX`; max_concurrency 0 / 65_535 / 65_536 | App `gateway().authed()` model reports defaults (1, 1000, 1000, 14400000, 1); 4/16/0/1000; 7; 0 / `u32::MAX` verbatim; 0 / 65_535 verbatim; 65_536 ⇒ twilight `model()` returns `Err` (u16 overflow), request still logged |
| 14 | Per-test route | `route(GET, "/channels/1/messages", json 200)`; also `"channels/1/messages"` (no slash); request with `?limit=5`; `POST` same path; trailing `/` | 200 body; same match; matches, `query==Some("limit=5")`; 404; 404 |
| 15 | Override + last wins | `route(GET,"/gateway/bot", 401)`; same route registered twice (500 then 204) | twilight error, request logged; 204 |
| 16 | Unknown / outside api | `/api/v10/nope`; `/gateway/bot`; `/api/v9/gateway/bot` | 404 Discord JSON; all logged with full `path`; `route_path()` = `Some("/nope")`, `None`, `None` |
| 17 | Raw path | route `"/guilds/1/emojis/%F0%9F%99%82"`; request same encoded path | matches (no decoding) |
| 18 | Body limit | raw TCP POST, `content-length: MAX_BODY_BYTES` + that many bytes; headers only with `content-length: MAX_BODY_BYTES + 1` + `expect: 100-continue`; `content-length: 0` | route reached, body logged in full; 413 code 40005, logged with empty body, route not consulted; route reached, empty body |
| 19 | route_with | closure echoing `req.body` | response body == request body |
| 20 | Delay / timeout | route delayed 2 s, App `http_timeout(100 ms)` | request error `Display` == `"request timed out"`; request logged; closes 5.1's deferred finding |
| 21 | Concurrency | 16 concurrent App requests | 16 answered, 16 logged |
| 22 | Log ops | `requests()` then `clear_requests()` | arrival order; then empty |
| 23 | Auth header | App with token `fake-token` / without token | `authorization: Bot fake-token` / header absent |
| 24 | Dropped fake | fake started (never used) and dropped, then App (`http_timeout` 2 s) request | `Err` (refused/reset/timeout), no panic |
| 25 | Smoke (AC) | App on `Runtime::for_test()` + token + `.http_proxy(fake.url())`, `app.http().gateway().authed()` | Ok with canned values; log holds exactly one `GET /api/v10/gateway/bot` |

</intent-contract>

## Code Map

- `crates/noctis-core/src/app.rs` -- `App` struct L51-60 (add `http_proxy: Option<String>`), `AppBuilder` L243-256 (add `http_proxy: Option<String>`), `new` L279-291, `from_lookup` L294-310 (add `NOCTIS_HTTP_PROXY` after the token, const beside `TOKEN_VAR` L26), `build` L527-560 (checks L529-540; client L545-548 — insert `.proxy(..).ratelimiter(None)`), Debug impls ~L205-215 and L579-595. Getters pattern L100-165.
- `crates/noctis-core/src/app/tests.rs` -- helper `lookup(value)` L21-26 does `assert_eq!(var, "NOCTIS_TOKEN")` and would panic once `from_env` reads `NOCTIS_HTTP_PROXY`: change it to answer only `NOCTIS_TOKEN` (add a per-var helper) or token tests will also set the proxy. Existing tests L110-510 show `#[tokio::test]` and sync patterns.
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (add `InvalidHttpProxy` after `InvalidHttpTimeout` L139), `EnvError` L218.
- `crates/noctis-core/src/lib.rs` L41-46 -- add `mod proxy;` and re-export `HttpProxyError` (define it in `proxy.rs` or `error.rs`).
- twilight-http 0.17.1 (read-only): `ClientBuilder::proxy(String, bool)` / `ratelimiter(Option<RateLimiter>)` (`src/client/builder.rs` L86-103); request URL `"{http|https}://{proxy or discord.com}/api/v10/{path}"` (`src/client/mod.rs` L2915-2918) — so the proxy is an authority, no scheme; adds `authorization`, `user-agent`, `accept-encoding: br`; `Display` of `RequestTimedOut` = `"request timed out"` (`src/error.rs` L72).
- `crates/noctis-testkit/{Cargo.toml,src/lib.rs}` -- empty skeleton; add deps, `pub mod fake;`, docs.
- `crates/noctis/src/lib.rs` L133-136 -- root `pub use noctis_core::{..}`: add `HttpProxyError`; App doc section may mention `http_proxy`.
- `xtask/src/deps_check.rs` -- read-only: testkit → core edge is drawn (dev-dep allowed); `http-client` rule forbids testkit → `twilight-http`.
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- methods table L60-80 (move `http_proxy` to Implemented, add `App::http_proxy()` to the getters L47-50), Errors L82-110 (add `InvalidHttpProxy`, `HttpProxyError` + variants), §2 env table L131-148 (`NOCTIS_HTTP_PROXY` implemented, rules), §5 HTTP L228-235 (accepted forms, local limiter off). `SKILL.md` L21 API hash. `examples/project-template/README.md` L59 -- say `host:port` or `http://host:port`.
- Continuity (5.1): mutable `&mut` builder; collect-all `BuildError` in recorded order; errors `thiserror` `#[non_exhaustive]`; never echo secrets; memlog decisions; gates via the AGENTS.md list.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-core/src/proxy.rs` -- `normalize_http_proxy` + `HttpProxyError`; unit tests Matrix 1-8.
2. `crates/noctis-core/src/error.rs`, `lib.rs` -- `InvalidHttpProxy` variant, re-exports.
3. `crates/noctis-core/src/app.rs` + `app/tests.rs` -- setter, `App::http_proxy`, build check + client wiring, from_env, Debug; fix `lookup` helper; tests Matrix 9-11.
4. `crates/noctis-testkit/Cargo.toml`, `src/lib.rs`, `src/fake.rs` (+ `src/fake/http.rs` if split) -- the fake per Boundaries.
5. `crates/noctis-testkit/tests/http_fake.rs` -- Matrix 12-25 (`#[tokio::test]`; `flavor = "multi_thread"` for 21); a test-local helper `raw_request(addr, head: &str, body: &[u8]) -> (u16, Vec<u8>)` over `tokio::net::TcpStream` (HTTP/1.1, `connection: close`, read to EOF, parse the status line) drives the rows twilight cannot send: 14 (trailing `/`), 16, 17, 18; all others go through `app.http()`.
6. `crates/noctis/src/lib.rs` -- re-export `HttpProxyError`.
7. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; copy the printed hash to `SKILL.md` L21.
8. `examples/project-template/` -- skill and README updates per Code Map; run `skill-check`.

**Acceptance Criteria:**
- Given `noctis-testkit`, when the story is done, then `noctis_testkit::fake::FakeDiscordHttp` is a local hyper server (versions pinned in `[workspace.dependencies]`) with per-test routes, a configurable canned `GET /gateway/bot` and a request log.
- Given an App built on `Runtime::for_test()` with `.http_proxy(fake.url())`, when it sends a request, then it reaches the fake only through twilight's `proxy(url, true)` with `ratelimiter(None)`, and `crates/noctis-testkit/tests/http_fake.rs` asserts it in the request log.
- Given an invalid proxy value, when `build()` runs, then the one `BuildError` contains `InvalidHttpProxy` with a typed reason and never the value.
- Given `http_timeout(100 ms)` and a 2 s delayed fake route, when the App requests it, then the request fails with a timeout.
- Given the public API change, when `api-snapshot`, `skill-check` and `deps-check` run, then they pass and the Usage Skill documents `http_proxy`, `NOCTIS_HTTP_PROXY` and `HttpProxyError`.

## Spec Change Log

## Review Triage Log

## Design Notes

- The proxy is an authority because twilight formats `"{protocol}://{proxy}/api/v10/.."` itself; accepting `http://` keeps the skill's `NOCTIS_HTTP_PROXY=http://proxy:3000` working. `https` is rejected because AD-24 fixes `proxy(url, true)` (plain HTTP).
- Validation happens in `build()` (not the setter) so the setter chains and every violation is collected, as with `http_timeout`.
- The fake stays internal until `TestApp` (8.6) wires the facade `testkit` feature; "test Runtime" is 5.1's `Runtime::for_test()` (memlog decision).
- The 413 path checks `content-length` before polling the body so Matrix 18 is deterministic (closing a socket with unread data sends RST and can hide the response); the `Limited` path for chunked over-limit bodies is defense in depth, covered by review, not by an E2E test, for the same reason.
- Smoke test shape:
  ```rust
  let fake = FakeDiscordHttp::start().await?;
  let app = App::builder().runtime(&Runtime::for_test()).token(Token::new("fake-token")?)
      .http_proxy(fake.url()).build()?;
  let info = app.http().gateway().authed().await?.model().await?;
  assert_eq!(fake.requests()[0].path, "/api/v10/gateway/bot");
  ```

## Verification

**Commands** (repo root; CI paused, local gates only; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (includes `noctis-testkit/tests/http_fake.rs`).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- each exit 0 (api-snapshot needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0, see AGENTS.md pitfalls).
- `cargo tree -p noctis-testkit -e normal -i twilight-http` -- fails with "did not match" or shows no path from noctis-testkit's own deps; `test ! -d target/debug` -- true.
