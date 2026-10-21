//! Story 7.8: modal Routes and typed submissions end to end (FR-21, FR-29,
//! FR-64; INT-04, INT-05, MOD-08, MOD-09). A `FakeGateway`
//! `INTERACTION_CREATE` click opens a `#[derive(Modal)]` struct, its submit
//! is decoded from the raw interaction bytes (Radio Group, Checkbox Group
//! and Checkbox included, which twilight drops) and the typed `#[handler]`
//! answers per `#[modal(.., response = .., auto_defer = ..)]`. Every
//! response is asserted on the `FakeDiscordHttp` wire (method, path, query,
//! headers, body).

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
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{Components, ResolvedAttachment, ResolvedUser, TextDisplay, V2Message};
use noctis::{Runtime, Token};
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
/// `noctis.guard.owner` in en-US.
const OWNER: &str = "Only the person who started this can use it.";
/// `noctis.error.generic` in en-US.
const GENERIC: &str = "Something went wrong. Please try again later.";
/// `IS_COMPONENTS_V2`.
const V2: u64 = 32768;
/// `IS_COMPONENTS_V2 | EPHEMERAL`.
const V2_EPHEMERAL: u64 = 32832;

// --- Log capture -------------------------------------------------------------

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

/// The shared [`Seen`] of `ctx`'s App.
fn seen(ctx: &ModalSubmitCtx) -> Arc<Seen> {
    Arc::clone(&ctx.state::<Arc<Seen>>().unwrap())
}

fn view(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

// --- The structs -------------------------------------------------------------

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    #[choice("Low", value = "l")]
    Low,
    #[choice("High", value = "h")]
    High,
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Topic {
    #[choice("Rust")]
    Rust,
    #[choice("Art", value = "art")]
    Art,
    #[choice("Music", value = "music")]
    Music,
}

/// The button that opens [`Survey`]: `auto_defer = false` with the
/// `Manual` responder (INT-05).
#[derive(Button, Debug, Clone, Copy)]
#[button("survey.open", auto_defer = false, defaults(style = Primary, label = "Answer"))]
struct OpenSurvey {
    user: Id<UserMarker>,
}

impl OpenSurvey {
    #[handler]
    async fn handle(
        self,
        _ctx: ComponentCtx,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.show_modal(Survey::blank(self.user)).await?.done())
    }
}

/// `/survey` opens the same modal from a slash command (row 33).
#[derive(SlashCommand, Debug)]
#[command("survey", description = "Answer the survey", auto_defer = false)]
struct SurveyCommand;

impl SurveyCommand {
    #[handler]
    async fn handle(
        self,
        ctx: CommandCtx,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.show_modal(Survey::blank(ctx.user_id())).await?.done())
    }
}

/// Text Input, Radio Group, Checkbox Group and Checkbox (rows 25, 27, 28),
/// with the default `response = update`; only `user` may submit it.
#[derive(Modal, Debug)]
#[modal("survey", owner = user, defaults(title = "Survey"))]
struct Survey {
    user: Id<UserMarker>,
    #[text_input("Note", max_length = 20)]
    note: Option<String>,
    #[radio_group("Severity")]
    severity: Severity,
    #[checkbox_group("Topics", min_values = 0, max_values = 2)]
    topics: Vec<Topic>,
    #[checkbox("I agree")]
    agree: bool,
}

impl Survey {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<V2Message> {
        seen(&ctx).push(format!(
            "survey {} {:?} {:?} {:?} {}",
            self.user, self.note, self.severity, self.topics, self.agree
        ));
        Ok(view("saved"))
    }
}

/// A user select and a File Upload (row 26).
#[derive(Modal, Debug)]
#[modal("evidence", response = ephemeral, defaults(title = "Evidence"))]
struct Evidence {
    item: u64,
    #[user_select("Who")]
    who: ResolvedUser,
    #[file_upload("Files", max_values = 2)]
    files: Vec<ResolvedAttachment>,
}

impl Evidence {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<V2Message> {
        let files: Vec<String> = self
            .files
            .iter()
            .map(|f| {
                format!(
                    "{} {} {} {:?}",
                    f.filename(),
                    f.size(),
                    f.url(),
                    f.content_type()
                )
            })
            .collect();
        seen(&ctx).push(format!(
            "evidence {} {} {files:?}",
            self.item, self.who.user.id
        ));
        Ok(view("thanks"))
    }
}

