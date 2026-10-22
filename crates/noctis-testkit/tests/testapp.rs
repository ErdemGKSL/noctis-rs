//! Story 8.4: `TestApp` end to end (FR-51). Every run travels `FakeGateway`
//! `INTERACTION_CREATE` → shard → Interaction Lane → pipeline, and the
//! `Response` is read off the `FakeDiscordHttp` wire.
//!
//! Fixture: guild `G` "Guild" with text channel `C` and role `R`, users `A`
//! (`MANAGE_GUILD`, role `R`) and `M`. Rows refer to the spec's I/O matrix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use noctis::codec::PayloadKey;
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use noctis::ui::{
    ActionRow, Button as AnyButton, ButtonStyle, Components, Container, LegacyMessage, LinkButton,
    Mentionable, ResolvedChannel, ResolvedMessage, ResolvedRole, ResolvedUser, Section,
    TextDisplay, V2Message,
};
use noctis::{Permissions, Plugin, ResponderError};
use noctis_testkit::{
    Response, ResponseKind, TestApp, TestComponent, TestError, TestGuild, TestMessage, TestUser,
    WireCost, render_json,
};
use serde_json::Value;

const G: Id<GuildMarker> = Id::new(10);
const C: Id<ChannelMarker> = Id::new(20);
const R: Id<RoleMarker> = Id::new(30);
const A: Id<UserMarker> = Id::new(40);
const M: Id<UserMarker> = Id::new(41);

/// `noctis.error.generic` in en-US.
const GENERIC: &str = "Something went wrong. Please try again later.";

// --- What the handlers saw ---------------------------------------------------

/// The handlers' observations, shared through `State` (one per App, so
/// parallel tests do not mix).
#[derive(Default)]
struct Log(Mutex<Vec<String>>);

impl Log {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn has(&self, line: &str) -> bool {
        self.lines().iter().any(|l| l == line)
    }
}

macro_rules! note {
    ($ctx:expr, $($fmt:tt)*) => {
        $ctx.state::<Arc<Log>>().unwrap().push(format!($($fmt)*))
    };
}

fn view(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

// --- Commands ------------------------------------------------------------------

#[derive(SlashCommand)]
#[command("ping", description = "Ping")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(view("pong")).await
    }
}

/// Echoes where and as whom it ran.
#[derive(SlashCommand)]
#[command("whoami", description = "Who am I", response = ephemeral)]
struct Whoami;

impl Whoami {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        note!(
            ctx,
            "whoami {} {:?} {:?} {:?} {:?}",
            ctx.user_id(),
            ctx.guild_id().map(Id::get),
            ctx.channel_id().map(Id::get),
            ctx.user_locale(),
            ctx.guild_locale()
        );
        ctx.respond(view("you")).await
    }
}

#[derive(SlashCommand)]
#[command("setup start", description = "Start the setup")]
struct SetupStart;

impl SetupStart {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        note!(ctx, "path {}", ctx.command_path());
        ctx.respond(view("step 1")).await
    }
}

#[derive(SlashCommand)]
#[command("config set prefix", description = "Set the prefix")]
struct ConfigSetPrefix {
    #[option(description = "The prefix")]
    value: String,
}

impl ConfigSetPrefix {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        note!(ctx, "path {} {}", ctx.command_path(), self.value);
        ctx.respond(view("set")).await
    }
}

/// A guild-scoped command (C3).
#[derive(SlashCommand)]
#[command("dev", description = "Dev only", guilds = [10])]
struct Dev;

impl Dev {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(view("dev")).await
    }
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[choice("Fast", value = "fast")]
    Fast,
    #[choice("Safe", value = "safe")]
    Safe,
}

#[allow(clippy::ptr_arg, reason = "validators take `&T`")]
fn short(reason: &String, _ctx: &CommandCtx) -> Result<(), MessageKey> {
    if reason.chars().count() <= 10 {
        Ok(())
    } else {
        Err(MessageKey::new("noctis.validation.failed"))
    }
}

fn mention(m: &Mentionable) -> String {
    match m {
        Mentionable::User(u) => format!("user {}", u.user.id),
        Mentionable::Role(r) => format!("role {}", r.role.id),
        _ => "other".to_owned(),
    }
}

/// Every option type (C4-C13).
#[derive(SlashCommand)]
#[command("opts", description = "Options")]
struct Opts {
    #[option(description = "How many")]
    count: i64,
    #[option(description = "Why", max_length = 10, validate = short)]
    reason: Option<String>,
    #[option(description = "Ratio")]
    ratio: Option<f64>,
    #[option(description = "Mode")]
    mode: Option<Mode>,
    #[option(description = "A user")]
    user: Option<ResolvedUser>,
    #[option(description = "A channel")]
    channel: Option<ResolvedChannel>,
    #[option(description = "A role")]
    role: Option<ResolvedRole>,
    #[option(description = "Anyone")]
    who: Option<Mentionable>,
}

impl Opts {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        note!(
            ctx,
            "opts count={} reason={:?} ratio={:?} mode={:?} user={:?} member={} channel={:?} role={:?} who={:?}",
            self.count,
            self.reason,
            self.ratio,
            self.mode,
            self.user.as_ref().map(|u| u.user.id.get()),
            self.user.as_ref().is_some_and(|u| u.member.is_some()),
            self.channel.as_ref().map(|c| c.channel.id.get()),
            self.role.as_ref().map(|r| r.role.id.get()),
            self.who.as_ref().map(mention),
        );
        ctx.respond(view("ok")).await
    }
}

#[derive(UserCommand)]
#[user_command("Show profile")]
struct ShowProfile;

impl ShowProfile {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Responded> {
        note!(
            ctx,
            "profile {} member={}",
            target.user.id,
            target.member.is_some()
        );
        ctx.respond(view("profile")).await
    }
}

#[derive(MessageCommand)]
#[message_command("Quote")]
struct Quote;

impl Quote {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<Responded> {
        note!(ctx, "quote {}", target.message.id);
        ctx.respond(view("quoted")).await
    }
}

/// M1's message: a Container with a text, a Section with an accessory
/// button and a row of Primary, disabled Secondary and Link buttons.
fn panel() -> V2Message {
    V2Message::new(
        Components::new().push(
            Container::new()
                .push(TextDisplay::new("## Title"))
                .push(Section::new(
                    [TextDisplay::new("a")],
                    Echo { n: 1 }.build().label("Go"),
                ))
                .push(ActionRow::buttons([
                    AnyButton::from(Echo { n: 2 }.build().label("Next")),
                    AnyButton::from(
                        Echo { n: 3 }
                            .build()
                            .label("Prev")
                            .style(ButtonStyle::Secondary)
                            .disabled(true),
                    ),
                    AnyButton::from(LinkButton::new("https://docs.rs").label("Docs")),
                ])),
        ),
    )
}

