//! Story 13.2 end to end: Boot Flags through the facade with real derives.
//! An App built with `boot_flags(only{economy})` dispatches its unflagged
//! and selected registrations and none of the unselected ones (a flagged
//! slash Command, button and `MESSAGE_DELETE` handler), publishes only the
//! selected Commands, and rejects a `--features` that differs from its
//! selection.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::__private::core::__testing::responded;
use noctis::cli::CliError;
use noctis::events::{EventCtx, MessageDelete};
use noctis::prelude::*;
use noctis::publish::PublishScope;
use noctis::ui::{Components, TextDisplay, V2Message};
use noctis::{App, AppBuilder, BootFlags, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method, StatusCode,
};
use serde_json::{Value, json};

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const CALLBACK: &str = "/interactions/100/tok/callback";
const APP: &str = "/applications/@me";
const GLOBAL: &str = "/applications/7/commands";

/// The handler runs, in order, shared through `State`.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn lines(&self) -> Vec<String> {
        let mut lines = self.0.lock().unwrap().clone();
        lines.sort();
        lines
    }
}

fn see(seen: Option<State<Arc<Seen>>>, line: &str) {
    seen.unwrap().0.lock().unwrap().push(line.to_owned());
}

fn reply() -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new("ok")))
}

/// Unflagged, in the unflagged Module `core`.
#[derive(SlashCommand)]
#[command("ping", description = "Pong")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        see(ctx.state(), "ping");
        ctx.respond(reply()).await
    }
}

/// Flagged `beta`, in the unflagged Module `core`.
#[derive(SlashCommand)]
#[command("beta stats", description = "Beta stats", flags = ["beta"])]
struct BetaStats;

impl BetaStats {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        see(ctx.state(), "beta stats");
        ctx.respond(reply()).await
    }
}

/// Flagged `beta`.
#[derive(EventHandler)]
#[event(flags = ["beta"])]
struct OnDelete;

impl OnDelete {
    #[handler]
    async fn handle(self, ctx: EventCtx, _ev: MessageDelete) -> HandlerResult<()> {
        see(ctx.state(), "delete");
        Ok(())
    }
}

/// Flagged `beta`.
#[derive(Button)]
#[button("core.beta", flags = ["beta"], defaults(style = Primary, label = "Beta"))]
struct CoreBeta;

impl CoreBeta {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        see(ctx.state(), "core.beta");
        Ok(responded())
    }
}

/// Unflagged, in the Module `eco` flagged `economy`.
#[derive(SlashCommand)]
#[command("balance", description = "Balance")]
struct Balance;

impl Balance {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        see(ctx.state(), "balance");
        ctx.respond(reply()).await
    }
}

/// Unflagged, in the Module `eco` flagged `economy`.
#[derive(Button)]
#[button("eco.buy", defaults(style = Primary, label = "Buy"))]
struct EcoBuy;

impl EcoBuy {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        see(ctx.state(), "eco.buy");
        Ok(responded())
    }
}

fn economy() -> BootFlags {
    BootFlags::only(["economy"]).unwrap()
}

/// A fake answering the callbacks and an empty global command list.
async fn fake() -> FakeDiscordHttp {
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    http.route(
        Method::POST,
        CALLBACK,
        FakeResponse::empty(StatusCode::NO_CONTENT),
    )
    .route(
        Method::GET,
        APP,
        FakeResponse::json(
            StatusCode::OK,
            &json!({"id": "7", "integration_types_config": {"0": {}}}),
        ),
    )
    .route(
        Method::GET,
        GLOBAL,
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    http
}

/// The App: `core` unflagged with flagged registrations, `eco` flagged
/// `economy`, built with `{economy}`.
fn app(http: &FakeDiscordHttp, configure: impl FnOnce(&mut AppBuilder)) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .module(
            Module::new("core")
                .add::<Ping>()
                .add::<BetaStats>()
                .add::<OnDelete>()
                .add::<CoreBeta>(),
        )
        .module(
            Module::new("eco")
                .flags(["economy"])
                .add::<Balance>()
                .add::<EcoBuy>(),
        )
        .boot_flags(economy());
    configure(&mut builder);
    builder.build().unwrap()
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn interaction(kind: u8, data: &Value) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": kind,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "channel": { "id": "55", "type": 1 },
        "locale": "en-US",
        "data": data,
    })
}

fn command(name: &str, options: &Value) -> Value {
    interaction(
        2,
        &json!({ "id": "9", "name": name, "type": 1, "options": options }),
    )
}

fn click(custom_id: &str) -> Value {
    interaction(3, &json!({ "custom_id": custom_id, "component_type": 2 }))
}

/// AC1: selected and unflagged registrations run; unselected ones never do.
#[tokio::test(flavor = "multi_thread")]
async fn dispatch_sees_only_the_selection() {
    let http = fake().await;
    let gw = FakeGateway::start().await.unwrap();
    let seen = Arc::new(Seen::default());
    let app = app(&http, |b| {
        b.gateway_url(gw.url()).state(Arc::clone(&seen));
    });
    assert_eq!(app.boot_flags(), &economy());
    let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
    let deadline = Instant::now() + WAIT;
    while gw.dispatch("TYPING_START", json!({})) == 0 {
        assert!(Instant::now() < deadline, "the shard has no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The unselected ones first.
    gw.dispatch(
        "INTERACTION_CREATE",
        command("beta", &json!([{ "type": 1, "name": "stats" }])),
    );
    gw.dispatch("INTERACTION_CREATE", click("0core.beta:"));
    gw.dispatch(
        "MESSAGE_DELETE",
        json!({ "id": "6", "channel_id": "5", "guild_id": "8" }),
    );
    // Then the selected and unflagged ones.
    gw.dispatch("INTERACTION_CREATE", command("ping", &json!([])));
    gw.dispatch("INTERACTION_CREATE", command("balance", &json!([])));
    gw.dispatch("INTERACTION_CREATE", click("0eco.buy:"));

    let deadline = Instant::now() + WAIT;
    while seen.lines().len() < 3 {
        assert!(
            Instant::now() < deadline,
            "handler runs missing: {:?}",
            seen.lines()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Leave room for a late unselected run to show up.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(seen.lines(), ["balance", "eco.buy", "ping"]);
    run.abort();
}

/// AC1/AC2: Publish holds only the selected and unflagged Commands; an
/// equal `--features` runs, a different one is a mismatch.
#[tokio::test(flavor = "multi_thread")]
async fn publish_and_cli_follow_the_selection() {
    let http = fake().await;
    let app = app(&http, |_| {});
    let report = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    let text = report.to_string();
    assert!(text.contains("\"name\": \"ping\""), "{text}");
    assert!(text.contains("\"name\": \"balance\""), "{text}");
    assert!(!text.contains("\"name\": \"beta\""), "{text}");
    assert_eq!(report.creates(), 2, "{text}");

    app.run_with_args(["--publish", "--dry-run", "--features", "economy"])
        .await
        .unwrap();

    http.clear_requests();
    let err = self::app(&http, |_| {})
        .run_with_args(["--publish", "--dry-run", "--features", "beta"])
        .await
        .unwrap_err();
    match err {
        noctis::Error::Cli(CliError::BootFlagsMismatch { given, built }) => {
            assert_eq!(given, BootFlags::only(["beta"]).unwrap());
            assert_eq!(built, economy());
        }
        other => panic!("{other:?}"),
    }
    assert!(http.requests().is_empty());
}
