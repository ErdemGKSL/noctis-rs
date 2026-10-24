//! Story 12.5 end to end (E1-E8): the Discord cache is fed from the real
//! gateway path and the interaction tasks, and every context reads it with
//! `ctx.cache()`. Its own test binary, because it installs a global
//! capturing tracing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::fmt::Write as _;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use noctis::events::{EventCtx, GuildCreate, RoleCreate};
use noctis::ports::memory::{DiscordCacheResources, MemoryDiscordCache};
use noctis::ports::{BoxFuture, DiscordCache, PortError};
use noctis::prelude::*;
use noctis::twilight::model::channel::{Channel, Message};
use noctis::twilight::model::gateway::Intents;
use noctis::twilight::model::gateway::event::{
    DispatchEvent, Event, GatewayEvent, GatewayEventDeserializer,
};
use noctis::twilight::model::guild::{Guild, Member, Role};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use noctis::twilight::model::user::User;
use noctis::ui::{Components, ResolvedUser, TextDisplay, V2Message};
use noctis::{CacheDisabled, CacheError, DiscordCacheConfig, ModalResponder};
use noctis_testkit::{TestApp, TestError, TestGuild, TestUser};
use serde::Serialize;
use serde::de::{DeserializeOwned, DeserializeSeed};
use serde_json::{Value, json};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

const G: Id<GuildMarker> = Id::new(10);
const WAIT: Duration = Duration::from_secs(15);

// --- Logs and observations --------------------------------------------------

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn count(&self, needles: &[&str]) -> usize {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|line| needles.iter().all(|n| line.contains(n)))
            .count()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The process-wide capture, installed once.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::INFO)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        capture
    })
}

/// What the handlers saw (one per App).
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

    async fn wait_for(&self, line: &str) {
        tokio::time::timeout(WAIT, async {
            while !self.has(line) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{line:?} never seen; saw {:?}", self.lines()));
    }
}

fn view(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

/// `guild 10` as one line: its name, or why there is none.
async fn guild_line(cache: &noctis::DiscordCacheReader) -> String {
    match cache.guild(G).await {
        Ok(Some(guild)) => format!("guild {}", guild.name),
        Ok(None) => "guild none".to_owned(),
        Err(CacheError::Disabled(CacheDisabled { resource, .. })) => {
            format!("guild disabled {resource}")
        }
        Err(other) => format!("guild error {other}"),
    }
}

// --- Handlers -----------------------------------------------------------------

/// Reads guild 10, its roles and channel 20.
#[derive(SlashCommand)]
#[command("info", description = "Guild info")]
struct Info;

impl Info {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let cache = ctx.cache();
        let mut line = guild_line(&cache).await;
        if let Ok(Some(roles)) = cache.roles(G).await {
            let names: Vec<String> = roles.into_iter().map(|r| r.name).collect();
            let _ = write!(line, " roles {}", names.join(","));
        }
        if let Ok(Some(channel)) = cache.channel(Id::new(20)).await {
            let _ = write!(line, " channel {}", channel.id);
        }
        ctx.state::<Arc<Log>>().unwrap().push(line.clone());
        ctx.respond(view(&line)).await
    }
}

/// Reads users 40 and 41 (`who` is a resolved user option).
#[derive(SlashCommand)]
#[command("users", description = "Users")]
struct Users {
    #[option(description = "Someone")]
    who: ResolvedUser,
}

impl Users {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let cache = ctx.cache();
        let a = cache.user(Id::new(40)).await?.map(|u| u.name);
        let b = cache.user(self.who.user.id).await?.map(|u| u.name);
        ctx.state::<Arc<Log>>()
            .unwrap()
            .push(format!("users {a:?} {b:?}"));
        ctx.respond(view("users")).await
    }
}

/// Shows [`Look`], which reads the cache from a `ComponentCtx` and opens
/// [`Ask`] (a `ModalSubmitCtx`).
#[derive(SlashCommand)]
#[command("panel", description = "Panel")]
struct Panel;

impl Panel {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(V2Message::new(
            Components::new().push(noctis::ui::ActionRow::buttons([Look.build()])),
        ))
        .await
    }
}

#[derive(Button, Debug, Clone, Copy)]
#[button("look", auto_defer = false, defaults(style = Primary, label = "Look"))]
struct Look;

