# Views and UI recipes

Covers: what a View is in Noctis (a plain render function), the update flow
(`ctx.respond` / `ctx.update` / `ctx.reply_text`), the budget helpers, attachments,
mentions and user text, select freshness, the UI recipes with complete code (tabs,
pagination, step flow, settings screen), the UI quality rulebook, and the best
practices (and pitfalls) taken from the maintainer's production bot. Doctrine:
UD-1..UD-8. Rules: CV2-*, CID-02, MSG-05, ATT-*.

**Noctis has no UI widgets.** There is no `View` trait, no Navbar, Wizard or Paginator,
no framework-managed UI state, no automatic re-render and no automatic pagination
(withdrawn on 2026-10-20). You write each screen as a function, keep its ids in your own
Route Payloads, and Noctis makes sure every message you build is valid, fits Discord's
limits and is sent with the right response type.

## 1. A View is a plain render function

```rust
/// Pure: loaded data in, message out. No I/O, no `.await`.
pub fn render_x(data: &XData, selected: Option<u32>, page: u16) -> V2Message
```

- **One render function per screen**, in the feature's `view.rs`. It is the single source
  of truth for that message: the slash Command calls it for the first response and every
  component and modal handler calls it again after changing something. Never build the
  same message in two places.
- **Arguments, in this order:** the data the handler loaded (`&XData`), then the ids
  that select what to show (selected entity, tab, page, step), taken from the Payload.
  Add `&Limits` (`ctx.limits()`, the App's Limits) when the function sizes a page with
  the budget helper. The strings for the response's locale (`&Strings`, from
  `Strings::of(ctx.response_locale())`, `i18n.md` §3) go second; the recipes below use
  literals to stay short.
- **Pure and deterministic.** All I/O (database, cache, `ctx.render(..)` for images)
  happens in the handler before the call. The same arguments give the same JSON (except
  per-render select nonces), so tests snapshot the function directly (`testing.md`).
- **Returns `V2Message`** (`components-v2.md` §12). Building is infallible; the
  Responder validates every Limit before the request (FR-24) and a violation is a typed
  error naming the path, never a 400 from Discord.
- **UI state lives only in typed Payload fields** of the feature's own Routes: the
  selected entity id, a tab `#[derive(PayloadEnum)]`, a `page: u16`, a `step` enum. The
  derive proves the `custom_id` fits 100 characters (CID-01); nothing is stored per
  message (UD-7). If the state is large, use a `by_ref` Route (`routes-payloads-refs.md`).

## 2. The update flow

Every handler has the same four steps:

```text
decode the Payload  →  validate / authorize  →  write to storage  →  ctx.update(render_x(..))
(`self` in #[handler])  (Route Guards + checks)   (ctx.state::<T>())    (or ctx.respond / ctx.reply_text)
```

| Call | Sends | Use for |
| --- | --- | --- |
| `ctx.respond(msg).await` | a **new** message: the initial response (declared ephemerality), the edit of the deferred original after an auto-defer, or a follow-up once answered | slash Commands, context menus, a component that opens a separate panel |
| `ctx.update(msg).await` | an **edit** of the message the component lives on (type 7, or `edit_reply` after the type-6 auto-defer; on a slash-opened modal's submit it returns `UpdateUnavailable` and sends nothing, so use `ctx.respond` there; once the handler answered it is `AlreadyResponded`) | every button, select and modal submit that changes the screen (`ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx`; not `CommandCtx`) |
| `ctx.reply_text(text).await` | a one-line V2 notice, **ephemeral** by default (`.public()` opts out); a follow-up once answered or after a type-6 defer, the edit of a matching loading message after a type-5 defer | "Pick a channel first.", "Saved.", "Not found." (`String`, `&'static str` or a `MessageKey`) |

Each returns `HandlerResult<Responded>`: end the handler with it, or `return` it early
(`return ctx.reply_text("Pick a log channel first.").await;`). Noctis picks the callback
type, adds `IS_COMPONENTS_V2`, applies the declared ephemerality, defers for you when
the handler is slow (FR-21) and validates the message. A second initial response is
`ResponderError::AlreadyResponded`. Use the typestate `Responder` (`responder.md` §4)
only for modals and multi-message replies. Returning a `V2Message` from a Command works
too (it is sent like `ctx.respond`). The full state table is `responder.md` §1b.

App state (a database handle, a cache, a config) is registered once with
`AppBuilder::state(value)` and read in any handler with `ctx.state::<T>()`, which
returns `Option<State<T>>` (`State<T>` derefs to `T`; `None` when `T` was never
registered). Handlers take no `State<T>` parameters.

Select handlers read the pick with `ctx.values()`, a `SelectedValues` typed by the
select's kind: `strings()` (every raw value, any kind; a String Select's option values),
`users()`, `roles()` and `channels()` (ids, in the order sent; a Mentionable Select's
values are split into `users()` and `roles()`). A value is client input: parse it
(`strings().first().and_then(|v| v.parse::<u32>().ok())`) and check it belongs to the
guild before using it.

