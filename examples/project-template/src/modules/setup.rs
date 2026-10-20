// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! `/setup start`: a three-Step Wizard (UD-4, FR-27).
//!
//! Every value the user picks travels in Payloads, so there is no listener, collector or
//! per-message state (UD-7). Back, Next and Cancel are handled by core on the View's own
//! Route (AD-20); this file only declares the Steps, their validation and the author
//! actions (two selects and Finish).

use noctis::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use noctis::BaseMeta;
use noctis::prelude::*;
use noctis::ui::{ActionRow, ButtonStyle, ContainerComponent, TextDisplay};
use serde::{Deserialize, Serialize};

use crate::strings::{Strings, keys};

const ACCENT: Rgb = rgb!(0x5865_F2);

/// The Module. `.base` declares the description of the `/setup` base command (FR-57),
/// equal to `cmd.setup.description` in `locales/en-US.yaml`; without it `App::build`
/// would borrow `setup start`'s description and log a warning.
pub fn module() -> Module {
    Module::new("setup")
        .base("setup", BaseMeta::new().description("Set up the bot for this server"))
        .add(setup_start())
        .add(open_setup())
        .add(pick_log_channel())
        .add(pick_mod_role())
        .add(finish_setup())
}

// ---------------------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------------------

/// The Wizard's own View Route. Its Payload is the whole Wizard state.
///
/// Width (CID-01, proven by the derive): owner 11 + log_channel 12 + mod_role 12
/// + 2 separators = 37; `custom_id` ≤ 1 + 5 ("setup") + 1 + 5 (view envelope) + 37 = 49.
/// `owner = owner` is a Route Guard: only the invoking admin can click (FR-19).
///
/// Width (`noctis_route_width`): `setup: F 1 + route 5 + sep 1 + S 0 + E 5 + N 0 + body 37 + T 0 = 49 (limit 100)`.
#[derive(Route, Debug, Clone, Copy, PartialEq, Eq)]
#[route("setup", view, owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct SetupState {
    pub owner: Id<UserMarker>,
    pub log_channel: Option<Id<ChannelMarker>>,
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Channel Select on Step 1. Carries the rest of the state; the picked channel arrives in
/// `ctx.values()`. `select` reserves the per-render nonce so the client always shows the
/// server's default after a re-render (FR-25).
///
/// Width (`noctis_route_width`): `setup.ch: F 1 + route 8 + sep 1 + S 0 + E 0 + N 2 + body 24 + T 0 = 36 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.ch", select, owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct PickLogChannel {
    pub owner: Id<UserMarker>,
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Role Select on Step 2.
///
/// Width (`noctis_route_width`): `setup.role: F 1 + route 10 + sep 1 + S 0 + E 0 + N 2 + body 24 + T 0 = 38 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.role", select, owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct PickModRole {
    pub owner: Id<UserMarker>,
    pub log_channel: Option<Id<ChannelMarker>>,
}

/// Finish button on Step 3. Only rendered once both values are set, so its fields are not
/// `Option`s: the type proves the configuration is complete.
///
/// Width (`noctis_route_width`): `setup.done: F 1 + route 10 + sep 1 + S 0 + E 0 + N 0 + body 35 + T 0 = 47 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("setup.done", owner = owner)] // [planned]: runtime arguments are rejected until their release
pub struct FinishSetup {
    pub owner: Id<UserMarker>,
    pub log_channel: Id<ChannelMarker>,
    pub mod_role: Id<RoleMarker>,
}

/// What the Wizard saves. Stored through the `CacheStore` port here; a real bot keeps this
/// in its own database behind a port or `State<T>`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GuildConfig {
    pub log_channel: Id<ChannelMarker>,
    pub mod_role: Id<RoleMarker>,
}

pub fn config_key(guild: Id<GuildMarker>) -> String {
    format!("guildcfg:{guild}")
}

// ---------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------

/// Entry point (UD-3): no options, returns a View, no response code (FR-61).
/// `ephemeral`: a single-admin flow, so it is private and uses the User Locale.
#[command(
    "setup start",
    description = "Configure the bot for this server, step by step",
    ephemeral,                  // [planned]: rejected until its release
    permissions = MANAGE_GUILD,
    contexts = [Guild],
)]
async fn setup_start(ctx: CommandCtx) -> HandlerResult<SetupWizard> {
    Ok(SetupWizard::new(SetupState {
        owner: ctx.user_id(),
        log_channel: None,
        mod_role: None,
    }))
}

/// Loader for the View Route. Core calls it for every click on the Route, including
/// Back and Next, after it has moved `ctx.nav()`; it only rebuilds the View.
/// (Cancel never reaches it: core edits the message to the cancelled state.)
#[handler]
async fn open_setup(_ctx: ComponentCtx, state: SetupState) -> HandlerResult<SetupWizard> {
    Ok(SetupWizard::new(state))
}