#[derive(SlashCommand)]
#[command("panel", description = "The panel")]
struct Panel;

impl Panel {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(panel()).await
    }
}

/// M9's render: a select, a `signed` button and a `by_ref` button.
fn render_panel() -> V2Message {
    V2Message::new(
        Components::new().push(
            Container::new()
                .push(TextDisplay::new("## Render"))
                .push(ActionRow::select(PickStr.build()))
                .push(ActionRow::buttons([
                    AnyButton::from(Sig { item: 5 }.build()),
                    AnyButton::from(ByRef { x: 3 }.build()),
                ])),
        ),
    )
}

#[derive(SlashCommand)]
#[command("render", description = "The render")]
struct Render;

impl Render {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render_panel()).await
    }
}

/// Every select kind and one button (X4-X12).
fn selects_view() -> V2Message {
    V2Message::new(
        Components::new()
            .push(ActionRow::select(PickStr.build()))
            .push(ActionRow::select(PickUser.build()))
            .push(ActionRow::select(PickRole.build()))
            .push(ActionRow::select(PickChan.build()))
            .push(ActionRow::select(PickWho.build()))
            .push(ActionRow::buttons([Echo { n: 6 }.build()])),
    )
}

#[derive(SlashCommand)]
#[command("selects", description = "Selects")]
struct Selects;

impl Selects {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(selects_view()).await
    }
}

#[derive(SlashCommand)]
#[command("eph", description = "Ephemeral", response = ephemeral)]
struct Eph;

impl Eph {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Ok(Components::new()
            .push(TextDisplay::new("secret"))
            .push(ActionRow::buttons([Echo { n: 9 }.build()])))
    }
}

/// A reply, then two ephemeral notices (K8).
#[derive(SlashCommand)]
#[command("follow", description = "Follow-ups")]
struct Follow;

impl Follow {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let _main = ctx.respond(view("main")).await?;
        let _one = ctx.reply_text("one").await?;
        ctx.reply_text("two").await
    }
}

/// A reply, then a follow-up with a button (X10).
#[derive(SlashCommand)]
#[command("followbtn", description = "Follow-up button")]
struct FollowButton;

impl FollowButton {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let _main = ctx.respond(view("main")).await?;
        ctx.respond(V2Message::new(
            Components::new().push(ActionRow::buttons([Echo { n: 8 }.build()])),
        ))
        .await
    }
}

#[derive(SlashCommand)]
#[command("broken", description = "Fails")]
struct Broken;

impl Broken {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Err(HandlerError::new(std::io::Error::other("database down")))
    }
}

/// Answers after the ack deadline (K3).
#[derive(SlashCommand)]
#[command("slow", description = "Slow")]
struct Slow;

impl Slow {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        ctx.respond(view("late")).await
    }
}

/// Fails after the auto-defer (K10).
#[derive(SlashCommand)]
#[command("sluggish", description = "Fails late")]
struct Sluggish;

impl Sluggish {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Err(HandlerError::new(std::io::Error::other("too slow")))
    }
}

/// Defers itself and never edits (K6).
#[derive(SlashCommand)]
#[command("deferonly", description = "Defers", auto_defer = false)]
struct DeferOnly;

impl DeferOnly {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }
}

/// Never answered: the Module's Middleware stops it silently (K13).
#[derive(SlashCommand)]
#[command("quiet", description = "Stopped")]
struct Quiet;

impl Quiet {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        note!(ctx, "quiet ran");
        ctx.respond(view("never")).await
    }
}

async fn stop_all(_ctx: BeforeCtx) -> HandlerResult<Flow> {
    Ok(Flow::Stop(None))
}

/// Calls a bot-token REST route (K14).
#[derive(SlashCommand)]
#[command("fetch", description = "Fetches a channel")]
struct Fetch;

impl Fetch {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let outcome = match ctx.http().channel(C).await {
            Ok(_) => "ok".to_owned(),
            Err(error) => error.to_string(),
        };
        note!(ctx, "fetch {outcome}");
        ctx.respond(view("fetched")).await
    }
}

#[derive(SlashCommand)]
#[command("sleepy", description = "Sleeps")]
struct Sleepy;

impl Sleepy {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_secs(1)).await;
        ctx.respond(view("awake")).await
    }
}

#[derive(SlashCommand)]
#[command("legacy", description = "A legacy reply")]
struct Legacy;

impl Legacy {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("hi").rows([ActionRow::buttons([
            Echo { n: 4 }.build(),
            Echo { n: 5 }.build(),
        ])]))
    }
}

/// Replies with a component of each guarded or modal-opening Route.
#[derive(SlashCommand)]
#[command("buttons", description = "Buttons")]
struct Buttons;

impl Buttons {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(V2Message::new(
            Components::new()
                .push(ActionRow::buttons([
                    AnyButton::from(
                        Owned {
                            user: ctx.user_id(),
                        }
                        .build(),
                    ),
                    AnyButton::from(Staff.build()),
                    AnyButton::from(OpenNote.build()),
                    AnyButton::from(OpenSurvey.build()),
                    AnyButton::from(SlowUpd.build()),
                ]))
                .push(ActionRow::buttons([
                    AnyButton::from(OldTurn { page: 1 }.build()),
                    AnyButton::from(Turn { page: 2 }.build()),
                ])),
        ))
        .await
    }
}

/// Opens [`SlashNote`] from a slash command (F7).
#[derive(SlashCommand)]
#[command("asknote", description = "Asks", auto_defer = false)]
struct AskNote;

impl AskNote {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.show_modal(SlashNote::blank()).await?.done())
    }
}

/// Autocomplete (A1-A4).
#[derive(SlashCommand)]
#[command("warn", description = "Warn")]
struct Warn {
    #[option(description = "The rule", autocomplete)]
    rule: String,
    #[option(description = "A note")]
    note: Option<String>,
}

impl Warn {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        note!(ctx, "warn {} {:?}", self.rule, self.note);
        ctx.respond(view("warned")).await
    }

    #[autocomplete(rule)]
    async fn rules(
        ctx: AutocompleteCtx,
        typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        ctx.state::<Arc<Log>>().push(format!("typed {typed:?}"));
        Ok((0..30)
            .map(|i| (format!("r{i}"), format!("{typed} rule {i}")))
            .collect())
    }
}

// --- Components ----------------------------------------------------------------

