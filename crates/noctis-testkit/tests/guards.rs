//! Route Guards end to end (Matrix 17, 18, 20-22, 25, 26, 30, 31,
//! 34 and 35): a `FakeGateway` click or modal submit on a guarded Route is
//! denied with one localized ephemeral notice (or, after an auto-defer, an
//! ephemeral follow-up) and its handler never runs; every response is
//! asserted on the `FakeDiscordHttp` wire (method, path, body).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use hyper::{Method, StatusCode};
use noctis::__private::core::__testing::responded;
use noctis::ports::RefStore as _;
use noctis::ports::memory::MemoryRefStore;
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::{AppBuilder, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, RecordedRequest,
};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The callback path of interaction 100 (token `tok`).
const CALLBACK: &str = "/interactions/100/tok/callback";
/// The original response of application 7 (token `tok`).
const ORIGINAL: &str = "/webhooks/7/tok/messages/@original";
/// The follow-up path of application 7 (token `tok`).
const FOLLOWUP: &str = "/webhooks/7/tok";
/// The Ref of the one-shot record.
const REF: &str = "0123456789A";

/// `noctis.guard.owner`, `.permissions` and `.failed` in en-US.
const OWNER: &str = "Only the person who started this can use it.";
const PERMISSIONS: &str = "You don't have permission to use this.";
const FAILED: &str = "You can't use this.";
/// `noctis.error.generic` in en-US.
const GENERIC: &str = "Something went wrong. Please try again later.";

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
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// A context that reads the `Seen` state.
trait HasState {
    fn seen(&self) -> Arc<Seen>;
}

impl HasState for ComponentCtx {
    fn seen(&self) -> Arc<Seen> {
        Arc::clone(&self.state::<Arc<Seen>>().unwrap())
    }
}

impl HasState for ModalSubmitCtx {
    fn seen(&self) -> Arc<Seen> {
        Arc::clone(&self.state::<Arc<Seen>>().unwrap())
    }
}

fn saw(ctx: &impl HasState, line: impl Into<String>) {
    ctx.seen().0.lock().unwrap().push(line.into());
}

/// Matrix 17: only `user` may click.
#[derive(Button)]
#[button("g.owned", owner = user, defaults(style = Primary, label = "Mine"))]
struct Owned {
    user: Id<UserMarker>,
}

impl Owned {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw(&ctx, format!("owned {}", self.user));
        Ok(responded())
    }
}

/// Matrix 18, 20-22: `MANAGE_GUILD` only.
#[derive(Button)]
#[button("g.staff", permissions = MANAGE_GUILD, defaults(style = Primary, label = "Staff"))]
struct Staff;

impl Staff {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw(&ctx, "staff");
        Ok(responded())
    }
}

/// Denies with the default key (`which = 0`) or an unknown one.
#[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
async fn keyed(_ctx: &ComponentCtx, this: &Keyed) -> HandlerResult<Access> {
    Ok(match this.which {
        0 => Access::deny(),
        _ => Access::Deny(MessageKey::new("mod.staff_only")),
    })
}

/// Matrix 25 and 26: a custom Guard's denial key.
#[derive(Button)]
#[button("g.keyed", guard = keyed, defaults(style = Primary, label = "Keyed"))]
struct Keyed {
    which: u8,
}

impl Keyed {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw(&ctx, "keyed");
        Ok(responded())
    }
}

/// Denies after the 50 ms ack deadline passed.
async fn slow_deny<T: Sync>(_ctx: &ComponentCtx, _this: &T) -> HandlerResult<Access> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(Access::deny())
}

/// Matrix 30: `response = ephemeral` (a type-5 auto-defer).
#[derive(Button)]
#[button("g.sloweph", response = ephemeral, guard = slow_deny, defaults(style = Primary, label = "E"))]
struct SlowEph;

impl SlowEph {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw(&ctx, "sloweph");
        Ok(responded())
    }
}

/// Matrix 31: `response = update` (a type-6 auto-defer on the message).
#[derive(Button)]
#[button("g.slowupd", guard = slow_deny, defaults(style = Primary, label = "U"))]
struct SlowUpd;

impl SlowUpd {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw(&ctx, "slowupd");
        Ok(responded())
    }
}

/// Matrix 34: a modal only `user` may submit.
#[derive(Modal)]
#[modal("g.form", owner = user, defaults(title = "Form"))]
struct Form {
    user: Id<UserMarker>,
    #[text_input("Bio")]
    bio: String,
}

