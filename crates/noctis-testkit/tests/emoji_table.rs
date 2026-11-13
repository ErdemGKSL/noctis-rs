//! The application emoji table end to end (Story 20.4): the start-up list,
//! its retry with backoff, the missing-name log, the re-list while names
//! are missing and after Publish, one table per App, against a
//! `FakeDiscordHttp`. Its own test binary, because it installs a global
//! capturing tracing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use noctis::emoji::{AppEmojis, EmojiDecl, EmojiFormat, EmojiImage, EmojiListError, Emojis};
use noctis::prelude::*;
use noctis::publish::PublishScope;
use noctis::ui::ButtonStyle;
use noctis::{AppBuilder, Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeResponse, GatewayBot, Method, StatusCode,
};
use serde_json::{Value, json};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const APP: &str = "/applications/@me";
const EMOJIS: &str = "/applications/7/emojis";

const PNG: EmojiImage = EmojiImage::File {
    bytes: b"\x89PNG\r\n\x1a\n",
    format: EmojiFormat::Png,
};
const GIF: EmojiImage = EmojiImage::File {
    bytes: b"GIF89a",
    format: EmojiFormat::Gif,
};

/// What `#[derive(AppEmojis)]` implements, by hand (no asset files).
#[derive(Clone, Copy, Debug)]
enum Icons {
    Check,
    Cross,
    Spin,
}

impl AppEmojis for Icons {
    const DECLARATIONS: &'static [EmojiDecl] = &[
        EmojiDecl::__new("check", PNG).__white("check_white", PNG),
        EmojiDecl::__new("cross", PNG),
        EmojiDecl::__new("spin", GIF),
    ];

    fn declaration(self) -> &'static EmojiDecl {
        &Self::DECLARATIONS[self as usize]
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

/// The process-wide log capture, installed once.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        capture
    })
}

// ---- the fake ----

/// The application's emojis the fake lists, replaceable mid-test.
#[derive(Clone, Default)]
struct Remote(Arc<Mutex<Vec<Value>>>);

impl Remote {
    fn set(&self, items: Vec<Value>) {
        *self.0.lock().unwrap() = items;
    }
}

fn emoji(id: u64, name: &str, animated: bool) -> Value {
    json!({"id": id.to_string(), "name": name, "animated": animated, "available": true})
}

/// A fake answering the application (id 7) and its emoji list; the first
/// `failures` lists answer 500.
async fn fake(items: Vec<Value>, failures: usize) -> (FakeDiscordHttp, Remote) {
    capture();
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(StatusCode::OK, &json!({"id": "7"})),
    );
    let remote = Remote::default();
    remote.set(items);
    let read = remote.clone();
    let failed = AtomicUsize::new(0);
    http.route_with(Method::GET, EMOJIS, move |_| {
        if failed.fetch_add(1, Ordering::SeqCst) < failures {
            return FakeResponse::json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &json!({"message": "down"}),
            );
        }
        let items = read.0.lock().unwrap().clone();
        FakeResponse::json(StatusCode::OK, &json!({ "items": items }))
    });
    // Publish's command routes: nothing remote, every PUT accepted.
    http.route(
        Method::GET,
        "/applications/7/commands",
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    http.route(
        Method::PUT,
        "/applications/7/commands",
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    (http, remote)
}

fn app_with(http: &FakeDiscordHttp, configure: impl FnOnce(&mut AppBuilder)) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .emojis::<Icons>();
    configure(&mut builder);
    builder.build().unwrap()
}

/// `METHOD /path` of every recorded API request, in order.
fn requests(http: &FakeDiscordHttp) -> Vec<String> {
    http.requests()
        .iter()
        .filter_map(|r| r.route_path().map(|p| format!("{} {p}", r.method)))
        .collect()
}

fn count(http: &FakeDiscordHttp, needle: &str) -> usize {
    requests(http).iter().filter(|r| *r == needle).count()
}

