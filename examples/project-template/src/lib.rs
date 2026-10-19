// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! A starter Noctis bot:
//!
//! - `setup` — `/setup start` opens a three-Step Wizard (UD-4).
//! - `dashboard` — `/dashboard` opens a Navbar View whose Roles tab paginates (UD-5, UD-6).
//! - `profile` — `/profile` and the "Show profile" user command render a card image into
//!   a Media Gallery (UD-2), with a modal to edit the bio.
//!
//! [`configure`] wires every Module, Adapter and piece of state onto an [`AppBuilder`];
//! `main.rs` and the tests share it.

pub mod adapters;
pub mod modules;
pub mod strings;

use std::time::SystemTime;

use noctis::prelude::*;

/// Read-only facts the dashboard shows. Registered once with `.state(..)` and
/// extracted in handlers with `State<BotMeta>` (AD-9).
#[derive(Debug, Clone)]
pub struct BotMeta {
    pub started_at: SystemTime,
    pub version: &'static str,
}

/// Mount every Module and Adapter. Token, shards and keys come from the builder the
/// caller passes in (`App::from_env()` in production, `TestApp::builder()` in tests).
pub fn configure(builder: AppBuilder) -> AppBuilder {
    builder
        .app_id("noctis-template")
        .locales(strings::catalog())
        // Our own CacheStore Adapter replaces the in-memory default (FR-38).
        .cache_store(adapters::BoundedCacheStore::new(50_000))
        .state(BotMeta {
            started_at: SystemTime::now(),
            version: env!("CARGO_PKG_VERSION"),
        })
        .module(modules::setup::module())
        .module(modules::dashboard::module())
        .module(modules::profile::module())
}
