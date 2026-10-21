//! Story 8.2: deterministic renders and select freshness end to end (FR-25,
//! MSG-05). A `FakeGateway` `INTERACTION_CREATE` reaches the pipeline; the
//! response bodies are read off the `FakeDiscordHttp` wire.
//!
//! The incoming select ids are encoded with `noctis::__private::codec`
//! and a known nonce. A select id is `F route ":" N body` (these Routes
//! declare no schema, so there is no `S`): the nonce is the two characters
//! after the `:`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::time::{Duration, Instant};

use noctis::__private::codec::{Parts, SelectNonce, encode};
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{ActionRow, Components, Container, TextDisplay, V2Message, escape_markdown};
use noctis::{Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method, RecordedRequest, StatusCode,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const CALLBACK: &str = "/interactions/100/tok/callback";
const ORIGINAL: &str = "/webhooks/7/tok/messages/@original";

// --- The View ----------------------------------------------------------------

#[derive(Choices, Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    #[choice("Fast", value = "fast")]
    Fast,
    #[choice("Safe", value = "safe")]
    Safe,
}

/// What the panel shows.
struct PanelState {
    name: String,
    mode: Mode,
    who: Id<UserMarker>,
}

/// The state every invocation loads: equal state, equal render.
fn state() -> PanelState {
    PanelState {
        name: "**Ana** _the_ [admin](https://e.com)".to_owned(),
        mode: Mode::Safe,
        who: Id::new(1),
    }
}

/// The panel's render function: pure, so equal state gives equal JSON
/// except the per-send select nonces.
fn render_panel(state: &PanelState) -> V2Message {
    let header = TextDisplay::new(format!(
        "## {}\nOwner <@1>, admins <@&2>, @everyone",
        escape_markdown(&state.name)
    ));
    // Every select gets a fresh nonce per send, so the client shows these
    // declared defaults: the current mode and the current user.
    let mode =
        Pick.build().options(Mode::ALL.iter().map(|m| {
            noctis::ui::SelectOption::new(m.label(), m.value()).default(*m == state.mode)
        }));
    let who = Who.build().default_users([state.who]);
    let pager = ActionRow::buttons([
        Page { page: 0 }.build().label("Prev").disabled(true),
        Page { page: 2 }.build().label("Next"),
    ]);
    V2Message::new(
        Components::new().push(
            Container::new()
                .push(header)
                .push(ActionRow::select(mode))
                .push(ActionRow::select(who))
                .push(pager),
        ),
    )
}

#[derive(SlashCommand)]
#[command("panel", description = "Show the panel")]
struct Panel;

impl Panel {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render_panel(&state())).await
    }
}

#[derive(StringSelectMenu)]
#[string_select("panel.mode", defaults(placeholder = "Pick", options = Mode))]
struct Pick;

impl Pick {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(render_panel(&state())).await
    }
}

#[derive(UserSelectMenu)]
#[user_select("panel.who", defaults(placeholder = "Who"))]
struct Who;

impl Who {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(render_panel(&state())).await
    }
}

#[derive(Button, Debug, Clone, Copy)]
#[button("panel.page", defaults(style = Secondary, label = "Page"))]
struct Page {
    page: u16,
}

impl Page {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        // Page 4 answers late, so a zero ack deadline auto-defers first.
        if self.page == 4 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        ctx.update(render_panel(&state())).await
    }
}