async fn wait_ready(gw: &FakeGateway) {
    let deadline = Instant::now() + WAIT;
    while gw.dispatch("TYPING_START", json!({})) == 0 {
        assert!(Instant::now() < deadline, "the shard has no session");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Builds and runs an App on `http` and a new gateway fake; its emoji
/// handle stays usable.
async fn run(
    http: &FakeDiscordHttp,
    configure: impl FnOnce(&mut AppBuilder),
) -> (
    FakeGateway,
    Emojis,
    tokio::task::JoinHandle<Result<(), noctis::Error>>,
) {
    let gw = FakeGateway::start().await.unwrap();
    let app = app_with(http, |b| {
        b.gateway_url(gw.url());
        configure(b);
    });
    let emojis = app.emojis().clone();
    let task = tokio::spawn(app.run_with_args(std::iter::empty::<&str>()));
    wait_ready(&gw).await;
    (gw, emojis, task)
}

async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The load: `@me` once, then the list; the table maps declared names
/// only, the missing names are logged in one `warn` that says to run
/// Publish, and a second load reuses the application id.
#[tokio::test(flavor = "multi_thread")]
async fn reload_builds_the_table_and_logs_missing_names() {
    let (http, _) = fake(
        vec![
            emoji(101, "check", false),
            emoji(102, "check_white", false),
            emoji(103, "spin", true),
            emoji(104, "undeclared", false),
        ],
        0,
    )
    .await;
    let app = app_with(&http, |_| {});
    let emojis = app.emojis();
    assert!(!emojis.is_loaded());
    assert_eq!(emojis.get(Icons::Check), None);

    emojis.reload().await.unwrap();
    assert_eq!(
        requests(&http),
        [format!("GET {APP}"), format!("GET {EMOJIS}")]
    );
    assert!(emojis.is_loaded());
    assert_eq!(
        emojis.on(ButtonStyle::Success, Icons::Check),
        Some(noctis::ui::Emoji::custom(
            Id::new(102),
            "check_white",
            false
        ))
    );
    assert_eq!(emojis.text(Icons::Spin).to_string(), "<a:spin:103>");
    assert_eq!(emojis.text(Icons::Cross).to_string(), "");
    assert_eq!(emojis.id("undeclared"), None);
    assert_eq!(emojis.missing(), ["cross"]);
    let warned = capture().lines_with(&["WARN", "noctis::emoji", "run Publish"]);
    assert!(
        warned.iter().any(|l| l.contains("missing=cross")),
        "{warned:?}"
    );

    http.clear_requests();
    emojis.reload().await.unwrap();
    assert_eq!(requests(&http), [format!("GET {EMOJIS}")]);
}

/// A failed list at start never stops the App: it warns, the table stays
/// empty, and the list is retried with backoff until it succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn failed_start_list_is_retried_with_backoff() {
    let (http, _) = fake(vec![emoji(201, "check", false)], 2).await;
    let (_gw, emojis, task) = run(&http, |_| {}).await;
    assert!(!task.is_finished());
    // Listed before `GET /gateway/bot`, so before any handler runs.
    let seen = requests(&http);
    let position = |needle: &str| seen.iter().position(|r| r == needle).unwrap();
    assert!(position(&format!("GET {EMOJIS}")) < position("GET /gateway/bot"));

    // 1 s, then 2 s of backoff.
    eventually("the third list", || emojis.get(Icons::Check).is_some()).await;
    assert_eq!(count(&http, &format!("GET {EMOJIS}")), 3);
    let warned = capture().lines_with(&["WARN", "could not list the application emojis"]);
    assert!(warned.iter().any(|l| l.contains("HTTP 500")), "{warned:?}");
    task.abort();
}

/// While a declared name is missing the table is re-listed on the reload
/// interval; once none is, the re-list stops.
#[tokio::test(flavor = "multi_thread")]
async fn missing_names_are_re_listed_until_present() {
    let (http, remote) = fake(vec![emoji(301, "check", false)], 0).await;
    let (_gw, emojis, task) = run(&http, |b| {
        b.emoji_reload_interval(Duration::from_millis(100));
    })
    .await;
    assert_eq!(emojis.get(Icons::Cross), None);
    remote.set(vec![
        emoji(301, "check", false),
        emoji(302, "check_white", false),
        emoji(303, "cross", false),
        emoji(304, "spin", true),
    ]);
    eventually("the re-list", || emojis.get(Icons::Cross).is_some()).await;
    assert!(emojis.missing().is_empty());
    let lists = count(&http, &format!("GET {EMOJIS}"));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(count(&http, &format!("GET {EMOJIS}")), lists);
    task.abort();
}

/// A Publish of this process re-lists (a dry run does not); an App without
/// a set lists nothing; without a token `reload` is an error.
#[tokio::test(flavor = "multi_thread")]
async fn publish_re_lists_and_edges() {
    let (http, remote) = fake(vec![], 0).await;
    let app = app_with(&http, |_| {});
    let _ = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    assert_eq!(count(&http, &format!("GET {EMOJIS}")), 0);
    remote.set(vec![emoji(401, "check", false)]);
    let _ = app.publish(PublishScope::Global).await.unwrap();
    assert_eq!(count(&http, &format!("GET {EMOJIS}")), 1);
    assert_eq!(app.emojis().id("check"), Some(Id::new(401)));

    let (http, _) = fake(vec![], 0).await;
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url());
    let bare = builder.build().unwrap();
    bare.emojis().reload().await.unwrap();
    assert!(requests(&http).is_empty());

    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test()).emojis::<Icons>();
    let tokenless = builder.build().unwrap();
    assert_eq!(
        tokenless.emojis().reload().await,
        Err(EmojiListError::MissingToken)
    );
}

/// Two Apps in one process each have their own table.
#[tokio::test(flavor = "multi_thread")]
async fn each_app_has_its_own_table() {
    let (http_a, _) = fake(vec![emoji(501, "check", false)], 0).await;
    let (http_b, _) = fake(vec![emoji(502, "check", false)], 0).await;
    let a = app_with(&http_a, |_| {});
    let b = app_with(&http_b, |_| {});
    a.emojis().reload().await.unwrap();
    b.emojis().reload().await.unwrap();
    assert_eq!(a.emojis().id("check"), Some(Id::new(501)));
    assert_eq!(b.emojis().id("check"), Some(Id::new(502)));
}
