//! Story 9.5: Views over webhooks end to end through `TestApp` (FR-65;
//! WH-01, WH-02, MSG-07): a command sends a `StaticMessage` through a
//! foreign webhook and a View with a Route button through an owned one.

#![cfg(feature = "testkit")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};

use noctis::prelude::*;
use noctis::testkit::TestApp;
use noctis::testkit::fake::{FakeResponse, Method, RecordedRequest, StatusCode};
use noctis::twilight::model::id::Id;
use noctis::ui::{
    ActionRow, Components, StaticComponents, StaticContainer, TextDisplay, V2Message,
};
use serde_json::json;

#[derive(Button)]
#[button("log.ack", defaults(style = Primary, label = "Ack"))]
struct Ack {
    case: u64,
}

impl Ack {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.reply_text(format!("acked {}", self.case)).await
    }
}

#[derive(SlashCommand)]
#[command("mirror", description = "Post through webhooks")]
struct Mirror;

impl Mirror {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let hooks = ctx.webhooks();
        let mirrored =
            StaticComponents::new().push(StaticContainer::new().push(TextDisplay::new("Mirrored")));
        hooks
            .foreign(Id::new(901), "foreign-tok")?
            .send(mirrored)
            .await?;
        let view = V2Message::new(
            Components::new()
                .push(TextDisplay::new("Case 7"))
                .push(ActionRow::buttons([Ack { case: 7 }.build()])),
        );
        hooks.owned(Id::new(900), "owned-tok")?.send(view).await?;
        ctx.reply_text("posted").await
    }
}

type Log = Arc<Mutex<Vec<RecordedRequest>>>;

/// Answers a message `id` in channel 5 and records the request in `log`.
fn message(log: &Log, id: &'static str) -> impl Fn(&RecordedRequest) -> FakeResponse + use<> {
    let log = Arc::clone(log);
    move |request| {
        log.lock().unwrap().push(request.clone());
        FakeResponse::json(
            StatusCode::OK,
            &json!({"id": id, "channel_id": "5", "attachments": []}),
        )
    }
}

fn calls_to(requests: &[RecordedRequest], path: &str) -> Vec<RecordedRequest> {
    requests
        .iter()
        .filter(|r| r.path == path)
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn wh_01_wh_02_webhooks_through_testapp() {
    let mut builder = App::builder();
    builder.module(Module::new("log").add::<Mirror>().add::<Ack>());
    let app = TestApp::build(&mut builder).await.unwrap();
    let log = Log::default();
    app.http_fake()
        .route_with(
            Method::POST,
            "/webhooks/901/foreign-tok",
            message(&log, "71"),
        )
        .route_with(Method::POST, "/webhooks/900/owned-tok", message(&log, "72"));

    let response = app.command("mirror").run().await.unwrap();
    assert!(response.text().contains("posted"), "{:?}", response.text());

    let requests = log.lock().unwrap().clone();
    let foreign = calls_to(&requests, "/api/v10/webhooks/901/foreign-tok");
    let owned = calls_to(&requests, "/api/v10/webhooks/900/owned-tok");
    assert_eq!((foreign.len(), owned.len()), (1, 1), "{requests:?}");
    for call in [&foreign[0], &owned[0]] {
        assert_eq!(
            call.query.as_deref(),
            Some("wait=true&with_components=true")
        );
        assert!(call.headers.get("authorization").is_none());
        let body = call.json().unwrap();
        assert_eq!(body["flags"], 32768);
        assert!(body.get("content").is_none());
        assert!(body.get("embeds").is_none());
    }
    let foreign = foreign[0].json().unwrap();
    assert_eq!(foreign["components"][0]["type"], 17);
    let owned = owned[0].json().unwrap();
    let id = owned["components"][1]["components"][0]["custom_id"]
        .as_str()
        .unwrap();
    assert!(id.starts_with("0log.ack:"), "{id}");
    app.stop().await.unwrap();
}