macro_rules! slow_modal {
    ($name:ident, $id:tt, $($response:tt)*) => {
        /// Sleeps `ms`, then returns a message; only `user` may submit it.
        #[derive(Modal, Debug)]
        #[modal($id, owner = user, $($response)* defaults(title = "Slow"))]
        struct $name {
            user: Id<UserMarker>,
            ms: u64,
            #[text_input("Note")]
            #[allow(dead_code, reason = "the submitted input is decoded, not read")]
            note: Option<String>,
        }

        impl $name {
            #[handler]
            async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<V2Message> {
                tokio::time::sleep(Duration::from_millis(self.ms)).await;
                Ok(view("later"))
            }
        }
    };
}

slow_modal!(SlowUpdate, "slow.upd",);
slow_modal!(SlowEphemeral, "slow.eph", response = ephemeral,);
slow_modal!(SlowPublic, "slow.pub", response = public,);

/// `auto_defer = false`: it answers itself (`step` 0), stays silent past
/// the 3 s deadline (1) or defers itself and edits (2) (rows 22-24).
#[derive(Modal, Debug)]
#[modal("manual", auto_defer = false, response = ephemeral, defaults(title = "Manual"))]
struct ManualForm {
    step: u8,
    #[text_input("Note")]
    #[allow(dead_code, reason = "the submitted input is decoded, not read")]
    note: Option<String>,
}

impl ManualForm {
    #[handler]
    async fn handle(
        self,
        _ctx: ModalSubmitCtx,
        r: ModalResponder<Manual>,
    ) -> HandlerResult<Responded> {
        match self.step {
            0 => Ok(r.reply(view("mine")).await?.done()),
            1 => {
                tokio::time::sleep(Duration::from_millis(3200)).await;
                drop(r);
                Ok(responded())
            }
            _ => {
                let acked = r.defer_reply().await?;
                acked.edit_reply(view("edited")).await?;
                Ok(acked.done())
            }
        }
    }
}

// --- The running App ---------------------------------------------------------

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    seen: Arc<Seen>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start() -> Self {
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
                Module::new("forms")
                    .add::<OpenSurvey>()
                    .add::<SurveyCommand>()
                    .add::<Survey>()
                    .add::<Evidence>()
                    .add::<SlowUpdate>()
                    .add::<SlowEphemeral>()
                    .add::<SlowPublic>()
                    .add::<ManualForm>(),
            );
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

    /// Polls until `n` requests other than `GET /gateway/bot` were sent,
    /// then checks that no more arrive for a moment.
    async fn exactly(&self, n: usize) -> Vec<RecordedRequest> {
        let deadline = Instant::now() + WAIT;
        loop {
            if self.sent().len() >= n {
                break;
            }
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
        for request in &sent {
            assert_interaction_request(request);
        }
        self.http.clear_requests();
        sent
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

// --- Wire helpers ------------------------------------------------------------

fn user(id: u64) -> Value {
    json!({ "id": id.to_string(), "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn body(request: &RecordedRequest) -> Value {
    request.json().unwrap_or(Value::Null)
}

/// Asserts `request` is an interaction request: no bot token and no
/// `with_components`.
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

/// Asserts `request` is a `POST` callback (any query).
fn assert_callback(request: &RecordedRequest) {
    assert_eq!(request.method, Method::POST, "{request:?}");
    assert_eq!(request.route_path(), Some(CALLBACK), "{request:?}");
}

/// Asserts `request` is the `PATCH @original` V2 edit, without `EPHEMERAL`.
fn assert_edit(request: &RecordedRequest) {
    assert_eq!(
        (&request.method, request.route_path()),
        (&Method::PATCH, Some(ORIGINAL)),
        "{request:?}"
    );
    let edit = body(request);
    assert_eq!(edit["flags"], V2, "{edit}");
    assert_eq!(edit["attachments"], json!([]), "{edit}");
}

/// Asserts `request` is the ephemeral type-4 notice of `text`.
fn assert_notice(request: &RecordedRequest, text: &str) {
    assert_callback(request);
    let notice = body(request);
    assert_eq!(notice["type"], 4, "{notice}");
    assert_eq!(notice["data"]["flags"], V2_EPHEMERAL, "{notice}");
    assert_eq!(notice["data"]["components"][0]["content"], text, "{notice}");
}

/// A complete V2 message 77 in channel 55: the clicked button's message.
fn source_message() -> Value {
    json!({
        "id": "77", "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "components": [],
    })
}

/// A button click (interaction 100, token `tok`, application 7) by user 3
/// on message 77.
fn click(custom_id: &str) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": 3,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(3),
        "channel": { "id": "55", "type": 0 },
        "message": source_message(),
        "data": { "custom_id": custom_id, "component_type": 2 },
    })
}

/// A modal submit (interaction 100) of `custom_id` by user `by` with the
/// Label-wrapped `inputs` and `resolved`; `from_message` adds message 77.
fn submit(
    custom_id: &str,
    by: u64,
    inputs: &[Value],
    resolved: Option<Value>,
    from_message: bool,
) -> Value {
    let components: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| json!({ "type": 18, "id": i * 2 + 1, "component": input }))
        .collect();
    let mut d = json!({
        "id": "100",
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(by),
        "channel": { "id": "55", "type": 0 },
        "data": { "custom_id": custom_id, "components": components },
    });
    if let Some(resolved) = resolved {
        d["data"]["resolved"] = resolved;
    }
    if from_message {
        d["message"] = source_message();
    }
    d
}