#[handler]
async fn pick_log_channel(
    ctx: ComponentCtx,
    picked: PickLogChannel,
) -> HandlerResult<impl IntoResponse> {
    let state = SetupState {
        owner: picked.owner,
        log_channel: ctx.values().channels().first().copied(),
        mod_role: picked.mod_role,
    };
    // Stay on Step 1 so the user sees the choice, then presses Next.
    Ok(SetupWizard::new(state).at(Nav::step(0)))
}

#[handler]
async fn pick_mod_role(ctx: ComponentCtx, picked: PickModRole) -> HandlerResult<impl IntoResponse> {
    let state = SetupState {
        owner: picked.owner,
        log_channel: picked.log_channel,
        mod_role: ctx.values().roles().first().copied(),
    };
    Ok(SetupWizard::new(state).at(Nav::step(1)))
}

#[handler]
async fn finish_setup(ctx: ComponentCtx, done: FinishSetup) -> HandlerResult<SetupDone> {
    let guild = ctx
        .guild_id()
        .ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))?;
    let config = GuildConfig {
        log_channel: done.log_channel,
        mod_role: done.mod_role,
    };
    ctx.kv().put(&config_key(guild), &config, None).await?;
    Ok(SetupDone { config })
}

// ---------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------

/// The Wizard. Pure: `render` does no I/O, so it is snapshot-testable.
pub struct SetupWizard {
    state: SetupState,
}

impl SetupWizard {
    pub fn new(state: SetupState) -> Self {
        Self { state }
    }
}

impl View for SetupWizard {
    type Route = SetupState;

    fn state(&self) -> &SetupState {
        &self.state
    }

    fn render(&self, cx: &mut ViewCx<'_, SetupState>) -> UiResult<V2Message> {
        let s = Strings::of(cx.locale());
        let st = self.state;

        // "Step n of m: Title" is rendered by the widget (UD-4). A failed `require`
        // keeps the user on that Step and shows the message inline (FR-27).
        let wizard = Wizard::new(
            cx,
            [
                s.setup.step.channel(),
                s.setup.step.roles(),
                s.setup.step.confirm(),
            ],
        )
        .require(0, st.log_channel.is_some(), s.setup.need_channel())
        .require(1, st.mod_role.is_some(), s.setup.need_role());

        let body: Vec<ContainerComponent> = match wizard.step() {
            0 => vec![
                TextDisplay::new(s.setup.channel_help()).into(),
                ActionRow::select(
                    PickLogChannel {
                        owner: st.owner,
                        mod_role: st.mod_role,
                    }
                    .channel_select()
                    .channel_types([ChannelType::GuildText])
                    .placeholder(s.setup.channel_placeholder())
                    .default_channels(st.log_channel),
                )
                .into(),
            ],
            1 => vec![
                TextDisplay::new(s.setup.role_help()).into(),
                ActionRow::select(
                    PickModRole {
                        owner: st.owner,
                        log_channel: st.log_channel,
                    }
                    .role_select()
                    .placeholder(s.setup.role_placeholder())
                    .default_roles(st.mod_role),
                )
                .into(),
            ],
            _ => vec![TextDisplay::new(summary(&s, st)).into()],
        };

        // Finish exists only when the state is complete; the Wizard puts it where Next
        // would be on the last Step (one primary action per View).
        let wizard = match (st.log_channel, st.mod_role) {
            (Some(log_channel), Some(mod_role)) => wizard.finish(
                FinishSetup {
                    owner: st.owner,
                    log_channel,
                    mod_role,
                }
                .button()
                .label(s.setup.finish())
                .style(ButtonStyle::Success),
            ),
            _ => wizard,
        };

        Layout::new().accent(ACCENT).wizard(wizard).body(body).build(cx)
    }
}

fn summary(s: &Strings, st: SetupState) -> String {
    let channel = st.log_channel.map_or_else(|| s.setup.unset(), |c| format!("<#{c}>"));
    let role = st.mod_role.map_or_else(|| s.setup.unset(), |r| format!("<@&{r}>"));
    // Mentions render but never ping: Views default to `allowed_mentions.parse = []`.
    s.setup.summary(channel, role)
}

/// Final, non-interactive state. A `StaticView` has no Route and no controls.
pub struct SetupDone {
    config: GuildConfig,
}

impl StaticView for SetupDone {
    fn render(&self, cx: &mut StaticCx<'_>) -> UiResult<V2Message> {
        let s = Strings::of(cx.locale());
        let text = s.setup.done(
            format!("<#{}>", self.config.log_channel),
            format!("<@&{}>", self.config.mod_role),
        );
        Ok(V2Message::new(components! {
            container(accent = rgb!(0x57_F287)) [ text(text) ],
        }))
    }
}