## 3. Recipes (complete code)

Every block below is a **verbatim copy** of one module of `examples/ui-recipes/` in
noctis-rs, a workspace crate that is compiled, linted and tested end to end with
`noctis::testkit` on every change; `xtask skill-check` fails when a block and its source
drift apart. Each block is the whole module, with its own `use` lines, so it compiles as
shown in `src/<name>.rs` of a bot crate; the `app` block is the crate root `src/lib.rs`
(not `src/app.rs`), and `store` is `src/store.rs`.

User-visible text in `defaults(..)` is a `keys::` path into the locale files
(`label = keys::SETUP_NEXT`); per-render overrides take a `MessageKey` too. Modals open pre-filled from a struct value
(`X { field: v, ..X::blank(..) }.build()`, `modals.md` §4a). Everything else is the
shipped API.

### 3.1 Tabs (UD-5)

A tab is a `PayloadEnum` field; the render function `match`es it; the tab row is one
Route button per tab, the active one Primary and disabled.

```rust recipe=tabs
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
```

- At most 5 tabs (one Action Row, CV2-06; 6 is a compile error). More sections: a String
  Select as the navigation, or split the screen.
- The slash Command calls the same function with `DashTab::Overview`; a tab click calls
  it with `self.tab`. Public, because a dashboard is shared content (rule 4).
