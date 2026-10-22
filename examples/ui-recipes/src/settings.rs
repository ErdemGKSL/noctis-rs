//! Settings screen (re-editable config, schwi-style): an entity switcher, Sections with
//! accessory buttons, a pre-filled select, a toggle, add and delete rows, a one-shot
//! delete confirmation, and a button → modal → re-render chain on the same Payload.

// recipe: settings
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Button, ButtonStyle, Components, Container, Section, SelectOption, Separator,
    TextDisplay, V2Message, escape_markdown,
};

use crate::store::{MAX_SYSTEMS, TicketStore, TicketSystem, missing};

const GUILD_ONLY: &str = "Use this in a server.";
const NOT_FOUND: &str = "That ticket system no longer exists.";
const AT_LIMIT: &str = "You can have at most 25 ticket systems.";

/// `/tickets`: an admin's private screen, so ephemeral, and only in servers.
#[derive(SlashCommand, Debug)]
#[command(
    "tickets",
    description = "Configure ticket systems",
    response = ephemeral,
    permissions = MANAGE_GUILD,
    contexts = [Guild]
)]
pub struct Tickets;

impl Tickets {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        ctx.respond(render_tickets(&store.systems(guild), None))
            .await
    }
}

// Every component below carries the id of the system it edits: the screen's whole state.

/// The entity switcher; its options come per render, the pick is in `ctx.values()`.
#[derive(StringSelectMenu, Debug, Clone, Copy)]
#[string_select("tickets.pick", defaults(placeholder = "Pick a ticket system"))]
pub struct PickSystem;

/// Opens [`RenameForm`]. A button that opens a modal is `auto_defer = false`.
#[derive(Button, Debug, Clone, Copy)]
#[button("tickets.name", auto_defer = false, defaults(style = Secondary, label = "Rename"))]
pub struct RenameSystem {
    /// The system to rename.
    pub system: u32,
}

/// The rename modal: the same Payload as [`RenameSystem`], plus the input.
#[derive(Modal, Debug)]
#[modal("tickets.rename", defaults(title = "Rename ticket system"))]
pub struct RenameForm {
    /// The system to rename.
    pub system: u32,
    /// The new name.
    #[text_input("Name", max_length = 40)]
    pub name: String,
}

/// The roles that handle the system's tickets, pre-filled with the current ones.
#[derive(RoleSelectMenu, Debug, Clone, Copy)]
#[role_select(
    "tickets.roles",
    defaults(
        placeholder = "Pick the roles that handle tickets",
        min_values = 0,
        max_values = 5
    )
)]
pub struct SetRoles {
    /// The system to edit.
    pub system: u32,
}

/// The toggle: its Payload is the action (`enable`), not the current state.
#[derive(Button, Debug, Clone, Copy)]
#[button("tickets.toggle", defaults(style = Success, label = "Enable"))]
pub struct ToggleSystem {
    /// The system to edit.
    pub system: u32,
    /// What a click sets.
    pub enable: bool,
}

/// Adds a system and switches to it.
#[derive(Button, Debug, Clone, Copy)]
#[button("tickets.add", defaults(style = Secondary, label = "Add"))]
pub struct AddSystem;

/// Delete: never on the first click; it opens the confirm screen.
#[derive(Button, Debug, Clone, Copy)]
#[button("tickets.delete", defaults(style = Danger, label = "Delete"))]
pub struct AskDelete {
    /// The system to delete.
    pub system: u32,
}

/// Confirm: stored by Ref for 15 minutes and consumed by the first click, so a second
/// click (or one after 15 minutes) gets the expired notice.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "tickets.delete.ok",
    by_ref,
    one_shot,
    ttl = "15m",
    defaults(style = Danger, label = "Confirm")
)]
pub struct ConfirmDelete {
    /// The system to delete.
    pub system: u32,
}

/// Cancel on the confirm screen: back to the settings of the same system.
#[derive(Button, Debug, Clone, Copy)]
#[button("tickets.back", defaults(style = Secondary, label = "Cancel"))]
pub struct BackToSettings {
    /// The system to show.
    pub system: u32,
}

/// The settings of `selected` (when it is one of `systems`, else the first system).
pub fn render_tickets(systems: &[TicketSystem], selected: Option<u32>) -> V2Message {
    let current = selected
        .and_then(|id| systems.iter().find(|t| t.id == id))
        .or(systems.first());
    let mut c = Container::new().push(TextDisplay::new("# Ticket systems"));
    // The switcher, only when there is something to switch between.
    if systems.len() > 1 {
        let options = systems.iter().map(|t| {
            SelectOption::new(t.name.clone(), t.id.to_string())
                .default(current.is_some_and(|cur| cur.id == t.id))
        });
        c = c.push(ActionRow::select(PickSystem.build().options(options)));
    }
    if let Some(t) = current {
        let (state, action, style) = if t.enabled {
            ("on", "Disable", ButtonStyle::Danger)
        } else {
            ("off", "Enable", ButtonStyle::Success)
        };
        c = c
            .push(Separator::small())
            .push(Section::new(
                [TextDisplay::new(format!(
                    "**{}**",
                    escape_markdown(&t.name)
                ))],
                RenameSystem { system: t.id }.build(),
            ))
            .push(TextDisplay::new("-# Roles that handle its tickets"))
            .push(ActionRow::select(
                SetRoles { system: t.id }
                    .build()
                    .default_roles(t.roles.iter().copied()),
            ))
            .push(Separator::small())
            .push(Section::new(
                [TextDisplay::new(format!("**Enabled** · {state}"))],
                ToggleSystem {
                    system: t.id,
                    enable: !t.enabled,
                }
                .build()
                .label(action)
                .style(style),
            ))
            .push(Separator::small())
            .push(Section::new(
                [TextDisplay::new("Delete this ticket system")],
                AskDelete { system: t.id }.build(),
            ));
    } else {
        c = c.push(TextDisplay::new("-# No ticket systems yet.")); // the empty state
    }
    if systems.len() < MAX_SYSTEMS {
        // The one primary action of the empty state.
        let style = if systems.is_empty() {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Secondary
        };
        c = c.push(Separator::small()).push(Section::new(
            [TextDisplay::new("Add a ticket system")],
            AddSystem.build().style(style),
        ));
    }
    V2Message::new(Components::new().push(c))
}

