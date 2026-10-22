// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! The setup flow's render functions. Pure: no I/O, so tests snapshot them directly.

use noctis::prelude::*;
use noctis::id::marker::UserMarker;
use noctis::ui::{
    ActionRow, Button, ButtonStyle, Components, Container, ContainerComponent,
    Separator, StaticComponents, StaticContainer, StaticMessage, TextDisplay, V2Message,
};

use super::{CancelSetup, FinishSetup, GuildConfig, PickLogChannel, PickModRole, SetupState, SetupStep};
use crate::strings::Strings;

const ACCENT: Rgb = rgb!(0x5865_F2);
const DONE: Rgb = rgb!(0x57_F287);

/// Renders the step in `st.step`: progress line, the step's input, then the navigation
/// row. Next stays disabled until the step's value is set (per-step validation).
pub fn render_setup(st: SetupState, s: &Strings) -> V2Message {
    let title = match st.step {
        SetupStep::Channel => s.setup.step.channel(),
        SetupStep::Role => s.setup.step.roles(),
        SetupStep::Confirm => s.setup.step.confirm(),
    };
    // "Step n of m: Title", never "page 2 of 3" (UD-4).
    let progress = TextDisplay::new(format!(
        "## {}",
        s.setup.progress(st.step.number(), SetupStep::ALL.len(), title)
    ));

    let body: Vec<ContainerComponent> = match st.step {
        SetupStep::Channel => vec![
            TextDisplay::new(s.setup.channel_help()).into(),
            ActionRow::select(
                // Placeholder and channel types come from the declared defaults.
                PickLogChannel { owner: st.owner, mod_role: st.mod_role }
                    .build()
                    .default_values(st.log_channel), // pre-filled with the current value
            )
            .into(),
        ],
        SetupStep::Role => vec![
            TextDisplay::new(s.setup.role_help()).into(),
            ActionRow::select(
                PickModRole { owner: st.owner, log_channel: st.log_channel }
                    .build()
                    .default_values(st.mod_role),
            )
            .into(),
        ],
        SetupStep::Confirm => vec![TextDisplay::new(summary(s, st)).into()],
    };

    let container = body.into_iter().fold(
        Container::new().accent(ACCENT).push(progress).push(Separator::small()),
        Container::push,
    );
    let container = container.push(Separator::small()).push(nav_row(st, s));
    V2Message::new(Components::new().push(container))
}

/// Back (not on the first step), Cancel, and Next or Finish. One primary action.
fn nav_row(st: SetupState, s: &Strings) -> ActionRow {
    // Defaults (label, style) come from each component's `#[button(.., defaults(..))]`;
    // Back reuses the `SetupState` button and overrides its label and style.
    let cancel = Button::from(CancelSetup { owner: st.owner }.build());
    let back = |step| {
        Button::from(
            SetupState { step, ..st }
                .build()
                .label(s.setup.back())
                .style(ButtonStyle::Secondary),
        )
    };

    match st.step {
        SetupStep::Channel => ActionRow::buttons([
            cancel,
            Button::from(
                SetupState { step: SetupStep::Role, ..st }
                    .build()
                    .disabled(st.log_channel.is_none()),
            ),
        ]),
        SetupStep::Role => ActionRow::buttons([
            back(SetupStep::Channel),
            cancel,
            Button::from(
                SetupState { step: SetupStep::Confirm, ..st }
                    .build()
                    .disabled(st.mod_role.is_none()),
            ),
        ]),
        SetupStep::Confirm => {
            let back = back(SetupStep::Role);
            match (st.log_channel, st.mod_role) {
                (Some(log_channel), Some(mod_role)) => ActionRow::buttons([
                    back,
                    cancel,
                    Button::from(FinishSetup { owner: st.owner, log_channel, mod_role }.build()),
                ]),
                _ => ActionRow::buttons([back, cancel]),
            }
        }
    }
}

fn summary(s: &Strings, st: SetupState) -> String {
    let channel = st.log_channel.map_or_else(|| s.setup.unset(), |c| format!("<#{c}>"));
    let role = st.mod_role.map_or_else(|| s.setup.unset(), |r| format!("<@&{r}>"));
    // Mentions render but never ping: messages default to `allowed_mentions.parse = []`.
    s.setup.summary(channel, role)
}

/// The final, non-interactive state.
pub fn render_setup_done(config: GuildConfig, s: &Strings) -> V2Message {
    let text = s.setup.done(
        format!("<#{}>", config.log_channel),
        format!("<@&{}>", config.mod_role),
    );
    V2Message::new(Components::new().push(Container::new().accent(DONE).push(TextDisplay::new(text))))
}

/// The line Finish posts to the log channel through its webhook: static, so it would
/// fit a webhook the bot does not own too (WH-02).
pub fn render_setup_log(config: GuildConfig, by: Id<UserMarker>, s: &Strings) -> StaticMessage {
    let text = s.setup.log_enabled(
        format!("<@{by}>"),
        format!("<#{}>", config.log_channel),
        format!("<@&{}>", config.mod_role),
    );
    StaticMessage::new(
        StaticComponents::new().push(StaticContainer::new().accent(DONE).push(TextDisplay::new(text))),
    )
}

/// After Cancel: the same message, with no components left to click.
pub fn render_setup_cancelled(s: &Strings) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(s.setup.cancelled())))
}