fn text(key: &str, value: &str) -> Value {
    json!({ "type": 4, "id": 2, "custom_id": key, "value": value })
}

/// The four Survey inputs.
fn survey_inputs(note: &str, severity: &str, topics: &[&str], agree: bool) -> Vec<Value> {
    vec![
        text("note", note),
        json!({ "type": 21, "id": 4, "custom_id": "severity", "value": severity }),
        json!({ "type": 22, "id": 6, "custom_id": "topics", "values": topics }),
        json!({ "type": 23, "id": 8, "custom_id": "agree", "value": agree }),
    ]
}

/// The encoded `custom_id` of `modal`.
fn modal_id(modal: noctis::ui::Modal) -> String {
    serde_json::to_value(modal).unwrap()["custom_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn survey_id(user: u64) -> String {
    modal_id(Survey::blank(Id::new(user)))
}

// --- The tests ---------------------------------------------------------------

/// Rows 32 and 16: the `auto_defer = false` button opens the modal with
/// one type-9 callback carrying the struct's encoded Route id and its
/// declared components; the submit from message 77 runs the typed handler,
/// whose returned View updates message 77 with one type-7 callback.
#[tokio::test(flavor = "multi_thread")]
async fn button_opens_modal_and_submit_updates_message() {
    let running = Running::start().await;
    let open = encode::<OpenSurvey>(&OpenSurvey { user: Id::new(3) }, &Parts::new())
        .unwrap()
        .to_string();
    running.dispatch(click(&open));
    let sent = running.exactly(1).await;
    assert_callback(&sent[0]);
    let modal = body(&sent[0]);
    assert_eq!(modal["type"], 9, "{modal}");
    let data = &modal["data"];
    assert_eq!(data["custom_id"], survey_id(3), "{modal}");
    assert_eq!(data["title"], "Survey");
    let kinds: Vec<(Value, Value)> = data["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["type"].clone(), c["component"]["type"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [(18, 4), (18, 21), (18, 22), (18, 23)].map(|(a, b)| (json!(a), json!(b)))
    );
    assert_eq!(data["components"][1]["component"]["custom_id"], "severity");
    assert_eq!(
        data["components"][2]["component"]["options"][1]["value"],
        "art"
    );

    let id = data["custom_id"].as_str().unwrap().to_owned();
    running.dispatch(submit(
        &id,
        3,
        &survey_inputs("hi", "h", &["Rust", "art"], true),
        None,
        true,
    ));
    assert_eq!(
        running.wait_seen(1).await,
        ["survey 3 Some(\"hi\") High [Rust, Art] true"]
    );
    let sent = running.exactly(1).await;
    assert_callback(&sent[0]);
    let update = body(&sent[0]);
    assert_eq!(update["type"], 7, "{update}");
    assert_eq!(update["data"]["flags"], V2, "{update}");
    assert_eq!(update["data"]["components"][0]["content"], "saved");
    assert_eq!(update["data"]["attachments"], json!([]));
    running.stop();
}

/// Rows 25-28: every FR-64 input kind decodes from the raw bytes, through
/// two modals (MOD-02's 5-component cap).
#[tokio::test(flavor = "multi_thread")]
async fn fr64_inputs_decode_end_to_end() {
    let running = Running::start().await;
    // Row 27: unicode text, verbatim.
    running.dispatch(submit(
        &survey_id(3),
        3,
        &survey_inputs("çğış 🦀", "l", &["music"], false),
        None,
        true,
    ));
    assert_eq!(
        running.wait_seen(1).await,
        ["survey 3 Some(\"çğış 🦀\") Low [Music] false"]
    );
    running.exactly(1).await;
    // Row 28: empty optional values.
    running.dispatch(submit(
        &survey_id(3),
        3,
        &survey_inputs("", "h", &[], false),
        None,
        true,
    ));
    assert_eq!(running.wait_seen(2).await[1], "survey 3 None High [] false");
    running.exactly(1).await;

    // Row 26: a user select and a File Upload joined with `resolved`.
    let id = modal_id(Evidence::blank(5));
    let resolved = json!({
        "users": { "3": user(3) },
        "attachments": {
            "31": { "id": "31", "filename": "one.png", "size": 4,
                    "url": "https://cdn.discordapp.com/ephemeral-attachments/1/31/one.png",
                    "proxy_url": "https://media.discordapp.net/ephemeral-attachments/1/31/one.png",
                    "content_type": "image/png", "ephemeral": true }
        }
    });
    running.dispatch(submit(
        &id,
        3,
        &[
            json!({ "type": 5, "id": 2, "custom_id": "who", "values": ["3"] }),
            json!({ "type": 19, "id": 4, "custom_id": "files", "values": ["31"] }),
        ],
        Some(resolved),
        false,
    ));
    assert_eq!(
        running.wait_seen(3).await[2],
        "evidence 5 3 [\"one.png 4 https://cdn.discordapp.com/ephemeral-attachments/1/31/one.png Some(\\\"image/png\\\")\"]"
    );
    let sent = running.exactly(1).await;
    let reply = body(&sent[0]);
    assert_eq!(reply["type"], 4, "{reply}");
    assert_eq!(reply["data"]["flags"], V2_EPHEMERAL, "{reply}");
    running.stop();
}

/// Rows 29-31: an invalid or malformed submission runs no handler: one
/// `warn` naming the reason and the generic ephemeral notice.
#[tokio::test(flavor = "multi_thread")]
async fn invalid_submissions_are_rejected() {
    let capture = capture();
    let running = Running::start().await;
    let rejected = |reason: &str| {
        capture.count(&[
            "noctis::dispatch",
            "component interaction rejected",
            &format!("reason=\"{reason}\""),
        ])
    };
    let evidence = modal_id(Evidence::blank(5));
    let cases = [
        // Row 29: an unknown radio value.
        submit(
            &survey_id(3),
            3,
            &survey_inputs("a", "zzz", &[], false),
            None,
            true,
        ),
        // Row 29: a required input is missing.
        submit(
            &survey_id(3),
            3,
            &survey_inputs("a", "h", &[], false)[..2],
            None,
            true,
        ),
        // Row 29: a File Upload id missing from `resolved`.
        submit(
            &evidence,
            3,
            &[
                json!({ "type": 5, "id": 2, "custom_id": "who", "values": ["3"] }),
                json!({ "type": 19, "id": 4, "custom_id": "files", "values": ["99"] }),
            ],
            Some(json!({ "users": { "3": user(3) } })),
            false,
        ),
        // Row 30: three values where `max_values = 2`.
        submit(
            &survey_id(3),
            3,
            &survey_inputs("a", "h", &["Rust", "art", "music"], false),
            None,
            true,
        ),
    ];
    for (i, case) in cases.into_iter().enumerate() {
        let before = rejected("submit_invalid");
        running.dispatch(case);
        let sent = running.exactly(1).await;
        assert_notice(&sent[0], GENERIC);
        assert_eq!(rejected("submit_invalid"), before + 1, "case {i}");
    }
    // Row 31: `data.components` is not an array.
    let before = rejected("malformed");
    let mut malformed = submit(&survey_id(3), 3, &[], None, true);
    malformed["data"]["components"] = json!({ "not": "an array" });
    running.dispatch(malformed);
    let sent = running.exactly(1).await;
    assert_notice(&sent[0], GENERIC);
    assert_eq!(rejected("malformed"), before + 1);
    assert!(
        running.seen.lines().is_empty(),
        "{:?}",
        running.seen.lines()
    );
    running.stop();
}

/// Rows 15, 17, 19 and 21: past the ack deadline the auto-defer is type 6
/// only for `update` with a source message, else type 5 with `EPHEMERAL`
/// exactly for `ephemeral`; the returned message then edits `@original`.
#[tokio::test(flavor = "multi_thread")]
async fn modal_auto_defer_follows_response() {
    let running = Running::start().await;
    let slow = |modal: noctis::ui::Modal, from_message: bool| {
        submit(
            &modal_id(modal),
            3,
            &[text("note", "n")],
            None,
            from_message,
        )
    };
    let me = Id::new(3);
    let cases = [
        // Row 15: update with a source message.
        (slow(SlowUpdate::blank(me, 300), true), json!({ "type": 6 })),
        // Row 17: update, slash-opened.
        (
            slow(SlowUpdate::blank(me, 300), false),
            json!({ "type": 5 }),
        ),
        // Row 19: ephemeral, with and without a source message.
        (
            slow(SlowEphemeral::blank(me, 300), true),
            json!({ "type": 5, "data": { "flags": 64 } }),
        ),
        (
            slow(SlowEphemeral::blank(me, 300), false),
            json!({ "type": 5, "data": { "flags": 64 } }),
        ),
        // Row 21: public on a component-opened modal: a new message.
        (slow(SlowPublic::blank(me, 300), true), json!({ "type": 5 })),
    ];
    for (i, (case, defer)) in cases.into_iter().enumerate() {
        running.dispatch(case);
        let sent = running.exactly(2).await;
        assert_callback(&sent[0]);
        assert_eq!(body(&sent[0]), defer, "case {i}");
        assert_edit(&sent[1]);
        assert_eq!(body(&sent[1])["components"][0]["content"], "later");
    }
    running.stop();
}

/// Rows 22-24: an `auto_defer = false` submit is never auto-deferred.
#[tokio::test(flavor = "multi_thread")]
async fn manual_modal_submit() {
    let capture = capture();
    let running = Running::start().await;
    let manual = |step: u8| {
        submit(
            &modal_id(ManualForm::blank(step)),
            3,
            &[text("note", "n")],
            None,
            true,
        )
    };
    // Row 22: one type-4 callback, nothing before it.
    running.dispatch(manual(0));
    let sent = running.exactly(1).await;
    assert_callback(&sent[0]);
    assert_eq!(sent[0].query.as_deref(), Some("with_response=true"));
    let reply = body(&sent[0]);
    assert_eq!(reply["type"], 4, "{reply}");
    assert_eq!(reply["data"]["flags"], V2_EPHEMERAL, "{reply}");

    // Row 24: it defers itself, then edits.
    running.dispatch(manual(2));
    let sent = running.exactly(2).await;
    assert_eq!(
        body(&sent[0]),
        json!({ "type": 5, "data": { "flags": 64 } })
    );
    assert_edit(&sent[1]);

    // Row 23: silent past 3 s: no request, one error naming the handler.
    let missed = [
        "noctis::dispatch",
        "did not answer within the 3 s ack deadline",
        "handler=\"modals::ManualForm\"",
        "module=\"forms\"",
    ];
    running.dispatch(manual(1));
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

/// Row 33: `/survey` opens the modal (type 9); its submit has no source
/// message, so the returned View is a public type-4 reply.
#[tokio::test(flavor = "multi_thread")]
async fn slash_opened_modal_flow() {
    let running = Running::start().await;
    running.dispatch(json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user(3),
        "data": { "id": "9", "name": "survey", "type": 1 },
    }));
    let sent = running.exactly(1).await;
    let modal = body(&sent[0]);
    assert_eq!(modal["type"], 9, "{modal}");
    assert_eq!(modal["data"]["custom_id"], survey_id(3));

    running.dispatch(submit(
        &survey_id(3),
        3,
        &survey_inputs("x", "l", &["Rust"], true),
        None,
        false,
    ));
    assert_eq!(
        running.wait_seen(1).await,
        ["survey 3 Some(\"x\") Low [Rust] true"]
    );
    let sent = running.exactly(1).await;
    assert_callback(&sent[0]);
    assert_eq!(sent[0].query.as_deref(), Some("with_response=true"));
    let reply = body(&sent[0]);
    assert_eq!(reply["type"], 4, "{reply}");
    assert_eq!(reply["data"]["flags"], V2, "{reply}");
    assert_eq!(reply["data"]["components"][0]["content"], "saved");
    running.stop();
}

/// Row 34: the `owner = user` Guard still runs on a submit, under every
/// declared `response`: another user gets the `noctis.guard.owner` notice.
#[tokio::test(flavor = "multi_thread")]
async fn guarded_modal_under_each_response() {
    let running = Running::start().await;
    let owner = Id::new(42);
    let cases = [
        submit(
            &survey_id(42),
            3,
            &survey_inputs("a", "h", &[], true),
            None,
            true,
        ),
        submit(
            &modal_id(SlowUpdate::blank(owner, 0)),
            3,
            &[text("note", "n")],
            None,
            false,
        ),
        submit(
            &modal_id(SlowEphemeral::blank(owner, 0)),
            3,
            &[text("note", "n")],
            None,
            true,
        ),
        submit(
            &modal_id(SlowPublic::blank(owner, 0)),
            3,
            &[text("note", "n")],
            None,
            true,
        ),
    ];
    for case in cases {
        running.dispatch(case);
        let sent = running.exactly(1).await;
        assert_notice(&sent[0], OWNER);
    }
    assert!(
        running.seen.lines().is_empty(),
        "{:?}",
        running.seen.lines()
    );
    running.stop();
}
