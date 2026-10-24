//! Story 13.4: declared rate limits end to end (FR-46, AD-23 stage 5,
//! INT-14). Every interaction travels `FakeGateway` → shard → pipeline and
//! every response is asserted on the `FakeDiscordHttp` wire: a refused run
//! is one type-4 ephemeral callback (no type-5 defer), its handler and
//! Middleware never run. Rows refer to the spec's runtime matrix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hyper::{Method, StatusCode};
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis::__private::core::__testing::responded;
use noctis::__private::i18n::Catalog;
use noctis::events::MessageCreate;
use noctis::ports::RefStore as _;
use noctis::ports::memory::MemoryRefStore;
use noctis::prelude::*;
use noctis::rules::Locale;
use noctis::ui::{Components, ResolvedMessage, TextDisplay, V2Message};
use noctis::{
    AppBuilder, CommandInput, CommandInvocation, ErrorReport, ReportKind, Runtime, Token,
};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, RecordedRequest,
};
use noctis_testkit::{TestApp, TestError};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The callback path of interaction 100 (token `tok`).
const CALLBACK: &str = "/interactions/100/tok/callback";
/// The channel the Dispatcher's message transport replies in.
const CHANNEL: &str = "/channels/5/messages";
/// The Ref of the one-shot records.
const REF_A: &str = "0123456789A";
const REF_B: &str = "0123456789B";

/// `noctis.rate_limited` in en-US, around `{retry}`.
const LIMITED_BEFORE: &str = "You're doing that too often. Try again ";
const LIMITED_AFTER: &str = ".";
/// The overlay's Turkish text, around `{retry}`.
const TR_BEFORE: &str = "Çok sık deniyorsun, ";
const TR_AFTER: &str = " tekrar dene.";

/// A catalog that overrides `noctis.rate_limited` in Turkish (R31).
static TR: Catalog = Catalog::__new(
    "rate-limits/locales",
    Locale::EnUs,
    &[
        (Locale::EnUs, &[] as &[(&str, &str)]),
        (
            Locale::Tr,
            &[(
                "noctis.rate_limited",
                "Çok sık deniyorsun, {retry} tekrar dene.",
            )],
        ),
    ],
    &[("noctis.rate_limited", &["retry"])],
);

// --- Logs and metrics ----------------------------------------------------------

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

/// The process-wide log capture and metrics snapshotter, installed once.
fn observe() -> &'static (Capture, Snapshotter) {
    static OBSERVE: OnceLock<(Capture, Snapshotter)> = OnceLock::new();
    OBSERVE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::WARN)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::set_global_recorder(recorder).unwrap();
        (capture, snapshotter)
    })
}

/// The `noctis_rate_limited_total{kind}` count so far.
fn limited_total(kind: &str) -> u64 {
    // A snapshot drains the counters (`swap(0)`), and the tests share one
    // recorder, so every read folds the drained deltas into running totals.
    static TOTALS: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());
    let mut totals = TOTALS.lock().unwrap();
    for (key, .., value) in observe().1.snapshot().into_vec() {
        let key = key.key();
        if key.name() != "noctis_rate_limited_total" {
            continue;
        }
        let DebugValue::Counter(n) = value else {
            continue;
        };
        let Some(label) = key.labels().find(|l| l.key() == "kind") else {
            continue;
        };
        match totals.iter_mut().find(|(k, _)| k == label.value()) {
            Some((_, total)) => *total += n,
            None => totals.push((label.value().to_owned(), n)),
        }
    }
    totals
        .iter()
        .find(|(k, _)| k == kind)
        .map_or(0, |(_, total)| *total)
}

// --- What ran --------------------------------------------------------------------

/// What the Middleware, handlers and overrides did, shared through `State`.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn count(&self, line: &str) -> usize {
        self.lines().iter().filter(|l| *l == line).count()
    }
}

macro_rules! saw {
    ($ctx:expr, $($fmt:tt)*) => {
        $ctx.state::<Arc<Seen>>().unwrap().0.lock().unwrap().push(format!($($fmt)*))
    };
}

