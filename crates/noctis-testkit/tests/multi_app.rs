//! Story 15.2 end to end: two Apps in one process (FR-47). Apps `main` and
//! `acme` share one `Runtime` and one `MemoryCacheStore`, each with its own
//! token, `FakeDiscordHttp` and `FakeGateway`, and mount the same Module
//! tree, which holds a Command restricted to `main`. `acme` serves only its
//! customer's guild (`GuildAccess::allow`), `main` skips it
//! (`GuildAccess::deny`). Its own test binary with one test, because it
//! installs a global `metrics` recorder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use metrics_util::debugging::DebuggingRecorder;
use noctis::ports::CacheStore as _;
use noctis::ports::memory::{GuildAccess, MemoryCacheStore};
use noctis::prelude::*;
use noctis::publish::PublishScope;
use noctis::twilight::model::id::Id;
use noctis::ui::{Components, TextDisplay};
use noctis::{App, Resource, Runtime, Token};
use noctis::{BeforeCtx, Flow, Middleware};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method, RecordedRequest, StatusCode,
};
use serde_json::{Value, json};

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The customer's guild: `acme` serves it, `main` does not.
const CUSTOMER: u64 = 4242;

/// How many times `/ping` and the Module's before-Middleware ran, over
/// both Apps.
static PING_RUNS: AtomicUsize = AtomicUsize::new(0);
static MIDDLEWARE_RUNS: AtomicUsize = AtomicUsize::new(0);

/// Unrestricted: writes `ping` through `ctx.kv()` and answers `pong`.
#[derive(SlashCommand)]
#[command("ping", description = "Pong")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Components> {
        PING_RUNS.fetch_add(1, Ordering::SeqCst);
        ctx.kv().put("ping", &1_u8, None).await?;
        Ok(Components::new().push(TextDisplay::new("pong")))
    }
}

/// Exists only on the App `main`.
#[derive(SlashCommand)]
#[command("admin", description = "Main bot only", apps = ["main"])]
struct Admin;

impl Admin {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Ok(Components::new().push(TextDisplay::new("admin")))
    }
}

/// A process-wide resource: one instance per Runtime.
struct Fonts;

impl Resource for Fonts {
    type Config = ();

    fn init((): &()) -> Self {
        Self
    }
}

/// The Module tree both Apps mount.
fn tree() -> Module {
    Module::new("core")
        .before(Middleware::before(|_: BeforeCtx| async {
            MIDDLEWARE_RUNS.fetch_add(1, Ordering::SeqCst);
            Ok(Flow::Continue)
        }))
        .add::<Ping>()
        .add::<Admin>()
}

