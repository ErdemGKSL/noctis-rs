//! `/setup start`: a hand-written three-step setup flow (UD-4).
//!
//! The current step and every value picked so far travel in the Payloads of this
//! module's Routes, so there is no listener, collector or per-message state (UD-7).
//! `view.rs` renders a step from that state; `handlers.rs` decodes a click, checks it,
//! and redraws with `ctx.update(render_setup(..))`.

mod handlers;
pub mod view;

use noctis::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker, WebhookMarker};
use noctis::prelude::*;
use serde::{Deserialize, Serialize};

use crate::strings::keys;

/// The Module. The `/setup` base command takes its description from
/// `cmd.setup.description` in `locales/en-US.yaml`, so it needs no `.base(..)`.
pub fn module() -> Module {
    Module::new("setup")
        .add::<handlers::SetupStart>()
        .add::<SetupState>()
        .add::<PickLogChannel>()
        .add::<PickModRole>()
        .add::<CancelSetup>()
        .add::<FinishSetup>()
}

// ---------------------------------------------------------------------------------------
// State and components (their `#[handler]` impls are in `handlers.rs`)
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
/// Guard: only the admin who ran the command can click (FR-19). The defaults are the
/// Next button; Back overrides label and style per render.
///
/// Width (`noctis_route_width`): `setup: F 1 + route 5 + sep 1 + S 0 + N 0 + body 39 + T 0 = 46 (limit 100)`.
#[derive(Button, Debug, Clone, Copy, PartialEq, Eq)]
#[button(
    "setup",
    owner = owner,
    defaults(style = Primary, label = keys::SETUP_NEXT),
)]
pub struct SetupState {
    pub owner: Id<UserMarker>,
    pub step: SetupStep,
    pub log_channel: Option<Id<ChannelMarker>>,
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Channel Select on the first step. Carries the rest of the state; the picked channel
/// arrives in `ctx.values()`. Every select carries the per-render nonce (`N`), so the
/// client always shows the server's default after a re-render (FR-25).
///
/// Width (`noctis_route_width`): `setup.ch: F 1 + route 8 + sep 1 + S 0 + N 2 + body 24 + T 0 = 36 (limit 100)`.
#[derive(ChannelSelectMenu, Debug, Clone, Copy)]
#[channel_select(
    "setup.ch",
    owner = owner,
    defaults(placeholder = keys::SETUP_CHANNEL_PLACEHOLDER, channel_types = [GuildText]),
)]
pub struct PickLogChannel {
    pub owner: Id<UserMarker>,
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Role Select on the second step.
///
/// Width (`noctis_route_width`): `setup.role: F 1 + route 10 + sep 1 + S 0 + N 2 + body 24 + T 0 = 38 (limit 100)`.
#[derive(RoleSelectMenu, Debug, Clone, Copy)]
#[role_select(
    "setup.role",
    owner = owner,
    defaults(placeholder = keys::SETUP_ROLE_PLACEHOLDER),
)]
pub struct PickModRole {
    pub owner: Id<UserMarker>,
    pub log_channel: Option<Id<ChannelMarker>>,
}

/// Cancel, on every step.
///
/// Width (`noctis_route_width`): `setup.cancel: F 1 + route 12 + sep 1 + S 0 + N 0 + body 11 + T 0 = 25 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "setup.cancel",
    owner = owner,
    defaults(style = Secondary, label = keys::SETUP_CANCEL),
)]
pub struct CancelSetup {
    pub owner: Id<UserMarker>,
}

/// Finish on the last step. Only rendered once both values are set, so its fields are
/// not `Option`s: the type proves the configuration is complete. A confirmation: its
/// Payload is stored by Ref for 15 minutes and consumed by the first click, so a second
/// click (or one after 15 minutes) gets the expired notice.
///
/// Width (`noctis_route_width`): `setup.done: F 1 + route 10 + sep 1 + S 0 + N 0 + ref 12 = 24 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "setup.done",
    by_ref,
    one_shot,
    ttl = "15m",
    owner = owner,
    defaults(style = Success, label = keys::SETUP_FINISH),
)]
pub struct FinishSetup {
    pub owner: Id<UserMarker>,
    pub log_channel: Id<ChannelMarker>,
    pub mod_role: Id<RoleMarker>,
}

// ---------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------

/// What the flow saves. Stored through the `CacheStore` port here; a real bot keeps this
/// in its own database behind a port or `State<T>`. Ids in a `Kv` value are plain `u64`s
/// (`Id::get()`): twilight's `Id` does not decode from the postcard bytes `Kv` stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildConfig {
    pub log_channel: u64,
    pub mod_role: u64,
}

impl GuildConfig {
    pub fn new(log_channel: Id<ChannelMarker>, mod_role: Id<RoleMarker>) -> Self {
        Self {
            log_channel: log_channel.get(),
            mod_role: mod_role.get(),
        }
    }
}

pub fn config_key(guild: Id<GuildMarker>) -> String {
    format!("guildcfg:{guild}")
}

/// The log channel's webhook, created by Finish (FR-65): an owned webhook, so a later
/// log may carry buttons. The token is a secret: store it like a password and never log
/// or show it. `channel` is the channel it posts in: a new log channel needs a new
/// webhook. Ids are `u64`s, as in [`GuildConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWebhook {
    pub id: u64,
    pub token: String,
    pub channel: u64,
}

impl StoredWebhook {
    /// The webhook and its channel; `None` when either stored id is `0` (no valid
    /// webhook stored), so the caller creates a new one.
    pub fn ids(&self) -> Option<(Id<WebhookMarker>, Id<ChannelMarker>)> {
        Some((Id::new_checked(self.id)?, Id::new_checked(self.channel)?))
    }
}

pub fn log_webhook_key(guild: Id<GuildMarker>) -> String {
    format!("logwebhook:{guild}")
}

#[cfg(test)]
mod tests {
    use noctis::ports::{decode_value, encode_value};

    use super::*;

    fn round_trip<T>(value: &T) -> Option<T>
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        decode_value(0, &encode_value(0, value).unwrap()).unwrap()
    }

    #[test]
    fn stored_setup_values_round_trip_through_the_kv_codec() {
        let config = GuildConfig {
            log_channel: 300,
            mod_role: 400,
        };
        assert_eq!(round_trip(&config), Some(config));
        for token in [String::from("t"), String::new(), "ş".repeat(200)] {
            let hook = StoredWebhook {
                id: 900,
                token,
                channel: 300,
            };
            assert_eq!(round_trip(&hook), Some(hook.clone()));
        }
    }

    #[test]
    fn stored_webhook_with_a_zero_id_is_no_webhook() {
        let hook = StoredWebhook {
            id: 0,
            token: "t".into(),
            channel: 300,
        };
        assert_eq!(hook.ids(), None);
        let hook = StoredWebhook {
            id: 900,
            token: "t".into(),
            channel: 0,
        };
        assert_eq!(hook.ids(), None);
        let hook = StoredWebhook {
            id: 900,
            token: "t".into(),
            channel: 300,
        };
        assert_eq!(hook.ids(), Some((Id::new(900), Id::new(300))));
    }
}