/// Updates the clicked message with its number.
#[derive(Button, Debug, Clone, Copy)]
#[button("echo", defaults(style = Primary, label = "Echo"))]
struct Echo {
    n: u8,
}

impl Echo {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        note!(ctx, "echo {} by {}", self.n, ctx.user_id());
        ctx.update(view(&format!("echo {}", self.n))).await
    }
}

#[derive(Button, Debug, Clone, Copy)]
#[button("owned", owner = user, defaults(style = Primary, label = "Mine"))]
struct Owned {
    user: Id<UserMarker>,
}

impl Owned {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        note!(ctx, "owned {}", self.user);
        ctx.update(view("yours")).await
    }
}

#[derive(Button, Debug, Clone, Copy)]
#[button("staff", permissions = MANAGE_GUILD, defaults(style = Primary, label = "Staff"))]
struct Staff;

impl Staff {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        note!(
            ctx,
            "staff {:?}",
            ctx.member_permissions().map(|p| p.bits())
        );
        ctx.update(view("staff")).await
    }
}

/// Opens [`NoteForm`] (K7, F1-F4).
#[derive(Button, Debug, Clone, Copy)]
#[button("note.open", auto_defer = false, defaults(style = Secondary, label = "Note"))]
struct OpenNote;

impl OpenNote {
    #[handler]
    async fn handle(
        self,
        _ctx: ComponentCtx,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.show_modal(NoteForm::blank(7)).await?.done())
    }
}

#[derive(Modal, Debug)]
#[modal("note", defaults(title = "Note"))]
struct NoteForm {
    step: u8,
    #[text_input("Note", max_length = 5)]
    note: String,
}

impl NoteForm {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        note!(ctx, "note {} {}", self.step, self.note);
        ctx.update(view(&format!("noted {}", self.note))).await
    }
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    #[choice("Low", value = "l")]
    Low,
    #[choice("High", value = "h")]
    High,
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Topic {
    #[choice("Rust", value = "rust")]
    Rust,
    #[choice("Art", value = "art")]
    Art,
}

/// Opens [`Survey`] (F6).
#[derive(Button, Debug, Clone, Copy)]
#[button("survey.open", auto_defer = false, defaults(style = Secondary, label = "Survey"))]
struct OpenSurvey;

impl OpenSurvey {
    #[handler]
    async fn handle(
        self,
        _ctx: ComponentCtx,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.show_modal(Survey::blank()).await?.done())
    }
}

#[derive(Modal, Debug)]
#[modal("survey", defaults(title = "Survey"))]
struct Survey {
    #[radio_group("Severity")]
    severity: Severity,
    #[checkbox_group("Topics", min_values = 0, max_values = 2)]
    topics: Vec<Topic>,
    #[checkbox("I agree")]
    agree: bool,
}

impl Survey {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        note!(
            ctx,
            "survey {:?} {:?} {}",
            self.severity,
            self.topics,
            self.agree
        );
        ctx.update(view("saved")).await
    }
}

/// Opened from a slash command: `update` is unavailable, so it replies.
#[derive(Modal, Debug)]
#[modal("slashnote", defaults(title = "Ask"))]
struct SlashNote {
    #[text_input("Text")]
    text: Option<String>,
}

impl SlashNote {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx, r: ModalResponder) -> HandlerResult<Responded> {
        match r.update(view("x")).await {
            Ok(done) => Ok(done.done()),
            Err(ResponderError::UpdateUnavailable) => {
                note!(ctx, "unavailable {:?}", self.text);
                ctx.respond(view("answered")).await
            }
            Err(other) => Err(other.into()),
        }
    }
}

/// Sleeps past the ack deadline, then updates (K5).
#[derive(Button, Debug, Clone, Copy)]
#[button("slowupd", defaults(style = Primary, label = "Slow"))]
struct SlowUpd;

impl SlowUpd {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        ctx.update(view("later")).await
    }
}

/// The current `turn` Route (`schema = 2`).
#[derive(Button, Debug, Clone, Copy)]
#[button("turn", schema = 2, defaults(style = Secondary, label = "Turn"))]
struct Turn {
    page: u16,
}

impl Turn {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(view(&format!("page {}", self.page))).await
    }
}

/// An old build of `turn` (no schema), never mounted: its ids are stale
/// (K12).
#[derive(Button, Debug, Clone, Copy)]
#[button("turn", defaults(style = Secondary, label = "Old"))]
struct OldTurn {
    page: u16,
}

impl OldTurn {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(view(&format!("old page {}", self.page))).await
    }
}

#[derive(Button, Debug, Clone, Copy)]
#[button("byref", by_ref, defaults(style = Primary, label = "Ref"))]
struct ByRef {
    x: u64,
}

impl ByRef {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        note!(ctx, "byref {}", self.x);
        ctx.update(view("ref ok")).await
    }
}

#[derive(Button, Debug, Clone, Copy)]
#[button("sig", signed, defaults(style = Primary, label = "Sig"))]
struct Sig {
    item: u64,
}

impl Sig {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        note!(ctx, "sig {}", self.item);
        ctx.update(view("signed ok")).await
    }
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Letter {
    #[choice("A", value = "a")]
    A,
    #[choice("B", value = "b")]
    B,
    #[choice("C", value = "c")]
    C,
}

/// Re-renders the selects (X12).
#[derive(StringSelectMenu)]
#[string_select(
    "pick",
    defaults(placeholder = "Pick", options = Letter, min_values = 1, max_values = 2)
)]
struct PickStr;

impl PickStr {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        note!(ctx, "pick");
        ctx.update(selects_view()).await
    }
}

macro_rules! select_route {
    ($derive:ident, $attr:ident, $name:ident, $id:tt, $($defaults:tt)*) => {
        #[derive($derive)]
        #[$attr($id, defaults($($defaults)*))]
        struct $name;

        impl $name {
            #[handler]
            async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
                note!(ctx, $id);
                ctx.update(view($id)).await
            }
        }
    };
}

select_route!(
    UserSelectMenu,
    user_select,
    PickUser,
    "pick.user",
    placeholder = "User"
);
select_route!(
    RoleSelectMenu,
    role_select,
    PickRole,
    "pick.role",
    placeholder = "Role"
);
select_route!(
    ChannelSelectMenu,
    channel_select,
    PickChan,
    "pick.chan",
    placeholder = "Channel"
);
select_route!(
    MentionableSelectMenu,
    mentionable_select,
    PickWho,
    "pick.who",
    placeholder = "Who",
    max_values = 2
);

// --- The App -------------------------------------------------------------------

