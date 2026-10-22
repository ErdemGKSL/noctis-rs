//! Tabs (UD-5): a `PayloadEnum` field, a `match` in the render function, and a row of
//! one Route button per tab with the active tab disabled.

// recipe: tabs
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, ButtonStyle, Components, Container, Separator, TextDisplay, V2Message,
};

/// The dashboard's tabs. One character in a Payload: append variants, never reorder.
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashTab {
    /// The first tab.
    Overview,
    /// Recent events.
    Activity,
    /// About the bot.
    About,
}

impl DashTab {
    /// Every tab, in row order (at most 5: one Action Row, CV2-06).
    pub const ALL: [Self; 3] = [Self::Overview, Self::Activity, Self::About];

    /// The tab button's label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Activity => "Activity",
            Self::About => "About",
        }
    }
}

/// A tab button: its Payload is the tab it opens, so the three ids differ (CID-02).
#[derive(Button, Debug, Clone, Copy)]
#[button("tabs.open", defaults(style = Secondary, label = "Overview"))]
pub struct OpenTab {
    /// The tab to show.
    pub tab: DashTab,
}

/// `/dashboard`: no options, so a unit struct. Public: everyone may see the dashboard.
#[derive(SlashCommand, Debug)]
#[command("dashboard", description = "Open the dashboard")]
pub struct Dashboard;

impl Dashboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render_dashboard(DashTab::Overview)).await
    }
}

/// The dashboard on `tab`. Pure: the Command and every tab click call it.
pub fn render_dashboard(tab: DashTab) -> V2Message {
    let body = match tab {
        DashTab::Overview => {
            "Three tabs, one message, no stored state: the tab is a Payload field."
        }
        DashTab::Activity => "-# No activity in the last 24 hours.",
        DashTab::About => "Built with Noctis. Each tab is a `match` arm of one render function.",
    };
    let tabs = ActionRow::buttons(DashTab::ALL.map(|t| {
        OpenTab { tab: t }
            .build()
            .label(t.label())
            .style(if t == tab {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            })
            .disabled(t == tab) // the visible active state
    }));
    let container = Container::new()
        .push(TextDisplay::new("## Dashboard"))
        .push(TextDisplay::new(body))
        .push(Separator::small())
        .push(tabs);
    V2Message::new(Components::new().push(container))
}

impl OpenTab {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(render_dashboard(self.tab)).await
    }
}
// recipe-end: tabs

#[cfg(test)]
mod tests {
    use noctis::rules::Limits;
    use noctis::ui::validate;

    use super::*;

    /// T3: every tab validates, and the three tab ids are distinct (no CID-02).
    #[test]
    fn every_tab_validates() {
        for tab in DashTab::ALL {
            let mut msg = render_dashboard(tab);
            assert!(validate(&mut msg, &Limits::default()).is_ok(), "{tab:?}");
        }
    }
}
