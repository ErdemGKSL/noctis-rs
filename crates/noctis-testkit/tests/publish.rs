//! Publish end to end (Story 6.9, Matrix 6, 7, 10, 15-17, 19-21, 23, 25,
//! 28-30 and 32): the App publishes against a `FakeDiscordHttp` whose
//! command routes keep state, so a `PUT` is visible to the next `GET`. Its
//! own test binary, because it installs a global capturing tracing
//! subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use noctis::cli::CliError;
use noctis::prelude::*;
use noctis::publish::{PublishError, PublishScope};
use noctis::twilight::model::id::Id;
use noctis::ui::Components;
use noctis::{AppBuilder, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method, RecordedRequest, StatusCode,
};
use serde_json::{Value, json};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const APP: &str = "/applications/@me";
const GLOBAL: &str = "/applications/7/commands";

fn guild_path(guild: u64) -> String {
    format!("/applications/7/guilds/{guild}/commands")
}

#[derive(SlashCommand)]
#[command("ping", description = "Pong")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        unreachable!("never invoked")
    }
}

#[derive(SlashCommand)]
#[command("cfg", description = "Configure")]
struct Cfg;

impl Cfg {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        unreachable!("never invoked")
    }
}

#[derive(SlashCommand)]
#[command("ping", description = "Pong", contexts = [Guild])]
struct PingInGuilds;

impl PingInGuilds {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        unreachable!("never invoked")
    }
}

#[derive(SlashCommand)]
#[command("admin", description = "Admin tools", guilds = [42])]
struct Admin;

impl Admin {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        unreachable!("never invoked")
    }
}

#[derive(SlashCommand)]
#[command("ping", description = "Guild pong", guilds = [42])]
struct GuildPing;

impl GuildPing {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        unreachable!("never invoked")
    }
}

#[derive(SlashCommand)]
#[command("me", description = "Mine", integration_types = [UserInstall])]
struct Me;

impl Me {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        unreachable!("never invoked")
    }
}

// ---- capture ----

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn lines_with(&self, needles: &[&str]) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .filter(|line| needles.iter().all(|n| line.contains(n)))
            .map(str::to_owned)
            .collect()
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

/// The process-wide log capture (spans included), installed once.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_span_events(FmtSpan::NEW)
            .with_writer(capture.clone())
            .init();
        capture
    })
}

// ---- the fake ----

/// The remote command lists, by route path.
#[derive(Clone, Default)]
struct Remote(Arc<Mutex<HashMap<String, Value>>>);

impl Remote {
    fn get(&self, path: &str) -> Value {
        self.0
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_else(|| json!([]))
    }
}

/// A fake answering the application object and keeping the command lists
/// of the global target and of guilds `guilds`.
fn serve(http: &FakeDiscordHttp, application: &Value, remote: &Remote, guilds: &[u64]) {
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(StatusCode::OK, application),
    );
    let paths = std::iter::once(GLOBAL.to_owned()).chain(guilds.iter().map(|&g| guild_path(g)));
    for path in paths {
        let read = remote.clone();
        let key = path.clone();
        http.route_with(Method::GET, path.clone(), move |_| {
            FakeResponse::json(StatusCode::OK, &read.get(&key))
        });
        let write = remote.clone();
        let key = path.clone();
        http.route_with(Method::PUT, path, move |request: &RecordedRequest| {
            let body = request.json().unwrap_or(Value::Null);
            write.0.lock().unwrap().insert(key.clone(), body.clone());
            FakeResponse::json(StatusCode::OK, &body)
        });
    }
}

fn application() -> Value {
    json!({"id": "7", "integration_types_config": {"0": {}}})
}

/// A fake with [`application`] and empty remote lists for guilds 42 and 9.
async fn fake() -> (FakeDiscordHttp, Remote) {
    capture();
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let remote = Remote::default();
    serve(&http, &application(), &remote, &[42, 9]);
    (http, remote)
}

fn app_with(
    http: &FakeDiscordHttp,
    module: Module,
    configure: impl FnOnce(&mut AppBuilder),
) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .module(module);
    configure(&mut builder);
    builder.build().unwrap()
}

fn app(http: &FakeDiscordHttp, module: Module) -> App {
    app_with(http, module, |_| {})
}

/// `METHOD /path` of every recorded API request, in order.
fn requests(http: &FakeDiscordHttp) -> Vec<String> {
    http.requests()
        .iter()
        .filter_map(|r| r.route_path().map(|p| format!("{} {p}", r.method)))
        .collect()
}