fn view(text: impl Into<String>) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.into())))
}

/// The Module's before-Middleware: records every run it sees.
async fn mw(ctx: BeforeCtx) -> HandlerResult<Flow> {
    saw!(ctx, "mw");
    Ok(Flow::Continue)
}

// --- Commands ----------------------------------------------------------------------

/// R1, R2: three uses per 10 s per user.
#[derive(SlashCommand)]
#[command("profile", description = "Profile", rate_limit(user, 3, per = "10s"))]
struct Profile;

impl Profile {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "profile");
        Ok(responded())
    }
}

/// R10: two limits; the guild's is hit first. Clippy's `duplicated_attributes`
/// reads the two equal `per = ".."` as a repeated attribute.
#[derive(SlashCommand)]
#[allow(
    clippy::duplicated_attributes,
    reason = "two rate limits with the same window are not duplicates"
)]
#[command(
    "both",
    description = "Both",
    rate_limit(user, 3, per = "10s"),
    rate_limit(guild, 1, per = "10s")
)]
struct Both;

impl Both {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "both");
        Ok(responded())
    }
}

/// R12: only a success is charged.
#[derive(SlashCommand)]
#[command(
    "daily",
    description = "Daily",
    rate_limit(user, 1, per = "1m", manual)
)]
struct Daily {
    #[option(description = "Charge it")]
    charge: bool,
}

impl Daily {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "daily");
        if self.charge {
            ctx.rate_limits().hit(RateScope::User).await?;
        }
        Ok(responded())
    }
}

/// R13, R17: `op = 1` blocks for 30 s, `op = 2` resets.
#[derive(SlashCommand)]
#[command("ctl", description = "Ctl", rate_limit(user, 1, per = "10s"))]
struct Ctl {
    #[option(description = "Op")]
    op: i64,
}

impl Ctl {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "ctl {}", self.op);
        let limits = ctx.rate_limits();
        match self.op {
            1 => {
                limits
                    .block(RateScope::User, Duration::from_secs(30))
                    .await?;
            }
            2 => limits.reset(RateScope::User).await?,
            _ => {}
        }
        Ok(responded())
    }
}

/// R24: the struct's own override wins over the App's.
#[derive(SlashCommand)]
#[command("own", description = "Own", rate_limit(user, 1, per = "1m"))]
struct Own;

impl Own {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "own");
        Ok(responded())
    }

    #[on_rate_limited]
    async fn slow_down(ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
        saw!(ctx, "own override {}", hit.scope());
        Ok(view(format!(
            "own {} {}",
            ctx.path(),
            hit.relative_timestamp()
        )))
    }
}

/// R25: no struct override.
#[derive(SlashCommand)]
#[command("plain", description = "Plain", rate_limit(user, 1, per = "1m"))]
struct Plain;

impl Plain {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "plain");
        Ok(responded())
    }
}

/// R26: an override that errs.
#[derive(SlashCommand)]
#[command("failing", description = "Failing", rate_limit(user, 1, per = "1m"))]
struct Failing;

impl Failing {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }

    #[on_rate_limited]
    async fn bad(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        Err(HandlerError::new(io::Error::other("override failed")))
    }
}

/// R26: an override that panics.
#[derive(SlashCommand)]
#[command("boom", description = "Boom", rate_limit(user, 1, per = "1m"))]
struct Boom;

impl Boom {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }

    #[on_rate_limited]
    async fn boom(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        panic!("override boom")
    }
}

/// R27: an override past the ack deadline.
#[derive(SlashCommand)]
#[command("sluggish", description = "Sluggish", rate_limit(user, 1, per = "1m"))]
struct Sluggish;

impl Sluggish {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }

    #[on_rate_limited]
    async fn late(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        tokio::time::sleep(Duration::from_millis(400)).await;
        Ok(view("too late"))
    }
}

/// R32: autocomplete is never counted.
#[derive(SlashCommand)]
#[command("find", description = "Find", rate_limit(user, 1, per = "1m"))]
struct Find {
    #[option(description = "Query", autocomplete)]
    q: String,
}