impl Look {
    #[handler]
    async fn handle(
        self,
        ctx: ComponentCtx,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        let line = guild_line(&ctx.cache()).await;
        let user = ctx.cache().user(ctx.user_id()).await;
        let log = ctx.state::<Arc<Log>>().unwrap();
        log.push(format!("component {line}"));
        log.push(format!(
            "component user {} {}",
            ctx.user_id(),
            matches!(user, Ok(Some(_)))
        ));
        Ok(r.show_modal(Ask::blank()).await?.done())
    }
}

#[derive(Modal, Debug)]
#[modal("ask", defaults(title = "Ask"))]
struct Ask {
    #[text_input("Text")]
    text: Option<String>,
}

impl Ask {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx, _r: ModalResponder) -> HandlerResult<Responded> {
        let line = guild_line(&ctx.cache()).await;
        ctx.state::<Arc<Log>>()
            .unwrap()
            .push(format!("modal {line}"));
        ctx.respond(view("asked")).await
    }
}

/// Autocomplete reads the cache from an `AutocompleteCtx`.
#[derive(SlashCommand)]
#[command("pick", description = "Pick")]
struct Pick {
    #[option(description = "Thing", autocomplete)]
    thing: String,
}

impl Pick {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(view(&self.thing)).await
    }

    #[autocomplete(thing)]
    async fn things(
        ctx: AutocompleteCtx,
        _typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        let line = guild_line(&ctx.cache()).await;
        let user = ctx.cache().user(ctx.user_id()).await;
        ctx.state::<Arc<Log>>().push(format!("autocomplete {line}"));
        ctx.state::<Arc<Log>>().push(format!(
            "autocomplete user {} {}",
            ctx.user_id(),
            matches!(user, Ok(Some(_)))
        ));
        Ok(Suggestions::default())
    }
}

/// Logs what the before-Middleware's `BeforeCtx` reads.
async fn before(ctx: BeforeCtx) -> HandlerResult<Flow> {
    let line = guild_line(&ctx.cache()).await;
    if let Some(log) = ctx.state::<Arc<Log>>() {
        log.push(format!("before {line}"));
    }
    Ok(Flow::Continue)
}

/// Reads the created role and guild 10 from an `EventCtx` (fed before
/// dispatch).
#[derive(EventHandler)]
struct OnRole;

impl OnRole {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: RoleCreate) -> HandlerResult<()> {
        let role = ctx.cache().role(ev.role.id).await?.map(|r| r.name);
        let line = guild_line(&ctx.cache()).await;
        if let Some(log) = ctx.state::<Arc<Log>>() {
            log.push(format!("role {role:?}"));
            log.push(format!("event {line}"));
        }
        Ok(())
    }
}

/// Notes each `GUILD_CREATE` after its handlers' cache feed.
#[derive(EventHandler)]
struct OnGuild;

impl OnGuild {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: GuildCreate) -> HandlerResult<()> {
        if let Some(log) = ctx.state::<Arc<Log>>() {
            log.push(format!("guild-create {}", ev.id()));
        }
        Ok(())
    }
}

/// Reads guild 10 and channel 21 (lives in a Module with no event handlers).
#[derive(SlashCommand)]
#[command("chan", description = "Channel")]
struct Chan;

impl Chan {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let cache = ctx.cache();
        let mut line = guild_line(&cache).await;
        let channel = cache.channel(Id::new(21)).await;
        let _ = write!(line, " channel21 {}", matches!(channel, Ok(Some(_))));
        ctx.state::<Arc<Log>>().unwrap().push(line.clone());
        ctx.respond(view(&line)).await
    }
}

// --- Fixtures -------------------------------------------------------------------

fn module() -> Module {
    Module::new("cache")
        .before(Middleware::before(before))
        .add::<Info>()
        .add::<Users>()
        .add::<Panel>()
        .add::<Look>()
        .add::<Ask>()
        .add::<Pick>()
        .add::<OnRole>()
        .add::<OnGuild>()
}

async fn start(extra: impl FnOnce(&mut AppBuilder)) -> (TestApp, Arc<Log>) {
    start_with(module(), extra).await
}

async fn start_with(module: Module, extra: impl FnOnce(&mut AppBuilder)) -> (TestApp, Arc<Log>) {
    let log = Arc::new(Log::default());
    let mut builder = App::builder();
    builder.state(Arc::clone(&log)).module(module);
    extra(&mut builder);
    let app = TestApp::build(&mut builder)
        .await
        .unwrap()
        .user(TestUser::new(Id::new(40)).name("ana"))
        .user(TestUser::new(Id::new(41)).name("mo"))
        .user(TestUser::new(Id::new(42)).name("ivo"))
        .user(TestUser::new(Id::new(43)).name("lu"));
    (app, log)
}