fn put_bodies(http: &FakeDiscordHttp, path: &str) -> Vec<Value> {
    http.requests()
        .iter()
        .filter(|r| r.method == Method::PUT && r.route_path() == Some(path))
        .map(|r| r.json().unwrap())
        .collect()
}

fn names(body: &Value) -> Vec<&str> {
    body.as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect()
}

/// Matrix 6, then a second Publish against the stored state writes
/// nothing.
#[tokio::test(flavor = "multi_thread")]
async fn publish_sends_guild_commands_only_to_their_guilds() {
    let (http, _) = fake().await;
    let app = app(&http, Module::new("m").add::<Ping>().add::<Admin>());
    let report = app.publish(PublishScope::Global).await.unwrap();
    assert_eq!(
        requests(&http),
        [
            format!("GET {APP}"),
            format!("GET {GLOBAL}"),
            format!("PUT {GLOBAL}"),
            format!("GET {}", guild_path(42)),
            format!("PUT {}", guild_path(42)),
        ]
    );
    assert_eq!(names(&put_bodies(&http, GLOBAL)[0]), ["ping"]);
    assert_eq!(names(&put_bodies(&http, &guild_path(42))[0]), ["admin"]);
    assert_eq!(report.creates(), 2);
    assert!(report.wrote());

    http.clear_requests();
    let again = app.publish(PublishScope::Global).await.unwrap();
    assert!(!again.wrote());
    assert_eq!(again.creates(), 0);
    assert!(requests(&http).iter().all(|r| r.starts_with("GET ")));
}

/// Matrix 7: Discord's server fields and defaults never cause a write.
#[tokio::test(flavor = "multi_thread")]
async fn publish_skips_equal_remote() {
    let (http, remote) = fake().await;
    remote.0.lock().unwrap().insert(
        GLOBAL.to_owned(),
        json!([{
            "id": "100", "application_id": "7", "version": "1", "type": 1,
            "name": "ping", "description": "Pong", "options": [],
            "dm_permission": null, "nsfw": false, "contexts": null,
            "integration_types": [0], "default_member_permissions": null,
            "description_localizations": null, "name_localizations": null,
        }]),
    );
    let app = app(&http, Module::new("m").add::<Ping>());
    let report = app.publish(PublishScope::Global).await.unwrap();
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {GLOBAL}")]
    );
    let target = &report.targets()[0];
    assert!(target.diff().is_empty() && !target.changed() && !target.wrote());
    assert_eq!(report.creates(), 0);
    assert!(
        !capture()
            .lines_with(&["noctis::publish", "commands unchanged; nothing written"])
            .is_empty()
    );
}

/// Matrix 10 and 13: one PUT with the local array, the remote Entry Point
/// kept at its end, and the diff logged at info.
#[tokio::test(flavor = "multi_thread")]
async fn publish_writes_the_diff_once() {
    let (http, remote) = fake().await;
    let launch =
        json!({"id": "9", "type": 4, "name": "launch", "description": "Play", "handler": 2});
    remote.0.lock().unwrap().insert(
        GLOBAL.to_owned(),
        json!([
            {"type": 1, "name": "old", "description": "Old", "options": []},
            {"type": 1, "name": "cfg", "description": "Before", "options": []},
            launch,
        ]),
    );
    let app = app(&http, Module::new("m").add::<Ping>().add::<Cfg>());
    let report = app.publish(PublishScope::Global).await.unwrap();
    let target = &report.targets()[0];
    assert_eq!(
        target.diff(),
        "+ create chat_input `ping`\n~ update chat_input `cfg` (description)\n- delete chat_input `old`"
    );
    assert_eq!(report.creates(), 1);
    let puts = put_bodies(&http, GLOBAL);
    assert_eq!(puts.len(), 1);
    assert_eq!(names(&puts[0]), ["ping", "cfg", "launch"]);
    assert_eq!(puts[0][2], launch);
    let logged = capture().lines_with(&["noctis::publish", "writing commands", "creates=1"]);
    assert!(
        logged
            .iter()
            .any(|l| l.contains("+ create chat_input `ping`")),
        "{logged:?}"
    );
    assert!(capture().lines_with(&["fake-token"]).is_empty());
}

