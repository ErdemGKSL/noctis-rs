//! Story 13.3 end to end: runtime Module enable and disable through the
//! facade with real derives. Two `TestApp`s A and B share one
//! `ModuleStateStore` under one App id; A's `/eco off` in guild 1 disables
//! the Module `eco` there at once for A and within B's poll interval for B:
//! its global Command and its button answer the ephemeral
//! `noctis.module.disabled` notice, its `MESSAGE_CREATE` handler is skipped
//! in guild 1 only, and `ctx.modules().is_enabled` agrees. `/eco on`
//! restores it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use noctis::events::{EventCtx, MessageCreate};
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::GuildMarker;
use noctis::ui::{ActionRow, Button as AnyButton, Components, TextDisplay, V2Message};
use noctis_ports::memory::MemoryModuleStateStore;
use noctis_testkit::{Response, TestApp, TestGuild};
use serde_json::json;

const G1: Id<GuildMarker> = Id::new(1);
const G2: Id<GuildMarker> = Id::new(2);
/// `noctis.module.disabled` in en-US.
const DISABLED: &str = "This feature is disabled here.";

/// What the handlers saw, shared through `State`.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn see(seen: Option<State<Arc<Seen>>>, line: String) {
    seen.unwrap().0.lock().unwrap().push(line);
}

fn view(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

/// A global Command of `eco`.
#[derive(SlashCommand)]
#[command("balance", description = "Balance")]
struct Balance;

impl Balance {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        see(
            ctx.state(),
            format!("balance {:?}", ctx.guild_id().map(Id::get)),
        );
        ctx.respond(view("100 coins")).await
    }
}

/// A button of `eco`.
#[derive(Button)]
#[button("eco.buy", defaults(style = Primary, label = "Buy"))]
struct EcoBuy;

impl EcoBuy {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        see(ctx.state(), "eco.buy".to_owned());
        ctx.update(view("bought")).await
    }
}

/// A `MESSAGE_CREATE` handler of `eco`.
#[derive(EventHandler)]
struct OnMessage;

impl OnMessage {
    #[handler]
    async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
        see(
            ctx.state(),
            format!("message {:?}", msg.guild_id.map(Id::get)),
        );
        Ok(())
    }
}

/// `admin`: shows `eco`'s button.
#[derive(SlashCommand)]
#[command("shop", description = "Shop")]
struct Shop;

impl Shop {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let msg = V2Message::new(
            Components::new()
                .push(TextDisplay::new("shop"))
                .push(ActionRow::buttons([AnyButton::from(EcoBuy.build())])),
        );
        ctx.respond(msg).await
    }
}

/// `admin`: `eco`'s state here, read from the snapshot.
#[derive(SlashCommand)]
#[command("status", description = "Status")]
struct Status;

impl Status {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let on = ctx.modules().is_enabled("eco", ctx.guild_id());
        ctx.respond(view(if on { "eco on" } else { "eco off" }))
            .await
    }
}

/// `admin`: switches `eco` off in this guild.
#[derive(SlashCommand)]
#[command("eco off", description = "Disable eco here")]
struct EcoOff;

impl EcoOff {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.modules()
            .set_enabled("eco", ctx.guild_id(), false)
            .await?;
        ctx.respond(view("off")).await
    }
}

/// `admin`: switches `eco` back on in this guild.
#[derive(SlashCommand)]
#[command("eco on", description = "Enable eco here")]
struct EcoOn;

impl EcoOn {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.modules()
            .set_enabled("eco", ctx.guild_id(), true)
            .await?;
        ctx.respond(view("on")).await
    }
}