impl Find {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "find {}", self.q);
        Ok(responded())
    }

    #[autocomplete(q)]
    async fn suggest(
        _ctx: AutocompleteCtx,
        typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        Ok([(typed.to_owned(), typed.to_owned())].into_iter().collect())
    }
}

/// R9: a message command keyed by its target message.
#[derive(MessageCommand)]
#[message_command("Quote", rate_limit(message, 1, per = "1m"))]
struct Quote;

impl Quote {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<Responded> {
        saw!(ctx, "quote {}", target.message.id);
        Ok(responded())
    }

    #[on_rate_limited]
    async fn slow_down(_ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(view(format!("quote override {}", hit.scope())))
    }
}

/// R29: the Dispatcher's Command.
#[derive(SlashCommand)]
#[command("dlim", description = "Dispatched", rate_limit(user, 1, per = "1m"))]
struct Dlim;

impl Dlim {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        saw!(ctx, "dlim");
        Ok(responded())
    }
}

/// Runs `!<path>` through the Dispatcher.
#[derive(EventHandler)]
struct Prefix;

impl Prefix {
    #[handler]
    async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
        if let Some(path) = msg.content.strip_prefix('!') {
            let invocation = CommandInvocation::new(
                CommandInput::new(path),
                msg.channel_id,
                msg.id,
                msg.author.id,
            );
            ctx.dispatcher().run_command(invocation).await?;
        }
        Ok(())
    }
}

// --- Components and modals -----------------------------------------------------------

/// R7: one click per message.
#[derive(Button)]
#[button("r.msg", rate_limit(message, 1, per = "1m"), defaults(style = Primary, label = "M"))]
struct PerMessage;

impl PerMessage {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw!(ctx, "msg");
        Ok(responded())
    }

    #[on_rate_limited]
    async fn slow_down(_ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(view(format!("msg override {}", hit.scope())))
    }
}

/// R12 on a component: only `ctx.rate_limits().hit(..)` counts.
#[derive(Button)]
#[button("r.charge", rate_limit(user, 1, per = "1m", manual), defaults(style = Primary, label = "C"))]
struct Charge;

impl Charge {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw!(ctx, "charge");
        ctx.rate_limits().hit(RateScope::User).await?;
        Ok(responded())
    }
}

/// R28 and AC2: a one-shot Ref limited per member.
#[derive(Button)]
#[button("r.once", by_ref, one_shot, rate_limit(member, 1, per = "1s"), defaults(style = Danger, label = "Ok"))]
struct Once {
    n: u8,
}

impl Once {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        saw!(ctx, "once {}", self.n);
        Ok(responded())
    }
}

/// R8: `message` on a slash-opened modal is unkeyed.
#[derive(Modal)]
#[modal("r.form", rate_limit(message, 1, per = "1m"), defaults(title = "Form"))]
struct Form {
    #[text_input("Bio")]
    bio: String,
}

impl Form {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        saw!(ctx, "form {}", self.bio);
        Ok(responded())
    }
}

/// AC2: a modal limited per member.
#[derive(Modal)]
#[modal(
    "r.member",
    rate_limit(member, 1, per = "1m"),
    defaults(title = "Member")
)]
struct MemberForm {
    #[text_input("Bio")]
    bio: String,
}

impl MemberForm {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        saw!(ctx, "member {}", self.bio);
        ctx.rate_limits()
            .hit(RateScope::Member)
            .await
            .map(drop)
            .ok();
        Ok(responded())
    }

    #[on_rate_limited]
    async fn slow_down(_ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(view(format!("member override {}", hit.scope())))
    }
}

// --- The harness ---------------------------------------------------------------------

/// Records every report as `<handler>:<kind>`.
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Vec<String>>>);

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => error.to_string(),
            ReportKind::Panic(message) => format!("panic {message}"),
            other => format!("{other:?}"),
        };
        let handler = report
            .handler
            .unwrap_or_default()
            .rsplit("::")
            .next()
            .unwrap_or_default()
            .to_owned();
        self.0.lock().unwrap().push(format!("{handler}:{kind}"));
    }
}