/// Matrix 15 and REG-05: a dry run reads, reports and writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn reg_05_dry_run_reports_creates() {
    let (http, _) = fake().await;
    let app = app(&http, Module::new("m").add::<Ping>().add::<Cfg>());
    let report = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    assert!(report.is_dry_run() && !report.wrote());
    assert_eq!(report.creates(), 2);
    let text = report.to_string();
    assert!(text.contains("\"name\": \"ping\""), "{text}");
    assert!(text.contains("+ create chat_input `cfg`"), "{text}");
    assert!(text.contains("dry run; nothing written"), "{text}");
    assert!(
        text.ends_with("creates: 2 (REG-05: Discord allows 200 command creates per day per guild)")
    );

    http.clear_requests();
    let app = self::app(&http, Module::new("m").add::<Ping>());
    app.run_with_args(["--publish", "--dry-run"]).await.unwrap();
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {GLOBAL}")]
    );
}

/// Matrix 16: the dev mirror is one guild target without install fields.
#[tokio::test(flavor = "multi_thread")]
async fn publish_guild_mirrors_global_commands() {
    let (http, _) = fake().await;
    let app = app(&http, Module::new("m").add::<PingInGuilds>().add::<Admin>());
    app.run_with_args(["--publish-guild", "42"]).await.unwrap();
    assert_eq!(
        requests(&http),
        [
            format!("GET {APP}"),
            format!("GET {}", guild_path(42)),
            format!("PUT {}", guild_path(42)),
        ]
    );
    let body = &put_bodies(&http, &guild_path(42))[0];
    assert_eq!(
        *body,
        json!([
            {"type": 1, "name": "ping", "description": "Pong", "options": []},
            {"type": 1, "name": "admin", "description": "Admin tools", "options": []},
        ])
    );
}

/// Matrix 17.
#[tokio::test(flavor = "multi_thread")]
async fn publish_guild_reports_name_clash() {
    let (http, _) = fake().await;
    let app = app(&http, Module::new("m").add::<Ping>().add::<GuildPing>());
    let error = app
        .publish(PublishScope::Guild(Id::new(42)))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        PublishError::NameClash { kind: "chat_input", ref name, .. } if name == "ping"
    ));
    assert!(error.to_string().starts_with("[REG-03]"));
    assert!(put_bodies(&http, &guild_path(42)).is_empty());
}

/// Matrix 19: META-04 fails before any PUT, in a dry run too.
#[tokio::test(flavor = "multi_thread")]
async fn meta_04_blocks_publish_before_put() {
    let (http, _) = fake().await;
    let app = app(&http, Module::new("m").add::<Me>());
    for dry_run in [false, true] {
        let error = app
            .publish(PublishScope::Global)
            .dry_run(dry_run)
            .await
            .unwrap_err();
        let text = error.to_string();
        assert!(
            text.starts_with("[META-04]") && text.contains("`me`") && text.contains("user_install"),
            "{text}"
        );
    }
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {APP}")]
    );

    // No `integration_types_config` at all: only guild install.
    let http = FakeDiscordHttp::start().await.unwrap();
    serve(&http, &json!({"id": "7"}), &Remote::default(), &[]);
    let app = self::app(&http, Module::new("m").add::<Me>());
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::IntegrationTypeNotEnabled { .. })
    ));

    // Both enabled: published.
    let http = FakeDiscordHttp::start().await.unwrap();
    let both = json!({"id": "7", "integration_types_config": {"0": {}, "1": {}}});
    serve(&http, &both, &Remote::default(), &[]);
    let app = self::app(&http, Module::new("m").add::<Me>());
    assert!(app.publish(PublishScope::Global).await.unwrap().wrote());
}

/// Matrix 20.
#[tokio::test(flavor = "multi_thread")]
async fn publish_clear_deletes() {
    let (http, remote) = fake().await;
    remote.0.lock().unwrap().insert(
        GLOBAL.to_owned(),
        json!([
            {"type": 1, "name": "a", "description": "A", "options": []},
            {"type": 3, "name": "B"},
        ]),
    );
    let app = app(&http, Module::new("m").add::<Ping>());
    let report = app.publish(PublishScope::Global).clear(true).await.unwrap();
    assert_eq!(
        report.targets()[0].diff(),
        "- delete chat_input `a`\n- delete message `B`"
    );
    assert_eq!(put_bodies(&http, GLOBAL), [json!([])]);

    http.clear_requests();
    app.run_with_args(["--publish-clear"]).await.unwrap();
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {GLOBAL}")]
    );

    http.clear_requests();
    let app = self::app(&http, Module::new("m").add::<Ping>());
    remote
        .0
        .lock()
        .unwrap()
        .insert(guild_path(42), json!([{"name": "x", "description": "X"}]));
    app.run_with_args(["--publish-clear", "--guild", "42"])
        .await
        .unwrap();
    assert_eq!(put_bodies(&http, &guild_path(42)), [json!([])]);
}

