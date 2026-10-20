//! Discord HTTP/gateway fakes, `TestApp`, simulated `Inbound`, conformance suites and snapshot helpers.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! - [`fake::FakeDiscordHttp`]: a local Discord HTTP fake (HTTP/1.1 on
//!   `127.0.0.1`) with per-test routes, a canned `GET /gateway/bot` and a
//!   request log. Point an App at it with `AppBuilder::http_proxy(fake.url())`.
//!
//! The end-to-end tests for Epics 5-7 live in this crate's `tests/` directory
//! (`crates/noctis-testkit/tests/`), driving real Apps against the fakes.

pub mod fake;