fn role_json(id: u64, name: &str, position: i64) -> Value {
    json!({
        "id": id.to_string(), "name": name, "color": 0,
        "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null},
        "hoist": false, "managed": false, "mentionable": false,
        "permissions": "0", "position": position, "flags": 0,
    })
}

/// Guild 10 "G" with role 30 "Mods" and channel 20.
fn guild_create() -> Value {
    json!({
        "id": "10", "name": "G", "owner_id": "1", "preferred_locale": "en-US",
        "afk_timeout": 300, "default_message_notifications": 0,
        "explicit_content_filter": 0, "features": [], "mfa_level": 0, "nsfw_level": 0,
        "premium_progress_bar_enabled": false, "premium_tier": 0,
        "system_channel_flags": 0, "verification_level": 0, "emojis": [], "stickers": [],
        "channels": [{"id": "20", "guild_id": "10", "type": 0, "name": "general",
                      "position": 0, "permission_overwrites": []}],
        "roles": [role_json(30, "Mods", 1)],
        "threads": [], "members": [], "voice_states": [], "presences": [],
        "stage_instances": [], "guild_scheduled_events": [],
    })
}

fn seeded_guild() -> TestGuild {
    TestGuild::new(G, "Seeded")
        .text_channel(Id::new(20), "general")
        .role(Id::new(30), "Mods")
        .role(Id::new(31), "Staff")
}

/// The intents of the App's first IDENTIFY.
fn identify_intents(app: &TestApp) -> u64 {
    let identify = app
        .gateway_fake()
        .frames()
        .into_iter()
        .find(|f| f.op() == Some(2))
        .expect("an IDENTIFY");
    identify.json().unwrap()["d"]["intents"].as_u64().unwrap()
}

// --- E1-E8 ------------------------------------------------------------------------

/// E1: a dispatched `GUILD_CREATE` reaches a later command's `ctx.cache()`.
#[tokio::test(flavor = "multi_thread")]
async fn e1_guild_create_feeds_command_reads() -> Result<(), TestError> {
    let (app, log) = start(|_| {}).await;
    assert_eq!(identify_intents(&app), Intents::GUILDS.bits());
    assert_eq!(
        app.gateway_fake().dispatch("GUILD_CREATE", guild_create()),
        1
    );
    log.wait_for("guild-create 10").await;
    let reply = app.command("info").by(Id::new(40)).run().await?;
    assert!(
        log.has("guild G roles Mods channel 20"),
        "{:?}",
        log.lines()
    );
    assert!(reply.text().contains("guild G"), "{}", reply.text());
    app.stop().await
}

/// E2 and E3 (`EventCtx`): the cache is fed before a handler of the same
/// event runs.
#[tokio::test(flavor = "multi_thread")]
async fn e2_event_is_fed_before_its_handlers() -> Result<(), TestError> {
    let (app, log) = start(|_| {}).await;
    app.gateway_fake().dispatch("GUILD_CREATE", guild_create());
    log.wait_for("guild-create 10").await;
    app.gateway_fake().dispatch(
        "GUILD_ROLE_CREATE",
        json!({ "guild_id": "10", "role": role_json(31, "Fresh", 2) }),
    );
    log.wait_for("role Some(\"Fresh\")").await;
    assert!(log.has("event guild G"), "{:?}", log.lines());
    app.stop().await
}

/// E3 and E7: every interaction context reads the seeded guild.
#[tokio::test(flavor = "multi_thread")]
async fn e3_e7_every_context_reads_the_seeded_guild() -> Result<(), TestError> {
    let (app, log) = start(|_| {}).await;
    let app = app.guild(seeded_guild());
    let run = |path| app.command(path).by(Id::new(40)).in_guild(G);
    run("info").run().await?;
    assert!(
        log.has("guild Seeded roles Mods,Staff channel 20"),
        "{:?}",
        log.lines()
    );
    let panel = run("panel").run().await?;
    let opened = app
        .click(panel.button("Look")?)
        .by(Id::new(40))
        .run()
        .await?;
    app.submit(opened.modal()?).by(Id::new(40)).run().await?;
    app.autocomplete("pick", "thing", "x")
        .by(Id::new(40))
        .in_guild(G)
        .run()
        .await?;
    for line in [
        "component guild Seeded",
        "modal guild Seeded",
        "autocomplete guild Seeded",
        "before guild Seeded",
    ] {
        assert!(log.has(line), "{line}: {:?}", log.lines());
    }
    app.stop().await
}