/// Matrix 21: HTTP errors are typed and stop the remaining targets.
#[tokio::test(flavor = "multi_thread")]
async fn publish_http_errors() {
    let (http, _) = fake().await;
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(
            StatusCode::UNAUTHORIZED,
            &json!({"message": "401", "code": 0}),
        ),
    );
    let app = app(&http, Module::new("m").add::<Ping>());
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::Rejected { status: 401, ref route, .. }) if route == "GET /applications/@me"
    ));

    let (http, _) = fake().await;
    http.route(
        Method::GET,
        GLOBAL,
        FakeResponse::json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({"message": "boom"}),
        ),
    );
    let app = self::app(&http, Module::new("m").add::<Ping>());
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::Rejected {
            status: 500,
            code: None,
            ..
        })
    ));

    let (http, _) = fake().await;
    http.route(
        Method::PUT,
        GLOBAL,
        FakeResponse::json(
            StatusCode::BAD_REQUEST,
            &json!({"message": "Invalid Form Body", "code": 50035}),
        ),
    );
    let app = self::app(&http, Module::new("m").add::<Ping>().add::<Admin>());
    let error = app.run_with_args(["--publish"]).await.unwrap_err();
    assert!(matches!(
        error,
        noctis::Error::Publish(PublishError::Rejected {
            status: 400,
            code: Some(50035),
            ..
        })
    ));
    // The guild target after the failed global PUT is skipped.
    assert!(!requests(&http).iter().any(|r| r.contains("/guilds/")));

    let (http, _) = fake().await;
    let app = self::app(&http, Module::new("m").add::<Ping>());
    drop(http);
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::Transport { .. })
    ));
}

/// Matrix 22.
#[tokio::test(flavor = "multi_thread")]
async fn publish_rejects_bad_bodies() {
    let (http, _) = fake().await;
    http.route(
        Method::GET,
        GLOBAL,
        FakeResponse::json(StatusCode::OK, &json!({})),
    );
    let app = app(&http, Module::new("m").add::<Ping>());
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::InvalidResponse { .. })
    ));

    let (http, _) = fake().await;
    http.route(
        Method::GET,
        GLOBAL,
        FakeResponse::new(StatusCode::OK, "nope"),
    );
    let app = self::app(&http, Module::new("m").add::<Ping>());
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::InvalidResponse { .. })
    ));

    let (http, _) = fake().await;
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(StatusCode::OK, &json!({"name": "bot"})),
    );
    let app = self::app(&http, Module::new("m").add::<Ping>());
    assert!(matches!(
        app.publish(PublishScope::Global).await,
        Err(PublishError::InvalidResponse { ref route, .. }) if route == "GET /applications/@me"
    ));
}

/// Matrix 23 and 25: no token, and a bad command line, send nothing.
#[tokio::test(flavor = "multi_thread")]
async fn cli_errors_send_nothing() {
    let (http, _) = fake().await;
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .http_proxy(http.url())
        .module(Module::new("m").add::<Ping>());
    let app = builder.build().unwrap();
    assert!(matches!(
        app.run_with_args(["--publish"]).await,
        Err(noctis::Error::Publish(PublishError::MissingToken))
    ));

    for (args, expected) in [
        (
            vec!["--bogus"],
            CliError::UnknownFlag {
                flag: "--bogus".into(),
            },
        ),
        (
            vec!["--publish", "--publish-clear"],
            CliError::ConflictingModes {
                first: "--publish",
                second: "--publish-clear",
            },
        ),
        (vec!["--dry-run"], CliError::DryRunWithoutPublish),
        (
            vec!["--publish-guild", "0"],
            CliError::InvalidGuildId {
                flag: "--publish-guild",
                value: "0".into(),
            },
        ),
    ] {
        let app = self::app(&http, Module::new("m").add::<Ping>());
        match app.run_with_args(args).await {
            Err(noctis::Error::Cli(error)) => assert_eq!(error, expected),
            other => panic!("{other:?}"),
        }
    }
    assert!(requests(&http).is_empty());
}

