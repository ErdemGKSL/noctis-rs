//! Components and modals end to end (Story 7.1 Matrix 1, 2, 16, 19, 26 and
//! 31; Story 7.2 Matrix 1-5, 9-12, 20, 29, 30, 35, 36, 41 and 46): a
//! `FakeGateway` `INTERACTION_CREATE` click or modal submit reaches the
//! handler of its Route through the component pipeline, and every response
//! is asserted on the `FakeDiscordHttp` wire (method, path, query, headers,
//! body).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use hyper::{Method, StatusCode};
use noctis::__private::codec::{Parts, encode};
use noctis::__private::core::__testing::responded;
use noctis::codec::PayloadKey;
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{Components, Modal, TextDisplay};
use noctis::{AppBuilder, BuildViolation, ResponderError, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, RecordedRequest,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

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
            .with_max_level(LevelFilter::WARN)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        capture
    })
}

/// What the handlers saw, shared through `State`.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn push(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let seen = ctx.state::<Arc<Seen>>().unwrap();
        seen.push(format!(
            "buy {} module={} user={} guild={:?} channel={:?} shard={} locale={:?}/{:?}",
            self.item,
            ctx.module(),
            ctx.user_id(),
            ctx.guild_id().map(Id::get),
            ctx.channel_id(),
            ctx.shard(),
            ctx.user_locale(),
            ctx.guild_locale(),
        ));
        Ok(responded())
    }
}

#[derive(StringSelectMenu)]
#[string_select("pick", defaults(placeholder = "Pick"))]
struct Pick {
    item: u64,
}

impl Pick {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let seen = ctx.state::<Arc<Seen>>().unwrap();
        seen.push(format!("pick {}", self.item));
        Ok(responded())
    }
}

#[derive(Modal, Debug)]
#[modal("bio", defaults(title = "Bio"))]
struct BioForm {
    user: Id<UserMarker>,
    #[text_input("Bio", style = paragraph, max_length = 300)]
    bio: Option<String>,
}

impl BioForm {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        let seen = ctx.state::<Arc<Seen>>().unwrap();
        seen.push(format!(
            "bio of {}: {:?} module={} channel={:?}",
            self.user,
            self.bio,
            ctx.module(),
            ctx.channel_id(),
        ));
        Ok(responded())
    }
}

/// A `signed` button: its ids need the App's payload key ring.
#[derive(Button)]
#[button("sig", signed, defaults(style = Primary, label = "Sig"))]
struct Sig {
    item: u64,
}

impl Sig {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.state::<Arc<Seen>>()
            .unwrap()
            .push(format!("sig {}", self.item));
        Ok(responded())
    }
}

/// Updates the clicked message in place (the default `response = update`).
#[derive(Button)]
#[button("upd", defaults(style = Primary, label = "Update"))]
struct Upd;

impl Upd {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        let panel = Components::new().push(TextDisplay::new("updated"));
        Ok(r.update(panel).await?.done())
    }
}

/// `response = ephemeral`: sleeps past the ack deadline, then returns a
/// message.
#[derive(Button)]
#[button("ban", response = ephemeral, defaults(style = Danger, label = "Ban"))]
struct Ban;

impl Ban {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(Components::new().push(TextDisplay::new("banned")))
    }
}

/// `response = update`, auto-deferred: sleeps past the deadline, then
/// updates.
#[derive(Button)]
#[button("slow.upd", defaults(style = Primary, label = "Slow"))]
struct SlowUpd;

impl SlowUpd {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let panel = Components::new().push(TextDisplay::new("later"));
        Ok(r.update(panel).await?.done())
    }
}

/// `auto_defer = false`: `0` opens the bio modal, `1` never answers.
#[derive(Button)]
#[button("open", auto_defer = false, defaults(style = Secondary, label = "Open"))]
struct Open {
    late: bool,
}

impl Open {
    #[handler]
    async fn handle(
        self,
        _ctx: ComponentCtx,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        if self.late {
            tokio::time::sleep(Duration::from_millis(3200)).await;
            drop(r);
            return Ok(responded());
        }
        Ok(r.show_modal(bio_modal()).await?.done())
    }
}

fn bio_modal() -> Modal {
    BioForm::blank(Id::new(42))
}