/// A fake answering the callbacks of interactions 100 and 101, the
/// application and an empty global command list.
async fn fake() -> FakeDiscordHttp {
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    for id in [100, 101] {
        http.route(
            Method::POST,
            format!("/interactions/{id}/tok{id}/callback"),
            FakeResponse::empty(StatusCode::NO_CONTENT),
        );
    }
    http.route(
        Method::GET,
        "/applications/@me",
        FakeResponse::json(
            StatusCode::OK,
            &json!({"id": "7", "integration_types_config": {"0": {}}}),
        ),
    )
    .route(
        Method::GET,
        "/applications/7/commands",
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    http
}

/// An App `id` with its own token and fakes, on `runtime`, over `store`.
fn app(
    id: &str,
    runtime: &Runtime,
    store: &Arc<MemoryCacheStore>,
    access: GuildAccess,
    (http, gw): (&FakeDiscordHttp, &FakeGateway),
) -> App {
    App::builder()
        .runtime(runtime)
        .token(Token::new(format!("fake-{id}-token")).unwrap())
        .app_id(id)
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .cache_store(Arc::clone(store))
        .guild_access(access)
        .module(tree())
        .build()
        .unwrap()
}

/// `/ping` as interaction `id` (token `tok<id>`), from `guild` or a DM.
fn ping(id: u64, guild: Option<u64>) -> Value {
    let user = json!({ "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null });
    let mut d = json!({
        "id": id.to_string(),
        "application_id": "7",
        "type": 2,
        "token": format!("tok{id}"),
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "channel": { "id": "55", "type": 0 },
        "locale": "en-US",
        "data": { "id": "9", "name": "ping", "type": 1 },
    });
    match guild {
        Some(guild) => {
            d["guild_id"] = json!(guild.to_string());
            d["member"] = json!({
                "user": user, "roles": [], "joined_at": "2024-01-01T00:00:00.000000+00:00",
                "deaf": false, "mute": false, "flags": 0, "permissions": "0"
            });
        }
        None => d["user"] = user,
    }
    d
}

/// The requests of interaction `id` (its callback, follow-ups and edits).
fn of_interaction(http: &FakeDiscordHttp, id: u64) -> Vec<RecordedRequest> {
    let token = format!("/tok{id}");
    http.requests()
        .into_iter()
        .filter(|r| r.route_path().is_some_and(|p| p.contains(&token)))
        .collect()
}

/// Polls until interaction `id` has at least one request on `http`.
async fn answered(http: &FakeDiscordHttp, id: u64) -> Vec<RecordedRequest> {
    let deadline = Instant::now() + WAIT;
    loop {
        let sent = of_interaction(http, id);
        if !sent.is_empty() {
            return sent;
        }
        assert!(Instant::now() < deadline, "interaction {id} unanswered");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Polls `gw` until its shard has a session.
async fn wait_session(gw: &FakeGateway) {
    let deadline = Instant::now() + WAIT;
    while gw.dispatch("TYPING_START", json!({})) == 0 {
        assert!(Instant::now() < deadline, "the shard has no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The text of a V2 callback's first component.
fn text(request: &RecordedRequest) -> String {
    request.json().unwrap()["data"]["components"][0]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// The `app` label values of the `noctis_handler_duration_seconds` series.
fn duration_apps(snapshotter: &metrics_util::debugging::Snapshotter) -> Vec<String> {
    let mut apps: Vec<String> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.key().name() == "noctis_handler_duration_seconds")
        .filter_map(|(key, ..)| {
            key.key()
                .labels()
                .find(|l| l.key() == "app")
                .map(|l| l.value().to_owned())
        })
        .collect();
    apps.sort();
    apps.dedup();
    apps
}

#[tokio::test(flavor = "multi_thread")]
async fn two_apps_in_one_process() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::set_global_recorder(recorder).unwrap();

    let runtime = Runtime::for_test();
    let store = Arc::new(MemoryCacheStore::new());
    let (main_http, acme_http) = (fake().await, fake().await);
    let (main_gw, acme_gw) = (
        FakeGateway::start().await.unwrap(),
        FakeGateway::start().await.unwrap(),
    );
    let main = app(
        "main",
        &runtime,
        &store,
        GuildAccess::deny([Id::new(CUSTOMER)]),
        (&main_http, &main_gw),
    );
    let acme = app(
        "acme",
        &runtime,
        &store,
        GuildAccess::allow([Id::new(CUSTOMER)]),
        (&acme_http, &acme_gw),
    );

    // AC1: own HTTP clients, one Runtime, one process resource.
    assert!(!Arc::ptr_eq(main.http(), acme.http()));
    assert_eq!(main.runtime(), acme.runtime());
    let a = main.runtime().resource_or_init::<Fonts>(()).unwrap();
    let b = acme.runtime().resource_or_init::<Fonts>(()).unwrap();
    assert!(Arc::ptr_eq(&a, &b));

    // AC2: the same tree publishes `admin` only on `main`.
    let names = |report: &noctis::publish::PublishReport| report.to_string();
    let main_report = main
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    let acme_report = acme
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    let (main_json, acme_json) = (names(&main_report), names(&acme_report));
    assert!(main_json.contains("\"name\": \"ping\""), "{main_json}");
    assert!(main_json.contains("\"name\": \"admin\""), "{main_json}");
    assert!(acme_json.contains("\"name\": \"ping\""), "{acme_json}");
    assert!(!acme_json.contains("\"name\": \"admin\""), "{acme_json}");
    main_http.clear_requests();
    acme_http.clear_requests();

    let main_run = tokio::spawn(main.run_with_args(std::iter::empty::<&str>()));
    let acme_run = tokio::spawn(acme.run_with_args(std::iter::empty::<&str>()));
    wait_session(&main_gw).await;
    wait_session(&acme_gw).await;

    // AC1: a DM `/ping` on each App reaches only that App's HTTP fake.
    assert_eq!(main_gw.dispatch("INTERACTION_CREATE", ping(100, None)), 1);
    let main_sent = answered(&main_http, 100).await;
    assert_eq!(text(&main_sent[0]), "pong");
    assert_eq!(acme_gw.dispatch("INTERACTION_CREATE", ping(100, None)), 1);
    let acme_sent = answered(&acme_http, 100).await;
    assert_eq!(text(&acme_sent[0]), "pong");
    assert_eq!(of_interaction(&main_http, 100).len(), 1);
    assert_eq!(of_interaction(&acme_http, 100).len(), 1);
    let deadline = Instant::now() + WAIT;
    while store.get("main:cache:ping").await.unwrap().is_none()
        || store.get("acme:cache:ping").await.unwrap().is_none()
    {
        assert!(Instant::now() < deadline, "the kv writes are missing");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(store.get("ping").await.unwrap(), None);
    let deadline = Instant::now() + WAIT;
    while duration_apps(&snapshotter) != ["acme", "main"] {
        assert!(
            Instant::now() < deadline,
            "duration series: {:?}",
            duration_apps(&snapshotter)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(PING_RUNS.load(Ordering::SeqCst), 2);
    assert_eq!(MIDDLEWARE_RUNS.load(Ordering::SeqCst), 2);

    // AC3: the customer's guild — `acme` (allow list) replies...
    assert_eq!(
        acme_gw.dispatch("INTERACTION_CREATE", ping(101, Some(CUSTOMER))),
        1
    );
    let acme_sent = answered(&acme_http, 101).await;
    assert_eq!(text(&acme_sent[0]), "pong");
    // ...and `main` (deny list) sends exactly one ephemeral notice.
    assert_eq!(
        main_gw.dispatch("INTERACTION_CREATE", ping(101, Some(CUSTOMER))),
        1
    );
    answered(&main_http, 101).await;
    // Leave room for a late follow-up, edit or handler run to show up.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let main_sent = of_interaction(&main_http, 101);
    assert_eq!(main_sent.len(), 1, "{main_sent:?}");
    let body = main_sent[0].json().unwrap();
    assert_eq!(body["type"], 4, "{body}");
    assert_eq!(body["data"]["flags"].as_u64().unwrap() & 64, 64, "{body}");
    assert_eq!(text(&main_sent[0]), "This feature is disabled here.");
    assert_eq!(PING_RUNS.load(Ordering::SeqCst), 3);
    assert_eq!(MIDDLEWARE_RUNS.load(Ordering::SeqCst), 3);

    main_run.abort();
    acme_run.abort();
}