/// The App-level override: counts its calls.
async fn app_override(ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
    saw!(ctx, "app override");
    Ok(view(format!(
        "app {} {}",
        ctx.path(),
        hit.relative_timestamp()
    )))
}

/// A running App on one shard against both fakes.
struct Running {
    gw: FakeGateway,
    http: FakeDiscordHttp,
    seen: Arc<Seen>,
    reports: Reports,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(configure: impl FnOnce(&mut AppBuilder)) -> Self {
        observe();
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
            Method::POST,
            CHANNEL,
            FakeResponse::json(StatusCode::OK, &json!({ "id": "77" })),
        );
        let gw = FakeGateway::start().await.unwrap();
        let seen = Arc::new(Seen::default());
        let reports = Reports::default();
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .ack_deadline(Duration::from_millis(150))
            .state(Arc::clone(&seen))
            .module(
                Module::new("r")
                    .before(Middleware::before(mw))
                    .add::<Profile>()
                    .add::<Both>()
                    .add::<Daily>()
                    .add::<Ctl>()
                    .add::<Own>()
                    .add::<Plain>()
                    .add::<Failing>()
                    .add::<Boom>()
                    .add::<Sluggish>()
                    .add::<Find>()
                    .add::<Quote>()
                    .add::<Dlim>()
                    .add::<Prefix>()
                    .add::<PerMessage>()
                    .add::<Charge>()
                    .add::<Once>()
                    .add::<Form>()
                    .add::<MemberForm>(),
            );
        for origin in ErrorOrigin::ALL {
            builder.error_sink(origin, reports.clone());
        }
        configure(&mut builder);
        let app = builder.build().unwrap();
        let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
        let running = Self {
            gw,
            http,
            seen,
            reports,
            run,
        };
        let deadline = Instant::now() + WAIT;
        while running.gw.dispatch("TYPING_START", json!({})) == 0 {
            assert!(Instant::now() < deadline, "the shard has no session");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        running
    }

    fn dispatch(&self, kind: &str, d: Value) {
        assert_eq!(self.gw.dispatch(kind, d), 1);
    }

    /// Dispatches `d`, waits for `n` requests and a little longer, and
    /// returns exactly those (then forgets them).
    async fn exactly(&self, d: Value, n: usize) -> Vec<RecordedRequest> {
        self.exactly_event("INTERACTION_CREATE", d, n).await
    }

