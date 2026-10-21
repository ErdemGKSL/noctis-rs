//! Components and modals end to end (Story 7.1 Matrix 1, 2, 16, 19, 26 and
//! 31): a `FakeGateway` `INTERACTION_CREATE` click or modal submit reaches
//! the handler of its Route through the component pipeline, which sends
//! no request to `FakeDiscordHttp` (responses arrive with Story 7.2).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use noctis::__private::core::__testing::responded;
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::{BuildViolation, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot, RecordedRequest};
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
        let gw = FakeGateway::start().await.unwrap();
        let seen = Arc::new(Seen::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .state(Arc::clone(&seen))
            .module(
                Module::new("shop")
                    .add::<Buy>()
                    .module(Module::new("shop/forms").add::<BioForm>()),
            )
            .module(Module::new("dash").uses::<Pick>().add::<Pick>());
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
    assert!(running.sent().is_empty(), "{:?}", running.sent());
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