/// Mounts a `ping` Module, as a bot's own Plugin would (L4).
struct PingPlugin;

impl Plugin for PingPlugin {
    fn install(&self, app: &mut AppBuilder) {
        app.module(Module::new("plugged").add::<Ping>());
    }
}

/// The bot's `configure`.
fn configure<'b>(builder: &'b mut AppBuilder, log: &Arc<Log>) -> &'b mut AppBuilder {
    builder
        .state(Arc::clone(log))
        .payload_key(PayloadKey::from_hex(&"ab".repeat(32)).unwrap())
        .module(
            Module::new("bot")
                .add::<Ping>()
                .add::<Whoami>()
                .add::<SetupStart>()
                .add::<ConfigSetPrefix>()
                .add::<Dev>()
                .add::<Opts>()
                .add::<ShowProfile>()
                .add::<Quote>()
                .add::<Panel>()
                .add::<Render>()
                .add::<Selects>()
                .add::<Eph>()
                .add::<Follow>()
                .add::<FollowButton>()
                .add::<Broken>()
                .add::<Slow>()
                .add::<Sluggish>()
                .add::<DeferOnly>()
                .add::<Fetch>()
                .add::<Sleepy>()
                .add::<Legacy>()
                .add::<Buttons>()
                .add::<AskNote>()
                .add::<Warn>()
                .add::<Echo>()
                .add::<Owned>()
                .add::<Staff>()
                .add::<OpenNote>()
                .add::<NoteForm>()
                .add::<OpenSurvey>()
                .add::<Survey>()
                .add::<SlashNote>()
                .add::<SlowUpd>()
                .add::<Turn>()
                .add::<ByRef>()
                .add::<Sig>()
                .add::<PickStr>()
                .add::<PickUser>()
                .add::<PickRole>()
                .add::<PickChan>()
                .add::<PickWho>(),
        )
        .module(
            Module::new("quiet")
                .before(Middleware::before(stop_all))
                .add::<Quiet>(),
        )
}

fn guild() -> TestGuild {
    TestGuild::new(G, "Guild")
        .text_channel(C, "general")
        .role(R, "Staff")
}

/// The fixture App, after `extra` on its builder.
async fn start_with(extra: impl FnOnce(&mut AppBuilder)) -> (TestApp, Arc<Log>) {
    let log = Arc::new(Log::default());
    let mut builder = App::builder();
    configure(&mut builder, &log);
    extra(&mut builder);
    let app = TestApp::build(&mut builder)
        .await
        .unwrap()
        .guild(guild())
        .user(
            TestUser::new(A)
                .name("ana")
                .permissions(Permissions::MANAGE_GUILD)
                .roles([R]),
        )
        .user(TestUser::new(M).name("mo"));
    (app, log)
}

async fn start() -> (TestApp, Arc<Log>) {
    start_with(|_| {}).await
}

/// The first component of `response` on Route `route`.
fn by_route(response: &Response, route: &str) -> TestComponent {
    response
        .components()
        .into_iter()
        .find(|c| c.route() == Some(route))
        .unwrap_or_else(|| panic!("no {route} in {}", response.message_json()))
}

fn labels(components: &[TestComponent]) -> Vec<Option<&str>> {
    components.iter().map(TestComponent::label).collect()
}

// --- Build and lifecycle -------------------------------------------------------

/// L1, L5, L6.
#[tokio::test(flavor = "multi_thread")]
async fn build_connects_one_shard_and_stops() -> Result<(), TestError> {
    let (app, _log) = start_with(|b| {
        b.publisher(true);
    })
    .await;
    let requests = app.http_fake().requests();
    assert!(
        requests
            .iter()
            .any(|r| r.route_path() == Some("/gateway/bot")),
        "{requests:?}"
    );
    assert_eq!(app.gateway_fake().live_sessions(), 1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let ping = app.command("ping").run().await?;
    let puts = app
        .http_fake()
        .requests()
        .into_iter()
        .chain(requests)
        .filter(|r| r.method.as_str() == "PUT")
        .count()
        + ping
            .http_calls()
            .iter()
            .filter(|c| c.method() == "PUT")
            .count();
    assert_eq!(puts, 0);
    app.stop().await
}

/// L2: a current-thread runtime is refused before any fake starts.
#[test]
fn current_thread_runtime_is_refused() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = runtime
        .block_on(async {
            let mut builder = App::builder();
            builder.module(Module::new("m").add::<Ping>());
            TestApp::build(&mut builder).await
        })
        .unwrap_err();
    assert!(matches!(err, TestError::Runtime), "{err}");
    assert!(
        err.to_string().contains("flavor = \"multi_thread\""),
        "{err}"
    );
}

/// L3: a duplicate Route fails the build with its `BuildError`.
#[tokio::test(flavor = "multi_thread")]
async fn duplicate_route_is_a_build_error() {
    let mut builder = App::builder();
    builder
        .module(Module::new("a").add::<Echo>())
        .module(Module::new("b").add::<Echo>());
    let err = TestApp::build(&mut builder).await.unwrap_err();
    let TestError::Build(build) = &err else {
        panic!("{err}");
    };
    assert!(build.to_string().contains("echo"), "{build}");
}

/// L4: a Plugin's Module is mounted as in production.
#[tokio::test(flavor = "multi_thread")]
async fn plugin_modules_are_mounted() -> Result<(), TestError> {
    let mut builder = App::builder();
    builder.plugin(PingPlugin);
    let app = TestApp::build(&mut builder).await?;
    assert_eq!(app.command("ping").run().await?.text(), "pong");
    app.stop().await
}

/// L7: a run past the timeout fails; the next run works.
#[tokio::test(flavor = "multi_thread")]
async fn timed_out_run_does_not_break_the_app() -> Result<(), TestError> {
    let (app, _log) = start().await;
    let app = app.timeout(Duration::from_millis(200));
    let err = app.command("sleepy").run().await.unwrap_err();
    assert!(matches!(err, TestError::Timeout { .. }), "{err}");
    assert_eq!(app.command("ping").run().await?.text(), "pong");
    app.stop().await
}

// --- Seeds ---------------------------------------------------------------------

/// S1: the member's permissions reach the guard and the handler.
#[tokio::test(flavor = "multi_thread")]
async fn member_permissions_come_from_the_seed() -> Result<(), TestError> {
    let (app, log) = start().await;
    let buttons = app.command("buttons").by(A).in_guild(G).run().await?;
    let staff = buttons.button("Staff")?;
    let ok = app.click(staff.clone()).run().await?;
    assert_eq!(ok.text(), "staff");
    assert!(log.has(&format!(
        "staff {:?}",
        Some(Permissions::MANAGE_GUILD.bits())
    )));
    let denied = app.click(staff).by(M).run().await?;
    assert!(denied.is_guard_notice(), "{}", denied.message_json());
    Ok(())
}