impl Form {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        saw(&ctx, format!("form {} {}", self.user, self.bio));
        Ok(responded())
    }
}

/// Matrix 35: a one-shot confirmation only `user` may use.
#[derive(Button)]
#[button("g.once", by_ref, one_shot, owner = user, defaults(style = Danger, label = "Ok"))]
struct Once {
    user: Id<UserMarker>,
}

impl Once {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw(&ctx, format!("once {}", self.user));
        Ok(responded())
    }
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    seen: Arc<Seen>,
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
        )
        .route(
            Method::DELETE,
            ORIGINAL,
            FakeResponse::empty(StatusCode::NO_CONTENT),
        )
        .route(
            Method::POST,
            FOLLOWUP,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "56", "channel_id": "55" })),
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
                Module::new("g")
                    .add::<Owned>()
                    .add::<Staff>()
                    .add::<Keyed>()
                    .add::<SlowEph>()
                    .add::<SlowUpd>()
                    .add::<Form>()
                    .add::<Once>(),
            );
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

    /// Dispatches `d`, waits for `n` requests and a little longer, and
    /// returns exactly those (then forgets them).
    async fn exactly(&self, d: Value, n: usize) -> Vec<RecordedRequest> {
        self.dispatch(d);
        let deadline = Instant::now() + WAIT;
        while self.sent().len() < n {
            assert!(
                Instant::now() < deadline,
                "{n} requests missing: {:?}",
                self.sent()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        let sent = self.sent();
        assert_eq!(sent.len(), n, "{sent:?}");
        self.http.clear_requests();
        sent
    }

    /// Polls until the handlers saw `n` lines.
    async fn wait_seen(&self, n: usize) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let lines = self.seen.lines();
            if lines.len() >= n {
                return lines;
            }
            assert!(Instant::now() < deadline, "{n} runs missing: {lines:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
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

fn user(id: u64) -> Value {
    json!({ "id": id.to_string(), "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// A click on `custom_id` by `by`: in guild 9 with the member
/// `permissions`, or in a DM for `None`.
fn click(custom_id: &str, by: u64, permissions: Option<&str>) -> Value {
    let mut d = json!({
        "id": "100",
        "application_id": "7",
        "type": 3,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "channel": { "id": "55", "type": 0 },
        "locale": "tr",
        "data": { "custom_id": custom_id, "component_type": 2 },
    });
    match permissions {
        Some(p) => {
            d["guild_id"] = json!("9");
            d["member"] = json!({ "user": user(by), "roles": [],
                "joined_at": "2024-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false,
                "flags": 0, "permissions": p });
        }
        None => d["user"] = user(by),
    }
    d
}

/// [`click`] in a DM on the V2 message 77.
fn click_on_message(custom_id: &str) -> Value {
    let mut d = click(custom_id, 3, None);
    d["message"] = json!({
        "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "components": [],
    });
    d
}

fn body(request: &RecordedRequest) -> Value {
    request.json().unwrap_or(Value::Null)
}

/// Asserts `request` is one ephemeral type-4 notice callback of `text`,
/// without a bot token.
fn assert_notice(request: &RecordedRequest, text: &str) {
    assert_eq!(
        (&request.method, request.route_path()),
        (&Method::POST, Some(CALLBACK)),
        "{request:?}"
    );
    assert!(request.headers.get("authorization").is_none());
    let b = body(request);
    assert_eq!(b["type"], 4, "{b}");
    assert_eq!(b["data"]["flags"], 32832, "{b}");
    assert_eq!(
        b["data"]["components"],
        json!([{ "type": 10, "id": 1, "content": text }]),
        "{b}"
    );
}

/// Asserts `request` is an ephemeral follow-up notice of `text`.
fn assert_followup(request: &RecordedRequest, text: &str) {
    assert_eq!(
        (&request.method, request.route_path()),
        (&Method::POST, Some(FOLLOWUP)),
        "{request:?}"
    );
    let b = body(request);
    assert_eq!(b["flags"], 32832, "{b}");
    assert_eq!(b["components"][0]["content"], text, "{b}");
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_guard_denies_with_ephemeral_notice() {
    let running = Running::start(|_| {}).await;
    // `Owned { user: 42 }` (base62 `g`), clicked by 43.
    let sent = running.exactly(click("0g.owned:g", 43, None), 1).await;
    assert_notice(&sent[0], OWNER);
    assert!(running.seen.lines().is_empty());
    // The owner passes.
    running.dispatch(click("0g.owned:g", 42, None));
    assert_eq!(running.wait_seen(1).await, ["owned 42"]);
    running.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn permissions_guard_allows_admin_and_denies_dm() {
    let running = Running::start(|_| {}).await;
    for permissions in ["32", "8"] {
        running.dispatch(click("0g.staff:", 3, Some(permissions)));
    }
    assert_eq!(running.wait_seen(2).await, ["staff", "staff"]);
    tokio::time::sleep(Duration::from_millis(150)).await;
    running.http.clear_requests();
    for denied in [
        click("0g.staff:", 3, Some("0")),
        click("0g.staff:", 3, Some("4")),
        click("0g.staff:", 3, None),
    ] {
        let sent = running.exactly(denied, 1).await;
        assert_notice(&sent[0], PERMISSIONS);
    }
    assert_eq!(running.seen.lines().len(), 2);
    running.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_guard_deny_key() {
    let capture = capture();
    let running = Running::start(|_| {}).await;
    let sent = running.exactly(click("0g.keyed:0", 3, None), 1).await;
    assert_notice(&sent[0], FAILED);
    // An unknown key renders the generic text (for the `tr` user) and
    // warns once.
    let before = capture.count(&["noctis::i18n", "key=\"mod.staff_only\""]);
    let sent = running.exactly(click("0g.keyed:1", 3, None), 1).await;
    assert_notice(&sent[0], GENERIC);
    assert_eq!(
        capture.count(&["noctis::i18n", "key=\"mod.staff_only\""]),
        before + 1
    );
    assert!(running.seen.lines().is_empty());
    running.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn deny_after_type5_defer_deletes_original() {
    let running = Running::start(|_| {}).await;
    let sent = running.exactly(click("0g.sloweph:", 3, None), 3).await;
    assert_eq!(
        (&sent[0].method, sent[0].route_path()),
        (&Method::POST, Some(CALLBACK))
    );
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    assert_eq!(
        (&sent[1].method, sent[1].route_path()),
        (&Method::DELETE, Some(ORIGINAL))
    );
    assert_followup(&sent[2], FAILED);
    assert!(running.seen.lines().is_empty());
    running.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn deny_after_type6_defer_follows_up() {
    let running = Running::start(|_| {}).await;
    let sent = running.exactly(click_on_message("0g.slowupd:"), 2).await;
    assert_eq!(body(&sent[0]), json!({ "type": 6 }));
    assert_followup(&sent[1], FAILED);
    assert!(sent.iter().all(|r| r.method != Method::DELETE));
    assert!(running.seen.lines().is_empty());
    running.stop();
}

/// A submit of `Form` for user 42 (`g`) by `by`.
fn submit(by: u64) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(by),
        "channel": { "id": "55", "type": 0 },
        "data": { "custom_id": "0g.form:g", "components": [
            { "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "bio", "value": "hi" } }
        ] },
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn modal_owner_guard() {
    let running = Running::start(|_| {}).await;
    let sent = running.exactly(submit(3), 1).await;
    assert_notice(&sent[0], OWNER);
    assert!(running.seen.lines().is_empty());
    running.dispatch(submit(42));
    assert_eq!(running.wait_seen(1).await, ["form 42 hi"]);
    running.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn one_shot_ref_survives_denial() {
    let refs = Arc::new(MemoryRefStore::new());
    let store = Arc::clone(&refs);
    let running = Running::start(move |b| {
        b.ref_store(store);
    })
    .await;
    let key = format!("default:ref:{REF}");
    let stored = bytes::Bytes::from_static(b"0g.once:g");
    assert!(
        refs.put_if_absent(&key, stored.clone(), Some(Duration::from_mins(1)))
            .await
            .unwrap()
    );
    let id = format!("0g.once:~{REF}");
    // A non-owner is denied, and the record is put back.
    let sent = running.exactly(click(&id, 3, None), 1).await;
    assert_notice(&sent[0], OWNER);
    assert_eq!(refs.get(&key).await.unwrap(), Some(stored));
    // The owner's click reaches the handler and consumes it.
    running.dispatch(click(&id, 42, None));
    assert_eq!(running.wait_seen(1).await, ["once 42"]);
    assert_eq!(refs.get(&key).await.unwrap(), None);
    running.stop();
}