    async fn exactly_event(&self, kind: &str, d: Value, n: usize) -> Vec<RecordedRequest> {
        self.dispatch(kind, d);
        let deadline = Instant::now() + WAIT;
        while self.sent().len() < n {
            assert!(
                Instant::now() < deadline,
                "{n} requests missing: {:?}",
                self.sent()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let sent = self.sent();
        assert_eq!(sent.len(), n, "{sent:?}");
        self.http.clear_requests();
        sent
    }

    /// Runs `d`, which answers nothing on the wire, until the handlers saw
    /// `line` once more.
    async fn ran(&self, d: Value, line: &str) {
        let before = self.seen.count(line);
        self.dispatch("INTERACTION_CREATE", d);
        let deadline = Instant::now() + WAIT;
        while self.seen.count(line) == before {
            assert!(
                Instant::now() < deadline,
                "`{line}` did not run: {:?}",
                self.seen.lines()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Past the ack deadline: the handler answered nothing.
        tokio::time::sleep(Duration::from_millis(250)).await;
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

fn user(id: u64) -> Value {
    json!({ "id": id.to_string(), "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

/// An interaction of `kind` with `data` by `by`, in guild `guild` (as a
/// member) or a DM, in the User Locale `locale`.
fn interaction(kind: u8, data: Value, by: u64, guild: Option<u64>, locale: &str) -> Value {
    let mut d = json!({
        "id": "100",
        "application_id": "7",
        "type": kind,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "channel": { "id": "55", "type": 0 },
        "locale": locale,
    });
    d["data"] = data;
    match guild {
        Some(g) => {
            d["guild_id"] = json!(g.to_string());
            d["member"] = json!({ "user": user(by), "roles": [],
                "joined_at": "2024-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false,
                "flags": 0, "permissions": "0" });
        }
        None => d["user"] = user(by),
    }
    d
}

/// `/name` with `options` by `by` in a DM, en-US.
fn slash(name: &str, options: Value, by: u64) -> Value {
    let mut data = json!({ "id": "9", "name": name, "type": 1 });
    data["options"] = options;
    interaction(2, data, by, None, "en-US")
}

fn message(id: u64) -> Value {
    json!({
        "id": id.to_string(), "channel_id": "55", "flags": 32768, "content": "", "tts": false,
        "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
        "timestamp": "2026-10-20T12:00:00.000000+00:00", "edited_timestamp": null,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "components": [],
    })
}

/// A click on `custom_id` by `by` in guild 9 on message `on`.
fn click(custom_id: &str, by: u64, on: u64) -> Value {
    let mut d = interaction(
        3,
        json!({ "custom_id": custom_id, "component_type": 2 }),
        by,
        Some(9),
        "en-US",
    );
    d["message"] = message(on);
    d
}

/// A slash-opened submit of `custom_id` by `by` in guild 9.
fn submit(custom_id: &str, by: u64, bio: &str) -> Value {
    interaction(
        5,
        json!({ "custom_id": custom_id, "components": [
            { "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "bio", "value": bio } }
        ] }),
        by,
        Some(9),
        "en-US",
    )
}

/// A component-opened submit of `custom_id` by `by` in guild 9 on message
/// `on`.
fn submit_on(custom_id: &str, by: u64, on: u64, bio: &str) -> Value {
    let mut d = submit(custom_id, by, bio);
    d["message"] = message(on);
    d
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The `N` of the one `<t:N:R>` in `text` between `before` and `after`.
fn retry_of(text: &str, before: &str, after: &str) -> u64 {
    let inner = text
        .strip_prefix(before)
        .and_then(|t| t.strip_suffix(after))
        .unwrap_or_else(|| panic!("not the notice: {text:?}"));
    inner
        .strip_prefix("<t:")
        .and_then(|t| t.strip_suffix(":R>"))
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no relative timestamp: {inner:?}"))
}

/// Asserts `request` is the one ephemeral type-4 callback of a single
/// Text Display, without a bot token, and returns its text.
fn ephemeral_text(request: &RecordedRequest) -> String {
    assert_eq!(
        (&request.method, request.route_path()),
        (&Method::POST, Some(CALLBACK)),
        "{request:?}"
    );
    assert!(request.headers.get("authorization").is_none());
    let b = request.json().unwrap();
    assert_eq!(b["type"], 4, "{b}");
    assert_eq!(b["data"]["flags"], 32832, "{b}");
    let components = b["data"]["components"].as_array().unwrap();
    assert_eq!(components.len(), 1, "{b}");
    components[0]["content"].as_str().unwrap().to_owned()
}

/// Asserts `request` is the default en-US notice retried about
/// `retry_secs` from now.
fn assert_limited(request: &RecordedRequest, retry_secs: u64) {
    let text = ephemeral_text(request);
    let at = retry_of(&text, LIMITED_BEFORE, LIMITED_AFTER);
    let expected = now_secs() + retry_secs;
    assert!(at.abs_diff(expected) <= 2, "{text} vs {expected}");
}

// --- Tests -------------------------------------------------------------------------------

/// R1, R2 and AC1: the fourth use within 10 s is one ephemeral type-4
/// notice before any auto-defer; no handler, no Middleware; another user
/// still runs.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_fourth_use_is_refused() {
    let running = Running::start(|_| {}).await;
    let before = limited_total("command");
    for _ in 0..3 {
        running.ran(slash("profile", json!([]), 3), "profile").await;
    }
    assert_eq!(running.seen.count("mw"), 3);
    let sent = running.exactly(slash("profile", json!([]), 3), 1).await;
    assert_limited(&sent[0], 10);
    assert_eq!(running.seen.count("profile"), 3);
    assert_eq!(running.seen.count("mw"), 3);
    assert!(limited_total("command") > before);
    running.ran(slash("profile", json!([]), 4), "profile").await;
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.stop();
}

/// R10: the guild limit refuses the second use; the user limit counted it.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_first_hit_wins() {
    let running = Running::start(|_| {}).await;
    let both = |by| {
        interaction(
            2,
            json!({ "id": "9", "name": "both", "type": 1 }),
            by,
            Some(9),
            "en-US",
        )
    };
    running.ran(both(3), "both").await;
    let sent = running.exactly(both(4), 1).await;
    assert_limited(&sent[0], 10);
    running.stop();
}

/// R11, R12: a manual limit counts only `hit`.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_manual_hit() {
    let running = Running::start(|_| {}).await;
    let daily = |charge| {
        slash(
            "daily",
            json!([{ "type": 5, "name": "charge", "value": charge }]),
            3,
        )
    };
    running.ran(daily(false), "daily").await;
    running.ran(daily(false), "daily").await;
    running.ran(daily(true), "daily").await;
    let sent = running.exactly(daily(false), 1).await;
    assert_limited(&sent[0], 60);
    running.stop();
}

/// R13, R17: `block` refuses for 30 s; `reset` frees the next use.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_block_and_reset() {
    let running = Running::start(|_| {}).await;
    let ctl = |op: i64, by| slash("ctl", json!([{ "type": 4, "name": "op", "value": op }]), by);
    running.ran(ctl(1, 3), "ctl 1").await;
    let sent = running.exactly(ctl(0, 3), 1).await;
    assert_limited(&sent[0], 30);
    // Another user: a reset makes the next use count from zero.
    running.ran(ctl(2, 4), "ctl 2").await;
    running.ran(ctl(0, 4), "ctl 0").await;
    let sent = running.exactly(ctl(0, 4), 1).await;
    assert_limited(&sent[0], 10);
    running.stop();
}

/// R24, R25: the struct's override wins; else the App's.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_override_precedence() {
    let running = Running::start(|b| {
        b.rate_limited_handler(app_override);
    })
    .await;
    running.ran(slash("own", json!([]), 3), "own").await;
    let sent = running.exactly(slash("own", json!([]), 3), 1).await;
    let text = ephemeral_text(&sent[0]);
    let at = retry_of(&text, "own own ", "");
    assert!(at.abs_diff(now_secs() + 60) <= 2, "{text}");
    assert_eq!(running.seen.count("own override user"), 1);
    assert_eq!(running.seen.count("app override"), 0);

    running.ran(slash("plain", json!([]), 3), "plain").await;
    let sent = running.exactly(slash("plain", json!([]), 3), 1).await;
    let text = ephemeral_text(&sent[0]);
    assert!(text.starts_with("app plain <t:"), "{text}");
    assert_eq!(running.seen.count("app override"), 1);
    assert!(running.reports.0.lock().unwrap().is_empty());
    running.stop();
}

/// R26, R27: a failing, panicking or late override sends the default
/// notice; only the first two are reported.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_override_failures() {
    let (capture, _) = observe();
    let running = Running::start(|_| {}).await;
    for name in ["failing", "boom"] {
        running.dispatch("INTERACTION_CREATE", slash(name, json!([]), 3));
        tokio::time::sleep(Duration::from_millis(300)).await;
        running.http.clear_requests();
        let sent = running.exactly(slash(name, json!([]), 3), 1).await;
        assert_limited(&sent[0], 60);
    }
    let reports = running.reports.0.lock().unwrap().clone();
    assert_eq!(
        reports,
        ["Failing:override failed", "Boom:panic override boom"]
    );
    let before = capture.count(&["noctis::rate_limit", "missed its deadline"]);
    running.dispatch("INTERACTION_CREATE", slash("sluggish", json!([]), 3));
    tokio::time::sleep(Duration::from_millis(300)).await;
    running.http.clear_requests();
    let sent = running.exactly(slash("sluggish", json!([]), 3), 1).await;
    assert_limited(&sent[0], 60);
    assert_eq!(
        capture.count(&["noctis::rate_limit", "missed its deadline"]),
        before + 1
    );
    assert_eq!(running.reports.0.lock().unwrap().len(), 2);
    running.stop();
}

/// R31: the User Locale's overlay text, `{retry}` replaced.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_localized_notice() {
    let running = Running::start(|b| {
        b.locales(TR);
    })
    .await;
    let tr = || {
        interaction(
            2,
            json!({ "id": "9", "name": "plain", "type": 1 }),
            3,
            None,
            "tr",
        )
    };
    running.ran(tr(), "plain").await;
    let sent = running.exactly(tr(), 1).await;
    let text = ephemeral_text(&sent[0]);
    let at = retry_of(&text, TR_BEFORE, TR_AFTER);
    assert!(at.abs_diff(now_secs() + 60) <= 2, "{text}");
    running.stop();
}

/// R32: autocomplete is never counted.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_autocomplete_is_not_counted() {
    let running = Running::start(|_| {}).await;
    let focused = json!([{ "type": 3, "name": "q", "value": "a", "focused": true }]);
    for _ in 0..3 {
        let d = interaction(
            4,
            json!({ "id": "9", "name": "find", "type": 1, "options": focused }),
            3,
            None,
            "en-US",
        );
        let sent = running.exactly(d, 1).await;
        assert_eq!(sent[0].json().unwrap()["type"], 8);
    }
    let q = json!([{ "type": 3, "name": "q", "value": "x" }]);
    running.ran(slash("find", q.clone(), 3), "find x").await;
    let sent = running.exactly(slash("find", q, 3), 1).await;
    assert_limited(&sent[0], 60);
    running.stop();
}

/// R9: a message command is keyed by its target.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_message_command_target() {
    let running = Running::start(|_| {}).await;
    let quote = |target: u64| {
        let mut messages = serde_json::Map::new();
        messages.insert(target.to_string(), message(target));
        interaction(
            2,
            json!({ "id": "9", "name": "Quote", "type": 3, "target_id": target.to_string(),
                "resolved": { "messages": messages } }),
            3,
            Some(9),
            "en-US",
        )
    };
    running.ran(quote(500), "quote 500").await;
    let sent = running.exactly(quote(500), 1).await;
    assert_eq!(ephemeral_text(&sent[0]), "quote override message");
    running.ran(quote(501), "quote 501").await;
    running.stop();
}

/// R7: one click per message.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_component_message_scope() {
    let running = Running::start(|_| {}).await;
    let before = limited_total("component");
    running.ran(click("0r.msg:", 3, 77), "msg").await;
    let sent = running.exactly(click("0r.msg:", 4, 77), 1).await;
    assert_eq!(ephemeral_text(&sent[0]), "msg override message");
    running.ran(click("0r.msg:", 3, 78), "msg").await;
    assert_eq!(running.seen.count("msg"), 2);
    assert!(limited_total("component") > before);
    running.stop();
}

/// R12 on a component: the handler's own `hit` charges a `manual` limit.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_component_manual_hit() {
    let running = Running::start(|_| {}).await;
    running.ran(click("0r.charge:", 3, 77), "charge").await;
    let sent = running.exactly(click("0r.charge:", 3, 78), 1).await;
    assert_limited(&sent[0], 60);
    assert_eq!(running.seen.count("charge"), 1);
    running.stop();
}