/// S2, S5, S6, S7, S8.
#[tokio::test(flavor = "multi_thread")]
async fn defaults_dms_and_locales() -> Result<(), TestError> {
    let (app, log) = start().await;
    let app = app
        .guild(TestGuild::new(Id::new(11), "Old").locale("de"))
        .guild(TestGuild::new(Id::new(11), "Bare").locale("fr"));
    app.command("whoami").run().await?;
    app.command("whoami")
        .by(A)
        .in_guild(G)
        .locale("tr")
        .run()
        .await?;
    app.command("whoami").in_guild(Id::new(11)).run().await?;
    assert_eq!(
        log.lines(),
        [
            "whoami 1 None Some(1) Some(\"en-US\") None",
            "whoami 40 Some(10) Some(20) Some(\"tr\") Some(\"en-US\")",
            "whoami 1 Some(11) Some(11) Some(\"en-US\") Some(\"fr\")",
        ]
    );
    Ok(())
}

/// S3, S4: unseeded ids fail before dispatch.
#[tokio::test(flavor = "multi_thread")]
async fn unseeded_ids_are_refused() {
    let (app, log) = start().await;
    let err = app
        .command("whoami")
        .by(Id::new(99))
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::Unknown {
                kind: "user",
                id: 99
            }
        ),
        "{err}"
    );
    let err = app
        .command("whoami")
        .in_guild(Id::new(98))
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::Unknown {
                kind: "guild",
                id: 98
            }
        ),
        "{err}"
    );
    let err = app
        .command("whoami")
        .in_guild(G)
        .in_channel(Id::new(97))
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::Unknown {
                kind: "channel",
                id: 97
            }
        ),
        "{err}"
    );
    assert!(log.lines().is_empty());
    assert!(
        app.http_fake()
            .take_requests()
            .iter()
            .all(|r| r.route_path() == Some("/gateway/bot"))
    );
}

// --- Commands ------------------------------------------------------------------

/// C1, C2, C3.
#[tokio::test(flavor = "multi_thread")]
async fn command_paths_and_scopes() -> Result<(), TestError> {
    let (app, log) = start().await;
    app.command("setup start").run().await?;
    app.command("config set prefix")
        .option("value", "!")
        .run()
        .await?;
    assert_eq!(
        log.lines(),
        ["path setup start", "path config set prefix !"]
    );
    let err = app.command("setp start").run().await.unwrap_err();
    assert!(
        matches!(&err, TestError::UnknownCommand { path } if path == "setp start"),
        "{err}"
    );
    let app = app.guild(TestGuild::new(Id::new(11), "Other"));
    assert_eq!(app.command("dev").in_guild(G).run().await?.text(), "dev");
    for guild in [Some(Id::new(11)), None] {
        let run = app.command("dev");
        let run = match guild {
            Some(guild) => run.in_guild(guild),
            None => run,
        };
        let err = run.run().await.unwrap_err();
        assert!(matches!(err, TestError::UnknownCommand { .. }), "{err}");
    }
    Ok(())
}

/// C4, C5, C7 (edge), C10 (valid), C11.
#[tokio::test(flavor = "multi_thread")]
async fn option_values_reach_the_handler() -> Result<(), TestError> {
    let (app, log) = start().await;
    app.command("opts")
        .option("count", 5)
        .option("reason", "😀 ünï")
        .option("ratio", 5)
        .option("mode", "fast")
        .run()
        .await?;
    app.command("opts")
        .option("count", 2_i64.pow(53) - 1)
        .option("ratio", 0.5)
        .run()
        .await?;
    app.command("opts")
        .by(A)
        .in_guild(G)
        .option("count", 1)
        .option("user", M)
        .option("channel", C)
        .option("role", R)
        .option("who", M)
        .run()
        .await?;
    app.command("opts")
        .in_guild(G)
        .option("count", 2)
        .option("who", R)
        .run()
        .await?;
    assert_eq!(
        log.lines(),
        [
            "opts count=5 reason=Some(\"😀 ünï\") ratio=Some(5.0) mode=Some(Fast) user=None member=false channel=None role=None who=None",
            "opts count=9007199254740991 reason=None ratio=Some(0.5) mode=None user=None member=false channel=None role=None who=None",
            "opts count=1 reason=None ratio=None mode=None user=Some(41) member=true channel=Some(20) role=Some(30) who=Some(\"user 41\")",
            "opts count=2 reason=None ratio=None mode=None user=None member=false channel=None role=None who=Some(\"role 30\")",
        ]
    );
    Ok(())
}

/// C6, C7, C8, C9, C10, C12: refused before dispatch.
#[tokio::test(flavor = "multi_thread")]
async fn invalid_options_are_refused() {
    let (app, log) = start().await;
    let err = app
        .command("opts")
        .option("count", 0.5)
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TestError::OptionType { option, expected: "INTEGER", .. } if option == "count"),
        "{err}"
    );
    let err = app
        .command("opts")
        .option("count", "x")
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::OptionType {
                expected: "INTEGER",
                ..
            }
        ),
        "{err}"
    );
    let err = app
        .command("opts")
        .option("count", 1)
        .option("ratio", f64::NAN)
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::InvalidOption { .. }), "{err}");
    let err = app
        .command("opts")
        .option("count", 2_i64.pow(53))
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::InvalidOption { .. }), "{err}");
    let err = app
        .command("opts")
        .option("count", 1)
        .option("nope", 1)
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TestError::UnknownOption { path, option } if path == "opts" && option == "nope"),
        "{err}"
    );
    let err = app.command("opts").run().await.unwrap_err();
    assert!(
        matches!(&err, TestError::MissingOption { option } if option == "count"),
        "{err}"
    );
    let err = app
        .command("opts")
        .option("count", 1)
        .option("mode", "turbo")
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TestError::NotAChoice { option, value } if option == "mode" && value == "turbo"),
        "{err}"
    );
    let err = app
        .command("opts")
        .option("count", 1)
        .option("user", Id::<UserMarker>::new(99))
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::Unknown {
                kind: "user",
                id: 99
            }
        ),
        "{err}"
    );
    assert!(log.lines().is_empty());
}