- A tab whose body is a long list pages it with §3.2 inside the tab (add `page` to the
  tab button's Payload).

### 3.2 Pagination (UD-6)

A `page: u16` field, a slice, Previous/Next Route buttons disabled at the bounds. Size the
page with the budget helper (§4) against the fixed parts of the message, using a
worst-case row so page boundaries don't move when names get longer, and the page line at
its longest (`page_line(u16::MAX - 1, u16::MAX)`) so "Page 12 of 13" never overflows a
page sized with "Page 1 of 1". Convert with `u16::try_from(..)` and `usize::from(..)`,
never `as`.

```rust recipe=pagination
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    ActionRow, Budget, Components, Container, ContainerComponent, TextDisplay, V2Message,
    escape_markdown,
};

use crate::store::{Catalog, ITEM_NAME_MAX, missing};

/// The longest row: a row number of up to six digits plus `. ` (the `+ 8`), and a
/// name whose every character `escape_markdown` doubled. Sizing pages with it keeps page boundaries stable.
pub const ROW_MAX_CHARS: usize = 2 * ITEM_NAME_MAX + 8;

/// `/items`: the first page of the catalog.
#[derive(SlashCommand, Debug)]
#[command("items", description = "Browse the item list")]
pub struct Items;

impl Items {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let catalog = ctx.state::<Catalog>().ok_or_else(missing::<Catalog>)?;
        ctx.respond(render_items(&catalog.0, 0, ctx.limits())).await
    }
}

/// Previous and Next. The Payload is the target page, so the two ids differ (CID-02).
#[derive(Button, Debug, Clone, Copy)]
#[button("items.page", defaults(style = Secondary, label = "Next"))]
pub struct TurnPage {
    /// The page to show, 0-based.
    pub page: u16,
}

/// Page `page` (0-based, clamped to the last page) of `items`, sized to `limits`.
pub fn render_items(items: &[String], page: u16, limits: &Limits) -> V2Message {
    if items.is_empty() {
        return frame(vec![TextDisplay::new("-# No items yet.").into()]); // the empty state
    }
    // The fixed parts at their largest: the longest page line and the pager.
    let fixed = frame(vec![
        page_line(u16::MAX - 1, u16::MAX).into(),
        pager(0, u16::MAX).into(),
    ]);
    let per_page = Budget::of(&fixed, limits)
        .fits(&TextDisplay::new("x".repeat(ROW_MAX_CHARS)))
        .max(1);
    let pages = u16::try_from(items.len().div_ceil(per_page)).unwrap_or(u16::MAX);
    let page = page.min(pages - 1); // the page is client input: clamp it
    let start = usize::from(page) * per_page;
    let mut body: Vec<ContainerComponent> = items[start..]
        .iter()
        .take(per_page)
        .zip(start + 1..)
        .map(|(name, n)| TextDisplay::new(format!("{n}. {}", escape_markdown(name))).into())
        .collect();
    body.push(page_line(page, pages).into());
    body.push(pager(page, pages).into());
    frame(body)
}

/// `-# Page 2 of 7`.
fn page_line(page: u16, pages: u16) -> TextDisplay {
    TextDisplay::new(format!("-# Page {} of {pages}", u32::from(page) + 1))
}

/// Previous (disabled on the first page) and Next (disabled on the last).
fn pager(page: u16, pages: u16) -> ActionRow {
    ActionRow::buttons([
        TurnPage {
            page: page.saturating_sub(1),
        }
        .build()
        .label("Previous")
        .disabled(page == 0),
        TurnPage {
            page: page.saturating_add(1),
        }
        .build()
        .disabled(u32::from(page) + 1 >= u32::from(pages)),
    ])
}

/// The title, then `body`, in one Container.
fn frame(body: Vec<ContainerComponent>) -> V2Message {
    let header = Container::new().push(TextDisplay::new("## Items"));
    V2Message::new(Components::new().push(body.into_iter().fold(header, Container::push)))
}

impl TurnPage {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let catalog = ctx.state::<Catalog>().ok_or_else(missing::<Catalog>)?;
        ctx.update(render_items(&catalog.0, self.page, ctx.limits()))
            .await
    }
}
```

- Data from a database: query one page plus one row (`LIMIT per_page + 1 OFFSET …`) and
  disable Next when the extra row is missing. Checking `rows.len() < per_page` disables
  Next one page too late when the last page is exactly full.
- To pick one of many entities instead, use a String Select of at most 25 options per
  page (CV2-09) with the same Previous/Next row.

### 3.3 Step flow (UD-4)

A `step` field, a "Step n of m: Title" line, Back/Cancel/Next, and per-step validation
in the handler with an inline notice. Next stays enabled: a disabled button cannot tell
the user why, and a Payload is client input anyway, so the handler checks every click.

```rust recipe=step-flow
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
#[command(
    "setup",
    description = "Set up the bot",
    response = ephemeral,
    permissions = MANAGE_GUILD,
    contexts = [Guild]
)]
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
```

- Progress is "Step n of m: Title", never "page 2 of 3".
- The Finish button exists only when every value is set; its struct's fields are
  non-`Option`, so the type proves the configuration is complete.
- Cancel updates the same message to one Text Display: nothing is left to click (a V2
  message needs one component; `content` is not an interactive output, UD-1).
- Free text in a step: a button declared `auto_defer = false` opens a modal struct that
  carries the same state; its handler validates and calls
  `ctx.update(render_setup(next_state, None))`. Never chain modal → modal: go back to a
  message step first (`modals.md` §5).
- State too large for 100 characters: make the state Route `overflow = spill` (by Ref
  when it does not fit, with a TTL); an expired Ref reaches your `#[on_expired]` method,
  which can call `render_setup` for step 1 plus a `reply_text` notice.

### 3.4 Settings screen (re-editable config, schwi-style)

One Container per screen: an entity switcher, settings as Sections with an accessory
button, a pre-filled select, a toggle, add and delete rows, a one-shot delete
confirmation and a button → modal → re-render chain. Every component carries the id of
the system it edits, and every handler runs the same flow: guild, store operation, a
one-line notice when the id is gone, else `ctx.update(render_tickets(..))`.

```rust recipe=settings
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
```

- The modal (`RenameForm`) has the same Payload as the button that opens it
  (`RenameSystem`), so the submit knows which system to rename and updates the message
  the button was on.
- The switcher holds at most 25 options (CV2-09), so the store caps a guild at 25 systems
  and the Add section disappears at the cap; a stale Add still gets the limit notice.