/// Matrix 28.
#[tokio::test(flavor = "multi_thread")]
async fn publish_guild_uses_the_dev_guild() {
    let (http, _) = fake().await;
    let app = app(&http, Module::new("m").add::<Ping>());
    assert!(matches!(
        app.run_with_args(["--publish-guild"]).await,
        Err(noctis::Error::Cli(CliError::MissingDevGuild))
    ));
    assert!(requests(&http).is_empty());

    let app = app_with(&http, Module::new("m").add::<Ping>(), |b| {
        b.dev_guild(Id::new(9));
    });
    app.run_with_args(["--publish-guild", "--dry-run"])
        .await
        .unwrap();
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {}", guild_path(9))]
    );

    // An explicit id wins over the configured dev guild.
    http.clear_requests();
    let app = app_with(&http, Module::new("m").add::<Ping>(), |b| {
        b.dev_guild(Id::new(9));
    });
    app.run_with_args(["--publish-guild", "42", "--dry-run"])
        .await
        .unwrap();
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {}", guild_path(42))]
    );
}

/// Waits until the fake gateway holds a shard session.
async fn wait_ready(gw: &FakeGateway) {
    let deadline = Instant::now() + WAIT;
    while gw.dispatch("TYPING_START", json!({})) == 0 {
        assert!(Instant::now() < deadline, "the shard has no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Starts a normal run against both fakes and waits for READY.
async fn start_run(
    http: &FakeDiscordHttp,
    configure: impl FnOnce(&mut AppBuilder),
) -> (
    FakeGateway,
    tokio::task::JoinHandle<Result<(), noctis::Error>>,
) {
    let gw = FakeGateway::start().await.unwrap();
    let app = app_with(http, Module::new("m").add::<Ping>(), |b| {
        b.gateway_url(gw.url());
        configure(b);
    });
    let run = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
    wait_ready(&gw).await;
    (gw, run)
}

/// Matrix 29: a publisher publishes before `GET /gateway/bot`; a failed
/// Publish is logged and the App still starts.
#[tokio::test(flavor = "multi_thread")]
async fn publisher_publishes_before_the_gateway() {
    let (http, _) = fake().await;
    let (_gw, run) = start_run(&http, |b| {
        b.publisher(true);
    })
    .await;
    let seen = requests(&http);
    let position = |needle: &str| seen.iter().position(|r| r == needle).unwrap();
    assert!(position(&format!("GET {APP}")) < position("GET /gateway/bot"));
    assert!(position(&format!("PUT {GLOBAL}")) < position("GET /gateway/bot"));
    run.abort();

    let (http, _) = fake().await;
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({"message": "down"}),
        ),
    );
    let (_gw, run) = start_run(&http, |b| {
        b.publisher(true);
    })
    .await;
    assert!(!run.is_finished());
    let logged = capture().lines_with(&["ERROR", "noctis::publish", "startup Publish failed"]);
    assert!(logged.iter().any(|l| l.contains("HTTP 500")), "{logged:?}");
    run.abort();
}

/// Matrix 30: without `publisher`, no `/applications` request at all,
/// before or after READY.
#[tokio::test(flavor = "multi_thread")]
async fn publish_never_runs_on_shard_ready() {
    let (http, _) = fake().await;
    let (_gw, run) = start_run(&http, |_| {}).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !requests(&http).iter().any(|r| r.contains("/applications")),
        "{:?}",
        requests(&http)
    );
    run.abort();
}

/// Matrix 32: one `noctis.publish` span per Publish.
#[tokio::test(flavor = "multi_thread")]
async fn publish_runs_in_its_span() {
    let capture = capture();
    let http = FakeDiscordHttp::start().await.unwrap();
    serve(&http, &application(), &Remote::default(), &[77]);
    let app = app(&http, Module::new("m").add::<Cfg>());
    app.publish(PublishScope::Guild(Id::new(77)))
        .dry_run(true)
        .await
        .unwrap();
    let spans: Vec<String> = capture
        .lines_with(&["noctis.publish{", "scope=guild 77", "dry_run=true"])
        .into_iter()
        .filter(|line| line.ends_with("}: noctis_core::publish: new"))
        .collect();
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert!(spans[0].contains("app=default"), "{spans:?}");
}