/// C13: client-side lengths are not simulated, so the validator runs.
#[tokio::test(flavor = "multi_thread")]
async fn server_side_validation_is_reachable() -> Result<(), TestError> {
    let (app, log) = start().await;
    let response = app
        .command("opts")
        .option("count", 1)
        .option("reason", "far too long for ten")
        .run()
        .await?;
    assert!(response.is_error(), "{}", response.message_json());
    assert!(response.is_ephemeral());
    assert!(log.lines().is_empty());
    Ok(())
}

/// C14: context menus carry their resolved target.
#[tokio::test(flavor = "multi_thread")]
async fn context_menus_resolve_their_target() -> Result<(), TestError> {
    let (app, log) = start().await;
    app.user_command("Show profile")
        .target_user(M)
        .in_guild(G)
        .run()
        .await?;
    app.message_command("Quote")
        .target_message(Id::new(555))
        .run()
        .await?;
    assert_eq!(log.lines(), ["profile 41 member=true", "quote 555"]);
    Ok(())
}

// --- Response kinds ------------------------------------------------------------

/// K1, K2, K8, K9, K13, K14.
#[tokio::test(flavor = "multi_thread")]
async fn replies_follow_ups_errors_and_silence() -> Result<(), TestError> {
    let (app, log) = start().await;
    let pong = app.command("ping").run().await?;
    assert_eq!(
        (pong.kind(), pong.callback_type(), pong.is_ephemeral()),
        (ResponseKind::Reply, Some(4), false)
    );
    assert_eq!(pong.text(), "pong");

    let you = app.command("whoami").run().await?;
    assert_eq!(
        (you.kind(), you.callback_type(), you.is_ephemeral()),
        (ResponseKind::Reply, Some(4), true)
    );

    let follow = app.command("follow").run().await?;
    assert_eq!(follow.kind(), ResponseKind::Reply);
    assert_eq!(follow.text(), "main");
    let followups = follow.followups();
    assert_eq!(followups.len(), 2);
    assert!(followups.iter().all(TestMessage::is_ephemeral));
    assert_eq!(
        followups.iter().map(TestMessage::text).collect::<Vec<_>>(),
        ["one", "two"]
    );

    let broken = app.command("broken").run().await?;
    assert!(broken.is_error());
    assert_eq!(broken.text(), GENERIC);

    let quiet = app.command("quiet").run().await?;
    assert_eq!(
        (quiet.kind(), quiet.callback_type()),
        (ResponseKind::None, None)
    );
    assert!(quiet.message().is_none());
    assert!(!log.has("quiet ran"));

    let fetched = app.command("fetch").run().await?;
    assert!(
        fetched
            .http_calls()
            .iter()
            .any(|c| c.method() == "GET" && c.path() == "/channels/20"),
        "{:?}",
        fetched.http_calls()
    );
    let line = log
        .lines()
        .into_iter()
        .find(|l| l.starts_with("fetch "))
        .unwrap();
    assert!(line.contains("404"), "{line}");
    Ok(())
}

/// K6, M13: a defer without an edit.
#[tokio::test(flavor = "multi_thread")]
async fn bare_defer_is_deferred() -> Result<(), TestError> {
    let (app, _log) = start().await;
    let deferred = app.command("deferonly").run().await?;
    assert_eq!(
        (deferred.kind(), deferred.callback_type()),
        (ResponseKind::Deferred, Some(5))
    );
    assert!(deferred.message().is_none());
    assert_eq!(deferred.text(), "");
    assert_eq!(deferred.message_json(), Value::Null);
    assert_eq!(deferred.cost(), WireCost::default());
    assert!(matches!(
        deferred.button("Go"),
        Err(TestError::NotFound { .. })
    ));
    Ok(())
}

/// K3, K5, K10: the ack-deadline equivalences.
#[tokio::test(flavor = "multi_thread")]
async fn auto_defer_equivalences() -> Result<(), TestError> {
    let (app, _log) = start_with(|b| {
        b.ack_deadline(Duration::from_millis(100));
    })
    .await;
    let late = app.command("slow").run().await?;
    assert_eq!(
        (late.kind(), late.callback_type(), late.is_ephemeral()),
        (ResponseKind::Reply, Some(5), false)
    );
    assert_eq!(late.text(), "late");

    let buttons = app.command("buttons").run().await?;
    let updated = app.click(buttons.button("Slow")?).run().await?;
    assert_eq!(
        (updated.kind(), updated.callback_type()),
        (ResponseKind::Update, Some(6))
    );
    assert_eq!(updated.text(), "later");
    assert_eq!(
        updated.message().map(TestMessage::id),
        buttons.message().map(TestMessage::id)
    );

    let failed = app.command("sluggish").run().await?;
    assert_eq!(
        (failed.kind(), failed.callback_type()),
        (ResponseKind::Deferred, Some(5))
    );
    assert!(failed.message().is_none());
    assert_eq!(failed.followups().len(), 1);
    assert!(failed.followups()[0].is_ephemeral());
    assert!(failed.is_error());
    Ok(())
}

/// K4, X1: an update keeps the source's id and ephemerality.
#[tokio::test(flavor = "multi_thread")]
async fn click_updates_in_place() -> Result<(), TestError> {
    let (app, log) = start().await;
    let shown = app.command("panel").run().await?;
    let next = app.click(shown.button("Next")?).run().await?;
    assert_eq!(
        (next.kind(), next.callback_type(), next.is_ephemeral()),
        (ResponseKind::Update, Some(7), false)
    );
    assert_eq!(next.text(), "echo 2");
    assert_eq!(
        next.message().map(TestMessage::id),
        shown.message().map(TestMessage::id)
    );
    assert_eq!(log.lines(), ["echo 2 by 1"]);
    Ok(())
}

/// K7, K11, K12, F8.
#[tokio::test(flavor = "multi_thread")]
async fn modals_guards_and_stale_ids() -> Result<(), TestError> {
    let (app, log) = start().await;
    let buttons = app.command("buttons").by(A).in_guild(G).run().await?;
    let modal = app.click(buttons.button("Note")?).run().await?;
    assert_eq!(
        (modal.kind(), modal.callback_type()),
        (ResponseKind::Modal, Some(9))
    );
    assert_eq!(modal.modal()?.title(), "Note");

    let guarded = app.click(buttons.button("Mine")?).by(M).run().await?;
    assert!(guarded.is_guard_notice(), "{}", guarded.message_json());
    assert!(guarded.is_ephemeral());
    assert!(!log.lines().iter().any(|l| l.starts_with("owned")));

    let stale = app.click(buttons.button("Old")?).run().await?;
    assert!(stale.is_expired_notice(), "{}", stale.message_json());

    let err = buttons.modal().unwrap_err();
    assert!(
        matches!(&err, TestError::NotFound { what, .. } if what == "modal"),
        "{err}"
    );
    Ok(())
}