- Autocomplete over the same data (`/ticket send <system>`) is a method on the Command
  struct that filters and caps at 25 in the query itself (`commands.md` §4).

### 3.5 The App and the data

`configure` registers the state and mounts one Module per recipe; `main` and the tests
call it (`TestApp::build(configure(&mut App::builder(), data))`, `testing.md`). The
stores stand in for the bot's database.

```rust recipe=app
pub mod pagination;
pub mod settings;
pub mod steps;
pub mod store;
pub mod tabs;

use noctis::prelude::*;

use crate::store::Data;

/// Registers the recipes' state and Modules on `builder`; `main` and the tests share it.
pub fn configure(builder: &mut AppBuilder, data: Data) -> &mut AppBuilder {
    builder
        .state(data.catalog)
        .state(data.setup)
        .state(data.tickets)
        .module(
            Module::new("tabs")
                .add::<tabs::Dashboard>()
                .add::<tabs::OpenTab>(),
        )
        .module(
            Module::new("pagination")
                .add::<pagination::Items>()
                .add::<pagination::TurnPage>(),
        )
        .module(
            Module::new("steps")
                .add::<steps::Setup>()
                .add::<steps::SetupState>()
                .add::<steps::PickChannel>()
                .add::<steps::PickRole>()
                .add::<steps::CancelSetup>()
                .add::<steps::FinishSetup>(),
        )
        .module(
            Module::new("settings")
                .add::<settings::Tickets>()
                .add::<settings::PickSystem>()
                .add::<settings::RenameSystem>()
                .add::<settings::RenameForm>()
                .add::<settings::SetRoles>()
                .add::<settings::ToggleSystem>()
                .add::<settings::AddSystem>()
                .add::<settings::AskDelete>()
                .add::<settings::ConfirmDelete>()
                .add::<settings::BackToSettings>(),
        )
}
```