/// A modal that tries `update` (`step = 0`) or `defer_update` (`1`) and
/// records the error instead of answering.
#[derive(Modal, Debug)]
#[modal("note", defaults(title = "Note"))]
struct Note {
    step: u8,
    #[text_input("Note")]
    #[allow(dead_code, reason = "the submitted input is decoded, not read")]
    note: Option<String>,
}

impl Note {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx, r: ModalResponder) -> HandlerResult<Responded> {
        let err = if self.step == 0 {
            r.update(Components::new().push(TextDisplay::new("x")))
                .await
                .unwrap_err()
        } else {
            r.defer_update().await.unwrap_err()
        };
        assert!(matches!(err, ResponderError::UpdateUnavailable), "{err}");
        ctx.state::<Arc<Seen>>()
            .unwrap()
            .push(format!("note {}: {err}", self.step));
        Ok(responded())
    }
}

/// A `signed` modal: dispatch verifies its tag with the key ring and builds
/// it from the verified Payload.
#[derive(Modal, Debug)]
#[modal("sigbio", signed, defaults(title = "Signed bio"))]
struct SigBio {
    item: u64,
    #[text_input("Bio")]
    bio: String,
}

impl SigBio {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        ctx.state::<Arc<Seen>>()
            .unwrap()
            .push(format!("sigbio {}: {}", self.item, self.bio));
        Ok(responded())
    }
}

/// Updates the clicked message, then fetches and deletes it.
#[derive(Button)]
#[button("upd.fd", defaults(style = Primary, label = "Fetch"))]
struct UpdFetchDelete;

impl UpdFetchDelete {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        let acked = r
            .update(Components::new().push(TextDisplay::new("updated")))
            .await?;
        let fetched = acked.fetch_reply().await?;
        acked.delete_reply().await?;
        ctx.state::<Arc<Seen>>()
            .unwrap()
            .push(format!("fetched {}", fetched.id));
        Ok(acked.done())
    }
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    seen: Arc<Seen>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

/// The payload key `NOCTIS_PAYLOAD_KEY` would hold in these tests.
fn key_a() -> PayloadKey {
    PayloadKey::from_hex(&"ab".repeat(32)).unwrap()
}

/// The previous payload key of a rotation.
fn key_b() -> PayloadKey {
    PayloadKey::from_hex(&"cd".repeat(32)).unwrap()
}

impl Running {
    async fn start() -> Self {
        Self::start_with(|b| {
            b.payload_key(key_a());
        })
        .await
    }