// --- Messages ------------------------------------------------------------------

/// M1, M2, M5, M6.
#[tokio::test(flavor = "multi_thread")]
async fn message_finders_and_cost() -> Result<(), TestError> {
    let (app, _log) = start().await;
    let shown = app.command("panel").run().await?;
    assert_eq!(shown.text(), "## Title\na");
    let buttons = shown.buttons();
    assert_eq!(
        labels(&buttons),
        [Some("Go"), Some("Next"), Some("Prev"), Some("Docs")]
    );
    assert!(shown.button("Prev")?.is_disabled());
    assert_eq!(shown.button("Next")?.style(), Some(ButtonStyle::Primary));
    assert_eq!(shown.button("Docs")?.style(), None);

    let err = shown.button("Missing").unwrap_err();
    let TestError::NotFound { what, available } = &err else {
        panic!("{err}");
    };
    assert_eq!(what, "button \"Missing\"");
    assert_eq!(available, &["Go", "Next", "Prev", "Docs"]);

    assert_eq!(shown.component::<Echo>()?.label(), Some("Go"));
    let pong = app.command("ping").run().await?;
    assert!(matches!(
        pong.component::<Echo>(),
        Err(TestError::NotFound { .. })
    ));

    let report = noctis::ui::cost(&panel(), &Limits::default());
    assert_eq!(
        shown.cost(),
        WireCost {
            components: 9,
            text_chars: 9
        }
    );
    assert_eq!(
        (shown.cost().components, shown.cost().text_chars),
        (report.components, report.text_chars)
    );
    Ok(())
}

/// M8: a legacy reply.
#[tokio::test(flavor = "multi_thread")]
async fn legacy_reply_cost() -> Result<(), TestError> {
    let (app, _log) = start().await;
    let legacy = app.command("legacy").run().await?;
    assert_eq!(
        legacy.cost(),
        WireCost {
            components: 3,
            text_chars: 2
        }
    );
    assert_eq!(legacy.text(), "hi");
    Ok(())
}

/// M9, M10, M11, X11: masked snapshots of a render.
#[tokio::test(flavor = "multi_thread")]
async fn snapshots_mask_only_volatile_parts() -> Result<(), TestError> {
    let (app, log) = start().await;
    let first = app.command("render").run().await?;
    let second = app.command("render").run().await?;
    let masked = first.message_json();
    let ids: Vec<String> = first
        .components()
        .iter()
        .filter_map(|c| c.custom_id().map(str::to_owned))
        .collect();
    let masked_ids: Vec<String> = noctis_testkit_ids(&masked);
    assert_eq!(masked_ids.len(), 3, "{masked}");
    assert!(masked_ids[0].starts_with("4pick:NN"), "{masked_ids:?}");
    assert!(masked_ids[1].ends_with("TTTTTT"), "{masked_ids:?}");
    assert!(masked_ids[2].ends_with("~RRRRRRRRRRR"), "{masked_ids:?}");
    assert_eq!(ids.len(), 3);
    for id in &ids {
        assert!(
            !id.contains("NN") && !id.contains("TTTTTT") && !id.contains("RRRRRRRRRRR"),
            "{id}"
        );
    }
    let raw = first.message().unwrap().raw_json();
    assert_eq!(noctis_testkit_ids(raw), ids);

    assert_eq!(second.message_json(), masked);
    insta::assert_json_snapshot!("render_panel", first.message_json());
    insta::assert_json_snapshot!("render_panel", second.message_json());
    assert_eq!(render_json(&render_panel())?, masked);

    let clicked = app.click(first.button("Ref")?).run().await?;
    assert_eq!(clicked.text(), "ref ok");
    assert!(log.has("byref 3"));
    Ok(())
}

/// Every `custom_id` of `json`, in pre-order.
fn noctis_testkit_ids(json: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_ids(json, &mut out);
    out
}

fn collect_ids(json: &Value, out: &mut Vec<String>) {
    match json {
        Value::Object(map) => {
            if let Some(Value::String(id)) = map.get("custom_id") {
                out.push(id.clone());
            }
            for (key, value) in map {
                if key != "custom_id" {
                    collect_ids(value, out);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_ids(v, out)),
        _ => {}
    }
}

// --- Clicks and selects --------------------------------------------------------

/// X2, X3, X4.
#[tokio::test(flavor = "multi_thread")]
async fn unusable_components_are_refused() -> Result<(), TestError> {
    let (app, log) = start().await;
    let shown = app.command("panel").run().await?;
    let err = app.click(shown.button("Prev")?).run().await.unwrap_err();
    assert!(matches!(err, TestError::Disabled { .. }), "{err}");
    let err = app.click(shown.button("Docs")?).run().await.unwrap_err();
    assert!(matches!(err, TestError::NotInteractive), "{err}");

    let selects = app.command("selects").run().await?;
    let err = app
        .click(by_route(&selects, "pick"))
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::WrongComponent {
                expected: "button",
                ..
            }
        ),
        "{err}"
    );
    let err = app
        .select(selects.button("Echo")?)
        .values(["a"])
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::WrongComponent {
                found: "button",
                ..
            }
        ),
        "{err}"
    );
    assert!(log.lines().is_empty());
    Ok(())
}

/// X5, X6, X12.
#[tokio::test(flavor = "multi_thread")]
async fn string_select_bounds_and_freshness() -> Result<(), TestError> {
    let (app, log) = start().await;
    let selects = app.command("selects").run().await?;
    let pick = by_route(&selects, "pick");
    assert_eq!(pick.options(), ["a", "b", "c"]);
    for values in [&[][..], &["a", "b", "c"][..]] {
        let err = app
            .select(pick.clone())
            .values(values.iter().copied())
            .run()
            .await
            .unwrap_err();
        assert!(
            matches!(err, TestError::SelectValues { min: 1, max: 2, .. }),
            "{err}"
        );
    }
    let err = app
        .select(pick.clone())
        .values(["zzz"])
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TestError::NotAnOption { value } if value == "zzz"),
        "{err}"
    );
    app.select(pick.clone()).values(["a"]).run().await?;
    let rerendered = app.select(pick.clone()).values(["a", "b"]).run().await?;
    assert_eq!(rerendered.kind(), ResponseKind::Update);
    let fresh = by_route(&rerendered, "pick");
    assert_ne!(fresh.custom_id(), pick.custom_id());
    assert_eq!(log.lines(), ["pick", "pick"]);
    Ok(())
}