// --- The running App ---------------------------------------------------------

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    /// The App with `ack_deadline`, every test struct mounted.
    async fn start(ack_deadline: Duration) -> Self {
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
            Method::PATCH,
            ORIGINAL,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "77", "channel_id": "55" })),
        );
        let gw = FakeGateway::start().await.unwrap();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .ack_deadline(ack_deadline)
            .module(
                Module::new("panel")
                    .add::<Panel>()
                    .add::<Pick>()
                    .add::<Who>()
                    .add::<Page>(),
            );
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

    fn dispatch(&self, d: Value) {
        assert_eq!(self.gw.dispatch("INTERACTION_CREATE", d), 1);
    }

    /// Dispatches a chat-input interaction for `name`.
    fn invoke(&self, name: &str) {
        self.dispatch(json!({
            "id": "100",
            "application_id": "7",
            "type": 2,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "locale": "tr",
            "data": { "id": "9", "name": name, "type": 1 },
        }));
    }

    /// The requests other than `GET /gateway/bot`.
    fn sent(&self) -> Vec<RecordedRequest> {
        self.http
            .requests()
            .into_iter()
            .filter(|r| r.route_path() != Some("/gateway/bot"))
            .collect()
    }

    /// Polls until `n` requests were sent, then checks that no more arrive
    /// for a moment. Clears them.
    async fn exactly(&self, n: usize) -> Vec<RecordedRequest> {
        let deadline = Instant::now() + WAIT;
        while self.sent().len() < n {
            assert!(
                Instant::now() < deadline,
                "{n} requests missing: {:?}",
                self.sent()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let sent = self.sent();
        assert_eq!(sent.len(), n, "{sent:?}");
        self.http.clear_requests();
        sent
    }

    fn stop(self) {
        self.run.abort();
    }
}

// --- Wire helpers ------------------------------------------------------------

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn body(request: &RecordedRequest) -> Value {
    request.json().unwrap_or(Value::Null)
}

fn route(request: &RecordedRequest) -> (Method, &str) {
    (request.method.clone(), request.route_path().unwrap_or(""))
}

fn nonce(value: u16) -> SelectNonce {
    SelectNonce::new(value).expect("in range")
}

/// The `custom_id` of the mode select carrying `nonce`.
fn pick_id(nonce: SelectNonce) -> String {
    encode::<Pick>(&Pick, &Parts::new().with_nonce(nonce))
        .unwrap()
        .to_string()
}

/// The V2 message 77 in channel 55 holding a mode select whose id carries
/// `nonce`.
fn source_message(nonce: SelectNonce) -> Value {
    json!({
        "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0,
        "components": [{ "type": 1, "id": 1, "components": [
            { "type": 3, "id": 2, "custom_id": pick_id(nonce), "options": [] },
        ] }],
    })
}

/// A component interaction on message 77 (whose select carries
/// `message_nonce`) with `data`.
fn component_interaction(message_nonce: SelectNonce, data: &Value) -> Value {
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
        "message": source_message(message_nonce),
        "data": data,
    })
}

/// Every select `custom_id` (component types 3 and 5-8) in `value`.
fn select_ids(value: &Value) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if matches!(map.get("type").and_then(Value::as_u64), Some(3 | 5..=8))
                    && let Some(id) = map.get("custom_id").and_then(Value::as_str)
                {
                    out.push(id.to_owned());
                }
                map.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out);
    out
}

/// The nonce of a select id: the two characters after `:`.
fn nonce_of(id: &str) -> &str {
    &id.split_once(':').expect("a Noctis id").1[..2]
}

/// `value` with each select id's two nonce characters replaced by `NN`.
fn mask_nonces(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let select = matches!(map.get("type").and_then(Value::as_u64), Some(3 | 5..=8));
            if select && let Some(Value::String(id)) = map.get_mut("custom_id") {
                let colon = id.find(':').expect("a Noctis id");
                id.replace_range(colon + 1..colon + 3, "NN");
            }
            map.values_mut().for_each(mask_nonces);
        }
        Value::Array(items) => items.iter_mut().for_each(mask_nonces),
        _ => {}
    }
}