/// The confirm screen of a destructive action: Cancel and a one-shot Confirm.
pub fn render_confirm_delete(system: &TicketSystem) -> V2Message {
    let c = Container::new()
        .push(TextDisplay::new(format!(
            "Delete **{}**? This can't be undone.",
            escape_markdown(&system.name)
        )))
        .push(ActionRow::buttons([
            Button::from(BackToSettings { system: system.id }.build()),
            Button::from(ConfirmDelete { system: system.id }.build()),
        ]));
    V2Message::new(Components::new().push(c))
}

impl PickSystem {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        let systems = store.systems(guild);
        // Select values are client input: parse, never cast, and check the id.
        let picked = ctx
            .values()
            .strings()
            .first()
            .and_then(|v| v.parse::<u32>().ok());
        let Some(id) = picked.filter(|id| systems.iter().any(|t| t.id == *id)) else {
            return ctx.reply_text(NOT_FOUND).await;
        };
        ctx.update(render_tickets(&systems, Some(id))).await
    }
}

impl RenameSystem {
    /// Button → modal, pre-filled with the stored name: every input of `RenameForm`
    /// has an empty value, so `blank` returns a `RenameForm` to update.
    #[handler]
    async fn handle(
        self,
        ctx: ComponentCtx,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        let name = match (ctx.guild_id(), ctx.state::<TicketStore>()) {
            (Some(guild), Some(store)) => store
                .systems(guild)
                .into_iter()
                .find(|t| t.id == self.system)
                .map(|t| t.name)
                .unwrap_or_default(),
            _ => String::new(),
        };
        let form = RenameForm {
            name,
            ..RenameForm::blank(self.system)
        };
        Ok(r.show_modal(form).await?.done())
    }
}

impl RenameForm {
    /// Modal → re-render the message the button was on.
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        let name = self.name.trim();
        if name.is_empty() {
            return ctx.reply_text("The name can't be empty.").await;
        }
        if !store.rename(guild, self.system, name) {
            return ctx.reply_text(NOT_FOUND).await;
        }
        ctx.update(render_tickets(&store.systems(guild), Some(self.system)))
            .await
    }
}

impl SetRoles {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        if !store.set_roles(guild, self.system, ctx.values().roles()) {
            return ctx.reply_text(NOT_FOUND).await;
        }
        ctx.update(render_tickets(&store.systems(guild), Some(self.system)))
            .await
    }
}

impl ToggleSystem {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        if !store.set_enabled(guild, self.system, self.enable) {
            return ctx.reply_text(NOT_FOUND).await;
        }
        ctx.update(render_tickets(&store.systems(guild), Some(self.system)))
            .await
    }
}

impl AddSystem {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        let Some(id) = store.add(guild) else {
            return ctx.reply_text(AT_LIMIT).await;
        };
        ctx.update(render_tickets(&store.systems(guild), Some(id)))
            .await
    }
}

impl AskDelete {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        let systems = store.systems(guild);
        let Some(system) = systems.iter().find(|t| t.id == self.system) else {
            return ctx.reply_text(NOT_FOUND).await;
        };
        ctx.update(render_confirm_delete(system)).await
    }
}

impl ConfirmDelete {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        if !store.delete(guild, self.system) {
            return ctx.reply_text(NOT_FOUND).await;
        }
        ctx.update(render_tickets(&store.systems(guild), None))
            .await
    }
}

impl BackToSettings {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(GUILD_ONLY).await;
        };
        let store = ctx
            .state::<TicketStore>()
            .ok_or_else(missing::<TicketStore>)?;
        let systems = store.systems(guild);
        if !systems.iter().any(|t| t.id == self.system) {
            return ctx.reply_text(NOT_FOUND).await;
        }
        ctx.update(render_tickets(&systems, Some(self.system)))
            .await
    }
}
// recipe-end: settings

#[cfg(test)]
mod tests {
    use noctis::rules::Limits;
    use noctis::twilight::model::id::Id;
    use noctis::ui::validate;

    use super::*;

    fn system(id: u32) -> TicketSystem {
        TicketSystem {
            id,
            name: format!("*System* {id}"),
            enabled: id.is_multiple_of(2),
            roles: vec![Id::new(30)],
        }
    }

    #[test]
    fn every_screen_validates() {
        let limits = Limits::default();
        for n in [0_u32, 1, 2, 24, 25] {
            let systems: Vec<TicketSystem> = (1..=n).map(system).collect();
            let mut msg = render_tickets(&systems, Some(n));
            assert!(validate(&mut msg, &limits).is_ok(), "{n} systems");
        }
        let mut confirm = render_confirm_delete(&system(1));
        assert!(validate(&mut confirm, &limits).is_ok());
    }
}