/// E4: interactions feed USER (the invoker of a DM run and a resolved
/// user option).
#[tokio::test(flavor = "multi_thread")]
async fn e4_interactions_feed_users() -> Result<(), TestError> {
    let (app, log) = start(|builder| {
        builder.discord_cache(
            DiscordCacheConfig::default().with_resources(DiscordCacheResources::USER),
        );
    })
    .await;
    app.command("users")
        .by(Id::new(40))
        .option("who", Id::<UserMarker>::new(41))
        .run()
        .await?;
    assert!(
        log.has("users Some(\"ana\") Some(\"mo\")"),
        "{:?}",
        log.lines()
    );
    // A click and an autocomplete by users who never ran a command.
    let panel = app.command("panel").by(Id::new(40)).run().await?;
    app.click(panel.button("Look")?)
        .by(Id::new(42))
        .run()
        .await?;
    app.autocomplete("pick", "thing", "x")
        .by(Id::new(43))
        .run()
        .await?;
    for line in ["component user 42 true", "autocomplete user 43 true"] {
        assert!(log.has(line), "{line}: {:?}", log.lines());
    }
    app.stop().await
}

/// Runs `chan` until it logs `want`.
async fn chan_until(app: &TestApp, log: &Log, want: &str) -> Result<(), TestError> {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !log.has(want) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{want:?} never seen; saw {:?}",
            log.lines()
        );
        app.command("chan").by(Id::new(40)).run().await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

/// E1 without handlers: cache-fed event types pass the gateway filter even
/// when no handler registers them.
#[tokio::test(flavor = "multi_thread")]
async fn e1_cache_events_pass_the_filter_without_handlers() -> Result<(), TestError> {
    let (app, log) = start_with(Module::new("bare").add::<Chan>(), |_| {}).await;
    assert_eq!(
        app.gateway_fake().dispatch("GUILD_CREATE", guild_create()),
        1
    );
    chan_until(&app, &log, "guild G channel21 false").await?;
    assert_eq!(
        app.gateway_fake().dispatch(
            "CHANNEL_CREATE",
            json!({"id": "21", "guild_id": "10", "type": 0, "name": "new",
                   "position": 1, "permission_overwrites": []}),
        ),
        1
    );
    chan_until(&app, &log, "guild G channel21 true").await?;
    app.stop().await
}

/// E5: `none()` disables every getter and adds no intent.
#[tokio::test(flavor = "multi_thread")]
async fn e5_none_is_disabled() -> Result<(), TestError> {
    let (app, log) = start(|builder| {
        builder.discord_cache(DiscordCacheConfig::none());
    })
    .await;
    assert_eq!(identify_intents(&app), Intents::GUILDS.bits());
    let app = app.guild(seeded_guild());
    app.command("info")
        .by(Id::new(40))
        .in_guild(G)
        .run()
        .await?;
    assert!(log.has("guild disabled GUILD"), "{:?}", log.lines());
    app.stop().await
}

/// A Cluster stand-in: a `MemoryDiscordCache` behind a `serde_json` byte
/// boundary, as an out-of-process store would be.
struct SerdeCache(MemoryDiscordCache);

fn bytes<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn back<T: DeserializeOwned>(raw: &[u8]) -> T {
    serde_json::from_slice(raw).unwrap()
}

/// `value` through `serde_json` bytes and back.
fn round<T: Serialize + DeserializeOwned>(value: &T) -> T {
    back(&bytes(value))
}

/// `event` through `serde_json` bytes (as a gateway dispatch) and back.
fn round_event(event: &Event) -> Event {
    let dispatch = DispatchEvent::try_from(event.clone()).expect("a dispatch event");
    let gateway = GatewayEvent::Dispatch(1, dispatch);
    let json = String::from_utf8(bytes(&gateway)).unwrap();
    let seed = GatewayEventDeserializer::from_json(&json).expect("a gateway frame");
    let mut de = serde_json::Deserializer::from_str(&json);
    Event::from(seed.deserialize(&mut de).expect("a valid dispatch"))
}