/// R28 and AC2: a refused click puts the taken `one_shot` record back; the
/// next allowed click resolves it.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_one_shot_ref_survives_a_hit() {
    let refs = Arc::new(MemoryRefStore::new());
    let store = Arc::clone(&refs);
    let running = Running::start(move |b| {
        b.ref_store(store);
    })
    .await;
    for (r, n) in [(REF_A, "1"), (REF_B, "2")] {
        let record = bytes::Bytes::from(format!("0r.once:{n}"));
        let key = format!("default:ref:{r}");
        assert!(
            refs.put_if_absent(&key, record, Some(Duration::from_mins(1)))
                .await
                .unwrap()
        );
    }
    running
        .ran(click(&format!("0r.once:~{REF_A}"), 3, 77), "once 1")
        .await;
    let b = format!("default:ref:{REF_B}");
    let sent = running
        .exactly(click(&format!("0r.once:~{REF_B}"), 3, 77), 1)
        .await;
    assert_limited(&sent[0], 1);
    assert!(refs.get(&b).await.unwrap().is_some());
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    running
        .ran(click(&format!("0r.once:~{REF_B}"), 3, 77), "once 2")
        .await;
    assert_eq!(refs.get(&b).await.unwrap(), None);
    running.stop();
}

/// R8 and AC2: `message` on a slash-opened submit is unkeyed; `member` on a
/// modal refuses with the same notice.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_modal_scopes() {
    let running = Running::start(|_| {}).await;
    running.ran(submit("0r.form:", 3, "a"), "form a").await;
    running.ran(submit("0r.form:", 3, "b"), "form b").await;
    running.ran(submit("0r.member:", 3, "a"), "member a").await;
    let sent = running.exactly(submit("0r.member:", 3, "b"), 1).await;
    assert_eq!(ephemeral_text(&sent[0]), "member override member");
    assert_eq!(running.seen.count("member b"), 0);
    // A component-opened submit keys `message` by its source message.
    running
        .ran(submit_on("0r.form:", 3, 80, "c"), "form c")
        .await;
    let sent = running.exactly(submit_on("0r.form:", 4, 80, "d"), 1).await;
    assert_limited(&sent[0], 60);
    assert_eq!(running.seen.count("form d"), 0);
    running
        .ran(submit_on("0r.form:", 4, 81, "e"), "form e")
        .await;
    running.stop();
}

