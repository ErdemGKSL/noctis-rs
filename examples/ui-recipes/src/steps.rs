//! Step flow (UD-4): a `step` Payload field, "Step n of m: Title", Back/Cancel/Next,
//! per-step validation with an inline notice, and a Cancel that leaves nothing to click.

// recipe: step-flow
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, RoleMarker, UserMarker};
use noctis::ui::{
    ActionRow, Button, ButtonStyle, Components, Container, Separator, TextDisplay, V2Message,
};

use crate::store::{SetupConfig, SetupStore, missing};

const NEED_CHANNEL: &str = "Pick a log channel first.";
const NEED_ROLE: &str = "Pick a moderator role first.";

/// `/setup`: one admin's private flow, so ephemeral, and only in servers.
#[derive(SlashCommand, Debug)]
#[command("setup", description = "Set up the bot", response = ephemeral, contexts = [Guild])]
pub struct Setup;

impl Setup {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let st = SetupState {
            owner: ctx.user_id(),
            step: SetupStep::Channel,
            log_channel: None,
            mod_role: None,
        };
        ctx.respond(render_setup(st, None)).await
    }
}

/// The steps, in order.
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    /// Pick the log channel.
    Channel,
    /// Pick the moderator role.
    Role,
    /// Review and finish.
    Confirm,
}

impl SetupStep {
    /// The 1-based position, for "Step n of 3".
    pub const fn number(self) -> u8 {
        match self {
            Self::Channel => 1,
            Self::Role => 2,
            Self::Confirm => 3,
        }
    }

    /// The step's title.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Channel => "Log channel",
            Self::Role => "Moderator role",
            Self::Confirm => "Confirm",
        }
    }
}

/// Back and Next: the whole flow state plus the step to show. `owner = owner` is a
/// Route Guard, so only the admin who ran `/setup` can click. The defaults are Next;
/// Back overrides label and style per render.
#[derive(Button, Debug, Clone, Copy, PartialEq, Eq)]
#[button("setup.nav", owner = owner, defaults(style = Primary, label = "Next"))]
pub struct SetupState {
    /// Who may click.
    pub owner: Id<UserMarker>,
    /// The step to show.
    pub step: SetupStep,
    /// Picked on step 1.
    pub log_channel: Option<Id<ChannelMarker>>,
    /// Picked on step 2.
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Step 1's select. It carries the rest of the state; the pick is in `ctx.values()`.
#[derive(ChannelSelectMenu, Debug, Clone, Copy)]
#[channel_select(
    "setup.channel",
    owner = owner,
    defaults(placeholder = "Pick the log channel", channel_types = [GuildText])
)]
pub struct PickChannel {
    /// Who may pick.
    pub owner: Id<UserMarker>,
    /// Kept from step 2, if the user came back.
    pub mod_role: Option<Id<RoleMarker>>,
}

/// Step 2's select.
#[derive(RoleSelectMenu, Debug, Clone, Copy)]
#[role_select("setup.role", owner = owner, defaults(placeholder = "Pick the moderator role"))]
pub struct PickRole {
    /// Who may pick.
    pub owner: Id<UserMarker>,
    /// Kept from step 1.
    pub log_channel: Option<Id<ChannelMarker>>,
}

/// Cancel, on every step: the way out.
#[derive(Button, Debug, Clone, Copy)]
#[button("setup.cancel", owner = owner, defaults(style = Secondary, label = "Cancel"))]
pub struct CancelSetup {
    /// Who may click.
    pub owner: Id<UserMarker>,
}

/// Finish, rendered only when both values are set: its fields are not `Option`s, so
/// the type proves the configuration is complete.
#[derive(Button, Debug, Clone, Copy)]
#[button("setup.finish", owner = owner, defaults(style = Success, label = "Finish"))]
pub struct FinishSetup {
    /// Who may click.
    pub owner: Id<UserMarker>,
    /// The picked channel.
    pub log_channel: Id<ChannelMarker>,
    /// The picked role.
    pub mod_role: Id<RoleMarker>,
}

/// Checks that `st.step` may be shown. A Payload is client input (an old message, a
/// forged id), so the handler checks again whatever the buttons allowed.
///
/// # Errors
///
/// The first incomplete step, with every value kept, and the notice to show on it.
pub fn check_step(st: SetupState) -> Result<SetupState, (SetupState, &'static str)> {
    let past_channel = matches!(st.step, SetupStep::Role | SetupStep::Confirm);
    if past_channel && st.log_channel.is_none() {
        return Err((
            SetupState {
                step: SetupStep::Channel,
                ..st
            },
            NEED_CHANNEL,
        ));
    }
    if st.step == SetupStep::Confirm && st.mod_role.is_none() {
        return Err((
            SetupState {
                step: SetupStep::Role,
                ..st
            },
            NEED_ROLE,
        ));
    }
    Ok(st)
}

/// The flow on `st.step`, with an inline `notice` under the progress line.
pub fn render_setup(st: SetupState, notice: Option<&str>) -> V2Message {
    let step = st.step;
    let mut c = Container::new().push(TextDisplay::new(format!(
        "## Step {} of 3: {}",
        step.number(),
        step.title()
    )));
    if let Some(notice) = notice {
        c = c.push(TextDisplay::new(format!("-# ⚠️ {notice}")));
    }
    c = match step {
        SetupStep::Channel => c
            .push(TextDisplay::new("Where should the bot post its logs?"))
            .push(ActionRow::select(
                PickChannel {
                    owner: st.owner,
                    mod_role: st.mod_role,
                }
                .build()
                .default_channels(st.log_channel), // pre-filled with the current value
            )),
        SetupStep::Role => c
            .push(TextDisplay::new("Which role moderates this server?"))
            .push(ActionRow::select(
                PickRole {
                    owner: st.owner,
                    log_channel: st.log_channel,
                }
                .build()
                .default_roles(st.mod_role),
            )),
        SetupStep::Confirm => c.push(TextDisplay::new(summary(st))),
    };
    V2Message::new(Components::new().push(c.push(Separator::small()).push(nav_row(st))))
}