```rust recipe=store
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, RoleMarker};

/// The longest item name the catalog holds; it bounds a catalog row's width, which
/// `pagination::ROW_MAX_CHARS` sizes pages from.
pub const ITEM_NAME_MAX: usize = 100;
/// The most ticket systems a guild can have: the switcher holds at most 25 options.
pub const MAX_SYSTEMS: usize = 25;

/// Everything `configure` registers as App state.
#[derive(Debug, Clone, Default)]
pub struct Data {
    /// The list the pagination recipe pages through.
    pub catalog: Catalog,
    /// What the step flow saves.
    pub setup: SetupStore,
    /// What the settings screen edits.
    pub tickets: TicketStore,
}

impl Data {
    /// A 300-item catalog and empty stores.
    pub fn sample() -> Self {
        Self {
            catalog: Catalog::sample(300),
            ..Self::default()
        }
    }
}

/// A read-only list of item names, each at most [`ITEM_NAME_MAX`] characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog(pub Vec<String>);

impl Catalog {
    /// `n` deterministic names (`Item 001 …`) of varying length.
    pub fn sample(n: usize) -> Self {
        const FILLER: &str = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do \
                              eiusmod tempor incididunt ut labore et dolore magna aliqua";
        Self(
            (1..=n)
                .map(|i| {
                    let mut name = format!("Item {i:03} {}", &FILLER[..(i * 7) % FILLER.len()]);
                    name.truncate(ITEM_NAME_MAX); // ASCII only, so any index is a char boundary
                    name.trim_end().to_owned()
                })
                .collect(),
        )
    }
}

// The stores stand in for the bot's database: a real bot keeps this data in its own
// storage behind a port or a `State<T>` handle, and every method here becomes a query.

/// A guild's saved setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupConfig {
    /// Where the bot posts its logs.
    pub log_channel: Id<ChannelMarker>,
    /// Who moderates.
    pub mod_role: Id<RoleMarker>,
}

/// Saved setups by guild (a cheap clone of one shared map).
#[derive(Debug, Clone, Default)]
pub struct SetupStore(Arc<Mutex<BTreeMap<Id<GuildMarker>, SetupConfig>>>);

impl SetupStore {
    /// Saves `config` for `guild`, replacing an older one.
    pub fn save(&self, guild: Id<GuildMarker>, config: SetupConfig) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(guild, config);
    }

    /// The setup saved for `guild`, if any.
    pub fn get(&self, guild: Id<GuildMarker>) -> Option<SetupConfig> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&guild)
            .copied()
    }
}

/// One ticket system of a guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketSystem {
    /// Unique across guilds; the id every settings component carries.
    pub id: u32,
    /// Shown in the switcher and the name Section.
    pub name: String,
    /// Whether it takes new tickets.
    pub enabled: bool,
    /// The roles that handle its tickets.
    pub roles: Vec<Id<RoleMarker>>,
}

#[derive(Debug, Default)]
struct Tickets {
    by_guild: BTreeMap<Id<GuildMarker>, Vec<TicketSystem>>,
    next_id: u32,
}

/// Ticket systems by guild (a cheap clone of one shared store).
#[derive(Debug, Clone, Default)]
pub struct TicketStore(Arc<Mutex<Tickets>>);

impl TicketStore {
    /// A store holding `systems` for `guild` (test fixtures).
    pub fn with(guild: Id<GuildMarker>, systems: Vec<TicketSystem>) -> Self {
        let next_id = systems.iter().map(|t| t.id).max().unwrap_or(0);
        let tickets = Tickets {
            by_guild: BTreeMap::from([(guild, systems)]),
            next_id,
        };
        Self(Arc::new(Mutex::new(tickets)))
    }

    /// `guild`'s systems, oldest first.
    pub fn systems(&self, guild: Id<GuildMarker>) -> Vec<TicketSystem> {
        self.lock()
            .by_guild
            .get(&guild)
            .cloned()
            .unwrap_or_default()
    }

    /// Adds a system to `guild` and returns its id; `None` at [`MAX_SYSTEMS`].
    pub fn add(&self, guild: Id<GuildMarker>) -> Option<u32> {
        let mut tickets = self.lock();
        let id = tickets.next_id.checked_add(1)?;
        let systems = tickets.by_guild.entry(guild).or_default();
        if systems.len() >= MAX_SYSTEMS {
            return None;
        }
        systems.push(TicketSystem {
            id,
            name: format!("Ticket system {id}"),
            enabled: true,
            roles: Vec::new(),
        });
        tickets.next_id = id;
        Some(id)
    }

    /// Renames `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn rename(&self, guild: Id<GuildMarker>, id: u32, name: &str) -> bool {
        self.edit(guild, id, |t| name.clone_into(&mut t.name))
    }

    /// Enables or disables `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn set_enabled(&self, guild: Id<GuildMarker>, id: u32, enabled: bool) -> bool {
        self.edit(guild, id, |t| t.enabled = enabled)
    }

    /// Sets the roles of `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn set_roles(&self, guild: Id<GuildMarker>, id: u32, roles: &[Id<RoleMarker>]) -> bool {
        self.edit(guild, id, |t| roles.clone_into(&mut t.roles))
    }

    /// Deletes `guild`'s system `id`; `false` when it is not `guild`'s or gone.
    pub fn delete(&self, guild: Id<GuildMarker>, id: u32) -> bool {
        let mut tickets = self.lock();
        let Some(systems) = tickets.by_guild.get_mut(&guild) else {
            return false;
        };
        let before = systems.len();
        systems.retain(|t| t.id != id);
        systems.len() < before
    }

    fn edit(&self, guild: Id<GuildMarker>, id: u32, f: impl FnOnce(&mut TicketSystem)) -> bool {
        let mut tickets = self.lock();
        let found = tickets
            .by_guild
            .get_mut(&guild)
            .and_then(|systems| systems.iter_mut().find(|t| t.id == id));
        found.map(f).is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tickets> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A state type `configure` did not register.
#[derive(Debug)]
struct MissingState(&'static str);

impl fmt::Display for MissingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no `{}` state; register it with AppBuilder::state",
            self.0
        )
    }
}

impl std::error::Error for MissingState {}

/// The error for a missing `T` state: `ctx.state::<T>().ok_or_else(missing::<T>)?`.
pub fn missing<T>() -> HandlerError {
    HandlerError::new(MissingState(std::any::type_name::<T>()))
}
```

## 4. Budget helpers and cost

```rust
use noctis::ui::{Budget, cost};

let report = cost(&msg, &limits);                     // CostReport
report.components;                                    // every depth, accessories included
report.text_chars;                                    // all Text Displays (CV2-04)