/// The render's declared defaults are on the wire: the current mode is the
/// default option and the current user the default value.
fn assert_defaults(message: &Value) {
    let rows = &message["components"][0]["components"];
    let options = &rows[1]["components"][0]["options"];
    assert_eq!(options[0]["value"], "fast");
    assert!(options[0].get("default").is_none(), "{options}");
    assert_eq!(options[1]["value"], "safe");
    assert_eq!(options[1]["default"], true, "{options}");
    assert_eq!(
        rows[2]["components"][0]["default_values"],
        json!([{ "id": "1", "type": "user" }])
    );
}

// --- The tests ---------------------------------------------------------------

/// R1: a select click carrying nonce `ab` updates with one type-7 callback
/// whose select ids carry another nonce and the render's defaults.
#[tokio::test(flavor = "multi_thread")]
async fn select_click_update_gets_a_fresh_nonce() {
    let ab = nonce(371);
    let running = Running::start(Duration::from_secs(2)).await;
    for _ in 0..8 {
        running.dispatch(component_interaction(
            ab,
            &json!({ "custom_id": pick_id(ab), "component_type": 3, "values": ["fast"] }),
        ));
        let sent = running.exactly(1).await;
        assert_eq!(route(&sent[0]), (Method::POST, CALLBACK));
        let update = body(&sent[0]);
        assert_eq!(update["type"], 7, "{update}");
        let ids = select_ids(&update["data"]);
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert_eq!(nonce_of(&ids[0]), nonce_of(&ids[1]));
        assert_ne!(nonce_of(&ids[0]), ab.to_string());
        assert_defaults(&update["data"]);
    }
    running.stop();
}

/// R2 and R4: a button click on a message whose select carries `cd`
/// re-renders with another nonce, also as `PATCH @original` after the
/// type-6 auto-defer.
#[tokio::test(flavor = "multi_thread")]
async fn button_click_avoids_the_message_nonce() {
    let cd = nonce(2000);
    let click = json!({
        "custom_id": encode::<Page>(&Page { page: 2 }, &Parts::new()).unwrap().as_str(),
        "component_type": 2,
    });
    let running = Running::start(Duration::from_secs(2)).await;
    running.dispatch(component_interaction(cd, &click));
    let sent = running.exactly(1).await;
    let update = body(&sent[0]);
    assert_eq!(update["type"], 7, "{update}");
    let ids = select_ids(&update["data"]);
    assert_eq!(ids.len(), 2);
    assert_ne!(nonce_of(&ids[0]), cd.to_string());
    running.stop();

    let late = json!({
        "custom_id": encode::<Page>(&Page { page: 4 }, &Parts::new()).unwrap().as_str(),
        "component_type": 2,
    });
    let running = Running::start(Duration::ZERO).await;
    running.dispatch(component_interaction(cd, &late));
    let sent = running.exactly(2).await;
    assert_eq!(body(&sent[0]), json!({ "type": 6 }));
    assert_eq!(route(&sent[1]), (Method::PATCH, ORIGINAL));
    let edit = body(&sent[1]);
    let ids = select_ids(&edit);
    assert_eq!(ids.len(), 2);
    assert_ne!(nonce_of(&ids[0]), cd.to_string());
    assert_defaults(&edit);
    running.stop();
}

/// R6 and S1: the same Command invoked twice with the same state sends
/// bodies equal except the select nonces, and the listed mentions ping
/// nobody.
#[tokio::test(flavor = "multi_thread")]
async fn same_state_renders_identical_json() {
    let running = Running::start(Duration::from_secs(2)).await;
    let mut bodies = Vec::new();
    for _ in 0..2 {
        running.invoke("panel");
        let sent = running.exactly(1).await;
        let mut reply = body(&sent[0]);
        assert_eq!(reply["type"], 4, "{reply}");
        assert_eq!(reply["data"]["allowed_mentions"], json!({ "parse": [] }));
        assert_eq!(select_ids(&reply).len(), 2);
        mask_nonces(&mut reply);
        bodies.push(reply);
    }
    running.stop();
    assert_eq!(bodies[0], bodies[1]);
    insta::assert_json_snapshot!("panel_render", bodies[0]);
}