impl DiscordCache for SerdeCache {
    fn update<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async move { self.0.update(&round_event(event)).await })
    }

    fn guild(&self, id: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.guild(id).await?)) })
    }

    fn channel(&self, id: Id<ChannelMarker>) -> BoxFuture<'_, Result<Option<Channel>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.channel(id).await?)) })
    }

    fn role(&self, id: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.role(id).await?)) })
    }

    fn member(
        &self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
    ) -> BoxFuture<'_, Result<Option<Member>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.member(guild, user).await?)) })
    }

    fn user(&self, id: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.user(id).await?)) })
    }

    fn message(&self, id: Id<MessageMarker>) -> BoxFuture<'_, Result<Option<Message>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.message(id).await?)) })
    }

    fn channel_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.channel_ids(guild).await?)) })
    }

    fn role_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.role_ids(guild).await?)) })
    }

    fn member_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>> {
        Box::pin(async move { Ok(round(&self.0.member_ids(guild).await?)) })
    }
}

/// E6: one backend shared by two Apps; only App A receives the guild.
#[tokio::test(flavor = "multi_thread")]
async fn e6_shared_backend_across_apps() -> Result<(), TestError> {
    let shared = Arc::new(SerdeCache(MemoryDiscordCache::new(
        DiscordCacheResources::all(),
    )));
    let (a, log_a) = start(|builder| {
        builder
            .app_id("app-a")
            .discord_cache_backend(Arc::clone(&shared));
    })
    .await;
    let (b, log_b) = start(|builder| {
        builder
            .app_id("app-b")
            .discord_cache_backend(Arc::clone(&shared));
    })
    .await;
    a.gateway_fake().dispatch("GUILD_CREATE", guild_create());
    log_a.wait_for("guild-create 10").await;
    b.command("info").by(Id::new(40)).run().await?;
    assert!(
        log_b.has("guild G roles Mods channel 20"),
        "{:?}",
        log_b.lines()
    );
    a.stop().await?;
    b.stop().await
}

/// A backend whose every update fails.
struct Failing {
    updates: AtomicUsize,
    reads: MemoryDiscordCache,
}

impl DiscordCache for Failing {
    fn update<'a>(&'a self, _: &'a Event) -> BoxFuture<'a, Result<(), PortError>> {
        self.updates.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(PortError::unavailable("cache store down")) })
    }

    fn guild(&self, id: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>> {
        self.reads.guild(id)
    }

    fn channel(&self, id: Id<ChannelMarker>) -> BoxFuture<'_, Result<Option<Channel>, PortError>> {
        self.reads.channel(id)
    }

    fn role(&self, id: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>> {
        self.reads.role(id)
    }

    fn member(
        &self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
    ) -> BoxFuture<'_, Result<Option<Member>, PortError>> {
        self.reads.member(guild, user)
    }

    fn user(&self, id: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>> {
        self.reads.user(id)
    }

    fn message(&self, id: Id<MessageMarker>) -> BoxFuture<'_, Result<Option<Message>, PortError>> {
        self.reads.message(id)
    }

    fn channel_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>> {
        self.reads.channel_ids(guild)
    }

    fn role_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>> {
        self.reads.role_ids(guild)
    }

    fn member_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>> {
        self.reads.member_ids(guild)
    }
}

/// E8: failing updates never stop events or interactions; one warning.
#[tokio::test(flavor = "multi_thread")]
async fn e8_failing_backend_never_blocks() -> Result<(), TestError> {
    let capture = capture();
    let failing = Arc::new(Failing {
        updates: AtomicUsize::new(0),
        reads: MemoryDiscordCache::new(DiscordCacheResources::all()),
    });
    let (app, log) = start(|builder| {
        builder
            .app_id("e8-failing")
            .discord_cache_backend(Arc::clone(&failing));
    })
    .await;
    app.gateway_fake().dispatch("GUILD_CREATE", guild_create());
    log.wait_for("guild-create 10").await;
    app.gateway_fake().dispatch(
        "GUILD_ROLE_CREATE",
        json!({ "guild_id": "10", "role": role_json(31, "Fresh", 2) }),
    );
    log.wait_for("role None").await;
    app.command("info").by(Id::new(40)).run().await?;
    assert!(log.has("guild none"), "{:?}", log.lines());
    assert!(failing.updates.load(Ordering::SeqCst) >= 3);
    assert_eq!(
        capture.count(&[
            "WARN",
            "noctis::cache",
            "e8-failing",
            "Discord cache updates are failing"
        ]),
        1
    );
    app.stop().await
}