/// X7, X8: entity selects carry their resolved data.
#[tokio::test(flavor = "multi_thread")]
async fn entity_selects_resolve_seeds() -> Result<(), TestError> {
    let (app, log) = start().await;
    let selects = app.command("selects").by(A).in_guild(G).run().await?;
    app.select(by_route(&selects, "pick.user"))
        .users([M])
        .run()
        .await?;
    app.select(by_route(&selects, "pick.role"))
        .roles([R])
        .run()
        .await?;
    app.select(by_route(&selects, "pick.chan"))
        .channels([C])
        .run()
        .await?;
    app.select(by_route(&selects, "pick.who"))
        .users([M])
        .roles([R])
        .run()
        .await?;
    let err = app
        .select(by_route(&selects, "pick.role"))
        .roles([Id::new(99)])
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            TestError::Unknown {
                kind: "role",
                id: 99
            }
        ),
        "{err}"
    );
    assert_eq!(
        log.lines(),
        ["pick.user", "pick.role", "pick.chan", "pick.who"]
    );
    Ok(())
}

/// X9, X10.
#[tokio::test(flavor = "multi_thread")]
async fn ephemeral_visibility_and_follow_up_sources() -> Result<(), TestError> {
    let (app, log) = start().await;
    let secret = app.command("eph").by(A).in_guild(G).run().await?;
    assert!(secret.is_ephemeral());
    let err = app
        .click(secret.button("Echo")?)
        .by(M)
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::NotVisible { user: 41 }), "{err}");
    let mine = app.click(secret.button("Echo")?).run().await?;
    assert_eq!(mine.text(), "echo 9");
    assert!(mine.is_ephemeral());

    let shown = app.command("followbtn").run().await?;
    let followup = &shown.followups()[0];
    let clicked = app.click(followup.button("Echo")?).run().await?;
    assert_eq!(clicked.message().map(TestMessage::id), Some(followup.id()));
    assert_eq!(log.lines(), ["echo 9 by 40", "echo 8 by 1"]);
    Ok(())
}

// --- Modals --------------------------------------------------------------------

/// F1, F2, F3, F4.
#[tokio::test(flavor = "multi_thread")]
async fn text_input_submits() -> Result<(), TestError> {
    let (app, log) = start().await;
    let buttons = app.command("buttons").run().await?;
    let opened = app.click(buttons.button("Note")?).run().await?;
    let form = opened.modal()?;
    assert_eq!(form.title(), "Note");

    let err = app.submit(form.clone()).run().await.unwrap_err();
    assert!(
        matches!(&err, TestError::InvalidInput { key, .. } if key == "note"),
        "{err}"
    );
    for too_long in ["123456", "😀😀😀"] {
        let err = app
            .submit(form.clone().text("note", too_long))
            .run()
            .await
            .unwrap_err();
        assert!(matches!(err, TestError::InvalidInput { .. }), "{err}");
    }
    let err = app
        .submit(form.clone().text("nope", "x"))
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TestError::UnknownInput { key } if key == "nope"),
        "{err}"
    );
    let err = app
        .submit(form.clone().checkbox("note", true))
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::WrongInput { .. }), "{err}");

    let edge = app.submit(form.clone().text("note", "12345")).run().await?;
    assert_eq!(edge.text(), "noted 12345");
    let submitted = app.submit(form.text("note", "hi")).run().await?;
    assert_eq!(submitted.kind(), ResponseKind::Update);
    assert_eq!(submitted.text(), "noted hi");
    assert_eq!(
        submitted.message().map(TestMessage::id),
        buttons.message().map(TestMessage::id)
    );
    assert_eq!(log.lines(), ["note 7 12345", "note 7 hi"]);
    Ok(())
}

/// F6: radio, checkbox group and checkbox inputs.
#[tokio::test(flavor = "multi_thread")]
async fn choice_inputs_submit() -> Result<(), TestError> {
    let (app, log) = start().await;
    let buttons = app.command("buttons").run().await?;
    let form = app.click(buttons.button("Survey")?).run().await?.modal()?;
    let saved = app
        .submit(
            form.radio("severity", "h")
                .values("topics", ["art"])
                .checkbox("agree", true),
        )
        .run()
        .await?;
    assert_eq!(saved.text(), "saved");
    assert_eq!(log.lines(), ["survey High [Art] true"]);
    Ok(())
}

/// F7: a slash-opened modal cannot update, so it replies.
#[tokio::test(flavor = "multi_thread")]
async fn slash_opened_modal_replies() -> Result<(), TestError> {
    let (app, log) = start().await;
    let form = app.command("asknote").run().await?.modal()?;
    let answered = app.submit(form.text("text", "x")).run().await?;
    assert_eq!(
        (answered.kind(), answered.callback_type()),
        (ResponseKind::Reply, Some(4))
    );
    assert_eq!(answered.text(), "answered");
    assert_eq!(log.lines(), ["unavailable Some(\"x\")"]);
    Ok(())
}

// --- Autocomplete --------------------------------------------------------------

/// A1-A4.
#[tokio::test(flavor = "multi_thread")]
async fn autocomplete_suggestions() -> Result<(), TestError> {
    let (app, log) = start().await;
    let typed = app.autocomplete("warn", "rule", "sp").run().await?;
    assert_eq!(
        (typed.kind(), typed.callback_type()),
        (ResponseKind::Autocomplete, Some(8))
    );
    assert_eq!(typed.suggestions().len(), 25);
    assert_eq!(
        typed.suggestions()[0],
        ("r0".to_owned(), "sp rule 0".to_owned())
    );
    app.autocomplete("warn", "rule", "").run().await?;
    assert_eq!(log.lines(), ["typed \"sp\"", "typed \"\""]);

    let err = app
        .autocomplete("warn", "note", "x")
        .run()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, TestError::NotAutocomplete { option } if option == "note"),
        "{err}"
    );
    let err = app
        .autocomplete("warn", "nope", "x")
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::UnknownOption { .. }), "{err}");
    Ok(())
}

// --- Memory --------------------------------------------------------------------

/// Mem5: this binary has no dhat allocator.
#[tokio::test(flavor = "multi_thread")]
async fn memory_check_needs_the_dhat_allocator() {
    let (app, _log) = start().await;
    let err = noctis_testkit::assert_memory_flat(&app, 10, |app| async move {
        app.command("ping").run().await.map(drop)
    })
    .await
    .unwrap_err();
    assert!(matches!(err, TestError::AllocatorMissing), "{err}");
    assert!(err.to_string().contains("dhat::Alloc"), "{err}");
}
