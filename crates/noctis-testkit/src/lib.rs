//! Discord HTTP/gateway fakes, `TestApp`, simulated `Inbound`, conformance suites and snapshot helpers.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! - [`fake::FakeDiscordHttp`]: a local Discord HTTP fake (HTTP/1.1 and h2c
//!   on `127.0.0.1`) with per-test routes, a canned `GET /gateway/bot`, a
//!   request and connection log (with HTTP/2 PINGs), stalled routes,
//!   black-holing and connection closing. Point an App at it with
//!   `fake.attach(&mut builder)` (h2c) or
//!   `AppBuilder::http_proxy(fake.url())` (HTTP/1.1).
//! - [`fake::FakeGateway`]: a local Discord gateway fake (WebSocket on
//!   `127.0.0.1`) speaking HELLO, IDENTIFY/READY, RESUME/RESUMED and heartbeats,
//!   with scripted dispatches and close codes and a frame log. Point an App at
//!   it with `AppBuilder::gateway_url(fake.url())`.
//!
//! The end-to-end tests for Epics 5-7 live in this crate's `tests/` directory
//! (`crates/noctis-testkit/tests/`), driving real Apps against the fakes.

pub mod fake;
