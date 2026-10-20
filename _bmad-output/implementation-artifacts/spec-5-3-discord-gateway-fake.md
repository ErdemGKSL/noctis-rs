---
title: 'Story 5.3: Discord gateway fake'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: 'c5436851c1e741a33c14058050e7ebd66698bd0f'
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

**Problem:** Sharding, lanes, resume and shutdown (Stories 5.4-5.8) cannot be tested end to end: there is no local Discord gateway, and an App has no way to point its shards anywhere but `wss://gateway.discord.gg` (FR-51, AD-21, AD-24; Conventions Tests).

**Approach:** Add `AppBuilder::gateway_url(..)` to `noctis-core` (validated at `build()`, also read from `NOCTIS_GATEWAY_URL`), the first `noctis-gateway` code — `shard_config(..)`, the one place that turns App settings into a twilight `Config` with `proxy_url` (5.4's shard startup calls it for every shard) — and `noctis_testkit::fake::FakeGateway`, a local `tokio-websockets` 0.13 server speaking the gateway protocol, scriptable, with a frame log. A smoke test in `crates/noctis-testkit/tests/` connects one twilight shard, built from a real App on `Runtime::for_test()`, to both fakes.

## Boundaries & Constraints

**Always:**
- **Shared authority check (core):** move steps 5-9 of 5.2's `normalize_http_proxy` (InvalidChar, NotAnAuthority, TooLong > 255 bytes, host incl. reg-name allowlist and `Ipv6Addr` brackets, port 1..=65535 with 1-5 digits) into `crates/noctis-core/src/authority.rs`: `pub(crate) fn check_authority(&str) -> Result<(), AuthorityError>` with `pub(crate) enum AuthorityError { InvalidChar { index }, NotAnAuthority, TooLong { len }, InvalidHost, InvalidPort }`. `proxy.rs` maps it 1:1 onto `HttpProxyError`; every existing proxy test passes unchanged.
- **gateway_url (core):** `AppBuilder::gateway_url(&mut self, url: impl Into<String>) -> &mut Self` stores the raw string (last call wins). `build()` runs `pub(crate) fn normalize_gateway_url(&str) -> Result<String, GatewayUrlError>` (new `crates/noctis-core/src/gateway_url.rs`) right after the HTTP proxy check and before the crypto/runtime checks; failure records `BuildViolation::InvalidGatewayUrl { reason: GatewayUrlError }` (message `"invalid gateway URL: {reason}"`). New getter `App::gateway_url(&self) -> Option<&str>` (normalized). No test-only branch in core.
- **Normalization, in order:** (1) empty ⇒ `Empty`; (2) split at the first `"://"`: no `"://"`, or a scheme other than `ws`/`wss` (ASCII-case-insensitive) ⇒ `UnsupportedScheme`; (3) drop one trailing `/` from the rest; (4) empty ⇒ `Empty`; (5-9) `check_authority` mapped to `InvalidChar { index }` (char index in the step-4 string), `NotAnAuthority`, `TooLong { len }`, `InvalidHost`, `InvalidPort`. Returns `"{scheme lowercased}://{step-4 string}"` (host case kept).
- **GatewayUrlError:** in `gateway_url.rs`; `thiserror`, `Debug, Clone, Copy, PartialEq, Eq`, `#[non_exhaustive]`, variants above; each message names the fix (`UnsupportedScheme`: "use `ws://host:port` or `wss://host:port`") and never contains the value. Re-exported from `noctis_core` and the `noctis` facade root.
- **from_env:** reads `NOCTIS_GATEWAY_URL` after `NOCTIS_HTTP_PROXY` with the same `read_var` rules (trim; empty ⇒ unset; non-UTF-8 ⇒ `EnvError::NotUnicode { var: "NOCTIS_GATEWAY_URL" }`), then `gateway_url(value)`; validated only at `build()`.
- **Debug:** `AppBuilder` prints `gateway_url` as a bool (raw value may hold `user:pw@`); `App` prints the normalized URL.
- **shard_config (noctis-gateway, `src/shard_config.rs`, re-exported at crate root):** `pub fn shard_config<Q: twilight_gateway::queue::Queue>(token: &str, intents: Intents, gateway_url: Option<&str>, queue: Q) -> Result<twilight_gateway::Config<Q>, ShardConfigError>`. Order: no current tokio runtime (`Handle::try_current`) ⇒ `NoRuntime` (twilight's `ConfigBuilder::new` spawns a queue task and would panic); with a URL that does not start with `ws://`/`wss://` (ASCII-case-insensitive) or for which `tokio_websockets::client::Builder::new().uri(&format!("{url}/?v=10&encoding=json"))` fails ⇒ `InvalidGatewayUrl` (twilight `expect`s this parse at connect); `tokio_websockets::Connector::new()` fails ⇒ `Tls(#[source] tokio_websockets::Error)` (twilight `unwrap`s it); then `ConfigBuilder::new(token.to_owned(), intents)`, `.proxy_url(url)` when `Some`, `.queue(queue)`, `.build()`. `ShardConfigError`: `thiserror`, `Debug`, `#[non_exhaustive]`; messages never contain the token or URL. Crate docs say core's shard startup (5.4) must build every shard through it.
- **FakeGateway (`crates/noctis-testkit/src/fake/gateway.rs`, re-exported from `fake`):** `start() -> io::Result<Self>` / `start_with(FakeGatewayConfig)` (async, current tokio runtime, bind `127.0.0.1:0`, one accept task owning a `JoinSet` of connection tasks; upgrade with `tokio_websockets::ServerBuilder::new().accept(stream)`; a failed upgrade (e.g. plain HTTP GET) is answered 400 by the library and records nothing); `url() -> String` = `"ws://127.0.0.1:{port}"`; `addr()`; `config() -> FakeGatewayConfig`; `set_config(&self, FakeGatewayConfig)` (applies to frames handled afterwards); `on_heartbeat(&self, F)` with `F: Fn(&HeartbeatSeen) + Send + Sync + 'static` (replaces the previous hook; called outside the state lock); `dispatch(&self, event_type: &str, data: serde_json::Value) -> usize`; `send_raw(&self, text: impl Into<String>) -> usize`; `close(&self, code: u16, reason: &str) -> Result<usize, CloseError>`; `frames() -> Vec<RecordedFrame>`; `clear_frames()`; `connections() -> Vec<RecordedConnection>`; `async wait_for_frame(&self, timeout: Duration, pred: impl Fn(&RecordedFrame) -> bool) -> Option<RecordedFrame>` (first match in the whole log, waiting on a `tokio::sync::Notify` signalled per recorded frame); `Drop` aborts the accept task. State in `Arc<Mutex<..>>` (std; poison recovered with `PoisonError::into_inner`; never held across `.await`); each connection task owns its stream and selects between incoming frames and an `mpsc::UnboundedReceiver` of outgoing messages registered in the state.
- **Protocol (server side):** on upgrade record a `RecordedConnection` and send HELLO `{"op":10,"d":{"heartbeat_interval":<ms, u64 saturating>}}`, then sends only uncompressed **text** frames (twilight requests `compress=zstd-stream` but decompresses only binary frames). Every received frame is recorded before it is handled. Text JSON by `op`: `1` HEARTBEAT ⇒ hook, then op 11 ACK if `heartbeat_ack`; `2` IDENTIFY ⇒ per `identify` reply (`Ready`: create session `fake-session-{n}` (n from 1 per fake), seq 1, send READY `{"op":0,"s":1,"t":"READY","d":{..}}` with `v:10`, `user` (id `ready.user_id` as string, `username:"fake-bot"`, `discriminator:"0000"`, `avatar:null`, `bot:true`, `mfa_enabled:false`), `application {id, flags:0}`, `guilds` (`{id, unavailable:true}` each), `session_id`, `resume_gateway_url: url()`, `shard` echoed from IDENTIFY `d.shard` when present; `InvalidSession { resumable }` ⇒ op 9 `d: resumable`; `Close(code)` ⇒ close with that code; `Ignore` ⇒ nothing); a second IDENTIFY or a RESUME on an authenticated connection ⇒ close 4005; `6` RESUME ⇒ per `resume` reply (`Auto`: known session ⇒ dispatch `RESUMED` with that session's next seq and mark authenticated, unknown ⇒ op 9 `d:false`; `InvalidSession { resumable }`; `Close(code)`); `3`, `4`, `8`, `31` before authentication ⇒ close 4003, after ⇒ recorded only; any other op ⇒ close 4001; invalid JSON, no integer `op`, or a binary frame ⇒ close 4002. A client Close frame with 1000/1001 forgets its session; any other code (twilight's resumable close is 4000) keeps it; fake-initiated closes keep sessions. `dispatch` sends `{"op":0,"s":<session seq+1>,"t":event_type,"d":data}` to every live authenticated connection (no validation of `event_type`/`data`).
- **Types:** `FakeGatewayConfig { pub heartbeat_interval: Duration, pub heartbeat_ack: bool, pub identify: IdentifyReply, pub resume: ResumeReply, pub ready: ReadyData }` (`Default`: 41_250 ms, `true`, `Ready`, `Auto`, default `ReadyData`); `ReadyData { pub user_id: u64, pub application_id: u64, pub guild_ids: Vec<u64> }` (`Default`: 1, 1, empty; no validation, so 0 is sent verbatim); `IdentifyReply { Ready, InvalidSession { resumable: bool }, Close(u16), Ignore }`; `ResumeReply { Auto, InvalidSession { resumable: bool }, Close(u16) }`; a scripted `Close(code)` whose code `close()` would reject closes with 1011 instead (never a panic). `HeartbeatSeen { pub connection: u64, pub received_at: Instant, pub since_hello: Duration, pub since_previous: Option<Duration>, pub seq: Option<u64> }`. `RecordedConnection { pub id: u64 /* from 1 */, pub uri: String /* path and query */, pub opened_at: Instant, pub closed_at: Option<Instant> }`. `RecordedFrame { pub connection: u64, pub received_at: Instant, pub kind: FrameKind }` + `op(&self) -> Option<u64>`, `json(&self) -> Option<serde_json::Value>`; `#[non_exhaustive] enum FrameKind { Text(String), Binary(Bytes), Close { code: Option<u16>, reason: String }, Ping(Bytes), Pong(Bytes) }`. `CloseError { InvalidCode { code: u16 }, ReasonTooLong { len: usize } }` (`thiserror`, `Copy, PartialEq, Eq`): sendable codes are 1000-1003, 1007-1014 and 3000-4999 (tokio-websockets panics on the rest); reason ≤ 123 bytes. All public types `Debug, Clone` (+ `PartialEq, Eq` where fields allow).
- **Dependencies:** root `Cargo.toml` pins stay (`tokio-websockets = "0.13"` → lock 0.13.3, MSRV 1.89; `futures-util = "0.3"`). `crates/noctis-gateway/Cargo.toml`: `thiserror`, `tokio = { workspace, features = ["rt"] }`, `tokio-websockets = { workspace = true, features = ["client", "fastrand", "sha1_smol", "rustls-platform-verifier"] }` (exactly twilight's set, no new crates), `twilight-gateway.workspace = true`; dev: `tokio` (`macros`, `rt`, `rt-multi-thread`), `rustls.workspace = true` if a test must install `ring`. `crates/noctis-core/Cargo.toml`: unchanged (core does not call `noctis-gateway` until 5.4). `crates/noctis-testkit/Cargo.toml` `[dependencies]` add `futures-util = { workspace = true, features = ["sink"] }`, `tokio-websockets = { workspace = true, features = ["server", "sha1_smol"] }`, and tokio feature `macros` (for `select!`); `[dev-dependencies]` add `noctis-gateway.workspace = true`, `twilight-gateway.workspace = true`, `twilight-model.workspace = true`. Never enable tokio-websockets `ring`/`aws_lc_rs`/`openssl`. New licenses (BSD-3-Clause `sha1_smol`, ISC queue, MIT/Apache zstd) are already allowed by `deny.toml`.
- Sources of `noctis-gateway` must not contain `ClientBuilder`, `Client::new(` or `Client::builder(` (deps-check `http-client` scans them): use `tokio_websockets::client::Builder`.
- Every new public item has rustdoc + a doctest (`no_run` when it needs a runtime/network). No `unwrap`/`expect` outside tests. Release-only cargo commands.

**Never:**
- No `App::run` shard startup, `GET /gateway/bot` call from core, `IdentifyQueue` adapter, lanes or `Inbound` (5.4/5.5), no shutdown sequence (5.8), no facade `testkit` wiring or `noctis::testkit`/`noctis::twilight::gateway` re-export (8.6), no zstd/zlib output, no TLS (`wss`) in the fake, no replay of missed events on RESUME.
- No `cfg(test)`/test branch in core; no network access in tests; no `std::env::set_var`.
- Never echo the gateway URL or token in an error, `BuildError` or `Debug` of the builder.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | URL accepted | `ws://127.0.0.1:3000`, `WS://Proxy:3000/`, `wss://gateway.discord.gg`, `Wss://[::1]:8080`, `ws://p:1`, `ws://p:65535`, `ws://p:08080`, `ws://` + 255×`a` | `ws://127.0.0.1:3000`, `ws://Proxy:3000`, same, `wss://[::1]:8080`, same, same, same, same |
| 2 | Empty | `""`, `"ws://"`, `"wss:///"`, `"ws:///"` | `Empty` ×4 |
| 3 | Scheme | `127.0.0.1:3000`, `p`, `http://p`, `https://p`, `://p`, `wsx://p`, `ftp://p:21`, `" ws://p"` | `UnsupportedScheme` ×8 |
| 4 | Bad char | `ws:// p`, `ws://a b`, `ws://p\n`, `ws://a\tb`, `ws://pröxy`, `ws://🙂`, `ws://a\u{7f}` | `InvalidChar{index}` = 0, 1, 1, 1, 2, 0, 1 |
| 5 | Not authority | `ws://p:3000/gateway`, `ws://p//`, `ws://p?v=10`, `ws://p#x`, `wss://u:pw@p`, `ws://a\b`, `ws://ws://p` | `NotAnAuthority` ×7 |
| 6 | Length | `ws://` + 256×`a` | `TooLong{len:256}` |
| 7 | Host | `ws://:3000`, `ws://::1`, `ws://[::1`, `ws://[]`, `ws://[::1]x`, `ws://[zz]`, `ws://a]b`, `ws://pro<xy` | `InvalidHost` ×8 |
| 8 | Port | `ws://p:`, `ws://p:0`, `ws://p:65536`, `ws://p:123456`, `ws://p:+80`, `ws://p:8a`, `ws://[::1]:0` | `InvalidPort` ×7 |
| 9 | Proxy regression | every 5.2 `proxy.rs` test | unchanged results after the `authority.rs` refactor |
| 10 | Build wiring | `.gateway_url("WS://p:1/")`; twice (`ws://a:1` then `ws://b:2`); unset; `"http://x"` + `.http_proxy("https://x")` + timeout 0 | `app.gateway_url()==Some("ws://p:1")`; `Some("ws://b:2")`; `None`; one `BuildError` [InvalidHttpTimeout, InvalidHttpProxy{UnsupportedScheme}, InvalidGatewayUrl{UnsupportedScheme}] in that order |
| 11 | Redaction | `.gateway_url("wss://u:SECRET@h")` build; builder `Debug` before build | `BuildError` `Display`/`Debug` lack `SECRET`; builder Debug shows `gateway_url: true` |
| 12 | from_env | `NOCTIS_GATEWAY_URL` absent / `""` / `" \t"` / `" ws://p:1 \n"` / non-UTF-8 / `"http://x"` | unset / unset / unset / `Some("ws://p:1")` after build / `EnvError::NotUnicode{var:"NOCTIS_GATEWAY_URL"}` / from_env Ok, build `InvalidGatewayUrl{UnsupportedScheme}` |
| 13 | shard_config | sync `#[test]` (no runtime); in runtime with `None`; with `Some("ws://127.0.0.1:1")`; with `Some("ws://a b")`, `Some("")`, `Some("http://p")` (bypassing core) | `Err(NoRuntime)`; Ok, `proxy_url()==None`, `intents()` and `token()=="Bot t"` as given; Ok, `proxy_url()==Some(..)`; `Err(InvalidGatewayUrl)` ×3, no panic; every row-1 output of `normalize_gateway_url` (via an App in testkit tests) gives Ok |
| 14 | Fake start | `FakeGateway::start()` twice | distinct non-zero ports; `url()=="ws://127.0.0.1:{port}"`; separate logs |
| 15 | Smoke (AC) | `FakeDiscordHttp` with `GatewayBot.url = gw.url()`; App on `Runtime::for_test()` + token `fake-token` + `.http_proxy(http.url())` + `.gateway_url(gw.url())`; `app.http().gateway().authed()`; shard `ShardId::ONE` from `shard_config("fake-token", app.intents(), app.gateway_url(), InMemoryQueue::new(1, 1000, Duration::from_secs(5), 1000))`; await READY; `gw.dispatch("MESSAGE_DELETE", {id:"3",channel_id:"2",guild_id:"1"})`; `shard.close(CloseFrame::NORMAL)` | authed URL == `gw.url()`; HTTP log has one `GET /api/v10/gateway/bot`; `Event::Ready` with `session_id=="fake-session-1"`, `shard==Some(ShardId::ONE)`, user id 1; `dispatch` returns 1 and the shard yields `Event::MessageDelete` (seq 2); the fake logs IDENTIFY (op 2, `d.token=="Bot fake-token"`, `d.intents==GUILDS bits`, `d.shard==[0,1]`) and a Close frame code 1000; connection uri starts `/?v=10&encoding=json`; `closed_at` set; exactly 1 connection |
| 16 | Heartbeat timing | `heartbeat_interval` 100 ms, `on_heartbeat` collects | ≥ 3 `HeartbeatSeen` within 2 s; each `since_previous` ≥ 50 ms; `seq` is `None` before READY and the session's last seq after it; one op 11 ACK per heartbeat |
| 17 | No ACK (zombie) | `heartbeat_ack: false`, 100 ms | twilight drops the connection; a second connection sends RESUME (op 6) with `fake-session-1`; fake answers `RESUMED`, shard yields `Event::Resumed` |
| 18 | Resumable close | after READY, `gw.close(4000, "reconnect")` | returns `Ok(1)`; shard yields `GatewayClose` 4000, reconnects to `resume_gateway_url` (= fake), sends RESUME, gets `RESUMED` with seq = last + 1 |
| 19 | Fatal 4004 / 4014 | `identify: Close(4004)`; `Close(4014)` | shard yields `GatewayClose` with that code, then the stream ends (`None`); no READY sent; after 1.5 s still exactly 1 connection |
| 20 | Invalid session | `identify: InvalidSession{resumable:false}`, flipped to `Ready` once recorded | op 9 `d:false` sent; a later connection's IDENTIFY gets READY `fake-session-2` |
| 21 | Resume unknown / scripted | RESUME with unknown session (raw client); `resume: InvalidSession{resumable:true}`; `resume: Close(4009)` | op 9 `d:false`; op 9 `d:true`; close 4009 |
| 22 | Protocol errors (raw `tokio_websockets` client) | `not json`; `{"d":1}`; binary frame; `{"op":99}`; op 8 before IDENTIFY; second IDENTIFY; op 3 after READY | close 4002; 4002; 4002; 4001; 4003; 4005; no close, frame recorded — every frame recorded first |
| 23 | Heartbeat before identify | raw client sends op 1 `d:null` right after HELLO | op 11 ACK; hook gets `seq: None` |
| 24 | close() validation | codes 999, 1000, 1003, 1004, 1005, 1006, 1007, 1014, 1015, 1016, 2999, 3000, 4999, 5000; reason 123×`a`, 124×`a`, 61×`ü` (122 B), 62×`ü` (124 B), 31×`🙂` (124 B) | `InvalidCode` for 999, 1004, 1005, 1006, 1015, 1016, 2999, 5000; `Ok(n)` for the rest; reason: Ok, `ReasonTooLong{len:124}`, Ok, `ReasonTooLong{124}`, `ReasonTooLong{124}`; an invalid scripted `IdentifyReply::Close(5000)` closes with 1011, no panic |
| 25 | Nothing live | `dispatch`, `send_raw`, `close(4000,"")` with no connection | `0`, `0`, `Ok(0)` |
| 26 | Multiple shards | shards `[0,2]` and `[1,2]` connect | two connections, READY `shard` echoes each; `dispatch` returns 2; seqs independent |
| 27 | Client close semantics | client close 1000 then RESUME with that session (raw); client close 4000 then RESUME | op 9 `d:false`; `RESUMED` |
| 28 | Not a WebSocket | plain `GET / HTTP/1.1` over TCP | `400`; no connection recorded; a later WebSocket connect still works |
| 29 | READY data | `ReadyData { user_id: 7, application_id: 8, guild_ids: [1,2] }`; `user_id: 0` | twilight `Ready` model has those ids and 2 unavailable guilds; READY sent with `"id":"0"`, twilight `next_event` returns a deserialize `Err` (no panic) |
| 30 | wait_for_frame | predicate matching an already-recorded frame; never-matching, 200 ms | returns it immediately; `None` after ≈200 ms |
| 31 | Log ops | `frames()`, `clear_frames()`, `connections()` | arrival order; empty frames; connections kept |
| 32 | Dropped fake | live shard, drop the fake | shard yields an abnormal close / reconnect error, then fails to reconnect (`Err` items), no panic in test or fake |

</intent-contract>

## Code Map

- `crates/noctis-core/src/proxy.rs` L60-135 -- `normalize_http_proxy`, `split_host_port`, `is_reg_name_byte`, `check_port`: source of the shared `authority.rs`; keep its tests (L137+) green.
- `crates/noctis-core/src/app.rs` -- consts `TOKEN_VAR`/`HTTP_PROXY_VAR` L27-29 (add `GATEWAY_URL_VAR`); `App` struct L53-63 and `AppBuilder` L243-260 (add `gateway_url: Option<String>`); getters ~L130 (add `gateway_url()` beside `http_proxy()`); `from_lookup` ~L295 (read after proxy); `http_proxy` setter ~L405 (add `gateway_url` setter after it); `build` ~L545 (check after the proxy match; field into `App`); Debug impls (App ~L215, builder ~L600); `run()` doc unchanged.
- `crates/noctis-core/src/app/tests.rs` L22-35 -- `lookup_var` asserts the var is `NOCTIS_TOKEN` or `NOCTIS_HTTP_PROXY`: add `NOCTIS_GATEWAY_URL` or every from_env test panics.
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (add `InvalidGatewayUrl` after `InvalidHttpProxy`); `EnvError` unchanged.
- `crates/noctis-core/src/lib.rs` L37-48 -- add `mod authority; mod gateway_url;`, `pub use gateway_url::GatewayUrlError;`.
- `crates/noctis/src/lib.rs` L133-136 -- root re-export list: add `GatewayUrlError`.
- `crates/noctis-gateway/{Cargo.toml,src/lib.rs}` -- empty skeleton; add deps, `mod shard_config; pub use shard_config::{shard_config, ShardConfigError};`, docs.
- twilight-gateway 0.17.1 (read-only, `~/.cargo/registry/src/*/twilight-gateway-0.17.1`): `ConfigBuilder::new` L162 (`Connector::new().unwrap()`, spawns `InMemoryQueue`), `proxy_url` L313, `queue` L325; `Shard::poll_connect` L555-580: URL = `resume_url` or `proxy_url` or `wss://gateway.discord.gg`, `+ "/?v=10&encoding=json&compress=zstd-stream"`, `.uri(..).expect("valid URL")`, reconnect delay `2^attempt` s (first 1 s); READY sets `resume_url` from `resume_gateway_url` (L770); only binary frames are decompressed (L884); fatal close codes stop the shard (`CloseCode::can_reconnect`). `Config::token()` returns the `Bot `-prefixed token.
- tokio-websockets 0.13.3 (read-only): `ServerBuilder::accept(stream) -> (http::Request<()>, WebSocketStream)` (failed upgrade writes 400); `Message::close` panics on reserved codes / reason > 123 B (`proto/types.rs` L337); `CloseCode::try_from(u16)` accepts 1000-1015, 3000-4999; `WebSocketStream: Stream + Sink<Message>`.
- `crates/noctis-testkit/src/fake.rs` -- 5.2 module (`mod http;` L19, re-exports L29-30): add `mod gateway;` + `pub use gateway::{..}`; follow its state/Drop/poison patterns (L318-537).
- `crates/noctis-testkit/src/lib.rs` -- crate docs: add the `FakeGateway` bullet.
- `crates/noctis-testkit/tests/http_fake.rs` L25-60 -- `app(..)` and `raw_request` helper style to copy.
- `xtask/src/deps_check.rs` L37-44 (read-only) -- `CLIENT_CONSTRUCTORS` scan of gateway sources; L63-77 edges (core→gateway, testkit→gateway drawn).
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- methods table L60-80 (`.gateway_url` to Implemented; `App::gateway_url()` in the getters paragraph ~L47), Errors L82-115 (`InvalidGatewayUrl`, `GatewayUrlError`), §2 env table (`NOCTIS_GATEWAY_URL`), §4 Session resume (note: resumes go to Discord's `resume_gateway_url`, so a gateway proxy must rewrite it). `SKILL.md` L21 API hash; `examples/project-template/README.md` env table L59; `.env.example` L9 (add `# NOCTIS_GATEWAY_URL=`).

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-core/src/authority.rs`, `proxy.rs` -- extract the shared check; proxy tests stay green (Matrix 9).
2. `crates/noctis-core/src/gateway_url.rs` -- `normalize_gateway_url` + `GatewayUrlError`; unit tests Matrix 1-8.
3. `crates/noctis-core/src/{error.rs,lib.rs,app.rs,app/tests.rs}` -- violation, re-exports, setter/getter/env/Debug/build check; tests Matrix 10-12.
4. `crates/noctis-gateway/{Cargo.toml,src/lib.rs,src/shard_config.rs}` -- `shard_config` + `ShardConfigError`; unit tests Matrix 13 (runtime rows `#[tokio::test]`).
5. `crates/noctis-testkit/{Cargo.toml,src/lib.rs,src/fake.rs,src/fake/gateway.rs}` -- `FakeGateway` per Boundaries.
6. `crates/noctis-testkit/tests/gateway_fake.rs` -- Matrix 13 (last row), 14-32 with `#[tokio::test(flavor = "multi_thread")]`; twilight-driven rows use `Shard` + `twilight_gateway::StreamExt::next_event(EventTypeFlags::all())` with `tokio::time::timeout` (≤ 10 s each); protocol rows use a test-local raw `tokio_websockets::ClientBuilder` client (allowed in testkit) over `gw.url()`; a test-local helper builds the App as in Matrix 15. A unit test in `gateway.rs` deserializes the READY `d` into `twilight_model::gateway::payload::incoming::Ready`.
7. `crates/noctis/src/lib.rs` -- re-export `GatewayUrlError`.
8. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; copy the printed hash to `SKILL.md` L21.
9. `examples/project-template/` -- skill, README, `.env.example` per Code Map; `skill-check`.
10. `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already holds this story's planning decisions; append implementation decisions if any.

**Acceptance Criteria:**
- Given `noctis_testkit::fake::FakeGateway`, when a test starts it, then it is a local `tokio-websockets` 0.13 server speaking HELLO, IDENTIFY/READY, RESUME/RESUMED, heartbeat ACKs (controllable interval, `on_heartbeat` timing hook), scripted dispatches and close codes (4004, 4014, resumable 4000), and records every frame it receives.
- Given an App built on `Runtime::for_test()` with `.gateway_url(fake.url())` and `.http_proxy(http_fake.url())`, when its shard config connects, then the shard reaches the fake only through twilight's `proxy_url`, and `crates/noctis-testkit/tests/gateway_fake.rs` connects one shard to both fakes, receives READY and one dispatch, and closes cleanly (Matrix 15).
- Given an invalid gateway URL, when `build()` runs, then the one `BuildError` holds `InvalidGatewayUrl` with a typed reason and never the value.
- Given any input to `shard_config`, when it runs, then it returns `Ok` or a `ShardConfigError` and never panics.
- Given the public API change, when `api-snapshot`, `skill-check`, `deps-check` and `cargo deny check` run, then they pass and the Usage Skill documents `gateway_url`, `NOCTIS_GATEWAY_URL` and `GatewayUrlError`.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 40 findings — high 0, medium 1, low 27, false 12, maybe-false 0
- findings:
  - `[low]` `[patch]` (blind) `dropped_fake`: loop exits only at `errors >= 2`, so `assert!(closes + errors >= 2)` is vacuous and `closes` dead — confirmed at gateway_fake.rs L783-797; patch: delete the dead counter and assert (grouped with edge "Matrix 32 closes never checked").
  - `[low]` `[reject]` (blind) `shard_config` claims no panic but the rustls provider is unchecked — with the workspace's ring-only rustls, rustls auto-selects ring, and `AppBuilder::build` installs it before any App-driven call; a panic needs a second provider unified in by a bot crate plus a direct call before `build()`, which is documented as a precondition; a typed check needs a new `rustls` dependency (grouped with edge "Connector::new provider panic").
  - `[low]` `[reject]` (blind) `check_gateway_url` accepts `ws://p/path`, `ws://u@p`, `ws://p:0` that core rejects — true, but the spec fixes this check deliberately (core cannot be called from noctis-gateway) and 5.4 passes only core-normalized URLs; sharing the authority check across crates is more than a direct correction (grouped with edge "uppercase scheme" and edge "path/userinfo accepted").
  - `[low]` `[reject]` (blind) `Connector::new()` built and dropped per call — twilight's `ConfigBuilder::new` already builds one per shard, so the cost is at most doubled at startup; spec mandates the check; caching adds state.
  - `[false]` `[reject]` (blind) tokio-websockets features unify differently from twilight's TLS choice — workspace `twilight-gateway` enables `rustls-platform-verifier`, exactly the feature noctis-gateway sets; no second root backend is added.
  - `[low]` `[reject]` (blind) RESUME Auto attaches a session another live connection still holds, so dispatch double-counts seq — mechanically true at gateway.rs L992-997, but needs a test to resume a still-live session; twilight closes the old connection long before its 1 s reconnect delay; fix adds a detach loop (grouped with edge "resume of attached session" and edge "close 1000 forgets a shared session").
  - `[false]` `[reject]` (blind) `dispatch` keeps the seq increment when `tx.send` fails — `rx` lives until the end of `serve`, after `live.remove(&id)` under the lock, so `send` cannot fail while the `live` entry exists.
  - `[low]` `[patch]` (blind) `heartbeat_interval` under 1 ms announces 0 and twilight's `interval_at` panics on a zero period — confirmed (twilight shard.rs L801-811); patch: clamp the HELLO value to at least 1 ms and document it (grouped with edge "heartbeat_interval below 1 ms").
  - `[low]` `[patch]` (blind) `identifies` counted for `Close`/`Ignore` while docs say "IDENTIFYs answered" — confirmed at gateway.rs L121 and L954-956; patch: reword doc and comment, no behaviour change.
  - `[false]` `[reject]` (blind) `wait_for_frame` returns the first match of the whole log, undocumented — the spec mandates whole-log search and the rustdoc says "The first frame in the log matching `pred`".
  - `[low]` `[reject]` (blind) `accept_loop` retries accept errors silently — permanent accept failures on a local 127.0.0.1 listener are not met in practice; recording them adds state.
  - `[low]` `[reject]` (blind) `heartbeat_timing` has no upper bound per gap — `>= 3` beats in 2 s already bounds the average; a tight upper bound on wall-clock gaps invites flakes.
  - `[false]` `[reject]` (blind) `fatal_close_ends_the_shard` sleeps 1.5 s per code needlessly — Matrix 19 requires "after 1.5 s still exactly 1 connection"; the sleep is the specified check.
  - `[low]` `[patch]` (blind) Usage Skill presents "the testkit's `FakeGateway`" to bot authors though it stays crate-internal until 8.6 — confirmed at app-and-sharding.md L68 and L249; patch: mark `[planned, Story 8.6]`.
  - `[false]` `[reject]` (blind) proxy.rs `map_err` vs `From` lets the mappings drift — `AuthorityError` is a crate-private exhaustive enum, so a new variant fails both matches at compile time; no drift is possible.
  - `[low]` `[patch]` (blind) `GatewayUrlError::TooLong` says "host" but `len` is the whole authority — confirmed at gateway_url.rs L42; patch: say "authority (host and port)".
  - `[low]` `[patch]` (blind) `.env.example` omits the `resume_gateway_url` caveat — confirmed at .env.example L10-11; patch: add the one-line caveat.
  - `[low]` `[patch]` (edge) heartbeat_interval below 1 ms ⇒ twilight zero-period panic — same root cause as the blind row; patched with it.
  - `[low]` `[reject]` (edge) RESUME Auto for a session attached to another live connection — grouped with the blind double-resume row; rejected with it.
  - `[low]` `[reject]` (edge) client close 1000 forgets a session another live connection holds — same root cause (a session attached to two connections); rejected with that group.
  - `[low]` `[reject]` (edge) a panicking `on_heartbeat` hook kills the task before cleanup — the hook is test code and a panic there already fails the test loudly; `catch_unwind` adds complexity.
  - `[low]` `[reject]` (edge) `wait_for_frame(Duration::MAX)` overflows `Instant + Duration` and panics — true (tokio `Instant` add panics), but tests pass bounded timeouts; the guard adds a branch.
  - `[low]` `[reject]` (edge) a TCP client that never sends the upgrade lingers until the fake drops — only a stuck task inside a test fake; the timeout adds code for no observed harm.
  - `[low]` `[reject]` (edge) `shard_config` accepts `WS://` uppercase and tokio-websockets then fails `UnsupportedScheme` at connect (http keeps the scheme case, client.rs L284-295) — true for direct callers only; core lowercases the scheme and 5.4 passes core's value; normalizing in shard_config deviates from the spec's check; grouped with the blind validator-drift row.
  - `[low]` `[reject]` (edge) `shard_config` accepts path/trailing slash/userinfo — grouped with the blind validator-drift row; rejected with it.
  - `[low]` `[reject]` (edge) `%` in a gateway reg-name passes `build()` but http rejects it in `shard_config` — true (http 1.5 `rejects_percent_in_hostname`), but no real host contains `%`, and removing `%` from the shared allowlist changes 5.2 proxy behaviour.
  - `[low]` `[reject]` (edge) `Connector::new().unwrap()` in twilight can panic with two rustls providers — grouped with the blind crypto-provider row; rejected with it.
  - `[low]` `[patch]` (edge) Matrix 32 `closes` never checked — same root cause as the blind `dropped_fake` row; patched with it.
  - `[low]` `[reject]` (edge) Matrix 16 "one op 11 ACK per heartbeat" not counted — missing ACKs are caught (the shard would zombie-reconnect and `connections().len() == 1` fails) and Matrix 23 checks one ACK per raw heartbeat; counting client-side adds a harness.
  - `[low]` `[patch]` (vgap) `send_raw` never exercised with a live connection — pre-verified gap; patch: raw-client test with two clients asserting count 2 and exact text.
  - `[low]` `[patch]` (vgap) `IdentifyReply::Ignore` untested — pre-verified gap; patch: `identify_ignored` raw-client test (next frame after a heartbeat is op 11).
  - `[medium]` `[patch]` (vgap) RESUME's authenticated effects unverified (dispatch after RESUMED, second RESUME ⇒ 4005) — pre-verified gap on the path 5.6 resume tests rely on; patch: extend `resumable_close_resumes` with a dispatch and seq 3, add a RESUME-twice ⇒ 4005 raw case.
  - `[low]` `[patch]` (vgap) client close 1001 forgetting the session untested — filed as defer, but it is this story's code and the fix is one test block; patch: add a 1001 block to `client_close_semantics`.
  - `[false]` `[reject]` (intent) the App does not connect, a test-assembled shard does — the spec settles reading R2 (Design Notes "Why `shard_config` and not `App::run`"); shard startup is Story 5.4 in epics.
  - `[false]` `[reject]` (intent) "only through `proxy_url`, no test branch" asserted for a path core does not take — core has no shard path until 5.4, and the spec binds 5.4 to `shard_config` in crate docs; no test branch exists in core.
  - `[false]` `[reject]` (intent) the HTTP fake is not on the shard's connect path — `GET /gateway/bot` from core is excluded by the spec's Never list (5.4); Matrix 15 deliberately drives both fakes from the App.
  - `[false]` `[reject]` (intent) resumes reach the fake via `resume_gateway_url`, not `proxy_url` — Discord's protocol sends resumes there; the spec settles it (fake points it at itself, skill warns proxies must rewrite it).
  - `[false]` `[reject]` (intent) the smoke test passes a literal token, not an App-derived one — the App has no token getter by design; token plumbing belongs to 5.4's shard startup.
  - `[false]` `[reject]` (intent) lanes and shutdown are not exercised end to end — they are Stories 5.5 and 5.8; the story provides the fake they will use.
  - `[false]` `[reject]` (intent) extra public surface beyond `.gateway_url` — env var, `GatewayUrlError`, `BuildViolation::InvalidGatewayUrl` and `shard_config` are the spec's deliberate settlement (Boundaries) and follow the 5.2 `http_proxy` pattern.

## Design Notes

- **Why `shard_config` and not `App::run`:** shard startup is Story 5.4 (needs `GET /gateway/bot`, the `IdentifyQueue` adapter and lanes) and shutdown is 5.8. The App's gateway path in 5.3 is therefore `App` getters → `noctis_gateway::shard_config` → twilight `Config::proxy_url`; 5.4's `App::run` must build every shard through the same function (crate docs say so), and its E2E tests then go through `App::run` itself. Core has no test branch. Generic `Q` lets 5.4 pass its port-backed queue (GW-01: never `InMemoryQueue::default()` in production); tests pass an explicit `InMemoryQueue::new(..)`.
- **Authority-only URL:** twilight appends `/?v=10&..` itself, so a path would break it; `ws`/`wss` only because tokio-websockets connects nothing else. twilight resumes at READY's `resume_gateway_url`, bypassing `proxy_url`; the fake sets it to its own URL, and the skill warns that a real gateway proxy must rewrite it.
- **Text frames only:** twilight decompresses only binary frames, so the fake never needs zstd.
- **Fake closes are validated** because `tokio_websockets::Message::close` panics on reserved codes or > 123-byte reasons; a test helper must never panic the fake's task.
- Smoke shape:
  ```rust
  let config = shard_config("fake-token", app.intents(), app.gateway_url(), queue)?;
  let mut shard = Shard::with_config(ShardId::ONE, config);
  let ready = next(&mut shard).await; // Event::Ready
  assert_eq!(gw.dispatch("MESSAGE_DELETE", json!({"id":"3","channel_id":"2","guild_id":"1"})), 1);
  shard.close(CloseFrame::NORMAL);
  ```

## Verification

**Commands** (repo root; CI paused, local gates only; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (includes `noctis-testkit/tests/gateway_fake.rs`; run it 3× to check for flakes).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- each exit 0 (api-snapshot needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0, see AGENTS.md pitfalls).
- `cargo tree -p noctis-testkit -e normal -i twilight-http` -- no path from noctis-testkit's own deps; `cargo tree -i aws-lc-rs` -- no match; `test ! -d target/debug` -- true.

## Auto Run Result

- **Summary:** Added the in-process Discord gateway fake (`noctis-testkit::fake::gateway`: HELLO/IDENTIFY/READY/RESUME, heartbeats + ACK, dispatch with seq, scripted/validated closes, `send_raw`, frame log with `wait_for_frame`), the `App` gateway URL setting (`.gateway_url` builder, env var, `GatewayUrlError`, `BuildViolation::InvalidGatewayUrl`, shared authority validation with the 5.2 proxy), and `noctis_gateway::shard_config` which builds twilight shard configs via `proxy_url`; 5.4 must build every shard through it.
- **Files changed:** `crates/noctis-core/src/{app.rs,app/tests.rs,authority.rs,error.rs,gateway_url.rs,lib.rs,proxy.rs}` (gateway URL setting + shared authority check); `crates/noctis-gateway/{Cargo.toml,src/lib.rs,src/shard_config.rs}` (shard config builder); `crates/noctis-testkit/{Cargo.toml,src/fake.rs,src/fake/gateway.rs,src/lib.rs,tests/gateway_fake.rs}` (fake + matrix tests); `crates/noctis/src/lib.rs` (re-export); `examples/project-template/` (Usage Skill, `.env.example`, README); `xtask/public-api.txt` (re-blessed); `Cargo.lock`.
- **Review findings:** 40 total (high 0, medium 1, low 27, false 12). Patched 11 entries (medium 1, low 10): dead `closes` counter/vacuous assert, HELLO heartbeat_interval clamp to >= 1 ms (twilight zero-period panic), `identifies` doc wording, Usage Skill `FakeGateway` marked `[planned, Story 8.6]`, `TooLong` message says authority, `.env.example` resume_gateway_url caveat, `send_raw` live test, `IdentifyReply::Ignore` test, RESUME dispatch/seq + RESUME-twice => 4005 (medium), client close 1001 test. Deferred: none. Rejected: 29 (12 false, 17 low) with reasons recorded in the Review Triage Log (validator drift vs core, rustls provider precondition, double-attached session on resume, hook panics, unbounded timeouts, etc.).
- **Follow-up review recommended:** false (first pass; patched high 0, medium 1, low 10).
- **Verification:** all local gates green via the gates script (fmt, clippy -D warnings, workspace tests incl. gateway_fake, cargo-hack each-feature, MSRV 1.91 check, cargo deny, xtask deps-check/rules-catalog/api-snapshot/skill-check). CI is paused by the maintainer.
- **Residual risks:** `shard_config` accepts some URLs core rejects (uppercase scheme, path/userinfo) for direct callers; a second rustls provider in a bot crate could panic twilight's connector before `build()`; the fake can attach one session to two live connections on RESUME; twilight resumes via `resume_gateway_url`, so real gateway proxies must rewrite it.
