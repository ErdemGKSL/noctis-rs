//! Refs end to end (Story 7.5, FR-17, CID-05): a slash command replies
//! with `by_ref` buttons, the Responder stores their records in the App's
//! namespaced `RefStore` before the callback, and clicks through
//! `FakeGateway` resolve them; every response is asserted on the
//! `FakeDiscordHttp` wire.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::{Method, StatusCode};
use noctis::ports::RefStore;
use noctis::ports::memory::MemoryRefStore;
use noctis::prelude::*;
use noctis::ui::{ActionRow, Components, TextDisplay};
use noctis::{AppBuilder, RefOps, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, RecordedRequest,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The callback path of interaction 100 (token `tok`).
const CALLBACK: &str = "/interactions/100/tok/callback";

fn text(t: &str) -> Components {
    Components::new().push(TextDisplay::new(t.to_owned()))
}

/// A confirmation: stored by Ref for 15 minutes, consumed by its click.
#[derive(Button)]
#[button("mod.ban.ok", by_ref, one_shot, ttl = "15m", defaults(style = Danger, label = "Ban"))]
struct ConfirmBan {
    target: u64,
    reason: String,
}

impl ConfirmBan {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<(Components, RefOps)> {
        assert!(ctx.current_ref().is_some());
        let done = text(&format!("banned {} ({})", self.target, self.reason));
        // The Ref was taken at dispatch: nothing extra to delete.
        Ok((done, RefOps::new()))
    }
}

/// A step-flow Cancel: reusable, so it deletes its own Ref.
#[derive(Button)]
#[button("flow.cancel", by_ref, defaults(style = Secondary, label = "Cancel"))]
struct Cancel {
    step: u8,
}

impl Cancel {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<(Components, RefOps)> {
        let ops = RefOps::new().delete(ctx.current_ref().expect("a Ref id"));
        Ok((text(&format!("cancelled at {}", self.step)), ops))
    }
}

/// Replies with a `ConfirmBan` and a `Cancel` button.
#[derive(SlashCommand)]
#[command("ban", description = "Ban someone")]
struct Ban;

impl Ban {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Ok(text("Sure?")
            .push(ActionRow::buttons([ConfirmBan {
                target: 42,
                reason: "spam, again".to_owned(),
            }
            .build()]))
            .push(ActionRow::buttons([Cancel { step: 2 }.build()])))
    }
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(callback: StatusCode, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards: 1,
            ..GatewayBot::default()
        })
        .await
        .unwrap();
        http.route(Method::POST, CALLBACK, FakeResponse::empty(callback));
        let gw = FakeGateway::start().await.unwrap();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .module(
                Module::new("mod")
                    .add::<Ban>()
                    .add::<ConfirmBan>()
                    .add::<Cancel>(),
            );
        configure(&mut builder);
        let app = builder.build().unwrap();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let running = Self { gw, http, run };
        let deadline = Instant::now() + WAIT;
        while running.gw.dispatch("TYPING_START", json!({})) == 0 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        running
    }

    /// Dispatches `d` and waits for one more interaction callback.
    async fn answer(&self, d: Value) -> Value {
        let before = self.callbacks().len();
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
        let deadline = Instant::now() + WAIT;
        loop {
            let sent = self.callbacks();
            if sent.len() > before {
                return sent[before].json().unwrap();
            }
            assert!(Instant::now() < deadline, "no callback: {sent:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn callbacks(&self) -> Vec<RecordedRequest> {
        self.http
            .requests()
            .into_iter()
            .filter(|r| r.route_path() == Some(CALLBACK))
            .collect()
    }

    /// Runs `/ban` and returns the two button ids of the reply.
    async fn ban(&self) -> (String, String) {
        let reply = self.answer(command("ban")).await;
        assert_eq!(reply["type"], 4, "{reply}");
        let id = |i: usize| {
            reply["data"]["components"][i]["components"][0]["custom_id"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        (id(1), id(2))
    }

    fn stop(self) {
        self.run.abort();
    }
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// The slash command `name` (interaction 100, token `tok`, application 7).
fn command(name: &str) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "channel": { "id": "55", "type": 0 },
        "data": { "id": "9", "name": name, "type": 1 },
    })
}

/// A click on `custom_id` on the message 77.
fn click(custom_id: &str) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": 3,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "locale": "en-US",
        "channel": { "id": "55", "type": 0 },
        "message": {
            "id": "77", "channel_id": "55", "author": user(), "content": "",
            "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0, "flags": 32768,
            "components": []
        },
        "data": { "custom_id": custom_id, "component_type": 2 },
    })
}