let budget = Budget::of(&fixed_parts, ctx.limits());  // what is left beside the fixed parts
budget.components_left();                             // limits.components - cost, saturating at 0
budget.text_left();                                   // limits.text_total_len - text, saturating at 0
budget.fits(&sample_row);                             // largest n such that n copies fit both
budget.fits_prefix(rows.iter());                      // how many rows from the front fit, in order
```

- `Budget::of(&V2Message, &Limits)` only reads the message (no validation, truncation or
  logging) and uses the configured Limits, never hard-coded 40/4000. Over a budget
  already, both numbers are 0.
- `fits(&item)` = `min(components_left / c, text_left / t)` for an item of cost `(c, t)`
  (`c` components at every depth, `t` Text Display characters by `discord_len`); an axis
  the item does not use (`t = 0`, a Separator) does not bind. The item is any Container
  child (`TextDisplay`, `Section`, `ActionRow`, `Separator`, `MediaGallery`, `File`, or a
  `ContainerComponent`); inside a Container it costs the same, since the Container is a
  fixed part.
- `fits_prefix(items)` is greedy and in order: it adds up the costs and stops at the first
  item that does not fit, even if a later, smaller one would. It borrows the items
  (`rows.iter()` or `rows.iter().skip(offset)`) and pulls at most `k + 1` of them, so
  build owned rows into a `Vec` first; an owned `.map(..)` iterator does not compile.
- Budgets only: CV2-03 and CV2-04. Per-field rules (label 80, URLs, CV2-06 row shape) and
  CID-02 are not counted. `n` copies of one Route button with the same Payload fit the
  budget but fail CID-02, so real rows differ by Payload.
- **Measure fixed parts with worst-case variable text**: header, tab row, the pager row
  and the page line at their largest (`page_line(s, u16::MAX - 1, u16::MAX)`), then ask
  how many rows fit. Use a worst-case sample row for stable pages, or `fits_prefix` when
  rows differ a lot and you page by offset.
- Pass `ctx.limits()` (`&Limits`, what the Responder validates against, set with
  `AppBuilder::limits`) to the render function; never `Limits::default()` in a handler.
- Noctis never paginates, splits or truncates a message for you (Lenient mode only
  truncates over-long text, `limits-cheatsheet.md`). Over a Limit is a typed error with
  the path (`[CV2-03] components: limit at most 40, actual 41`).
- Cost cheatsheet: Container 1, Text Display 1, Separator 1, Section + 1 text + button
  = 3, Action Row + 5 buttons = 6, Action Row + 1 select = 2. Merge static text into one
  Text Display.

## 5. Attachments, mentions, user text, select freshness

- **Attachments** (FR-30): `ctx.render(card).await?` (or `Attachment::new(bytes, "png")`)
  gives an `Attachment`; add it to an `AttachmentSet` and put the returned
  `AttachmentRef` in a Media Gallery, Thumbnail or File; attach the set with
  `V2Message::attachments(set)` (`components-v2.md` §6). The same image used twice uploads
  once; on edit, unchanged names are kept without re-upload and unused ones dropped. Alt
  text is required for rendered images in strict mode.
- **Mentions**: messages ping nobody by default (`parse: []`, MSG-05) even though
  `<@id>` renders. Opt in per message with `.mentions(Mentions::none().users(Users::Only(ids)))`.
- **User text**: pass names, reasons and bios through `noctis::ui::escape_markdown`
  (Markdown: `\ * _ ~` `` ` `` `| [ ]` anywhere, `# > - +` and `1.` at a line start) and,
  where mentions must not render, `escape_mentions` (a U+200B after `@` before
  `everyone`/`here` and after `<` before `@ # /`) before putting them in a Text Display:
  `escape_mentions(&escape_markdown(bio))`. Nothing is escaped automatically; escape
  once (the helpers are not idempotent), in either order (they commute).
