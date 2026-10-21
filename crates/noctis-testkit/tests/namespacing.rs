//! App-namespaced ports end to end (Story 7.4): two Apps over one store
//! instance see only their own `ctx.kv()` entries unless the store is passed
//! as `Shared::new(..)`, and every context's `kv()` writes under
//! `<app>:cache:`. Clicks, commands and events arrive through `FakeGateway`;
//! responses go to `FakeDiscordHttp`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use hyper::{Method, StatusCode};
use noctis::events::{EventCtx, MessageDelete};
use noctis::ports::memory::MemoryCacheStore;
use noctis::ports::{CacheStore, Shared};
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};
use noctis::{AppBuilder, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, RecordedRequest,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The callback path of interaction 100 (token `tok`).
const CALLBACK: &str = "/interactions/100/tok/callback";

/// Counts clicks in `ctx.kv()` and answers with the count.
#[derive(Button)]
#[button("clicks", defaults(style = Primary, label = "Click"))]
struct Clicks;

impl Clicks {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Components> {
        let count = ctx.kv().incr("clicks", 1, None).await?;
        Ok(Components::new().push(TextDisplay::new(format!("count {count}"))))
    }
}

/// Writes `cmd` through the Command's `ctx.kv()`.
#[derive(SlashCommand)]
#[command("mark", description = "Writes a key")]
struct Mark;

impl Mark {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Components> {
        ctx.kv().put("cmd", &1_u8, None).await?;
        Ok(Components::new().push(TextDisplay::new("marked")))
    }
}

/// Writes `event` through the event's `ctx.kv()`.
#[derive(EventHandler)]
struct OnDelete;

impl OnDelete {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
        ctx.kv().put("event", &ev.id.get(), None).await?;
        Ok(())
    }
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(configure: impl FnOnce(&mut AppBuilder)) -> Self {
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
        );
        let gw = FakeGateway::start().await.unwrap();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .module(
                Module::new("kv")
                    .add::<Clicks>()
                    .add::<Mark>()
                    .add::<OnDelete>(),
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

    fn dispatch(&self, event: &str, d: Value) {
        assert_eq!(self.gw.dispatch(event, d), 1);
    }

    /// Polls until `n` interaction callbacks were sent.
    async fn wait_callbacks(&self, n: usize) -> Vec<RecordedRequest> {
        let deadline = Instant::now() + WAIT;
        loop {
            let sent: Vec<_> = self
                .http
                .requests()
                .into_iter()
                .filter(|r| r.route_path() == Some(CALLBACK))
                .collect();
            if sent.len() >= n {
                return sent;
            }
            assert!(Instant::now() < deadline, "{n} callbacks missing: {sent:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn stop(self) {
        self.run.abort();
    }
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// A click on `custom_id` (interaction 100, token `tok`, application 7).
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

/// The text of the last callback's message.
fn answered(request: &RecordedRequest) -> String {
    let body = request.json().unwrap();
    body["data"]["components"][0]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Polls `store` until `key` holds a value.
async fn wait_key(store: &MemoryCacheStore, key: &str) -> Bytes {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(value) = store.get(key).await.unwrap() {
            return value;
        }
        assert!(Instant::now() < deadline, "{key} was never written");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Clicks `Clicks` once on `running` and returns the answered text.
async fn click_once(running: &Running) -> String {
    let before = running.wait_callbacks(0).await.len();
    running.dispatch("INTERACTION_CREATE", click("0clicks:"));
    let sent = running.wait_callbacks(before + 1).await;
    answered(&sent[before])
}

#[tokio::test(flavor = "multi_thread")]
async fn two_apps_never_see_each_others_kv() {
    let store = Arc::new(MemoryCacheStore::new());
    let a = Running::start(|b| {
        b.app_id("bot-a").cache_store(Arc::clone(&store));
    })
    .await;
    let b = Running::start(|b| {
        b.app_id("bot-b").cache_store(Arc::clone(&store));
    })
    .await;
    assert_eq!(click_once(&a).await, "count 1");
    assert_eq!(click_once(&b).await, "count 1");
    let one = Bytes::copy_from_slice(&1_u64.to_be_bytes());
    assert_eq!(
        store.get("bot-a:cache:clicks").await.unwrap(),
        Some(one.clone())
    );
    assert_eq!(store.get("bot-b:cache:clicks").await.unwrap(), Some(one));
    assert_eq!(store.get("clicks").await.unwrap(), None);
    a.stop();
    b.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_cache_store_is_seen_by_both_apps() {
    let store = Arc::new(MemoryCacheStore::new());
    let a = Running::start(|b| {
        b.app_id("bot-a")
            .cache_store(Shared::new(Arc::clone(&store)));
    })
    .await;
    let b = Running::start(|b| {
        b.app_id("bot-b")
            .cache_store(Shared::new(Arc::clone(&store)));
    })
    .await;
    assert_eq!(click_once(&a).await, "count 1");
    assert_eq!(click_once(&b).await, "count 2");
    assert_eq!(
        store.get("_shared:cache:clicks").await.unwrap(),
        Some(Bytes::copy_from_slice(&2_u64.to_be_bytes()))
    );
    assert_eq!(store.get("bot-a:cache:clicks").await.unwrap(), None);
    a.stop();
    b.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn event_and_command_ctx_kv() {
    let store = Arc::new(MemoryCacheStore::new());
    let running = Running::start(|b| {
        b.app_id("bot-a").cache_store(Arc::clone(&store));
    })
    .await;
    running.dispatch(
        "INTERACTION_CREATE",
        json!({
            "id": "100",
            "application_id": "7",
            "type": 2,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "data": { "id": "9", "name": "mark", "type": 1 },
        }),
    );
    running.dispatch(
        "MESSAGE_DELETE",
        json!({ "id": "42", "channel_id": "2", "guild_id": "3" }),
    );
    let cmd = wait_key(&store, "bot-a:cache:cmd").await;
    assert_eq!(cmd.as_ref(), [0, 1]);
    let event = wait_key(&store, "bot-a:cache:event").await;
    assert_eq!(event.as_ref(), [0, 42]);
    running.stop();
}