/// R29: a refused Dispatcher run is a public reply to the invoking
/// message; the handler does not run.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_dispatcher_run() {
    let running = Running::start(|_| {}).await;
    let msg = |content: &str| {
        json!({
            "id": "6", "channel_id": "5", "author": user(3), "content": content,
            "timestamp": "2024-01-01T00:00:00.000000+00:00", "edited_timestamp": null,
            "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
            "attachments": [], "embeds": [], "pinned": false, "type": 0
        })
    };
    let before = running.seen.count("dlim");
    running.dispatch("MESSAGE_CREATE", msg("!dlim"));
    let deadline = Instant::now() + WAIT;
    while running.seen.count("dlim") == before {
        assert!(Instant::now() < deadline, "dlim did not run");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    running.http.clear_requests();
    let sent = running
        .exactly_event("MESSAGE_CREATE", msg("!dlim"), 1)
        .await;
    assert_eq!(sent[0].route_path(), Some(CHANNEL));
    let reply = sent[0].json().unwrap();
    assert_eq!(reply["flags"], 32768, "{reply}");
    let text = reply["components"][0]["content"].as_str().unwrap();
    let at = retry_of(text, LIMITED_BEFORE, LIMITED_AFTER);
    assert!(at.abs_diff(now_secs() + 60) <= 2, "{text}");
    assert_eq!(reply["message_reference"]["message_id"], "6", "{reply}");
    assert_eq!(running.seen.count("dlim"), 1);
    running.stop();
}

/// `TestApp`: `Response::is_rate_limited` reads the default notice.
#[tokio::test(flavor = "multi_thread")]
async fn int_14_test_app_reads_the_notice() -> Result<(), TestError> {
    let seen = Arc::new(Seen::default());
    let mut builder = App::builder();
    builder
        .state(Arc::clone(&seen))
        .module(Module::new("r").add::<Plain>());
    let app = TestApp::build(&mut builder).await?;
    let first = app.command("plain").run().await?;
    assert!(!first.is_rate_limited());
    let second = app.command("plain").run().await?;
    assert!(second.is_rate_limited(), "{second:?}");
    assert!(second.is_ephemeral());
    assert_eq!(second.callback_type(), Some(4));
    assert!(!second.is_error());
    app.stop().await
}