- **Select freshness**: every select kind carries a nonce, and Noctis draws a new one
  on each send that never equals a nonce of the interaction being answered (the clicked
  select, the message's selects), nor one of the handler run's last eight sends. After
  `ctx.update(..)` the client therefore shows the defaults your render function declared
  (`default_users`, `.default(true)`), not the user's previous local selection: always
  set them to the current value. Equal state renders equal JSON except these nonces
  (and the Refs of `by_ref` Routes, new per send), so snapshot tests mask the nonce.
- **Unique ids** (CID-02): two components with the same Route and Payload in one
  message are an error, selects and signed or by-Ref Routes included, with the help
  ``give the components a Payload field that tells them apart, …``. Give buttons that
  share a Route different Payloads (prev and next target different pages; per-row
  buttons carry the row id).

## 6. UI quality rulebook

1. **One primary action per View.** One Primary/Success button; everything else Secondary.
2. **Visible active state.** Disabled active tab, `default` on the current select
   option/value, state text next to toggles ("**Profiles** · enabled").
3. **Confirm destructive actions.** Danger button → small confirm screen ("Delete
   ticket system #3? This can't be undone.") with Cancel (Secondary) and Confirm (Danger)
   on a `by_ref, one_shot, ttl = "15m"` Route. Never delete on the first click.
4. **Ephemeral for single-user UI, public for shared content.** Settings, setup flows,
   errors, per-user panels → ephemeral (User Locale). Dashboards, cards, leaderboards
   others should see → public (Guild Locale).
5. **A way back or out.** Every interactive screen has Back, Cancel, a tab row, or a
   "Back to …" button that re-renders the parent. Drill-down state is just ids in
   Payloads, no navigation stack.
6. **Designed empty and error states.** "No ticket systems yet." in subtext, a disabled
   placeholder select; errors are one localized line (`ctx.reply_text(key)`), never a
   stack trace or a missing block.
7. **Hierarchy with Markdown, not components.** `#`/`##` title, `-#` subtext,
   `<t:…:R>` timestamps; merge static text into one Text Display.
8. **Short labels.** ≤ 34 chars with emoji; mobile wraps rows past ~3 labelled buttons.
9. **Images are read-only.** Wide visuals in Media Gallery; small icons in Thumbnail;
   never instructions inside an image; always alt text.
10. **Accent colour as state.** Neutral in progress, green complete, red error.

## 7. Best practices (schwi-style)

These come from the maintainer's production TypeScript bot (schwi-v2, on tescord) and
are how every Noctis bot should be written.

1. **One Module per feature, split by role.** `src/modules/<feature>/mod.rs` (the Module
   and its Routes, the feature's storage keys), `view.rs` (render functions),
   `handlers.rs` (slash Commands, component and modal handlers; schwi's `setup.ts` and
   `commands.ts`), `events.rs` (gateway events; schwi's `trigger.ts`). Module ids are
   path-like when nested (`Module::new("modules/ticket")`).
2. **`render_x(data, &strings, ids…) -> V2Message` is the single source of truth**,
   reused by the slash Command and every component and modal handler (§1).
3. **State only in typed Payload fields** of the module's own Routes: the selected
   entity id, the section (a `PayloadEnum` matched in the render function), the page,
   the step. Nothing per message, nothing in a global map.
4. **Handler flow: decode → validate/authorize → write → `ctx.update(render_x(..))`.**
   Payloads and select values are client input: check that the id belongs to the
   guild, clamp pages, parse strings; Route Guards (`owner = field`,
   `permissions = …`) run before the handler.
5. **Settings screens**: a Section per scalar setting (value in the text, "Edit"/"Set"
   accessory, Primary when unset), a caption plus a pre-filled select per channel/role
   setting, enable/disable toggles whose style encodes the action, "Add" and "Delete"
   rows at the bottom, separators between blocks (§3.4).
6. **Entity switcher**: a String Select of the guild's entities (≤ 25, `default` on the
   current one), shown only when there is more than one; its handler re-renders with
   the chosen id.
7. **Sections by `match`** on a `PayloadEnum` field (tabs, §3.1), never by string
   comparison.
8. **Prev/next disabled at the bounds**, on a Route whose Payload is the target page
   (§3.2); query one extra row to know whether a next page exists.
9. **Autocomplete backed by storage queries capped at 25** in the query itself
   (`commands.md` §4).
10. **Button → modal → re-render chains**: the button's Route and the modal's Route carry
    the same Payload; the button handler opens the modal pre-filled from storage
    (`X { field: stored, ..X::blank(..) }.build()`); the submit handler
    validates, saves and calls `ctx.update(render_x(..))` on the original message.
11. **Every user-visible string goes through the locale files** (`Strings::of(locale)`),
    modal titles and placeholders included; no "Todo localization" literals. (The
    recipes in §3 use literals only to stay short.)
12. **One-line notices with `ctx.reply_text(key)`**: validation failures, "not found",
    limits reached. Ephemeral, localized, never a whole Container.

### Pitfalls from schwi-v2 (and what Noctis does instead)

| schwi-v2 did | Problem | In Noctis |
| --- | --- | --- |
| `if (tree.components.length > 40)` before falling back to a smaller editor | counts only top-level components; nested ones are missed, so the check passes while the real total is over 40 | `noctis::ui::cost` and `Budget` count every depth; the Responder validates before sending |
| free text (names, reasons) in custom-id data | breaks the 100-character limit at runtime and lets users forge it | typed Payload fields with a compile-time length proof; free text goes in storage or a `by_ref` Route |
| legacy Action Row-wrapped Text Inputs in modals (`type: 1, components: [TextInput]`) | deprecated shape; `label` on Text Input is deprecated | `#[derive(Modal)]` emits Label-wrapped inputs only |
| `data[0] as number`, `as any` on overrides | untyped data; a stale or forged id becomes a wrong value silently | typed Payload decode; a bad id is a typed error routed to the Expired/stale handler |
| copy-pasted rate-limit code in handlers | inconsistent and easy to forget | `rate_limit(..)` on `#[command]` or the component's kind attribute (`modules-middleware-ratelimits.md`) |
| `flags: Ephemeral | IsComponentsV2` repeated in every handler, `deferUpdate` + `editReply` by hand | forgotten flags are 400s; a slow handler fails | `ctx.update(..)` / `ctx.respond(..)` set flags, ephemerality and deferral |
| `Math.random()` in select data to force fresh defaults | ad-hoc, wastes custom-id space | every select gets a fresh per-send nonce |
| next page disabled when `rows.length < 25` | one empty page too many when the last page is exactly full | query `per_page + 1` rows |

### Patterns and anti-patterns

- **Do** keep one render function per View and call it from every handler that changes
  its state. **Don't** keep a message alive with per-message listeners, collectors,
  `tokio::spawn` tasks or `await` loops (UD-7); every click is a fresh, stateless route.
- **Do** answer a component click with `ctx.update(..)` and a re-rendered View. **Don't**
  reply with legacy `content` messages or send a new message per step.
- **Do** carry state in typed Payload fields (ids, enums, page and step numbers).
  **Don't** put free text in a custom id; keep it in storage or behind a `by_ref` Route.
- **Do** guard a destructive action (delete, reset) with a one-shot confirm view.
  **Don't** delete on the first click.
- **Do** clamp a client-supplied page or step to the valid range in the handler.
  **Don't** trust a Payload value: it is client input and can be stale or forged.

## 8. Decision table: which pattern?

| Situation | Pattern |
| --- | --- |
| Configure something once, in order (setup, onboarding, create-X) | step flow (§3.3) |
| Re-editable settings of one or more entities | settings screen (§3.4) |
| A general screen with distinct sections that don't fit one Container | tabs (§3.1), ≤ 5 |
| A homogeneous list that can exceed 40 components / 4000 characters | pagination (§3.2), inside the active tab if any |
| Pick one of many entities | String Select (≤ 25 per page) + prev/next |
| Dense read-only data (cards, charts, leaderboards) | a rendered Template image in a Media Gallery (`images.md`) |
| A one-line answer | `ctx.reply_text(key)` |

## 9. Examples

- **`examples/ui-recipes/` in noctis-rs**: the four recipes of §3 as one bot crate,
  compiled and tested on every change (`tests/recipes.rs` drives each flow through
  `noctis::testkit`); the blocks of §3 are verbatim copies of its modules.
- **This template**: `src/modules/dashboard/` (tabs, a role list paginated with
  `Budget`, module toggles), `src/modules/setup/` (a three-step flow with pre-filled
  selects and Cancel) and `src/modules/profile/` (a rendered card, button → modal →
  re-render) apply the same recipes with localized text; read them next to this file.
  One variant: the template's `setup/view.rs` renders Next disabled until the step is
  set, as a hint, and its handler still checks the step on every click; the default is
  §3.3's always-enabled Next with an inline notice.
