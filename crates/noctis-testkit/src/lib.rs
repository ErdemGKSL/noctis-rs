//! The Noctis test kit (FR-51): [`TestApp`], simulated interactions,
//! message assertions and snapshots, [`assert_memory_flat`], and the local
//! Discord fakes.
//!
//! This crate is internal; use `noctis` instead (its `testkit` feature
//! re-exports the crate root as `noctis::testkit`, without [`fake`]). Its
//! API is not stable and may change in any release.
//!
//! - [`TestApp`]: the bot's own App, built from its `AppBuilder`, running
//!   on `Runtime::for_test()` against the fakes. Commands, context menus,
//!   clicks, selects, modal submits and autocomplete go through the real
//!   shard and pipeline; each run returns a [`Response`] (kind with the
//!   defer equivalences, ephemerality, the final message, follow-ups,
//!   built-in notices, the HTTP calls).
//! - [`render_json`] and [`Response::message_json`]: snapshot JSON with
//!   only per-send `custom_id` parts masked.
//! - [`assert_memory_flat`]: NFR-2 with dhat.
//! - [`assert_render_snapshot!`] and [`Tolerance`]: Template pixel
//!   snapshots through the App's `Renderer` port
//!   ([`TestApp::renderer`]), decoded with `png` and compared with
//!   `tests/snapshots/<name>.png` within a tolerance.
//! - [`fake::FakeDiscordHttp`]: a local Discord HTTP fake (HTTP/1.1 and h2c
//!   on `127.0.0.1`) with per-test routes, a fallback, a canned `GET
//!   /gateway/bot`, a request and connection log (with HTTP/2 PINGs),
//!   stalled routes, black-holing and connection closing. Point an App at
//!   it with `fake.attach(&mut builder)` (h2c) or
//!   `AppBuilder::http_proxy(fake.url())` (HTTP/1.1).
//! - [`fake::FakeGateway`]: a local Discord gateway fake (WebSocket on
//!   `127.0.0.1`) speaking HELLO, IDENTIFY/READY, RESUME/RESUMED and heartbeats,
//!   with scripted dispatches and close codes and a frame log. Point an App at
//!   it with `AppBuilder::gateway_url(fake.url())`.
//! - [`conformance`]: one async suite per store port, `IdentifyQueue` and
//!   `DiscordCache` (FR-39), proving a custom Adapter keeps the semantics
//!   core relies on (`conformance::cache_store(|| MyStore::new()).await`).
//!
//! The end-to-end tests for Epics 5-8 live in this crate's `tests/` directory
//! (`crates/noctis-testkit/tests/`), driving real Apps against the fakes.

mod app;
pub mod conformance;
mod error;
pub mod fake;
mod memory;
mod render;
mod response;
mod seed;
mod snapshot;

pub use app::{AutocompleteRun, ClickRun, CommandRun, MenuRun, SelectRun, SubmitRun, TestApp};
pub use dhat;
pub use error::TestError;
pub use fake::RecordedFile;
pub use memory::{MEMORY_FLAT_SLACK_BYTES, MemoryReport, assert_memory_flat};
pub use render::render_json;
pub use response::{
    HttpCall, ModalForm, Response, ResponseKind, TestComponent, TestMessage, WireCost,
};
pub use seed::{TestGuild, TestOption, TestUser};
pub use snapshot::Tolerance;

/// What [`assert_render_snapshot!`] expands to; not part of the API.
#[doc(hidden)]
pub mod __private {
    pub use crate::snapshot::render_snapshot;
}