/// The key of the Ref in `custom_id` under App `app`.
fn key(app: &str, custom_id: &str) -> String {
    format!("{app}:ref:{}", custom_id.split_once('~').unwrap().1)
}

/// The text of a callback's first Text Display.
fn content(body: &Value) -> &str {
    body["data"]["components"][0]["content"]
        .as_str()
        .unwrap_or_default()
}

/// Asserts `body` is the default `noctis.expired` ephemeral notice.
fn assert_expired(body: &Value) {
    assert_eq!(body["type"], 4, "{body}");
    assert_eq!(body["data"]["flags"], 32832, "{body}");
    assert_eq!(
        content(body),
        "This interaction has expired. Run the command again.",
        "{body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn confirm_flow_round_trip() {
    let refs = Arc::new(MemoryRefStore::new());
    let running = Running::start(StatusCode::NO_CONTENT, |b| {
        b.app_id("bot-a").ref_store(Arc::clone(&refs));
    })
    .await;
    let (confirm, _) = running.ban().await;
    // `F route ":" "~" <11 base62>`, stored before the callback.
    assert!(confirm.starts_with("0mod.ban.ok:~"), "{confirm}");
    assert_eq!(confirm.len(), "0mod.ban.ok:~".len() + 11);
    let stored = refs.get(&key("bot-a", &confirm)).await.unwrap().unwrap();
    assert_eq!(stored.as_ref(), b"0mod.ban.ok:g,spam!1 again");

    let first = running.answer(click(&confirm)).await;
    assert_eq!(first["type"], 7, "{first}");
    assert_eq!(content(&first), "banned 42 (spam, again)");
    assert_eq!(refs.get(&key("bot-a", &confirm)).await.unwrap(), None);

    let second = running.answer(click(&confirm)).await;
    assert_expired(&second);
    running.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn two_apps_cannot_resolve_each_others_refs() {
    let refs = Arc::new(MemoryRefStore::new());
    let a = Running::start(StatusCode::NO_CONTENT, |b| {
        b.app_id("bot-a").ref_store(Arc::clone(&refs));
    })
    .await;
    let b = Running::start(StatusCode::NO_CONTENT, |b| {
        b.app_id("bot-b").ref_store(Arc::clone(&refs));
    })
    .await;
    let (_, cancel) = a.ban().await;
    assert!(refs.get(&key("bot-a", &cancel)).await.unwrap().is_some());
    assert_expired(&b.answer(click(&cancel)).await);
    assert_eq!(refs.get(&key("bot-b", &cancel)).await.unwrap(), None);
    // App `a` still resolves it.
    let answer = a.answer(click(&cancel)).await;
    assert_eq!(content(&answer), "cancelled at 2");
    a.stop();
    b.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn ref_ops_delete_after_send() {
    let refs = Arc::new(MemoryRefStore::new());
    let running = Running::start(StatusCode::NO_CONTENT, |b| {
        b.ref_store(Arc::clone(&refs));
    })
    .await;
    let app = "default";
    let (_, cancel) = running.ban().await;
    let answer = running.answer(click(&cancel)).await;
    assert_eq!(answer["type"], 7, "{answer}");
    assert_eq!(content(&answer), "cancelled at 2");
    // The delete runs right after the send: poll briefly.
    let deadline = Instant::now() + WAIT;
    while refs.get(&key(app, &cancel)).await.unwrap().is_some() {
        assert!(Instant::now() < deadline, "the Ref was never deleted");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    running.stop();

    // A failed send deletes nothing.
    let failing = Running::start(StatusCode::INTERNAL_SERVER_ERROR, |b| {
        b.ref_store(Arc::clone(&refs));
    })
    .await;
    let (_, cancel) = failing.ban().await;
    failing.answer(click(&cancel)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(refs.get(&key(app, &cancel)).await.unwrap().is_some());
    failing.stop();
}
