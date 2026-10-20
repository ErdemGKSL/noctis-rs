// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! `/setup start`: a hand-written three-step setup flow (UD-4).
//!
//! The current step and every value picked so far travel in the Payloads of this
//! module's Routes, so there is no listener, collector or per-message state (UD-7).
//! `view.rs` renders a step from that state; `handlers.rs` decodes a click, checks it,
//! and redraws with `ctx.update(render_setup(..))`.

mod handlers;
pub mod view;

use noctis::BaseMeta;
use noctis::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use noctis::prelude::*;
use serde::{Deserialize, Serialize};

/// The Module. `.base` declares the description of the `/setup` base command (FR-57),
/// equal to `cmd.setup.description` in `locales/en-US.yaml`.
pub fn module() -> Module {
    Module::new("setup")
        .base("setup", BaseMeta::new().description("Set up the bot for this server"))
        .add(handlers::setup_start())
        .add(handlers::go_to_step())
        .add(handlers::pick_log_channel())
        .add(handlers::pick_mod_role())
        .add(handlers::cancel_setup())
        .add(handlers::finish_setup())
}

// ---------------------------------------------------------------------------------------
// State and Routes
// ---------------------------------------------------------------------------------------

/// The steps, in order. One character in a Payload.
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Channel,
    Role,
    Confirm,
}

impl SetupStep {
    pub const ALL: [Self; 3] = [Self::Channel, Self::Role, Self::Confirm];

    /// 1-based position for "Step n of 3".
    pub fn number(self) -> usize {
        self as usize + 1
    }
}

/// Back and Next: the whole flow state, with the step to show. `owner = owner` is a Route
/// Guard: only the admin who ran the command can click (FR-19).
///
/// Width (`noctis_route_width`): `setup: F 1 + route 5 + sep 1 + S 0 + N 0 + body 39 + T 0 = 46 (limit 100)`.
#[derive(Route, Debug, Clone, Copy, PartialEq, Eq)]
#[route("setup", owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct SetupState {
    pub owner: Id<UserMarker>,
    pub step: SetupStep,
    pub log_channel: Option<Id<ChannelMarker>>,
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Channel Select on the first step. Carries the rest of the state; the picked channel
/// arrives in `ctx.values()`. `select` adds the per-render nonce so the client always
/// shows the server's default after a re-render (FR-25).
///
/// Width (`noctis_route_width`): `setup.ch: F 1 + route 8 + sep 1 + S 0 + N 2 + body 24 + T 0 = 36 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.ch", select, owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct PickLogChannel {
    pub owner: Id<UserMarker>,
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Role Select on the second step.
///
/// Width (`noctis_route_width`): `setup.role: F 1 + route 10 + sep 1 + S 0 + N 2 + body 24 + T 0 = 38 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.role", select, owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct PickModRole {
    pub owner: Id<UserMarker>,
    pub log_channel: Option<Id<ChannelMarker>>,
}

/// Cancel, on every step.
///
/// Width (`noctis_route_width`): `setup.cancel: F 1 + route 12 + sep 1 + S 0 + N 0 + body 11 + T 0 = 25 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.cancel", owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct CancelSetup {
    pub owner: Id<UserMarker>,
}

/// Finish on the last step. Only rendered once both values are set, so its fields are
/// not `Option`s: the type proves the configuration is complete.
///
/// Width (`noctis_route_width`): `setup.done: F 1 + route 10 + sep 1 + S 0 + N 0 + body 35 + T 0 = 47 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.done", owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct FinishSetup {
    pub owner: Id<UserMarker>,
    pub log_channel: Id<ChannelMarker>,
    pub mod_role: Id<RoleMarker>,
}

// ---------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------

/// What the flow saves. Stored through the `CacheStore` port here; a real bot keeps this
/// in its own database behind a port or `State<T>`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GuildConfig {
    pub log_channel: Id<ChannelMarker>,
    pub mod_role: Id<RoleMarker>,
}

pub fn config_key(guild: Id<GuildMarker>) -> String {
    format!("guildcfg:{guild}")
}