    /// As [`Running::start`], with `configure` applied to the builder (a
    /// `signed` Route is mounted, so it sets a payload key).
    async fn start_with(configure: impl FnOnce(&mut AppBuilder)) -> Self {
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
        )
        .route(
            Method::POST,
            FOLLOWUP,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "56" })),
        );
        let gw = FakeGateway::start().await.unwrap();
        let seen = Arc::new(Seen::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .ack_deadline(Duration::from_millis(50))
            .state(Arc::clone(&seen))
            .module(
                Module::new("shop").add::<Buy>().add::<Sig>().module(
                    Module::new("shop/forms")
                        .add::<BioForm>()
                        .add::<Note>()
                        .add::<SigBio>(),
                ),
            )
            .module(
                Module::new("ui")
                    .add::<Upd>()
                    .add::<Ban>()
                    .add::<SlowUpd>()
                    .add::<Open>()
                    .add::<UpdFetchDelete>(),
            )
            .module(Module::new("dash").uses::<Pick>().add::<Pick>());
        configure(&mut builder);
        let app = builder.build().unwrap();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let running = Self {
            gw,
            http,
            seen,
            run,
        };
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

    /// Polls until the handlers saw `n` lines.
    async fn wait_seen(&self, n: usize) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let lines = self.seen.lines();
            if lines.len() >= n {
                return lines;
            }
            assert!(
                Instant::now() < deadline,
                "{n} handler runs missing: {lines:?}\n{}",
                String::from_utf8_lossy(&capture().0.lock().unwrap())
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Polls until `n` requests other than `GET /gateway/bot` were sent.
    async fn wait_sent(&self, n: usize) -> Vec<RecordedRequest> {
        let deadline = Instant::now() + WAIT;
        loop {
            let sent = self.sent();
            if sent.len() >= n {
                return sent;
            }
            assert!(Instant::now() < deadline, "{n} requests missing: {sent:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Forgets every recorded request.
    fn clear(&self) {
        self.http.clear_requests();
    }

    /// The requests other than `GET /gateway/bot`.
    fn sent(&self) -> Vec<RecordedRequest> {
        self.http
            .requests()
            .into_iter()
            .filter(|r| r.route_path() != Some("/gateway/bot"))
            .collect()
    }

    fn stop(self) {
        self.run.abort();
    }
}

fn user() -> Value {
    json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// The callback path of interaction 100 (token `tok`).
const CALLBACK: &str = "/interactions/100/tok/callback";
/// The original response of application 7 (token `tok`).
const ORIGINAL: &str = "/webhooks/7/tok/messages/@original";
/// The follow-up path of application 7 (token `tok`).
const FOLLOWUP: &str = "/webhooks/7/tok";

fn body(request: &RecordedRequest) -> Value {
    request.json().unwrap_or(Value::Null)
}

/// Asserts `request` is an interaction request: no bot token and no
/// `with_components` (Story 7.2 Matrix 46).
fn assert_interaction_request(request: &RecordedRequest) {
    assert!(
        request.headers.get("authorization").is_none(),
        "{request:?}"
    );
    assert!(
        !request
            .query
            .as_deref()
            .unwrap_or("")
            .contains("with_components"),
        "{request:?}"
    );
}

/// Asserts `request` is the CID-06 notice: an ephemeral type-4 callback.
fn assert_notice(request: &RecordedRequest) {
    let path = request.route_path().unwrap_or_default();
    assert_eq!(request.method, Method::POST, "{request:?}");
    assert!(
        path.starts_with("/interactions/") && path.ends_with("/tok/callback"),
        "{path}"
    );
    assert_interaction_request(request);
    let notice = body(request);
    assert_eq!(notice["type"], 4, "{notice}");
    assert_eq!(notice["data"]["flags"], 32832, "{notice}");
    assert_eq!(notice["data"]["components"][0]["type"], 10, "{notice}");
    assert_eq!(notice["data"]["allowed_mentions"], json!({ "parse": [] }));
}

/// A complete V2 message 77 in channel 55: the clicked component's message.
fn source_message() -> Value {
    json!({
        "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "components": [],
    })
}

/// [`click`] with interaction id 100 on the V2 message 77.
fn click_on_message(custom_id: &str) -> Value {
    let mut d = click(100, custom_id, 2);
    d["message"] = source_message();
    d
}

/// A click (interaction `id`, token `tok`, application 7) in guild 9.
fn click(id: u64, custom_id: &str, component_type: u8) -> Value {
    json!({
        "id": id.to_string(),
        "application_id": "7",
        "type": 3,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "guild_id": "9",
        "channel": { "id": "55", "type": 0 },
        "member": { "user": user(), "roles": [], "joined_at": "2024-01-01T00:00:00.000000+00:00",
                    "deaf": false, "mute": false, "flags": 0, "nick": null,
                    "communication_disabled_until": null, "permissions": "0" },
        "locale": "tr",
        "guild_locale": "de",
        "data": { "custom_id": custom_id, "component_type": component_type },
    })
}

/// Matrix 1, 19, 26 and the Matrix 16 warning: a click, a select and a
/// modal submit reach their handlers; an unknown Route is one warning; no
/// request is sent.
#[tokio::test(flavor = "multi_thread")]
async fn clicks_and_submits_reach_their_handlers() {
    let capture = capture();
    let running = Running::start().await;
    running.dispatch(click(100, "0shop.buy:47", 2));
    let lines = running.wait_seen(1).await;
    assert_eq!(
        lines,
        [
            "buy 255 module=shop user=3 guild=Some(9) channel=Some(Id<ChannelMarker>(55)) shard=0 locale=Some(\"tr\")/Some(\"de\")"
        ]
    );
    running.dispatch(click(101, "4pick:0147", 3));
    running.dispatch(json!({
        "id": "102",
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "channel": { "id": "56", "type": 0 },
        "data": { "custom_id": "0bio:g", "components": [
            { "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "bio", "value": "hello" } }
        ] },
    }));
    let lines = running.wait_seen(3).await;
    let mut later = lines[1..].to_vec();
    later.sort();
    assert_eq!(
        later,
        [
            "bio of 42: Some(\"hello\") module=shop/forms channel=Some(Id<ChannelMarker>(56))",
            "pick 255"
        ]
    );
    running.dispatch(click(103, "0nope:", 2));
    let deadline = Instant::now() + WAIT;
    while capture.count(&[
        "noctis::dispatch",
        "reason=\"unknown_route\"",
        "route=\"nope\"",
    ]) == 0
    {
        assert!(Instant::now() < deadline, "no unknown_route warning");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(running.seen.lines().len(), 3);
    // The rejected click is answered with the CID-06 notice only.
    let sent = running.wait_sent(1).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_notice(&sent[0]);
    assert_eq!(sent[0].path, "/api/v10/interactions/103/tok/callback");
    running.stop();
}

/// Matrix 31: N clicks run N handlers and leave no task behind.
#[tokio::test(flavor = "multi_thread")]
async fn alive_tasks_return_to_baseline() {
    const N: u64 = 50;
    let running = Running::start().await;
    let metrics = tokio::runtime::Handle::current().metrics();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let baseline = metrics.num_alive_tasks();
    for i in 0..N {
        running.dispatch(click(200 + i, "0shop.buy:47", 2));
    }
    assert_eq!(running.wait_seen(50).await.len(), 50);
    let deadline = Instant::now() + WAIT;
    while metrics.num_alive_tasks() > baseline {
        assert!(
            Instant::now() < deadline,
            "{} alive tasks, baseline {baseline}",
            metrics.num_alive_tasks()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(running.sent().is_empty());
    running.stop();
}

/// Matrix 2 through `AppBuilder::build`: one Route in two mounted Modules.
#[tokio::test(flavor = "multi_thread")]
async fn duplicate_route_fails_the_build() {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .module(Module::new("a").add::<Buy>())
        .module(Module::new("b").add::<Buy>());
    let err = builder.build().unwrap_err();
    let [
        BuildViolation::DuplicateRoute {
            route,
            first_module,
            first_handler,
            second_module,
            second_handler,
        },
    ] = err.violations()
    else {
        panic!("{err}");
    };
    assert_eq!(
        (*route, *first_module, *second_module),
        ("shop.buy", "a", "b")
    );
    assert_eq!(
        (*first_handler, *second_handler),
        ("components::Buy", "components::Buy")
    );
    assert!(err.violations()[0].to_string().starts_with("[CID-04]"));

    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .module(Module::new("dash").uses::<Buy>());
    let err = builder.build().unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [BuildViolation::UnmountedRoute {
                module: "dash",
                route: "shop.buy",
                ..
            }]
        ),
        "{err}"
    );
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .module(Module::new("dash").uses::<Buy>())
        .module(Module::new("shop").add::<Buy>());
    assert!(builder.build().is_ok());
}

/// Story 7.2 Matrix 1-5 (AC 1): every rejection is one warning and one
/// ephemeral type-4 notice, without a bot token; no handler runs.
#[tokio::test(flavor = "multi_thread")]
async fn rejections_send_the_notice() {
    let capture = capture();
    let running = Running::start().await;
    let mut anonymous = click(100, "0shop.buy:47", 2);
    anonymous.as_object_mut().unwrap().remove("member");
    let cases = [
        (click(100, "0nope:", 2), "unknown_route"),
        (click(100, "0shop.buy:47", 3), "kind_mismatch"),
        (click(100, "0shop.buy:", 2), "undecodable"),
        (click(100, "0Shop.buy:47", 2), "malformed"),
        (anonymous, "no_author"),
    ];
    for (d, reason) in cases {
        running.clear();
        let before = capture.count(&[
            "component interaction rejected",
            &format!("reason=\"{reason}\""),
        ]);
        running.dispatch(d);
        let sent = running.wait_sent(1).await;
        assert_eq!(sent.len(), 1, "{reason}: {sent:?}");
        assert_notice(&sent[0]);
        assert_eq!(
            capture.count(&[
                "component interaction rejected",
                &format!("reason=\"{reason}\"")
            ]),
            before + 1,
            "{reason}"
        );
    }
    assert!(running.seen.lines().is_empty());
    running.stop();
}

/// Story 7.2 Matrix 9-12 (AC 2): ids signed with the current or the
/// previous key run the handler; a flipped tag is `forged` with the
/// notice; no log line shows the keys.
#[tokio::test(flavor = "multi_thread")]
async fn signed_ids_and_key_rotation() {
    let capture = capture();
    let running = Running::start_with(|b| {
        b.payload_key(key_a()).previous_payload_key(key_b());
    })
    .await;
    let id = |item: u64, key: &PayloadKey| {
        encode::<Sig>(&Sig { item }, &Parts::new().with_signer(key))
            .unwrap()
            .to_string()
    };
    running.dispatch(click(100, &id(1, &key_a()), 2));
    running.dispatch(click(101, &id(2, &key_b()), 2));
    let mut lines = running.wait_seen(2).await;
    lines.sort();
    assert_eq!(lines, ["sig 1", "sig 2"]);
    let mut flipped = id(3, &key_a());
    let last = flipped.pop().unwrap();
    flipped.push(if last == 'a' { 'b' } else { 'a' });
    let before = capture.count(&["component interaction rejected", "reason=\"forged\""]);
    running.dispatch(click(102, &flipped, 2));
    let sent = running.wait_sent(1).await;
    assert_notice(&sent[0]);
    assert_eq!(
        capture.count(&["component interaction rejected", "reason=\"forged\""]),
        before + 1
    );
    assert_eq!(running.seen.lines().len(), 2);
    let text = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(!text.contains("abab") && !text.contains("cdcd"));
    running.stop();
}

/// Story 7.2 Matrix 20 and 46 (AC 3): `update` on a V2 message is exactly
/// one type-7 callback with the V2 flag, no mentions, the attachments
/// keep-list and no `EPHEMERAL`.
#[tokio::test(flavor = "multi_thread")]
async fn update_is_one_type_7_callback() {
    let running = Running::start().await;
    running.dispatch(click_on_message("0upd:"));
    let sent = running.wait_sent(1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let sent_after = running.sent();
    assert_eq!(sent_after.len(), 1, "{sent_after:?}");
    assert_eq!(
        (
            &sent[0].method,
            sent[0].route_path(),
            sent[0].query.as_deref()
        ),
        (&Method::POST, Some(CALLBACK), None)
    );
    assert_interaction_request(&sent[0]);
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 7, "data": {
            "flags": 32768,
            "components": [{ "type": 10, "id": 1, "content": "updated" }],
            "allowed_mentions": { "parse": [] },
            "attachments": [],
        } })
    );
    running.stop();
}

/// Story 7.2 Matrix 29 and 30 (AC 4): past the ack deadline the auto-defer
/// follows the declared response, then the answer edits `@original`.
#[tokio::test(flavor = "multi_thread")]
async fn auto_defer_follows_the_declared_response() {
    let running = Running::start().await;
    running.dispatch(click_on_message("0ban:"));
    let sent = running.wait_sent(2).await;
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    assert_eq!(
        (&sent[1].method, sent[1].route_path()),
        (&Method::PATCH, Some(ORIGINAL))
    );
    assert_eq!(body(&sent[1])["flags"], 32768);
    assert_eq!(body(&sent[1])["attachments"], json!([]));
    for request in &sent {
        assert_interaction_request(request);
    }

    running.clear();
    running.dispatch(click_on_message("0slow.upd:"));
    let sent = running.wait_sent(2).await;
    assert_eq!(body(&sent[0]), json!({ "type": 6 }));
    assert_eq!(
        (&sent[1].method, sent[1].route_path()),
        (&Method::PATCH, Some(ORIGINAL))
    );
    assert_eq!(body(&sent[1])["components"][0]["content"], "later");
    running.stop();
}

/// Story 7.2 Matrix 35 and 36 (AC 5): an `auto_defer = false` button opens
/// its modal with the only callback; one that never answers logs one error
/// naming the handler at the 2.9 s deadline and sends nothing.
#[tokio::test(flavor = "multi_thread")]
async fn manual_button_opens_a_modal() {
    let capture = capture();
    let running = Running::start().await;
    let open = encode::<Open>(&Open { late: false }, &Parts::new())
        .unwrap()
        .to_string();
    running.dispatch(click_on_message(&open));
    let sent = running.wait_sent(1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(running.sent().len(), 1);
    let modal = body(&sent[0]);
    assert_eq!(modal["type"], 9, "{modal}");
    assert_eq!(modal["data"]["title"], "Bio");
    assert_interaction_request(&sent[0]);

    running.clear();
    let late = encode::<Open>(&Open { late: true }, &Parts::new())
        .unwrap()
        .to_string();
    running.dispatch(click_on_message(&late));
    let missed = [
        "noctis::dispatch",
        "did not answer within the 3 s ack deadline",
        "handler=\"components::Open\"",
        "module=\"ui\"",
    ];
    let deadline = Instant::now() + WAIT;
    while capture.count(&missed) == 0 {
        assert!(Instant::now() < deadline, "no missed-deadline error");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(capture.count(&missed), 1);
    assert!(running.sent().is_empty(), "{:?}", running.sent());
    running.stop();
}

/// Story 7.2 Matrix 41 (AC 6): a slash-opened modal submit has no message
/// to update; `update` and `defer_update` send nothing.
#[tokio::test(flavor = "multi_thread")]
async fn slash_opened_modal_cannot_update() {
    let running = Running::start().await;
    for step in [0_u8, 1] {
        let form = serde_json::to_value(Note { step, note: None }.build()).unwrap();
        let id = form["custom_id"].as_str().unwrap().to_owned();
        running.dispatch(json!({
            "id": (300 + u64::from(step)).to_string(),
            "application_id": "7",
            "type": 5,
            "token": "tok",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "user": user(),
            "data": { "custom_id": id, "components": [
                { "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "note", "value": "n" } }
            ] },
        }));
    }
    let mut lines = running.wait_seen(2).await;
    lines.sort();
    let text = "there is no message to update: this modal was not opened from a component message";
    assert_eq!(
        lines,
        [format!("note 0: {text}"), format!("note 1: {text}")]
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(running.sent().is_empty(), "{:?}", running.sent());
    running.stop();
}

/// A modal submit (interaction `id`, token `tok`, application 7) of
/// `custom_id` with the text input `bio`.
fn bio_submit(id: u64, custom_id: &str) -> Value {
    json!({
        "id": id.to_string(),
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(),
        "data": { "custom_id": custom_id, "components": [
            { "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "bio", "value": "hi" } }
        ] },
    })
}

/// Story 7.2 S1 and S3: a derived `signed` modal tagged with the current
/// key runs its handler with the Payload and the input; a flipped tag is
/// `forged` with the notice.
#[tokio::test(flavor = "multi_thread")]
async fn signed_modal_submit_runs_its_handler() {
    let capture = capture();
    let running = Running::start().await;
    let id = |item: u64| {
        let mut modal = SigBio::blank(item);
        let id = modal.custom_id_mut();
        assert!(
            id.resolve(&Parts::new().with_signer(&key_a()))
                .unwrap()
                .is_none()
        );
        id.get().unwrap().to_owned()
    };
    running.dispatch(bio_submit(400, &id(47)));
    assert_eq!(running.wait_seen(1).await, ["sigbio 47: hi"]);

    let mut flipped = id(48);
    let last = flipped.pop().unwrap();
    flipped.push(if last == 'a' { 'b' } else { 'a' });
    let before = capture.count(&["component interaction rejected", "reason=\"forged\""]);
    running.dispatch(bio_submit(401, &flipped));
    let sent = running.wait_sent(1).await;
    assert_notice(&sent[0]);
    assert_eq!(
        capture.count(&["component interaction rejected", "reason=\"forged\""]),
        before + 1
    );
    assert_eq!(running.seen.lines().len(), 1);
    running.stop();
}

/// Story 7.2 S9 and S12: after `update`, `fetch_reply` and `delete_reply`
/// act on the clicked message over the interaction token.
#[tokio::test(flavor = "multi_thread")]
async fn fetch_and_delete_after_update() {
    let running = Running::start().await;
    running
        .http
        .route(
            Method::GET,
            ORIGINAL,
            FakeResponse::json(StatusCode::OK, &source_message()),
        )
        .route(
            Method::DELETE,
            ORIGINAL,
            FakeResponse::empty(StatusCode::NO_CONTENT),
        );
    running.dispatch(click_on_message("0upd.fd:"));
    assert_eq!(running.wait_seen(1).await, ["fetched 77"]);
    let sent = running.wait_sent(3).await;
    assert_eq!(body(&sent[0])["type"], 7);
    assert_eq!(
        (&sent[1].method, sent[1].route_path()),
        (&Method::GET, Some(ORIGINAL))
    );
    assert_eq!(
        (&sent[2].method, sent[2].route_path()),
        (&Method::DELETE, Some(ORIGINAL))
    );
    for request in &sent {
        assert_interaction_request(request);
    }
    running.stop();
}