/// Back (not on step 1), Cancel, and Next or Finish. Next is always enabled: the
/// handler validates the step and answers with an inline notice.
fn nav_row(st: SetupState) -> ActionRow {
    let cancel = Button::from(CancelSetup { owner: st.owner }.build());
    let next = |step| Button::from(SetupState { step, ..st }.build());
    let back = |step| {
        Button::from(
            SetupState { step, ..st }
                .build()
                .label("Back")
                .style(ButtonStyle::Secondary),
        )
    };
    match st.step {
        SetupStep::Channel => ActionRow::buttons([cancel, next(SetupStep::Role)]),
        SetupStep::Role => {
            ActionRow::buttons([back(SetupStep::Channel), cancel, next(SetupStep::Confirm)])
        }
        SetupStep::Confirm => match (st.log_channel, st.mod_role) {
            (Some(log_channel), Some(mod_role)) => {
                let finish = FinishSetup {
                    owner: st.owner,
                    log_channel,
                    mod_role,
                };
                ActionRow::buttons([back(SetupStep::Role), cancel, Button::from(finish.build())])
            }
            _ => ActionRow::buttons([back(SetupStep::Role), cancel]),
        },
    }
}

/// The values picked so far. Mentions render but never ping (`parse: []` by default).
fn summary(st: SetupState) -> String {
    let channel = st
        .log_channel
        .map_or_else(|| "not set".to_owned(), |c| format!("<#{c}>"));
    let role = st
        .mod_role
        .map_or_else(|| "not set".to_owned(), |r| format!("<@&{r}>"));
    format!("**Log channel:** {channel}\n**Moderator role:** {role}")
}

/// A message with one Text Display: nothing left to click.
fn closed(text: String) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text)))
}

impl SetupState {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        match check_step(self) {
            Ok(st) => ctx.update(render_setup(st, None)).await,
            Err((back, notice)) => ctx.update(render_setup(back, Some(notice))).await,
        }
    }
}

impl PickChannel {
    /// Stays on the step so the user sees the pick, then presses Next.
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let st = SetupState {
            owner: self.owner,
            step: SetupStep::Channel,
            log_channel: ctx.values().channels().first().copied(),
            mod_role: self.mod_role,
        };
        ctx.update(render_setup(st, None)).await
    }
}

impl PickRole {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let st = SetupState {
            owner: self.owner,
            step: SetupStep::Role,
            log_channel: self.log_channel,
            mod_role: ctx.values().roles().first().copied(),
        };
        ctx.update(render_setup(st, None)).await
    }
}

impl CancelSetup {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(closed("Setup cancelled.".to_owned())).await
    }
}

impl FinishSetup {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text("Use this in a server.").await;
        };
        let store = ctx
            .state::<SetupStore>()
            .ok_or_else(missing::<SetupStore>)?;
        let config = SetupConfig {
            log_channel: self.log_channel,
            mod_role: self.mod_role,
        };
        store.save(guild, config);
        ctx.update(closed(format!(
            "Setup complete: logs in <#{}>, moderators <@&{}>.",
            config.log_channel, config.mod_role
        )))
        .await
    }
}
// recipe-end: step-flow

#[cfg(test)]
mod tests {
    use noctis::rules::Limits;
    use noctis::ui::validate;

    use super::*;

    const A: Id<UserMarker> = Id::new(40);
    const C: Id<ChannelMarker> = Id::new(20);
    const R: Id<RoleMarker> = Id::new(30);

    fn state(
        step: SetupStep,
        c: Option<Id<ChannelMarker>>,
        r: Option<Id<RoleMarker>>,
    ) -> SetupState {
        SetupState {
            owner: A,
            step,
            log_channel: c,
            mod_role: r,
        }
    }

    /// S9: every target step against every value combination.
    #[test]
    fn check_step_table() {
        use SetupStep::{Channel, Confirm, Role};
        for (c, r) in [
            (None, None),
            (Some(C), None),
            (None, Some(R)),
            (Some(C), Some(R)),
        ] {
            assert_eq!(check_step(state(Channel, c, r)), Ok(state(Channel, c, r)));
            let role = check_step(state(Role, c, r));
            let confirm = check_step(state(Confirm, c, r));
            if c.is_none() {
                assert_eq!(role, Err((state(Channel, c, r), NEED_CHANNEL)));
                assert_eq!(confirm, Err((state(Channel, c, r), NEED_CHANNEL)));
            } else if r.is_none() {
                assert_eq!(role, Ok(state(Role, c, r)));
                assert_eq!(confirm, Err((state(Role, c, r), NEED_ROLE)));
            } else {
                assert_eq!(role, Ok(state(Role, c, r)));
                assert_eq!(confirm, Ok(state(Confirm, c, r)));
            }
        }
    }

    #[test]
    fn every_step_validates() {
        for step in [SetupStep::Channel, SetupStep::Role, SetupStep::Confirm] {
            for notice in [None, Some(NEED_ROLE)] {
                let mut msg = render_setup(state(step, Some(C), Some(R)), notice);
                assert!(validate(&mut msg, &Limits::default()).is_ok(), "{step:?}");
            }
        }
    }
}