/// One App over `store`, with its own `Seen`.
async fn start(store: &Arc<MemoryModuleStateStore>, poll: Duration) -> (TestApp, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let mut builder = App::builder();
    builder
        .app_id("t")
        .state(Arc::clone(&seen))
        .module_state_store(Arc::clone(store))
        .module_state_poll(poll)
        .module(
            Module::new("eco")
                .add::<Balance>()
                .add::<EcoBuy>()
                .add::<OnMessage>(),
        )
        .module(
            Module::new("admin")
                .add::<Shop>()
                .add::<Status>()
                .add::<EcoOff>()
                .add::<EcoOn>(),
        );
    let app = TestApp::build(&mut builder)
        .await
        .unwrap()
        .guild(TestGuild::new(G1, "One"))
        .guild(TestGuild::new(G2, "Two"));
    (app, seen)
}

async fn run(app: &TestApp, path: &str, guild: Id<GuildMarker>) -> Response {
    app.command(path).in_guild(guild).run().await.unwrap()
}

fn assert_disabled(response: &Response) {
    assert!(response.is_ephemeral(), "{response:?}");
    assert_eq!(response.text(), DISABLED, "{response:?}");
}

/// A guild message from user 1 (`id` doubles as the message id).
fn message(id: u64, guild: Id<GuildMarker>) -> serde_json::Value {
    json!({
        "id": id.to_string(), "channel_id": "5", "guild_id": guild.to_string(),
        "author": { "id": "1", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null },
        "content": "hi", "timestamp": "2024-01-01T00:00:00.000000+00:00",
        "edited_timestamp": null, "tts": false, "mention_everyone": false, "mentions": [],
        "mention_roles": [], "attachments": [], "embeds": [], "pinned": false, "type": 0
    })
}

/// Waits (at most 5 s) until `seen` has `line`.
async fn wait_for(seen: &Seen, line: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !seen.lines().iter().any(|l| l == line) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never saw {line}: {:?}",
            seen.lines()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_toggle_reaches_both_apps() {
    let store = Arc::new(MemoryModuleStateStore::new());
    let (a, a_seen) = start(&store, Duration::from_secs(5)).await;
    let (b, b_seen) = start(&store, Duration::from_millis(50)).await;
    assert_eq!(run(&b, "status", G1).await.text(), "eco on");

    // A switches `eco` off in guild 1: A sees it at once.
    assert_eq!(run(&a, "eco off", G1).await.text(), "off");
    assert_disabled(&run(&a, "balance", G1).await);
    assert!(a_seen.lines().is_empty(), "{:?}", a_seen.lines());

    // B within its poll interval (bounded at 2 s).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let response = run(&b, "balance", G1).await;
        if response.text() == DISABLED {
            assert_disabled(&response);
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "B never saw it");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(run(&b, "status", G1).await.text(), "eco off");
    assert_eq!(run(&b, "status", G2).await.text(), "eco on");
    assert_eq!(run(&b, "balance", G2).await.text(), "100 coins");

    // `eco`'s button in guild 1 answers the notice; its handler never runs.
    let shop = run(&b, "shop", G1).await;
    let click = b.click(shop.button("Buy").unwrap()).run().await.unwrap();
    assert_disabled(&click);
    assert!(!b_seen.lines().iter().any(|l| l == "eco.buy"));

    // The event handler is skipped in guild 1 and runs in guild 2.
    let gw = b.gateway_fake();
    assert_eq!(gw.dispatch("MESSAGE_CREATE", message(11, G1)), 1);
    assert_eq!(gw.dispatch("MESSAGE_CREATE", message(12, G2)), 1);
    wait_for(&b_seen, "message Some(2)").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !b_seen.lines().iter().any(|l| l == "message Some(1)"),
        "{:?}",
        b_seen.lines()
    );

    // `/eco on` restores it for both.
    assert_eq!(run(&a, "eco on", G1).await.text(), "on");
    assert_eq!(run(&a, "balance", G1).await.text(), "100 coins");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while run(&b, "status", G1).await.text() != "eco on" {
        assert!(tokio::time::Instant::now() < deadline, "B never restored");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(run(&b, "balance", G1).await.text(), "100 coins");
    a.stop().await.unwrap();
    b.stop().await.unwrap();
}
