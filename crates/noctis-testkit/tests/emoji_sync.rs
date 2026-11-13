//! The emoji sync of Publish end to end (Story 20.5, FR-82): create the
//! missing names, the EMO-01 guard, a dry run, changed art (kept or
//! replaced), deleting undeclared emojis, a drawn emoji through a fake
//! Renderer, a still fallback of an animation, a missing Renderer, a clear,
//! and a repeat after a failure, against a `FakeDiscordHttp` whose emoji
//! routes keep state.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use noctis::emoji::{
    AppEmojis, EmojiArt, EmojiDecl, EmojiFormat, EmojiImage, EmojiSync, EmojiTone,
};
use noctis::prelude::*;
use noctis::publish::{PublishError, PublishScope};
use noctis::rules::Limits;
use noctis::{AppBuilder, Runtime, Token};
use noctis_ports::{
    AnimatedFormat, AnimationSpec, BoxFuture, EncodeSettings, ErasedView, Format, OutputFormat,
    RenderError, RenderJob, RenderOutput, Renderer, Size, Template, TemplateDescriptor,
};
use noctis_testkit::fake::{FakeDiscordHttp, FakeResponse, GatewayBot, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const APP: &str = "/applications/@me";
const EMOJIS: &str = "/applications/7/emojis";
const COMMANDS: &str = "/applications/7/commands";

const PNG: EmojiImage = EmojiImage::File {
    bytes: b"\x89PNG\r\n\x1a\n",
    format: EmojiFormat::Png,
};
const GIF: EmojiImage = EmojiImage::File {
    bytes: b"GIF89a",
    format: EmojiFormat::Gif,
};

/// A drawn emoji's Template (the fake Renderer never draws it); declared
/// as WebP to prove the sync renders it as PNG.
#[derive(Serialize, Deserialize)]
struct DotArt {
    white: bool,
}

impl Template for DotArt {
    const ID: &'static str = "emoji.dot";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(128, 128);
    const FORMAT: Format = Format::WebpLossless;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

/// An animated drawn emoji (animated WebP, rendered as GIF).
#[derive(Serialize, Deserialize)]
struct PulseArt;

impl Template for PulseArt {
    const ID: &'static str = "emoji.pulse";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(128, 128);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = Some(AnimationSpec::new(
        Duration::from_secs(1),
        10,
        AnimatedFormat::DEFAULT,
    ));

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

fn dot(tone: EmojiTone) -> EmojiArt {
    EmojiArt::template(DotArt {
        white: tone == EmojiTone::White,
    })
}

fn pulse(_: EmojiTone) -> EmojiArt {
    EmojiArt::template(PulseArt)
}

/// What `#[derive(AppEmojis)]` implements, by hand (no asset files).
#[derive(Clone, Copy, Debug)]
enum Icons {
    Check,
    Cross,
    Spin,
    Dot,
}

impl AppEmojis for Icons {
    const DECLARATIONS: &'static [EmojiDecl] = &[
        EmojiDecl::__new("check", PNG).__white("check_white", PNG),
        EmojiDecl::__new("cross", PNG),
        EmojiDecl::__new("spin", GIF),
        EmojiDecl::__new("dot", EmojiImage::Render(dot)),
    ];

    fn declaration(self) -> &'static EmojiDecl {
        &Self::DECLARATIONS[self as usize]
    }
}

/// Every name [`Icons`] declares, in fold order.
const NAMES: [&str; 5] = ["check", "check_white", "cross", "spin", "dot"];

#[derive(Clone, Copy, Debug)]
enum Pulses {
    Pulse,
}

impl AppEmojis for Pulses {
    const DECLARATIONS: &'static [EmojiDecl] =
        &[EmojiDecl::__new("pulse", EmojiImage::Render(pulse))];

    fn declaration(self) -> &'static EmojiDecl {
        &Self::DECLARATIONS[self as usize]
    }
}

// ---- the fake Renderer ----

#[derive(Default)]
struct Drawn {
    registered: Vec<&'static str>,
    jobs: Vec<RenderJob>,
    /// The art revision: the last byte of every output.
    revision: u8,
    /// The frames of an animated output (1 is the still fallback).
    frames: u32,
}

#[derive(Clone, Default)]
struct FakeRenderer(Arc<Mutex<Drawn>>);

impl Renderer for FakeRenderer {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let mut drawn = self.0.lock().unwrap();
        let animated = job.animation.is_some();
        let mut bytes = if animated {
            b"GIF89a".to_vec()
        } else {
            b"\x89PNG\r\n\x1a\n".to_vec()
        };
        bytes.push(drawn.revision);
        let output = RenderOutput {
            bytes: Bytes::from(bytes),
            digest: [drawn.revision; 32],
            format: if animated {
                OutputFormat::Gif
            } else {
                OutputFormat::Png
            },
            settings_used: EncodeSettings {
                size: job.size,
                quality: None,
                fps: job.animation.map(|a| a.fps),
                frames: if animated { drawn.frames } else { 1 },
            },
        };
        drawn.jobs.push(job);
        Box::pin(std::future::ready(Ok(output)))
    }

    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        self.0.lock().unwrap().registered = templates.iter().map(|t| t.id).collect();
        Ok(())
    }
}

// ---- the fake application ----

/// The application's emojis, `(id, name)`, changed by the fake's routes.
#[derive(Clone, Default)]
struct Remote(Arc<Mutex<Vec<(u64, String)>>>);

impl Remote {
    fn names(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(_, n)| n.clone())
            .collect()
    }

    fn id(&self, name: &str) -> Option<u64> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|(_, n)| n == name)
            .map(|(id, _)| *id)
    }
}

/// A fake for application 7 with `names` uploaded; a create of a name in
/// `reject` answers 400 once.
async fn fake(names: &[&str], reject: &[&str]) -> (FakeDiscordHttp, Remote) {
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
    http.route(
        Method::GET,
        COMMANDS,
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    http.route(
        Method::PUT,
        COMMANDS,
        FakeResponse::json(StatusCode::OK, &json!([])),
    );
    let remote = Remote::default();
    *remote.0.lock().unwrap() = names
        .iter()
        .enumerate()
        .map(|(i, n)| (900 + i as u64, (*n).to_owned()))
        .collect();
    let list = remote.clone();
    http.route_with(Method::GET, EMOJIS, move |_| {
        let items: Vec<Value> = list
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(id, name)| json!({"id": id.to_string(), "name": name, "animated": false}))
            .collect();
        FakeResponse::json(StatusCode::OK, &json!({ "items": items }))
    });
    let created = remote.clone();
    let rejected: Arc<Mutex<Vec<String>>> =
        Arc::new(Mutex::new(reject.iter().map(|n| (*n).to_owned()).collect()));
    http.route_with(Method::POST, EMOJIS, move |request| {
        let body = request.json().unwrap();
        let name = body["name"].as_str().unwrap().to_owned();
        let mut rejected = rejected.lock().unwrap();
        if let Some(i) = rejected.iter().position(|n| *n == name) {
            rejected.remove(i);
            return FakeResponse::json(
                StatusCode::BAD_REQUEST,
                &json!({"code": 50035, "message": "Invalid Form Body"}),
            );
        }
        let mut remote = created.0.lock().unwrap();
        let id = 1000 + remote.len() as u64 + remote.iter().map(|(id, _)| *id).max().unwrap_or(0);
        remote.push((id, name.clone()));
        FakeResponse::json(
            StatusCode::CREATED,
            &json!({"id": id.to_string(), "name": name}),
        )
    });
    let deleted = remote.clone();
    http.fallback_with(move |request| {
        let id = request
            .route_path()?
            .strip_prefix(&format!("{EMOJIS}/"))?
            .parse::<u64>()
            .ok()?;
        (request.method == Method::DELETE).then(|| {
            deleted.0.lock().unwrap().retain(|(i, _)| *i != id);
            FakeResponse::empty(StatusCode::NO_CONTENT)
        })
    });
    (http, remote)
}

fn app_with(http: &FakeDiscordHttp, configure: impl FnOnce(&mut AppBuilder)) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url());
    configure(&mut builder);
    builder.build().unwrap()
}

/// An App declaring [`Icons`], drawing with `renderer`.
fn icons_app(
    http: &FakeDiscordHttp,
    renderer: &FakeRenderer,
    sync: EmojiSync,
    limits: Option<Limits>,
) -> App {
    app_with(http, |b| {
        b.emojis::<Icons>()
            .renderer(renderer.clone())
            .emoji_sync(sync);
        if let Some(limits) = limits {
            b.limits(limits);
        }
    })
}

/// `METHOD /path` of every recorded emoji request, in order.
fn emoji_requests(http: &FakeDiscordHttp) -> Vec<String> {
    http.requests()
        .iter()
        .filter_map(|r| r.route_path().map(|p| format!("{} {p}", r.method)))
        .filter(|r| r.contains("/emojis"))
        .collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// Publish lists after the Commands, creates every missing declared name
/// (a file as its own format, a drawn one rendered as PNG within 256 KiB),
/// one request at a time, and never touches an undeclared emoji; the art
/// Template is registered with the Renderer at build. A second Publish
/// writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn creates_the_missing_names_only() {
    let (http, remote) = fake(&["check", "manual"], &[]).await;
    let renderer = FakeRenderer::default();
    let app = icons_app(&http, &renderer, EmojiSync::new(), None);
    assert_eq!(renderer.0.lock().unwrap().registered, ["emoji.dot"]);

    let report = app.publish(PublishScope::Global).await.unwrap();
    let emojis = report.emojis().unwrap();
    assert_eq!(
        emojis.created(),
        strings(&["check_white", "cross", "spin", "dot"])
    );
    assert_eq!(emojis.unchanged(), strings(&["check"]));
    assert!(emojis.deleted().is_empty() && emojis.wrote());
    let text = report.to_string();
    assert!(
        text.contains("== emojis ==\n+ create emoji `check_white`"),
        "{text}"
    );

    let seen: Vec<String> = http
        .requests()
        .iter()
        .filter_map(|r| r.route_path().map(|p| format!("{} {p}", r.method)))
        .collect();
    let at = |needle: &str| seen.iter().position(|r| r == needle).unwrap();
    assert!(at(&format!("GET {COMMANDS}")) < at(&format!("GET {EMOJIS}")));
    let posts: Vec<Value> = http
        .requests()
        .iter()
        .filter(|r| r.method == Method::POST)
        .map(|r| r.json().unwrap())
        .collect();
    let image = |name: &str| {
        posts.iter().find(|p| p["name"] == name).unwrap()["image"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(image("cross"), "data:image/png;base64,iVBORw0KGgo=");
    assert_eq!(image("spin"), "data:image/gif;base64,R0lGODlh");
    assert_eq!(image("dot"), "data:image/png;base64,iVBORw0KGgoA");
    let jobs = renderer.0.lock().unwrap().jobs.clone();
    assert_eq!(jobs[0].template_id, "emoji.dot");
    assert_eq!(jobs[0].format, Format::Png);
    assert_eq!(jobs[0].upload_budget, 262_144);
    assert!(remote.names().contains(&"manual".to_owned()));
    // This process re-listed after its Publish.
    for icon in [Icons::Check, Icons::Cross, Icons::Spin, Icons::Dot] {
        assert!(app.emojis().get(icon).is_some(), "{icon:?}");
    }

    http.clear_requests();
    let again = app.publish(PublishScope::Global).await.unwrap();
    let emojis = again.emojis().unwrap();
    assert!(emojis.created().is_empty() && !emojis.wrote());
    assert_eq!(emojis.unchanged().len(), NAMES.len());
    assert!(
        emoji_requests(&http).iter().all(|r| r.starts_with("GET")),
        "{:?}",
        emoji_requests(&http)
    );
}

/// EMO-01: creates that would take the application over its limit fail
/// before the first write.
#[tokio::test(flavor = "multi_thread")]
async fn emo_01_guard_fails_before_any_write() {
    let (http, _) = fake(&["a1", "a2", "a3"], &[]).await;
    let renderer = FakeRenderer::default();
    let mut limits = Limits::default();
    limits.application_emojis = 7;
    let app = icons_app(&http, &renderer, EmojiSync::new(), Some(limits.clone()));
    let error = app.publish(PublishScope::Global).await.unwrap_err();
    assert_eq!(error, PublishError::TooManyEmojis { count: 8, max: 7 });
    assert!(error.to_string().starts_with("[EMO-01]"), "{error}");
    assert_eq!(emoji_requests(&http), [format!("GET {EMOJIS}")]);

    // Deleting the undeclared ones makes room.
    let app = icons_app(
        &http,
        &renderer,
        EmojiSync::new().delete_undeclared(true),
        Some(limits),
    );
    let report = app.publish(PublishScope::Global).await.unwrap();
    assert_eq!(
        report.emojis().unwrap().deleted(),
        strings(&["a1", "a2", "a3"])
    );
}

/// A dry run lists and reports the plan, and sends no other emoji request.
#[tokio::test(flavor = "multi_thread")]
async fn dry_run_only_lists() {
    let (http, remote) = fake(&["check", "manual"], &[]).await;
    let renderer = FakeRenderer::default();
    let app = icons_app(
        &http,
        &renderer,
        EmojiSync::new().delete_undeclared(true),
        None,
    );
    let report = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    let emojis = report.emojis().unwrap();
    assert_eq!(emojis.created().len(), 4);
    assert_eq!(emojis.deleted(), strings(&["manual"]));
    assert!(emojis.has_writes() && !emojis.wrote());
    assert!(report.to_string().contains("- delete emoji `manual`"));
    assert_eq!(emoji_requests(&http), [format!("GET {EMOJIS}")]);
    assert_eq!(remote.names(), ["check", "manual"]);
}

/// Changed art is found by the stored digest: by default it is reported
/// and kept; with `replace_changed` it is deleted and re-created (a new
/// id). An emoji with no stored digest is adopted as unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn changed_art_is_kept_or_replaced() {
    for replace in [false, true] {
        let (http, remote) = fake(&NAMES, &[]).await;
        let renderer = FakeRenderer::default();
        let app = icons_app(
            &http,
            &renderer,
            EmojiSync::new().replace_changed(replace),
            None,
        );
        // Adopted: every name has no digest yet.
        let first = app.publish(PublishScope::Global).await.unwrap();
        assert_eq!(first.emojis().unwrap().unchanged().len(), NAMES.len());
        assert!(!first.emojis().unwrap().wrote());

        renderer.0.lock().unwrap().revision = 1;
        let old = remote.id("dot").unwrap();
        http.clear_requests();
        let report = app.publish(PublishScope::Global).await.unwrap();
        let emojis = report.emojis().unwrap();
        if replace {
            assert_eq!(emojis.replaced(), strings(&["dot"]));
            assert!(emojis.changed().is_empty());
            assert_eq!(
                emoji_requests(&http)[..3],
                [
                    format!("GET {EMOJIS}"),
                    format!("DELETE {EMOJIS}/{old}"),
                    format!("POST {EMOJIS}"),
                ]
            );
            assert_ne!(remote.id("dot"), Some(old));
            // The new digest is stored: the next Publish is unchanged.
            let next = app.publish(PublishScope::Global).await.unwrap();
            assert_eq!(next.emojis().unwrap().unchanged().len(), NAMES.len());
        } else {
            assert_eq!(emojis.changed(), strings(&["dot"]));
            assert!(emojis.replaced().is_empty() && !emojis.wrote());
            assert_eq!(remote.id("dot"), Some(old));
            assert!(report.to_string().contains("~ changed emoji `dot`"));
        }
        assert_eq!(emojis.unchanged().len(), NAMES.len() - 1);
    }
}

/// `delete_undeclared` deletes what no set declares; the default keeps it.
#[tokio::test(flavor = "multi_thread")]
async fn delete_undeclared_is_opt_in() {
    let (http, remote) = fake(&NAMES, &[]).await;
    let renderer = FakeRenderer::default();
    let keep = icons_app(&http, &renderer, EmojiSync::new(), None);
    let (http2, remote2) = fake(&["manual", "check"], &[]).await;
    let _ = keep.publish(PublishScope::Global).await.unwrap();
    assert_eq!(remote.names().len(), NAMES.len());

    let drop = icons_app(
        &http2,
        &renderer,
        EmojiSync::new().delete_undeclared(true),
        None,
    );
    let manual = remote2.id("manual").unwrap();
    let report = drop.publish(PublishScope::Global).await.unwrap();
    assert_eq!(report.emojis().unwrap().deleted(), strings(&["manual"]));
    assert!(emoji_requests(&http2).contains(&format!("DELETE {EMOJIS}/{manual}")));
    assert!(!remote2.names().contains(&"manual".to_owned()));
}

/// A drawn emoji without a Renderer fails before any request; an
/// animation that fits only as a still is an error, not an upload; a
/// clear leaves the emojis alone.
#[tokio::test(flavor = "multi_thread")]
async fn renderer_errors_and_clear() {
    let (http, _) = fake(&[], &[]).await;
    let app = app_with(&http, |b| {
        b.emojis::<Icons>();
    });
    let error = app.publish(PublishScope::Global).await.unwrap_err();
    assert_eq!(error, PublishError::EmojiNeedsRenderer { name: "dot" });
    assert!(http.requests().is_empty());

    let renderer = FakeRenderer::default();
    renderer.0.lock().unwrap().frames = 1;
    let app = app_with(&http, |b| {
        b.emojis::<Pulses>().renderer(renderer.clone());
    });
    let error = app.publish(PublishScope::Global).await.unwrap_err();
    assert!(
        matches!(&error, PublishError::EmojiArt { name: "pulse", reason } if reason.contains("still frame")),
        "{error}"
    );
    assert_eq!(emoji_requests(&http), [format!("GET {EMOJIS}")]);
    let job = renderer.0.lock().unwrap().jobs[0].clone();
    assert_eq!(job.animation.unwrap().format, AnimatedFormat::Gif);

    renderer.0.lock().unwrap().frames = 10;
    let report = app.publish(PublishScope::Global).await.unwrap();
    assert_eq!(report.emojis().unwrap().created(), strings(&["pulse"]));
    assert!(app.emojis().get(Pulses::Pulse).is_some());

    http.clear_requests();
    let report = app.publish(PublishScope::Global).clear(true).await.unwrap();
    assert!(report.emojis().is_none());
    assert!(emoji_requests(&http).is_empty());
}

/// A 400 on create names the emoji and the likely rules; repeating the
/// Publish skips the names created before the failure.
#[tokio::test(flavor = "multi_thread")]
async fn emo_02_emo_04_rejected_create_is_safe_to_repeat() {
    let (http, remote) = fake(&[], &["cross"]).await;
    let renderer = FakeRenderer::default();
    let app = icons_app(&http, &renderer, EmojiSync::new(), None);
    let error = app.publish(PublishScope::Global).await.unwrap_err();
    assert_eq!(
        error,
        PublishError::EmojiRejected {
            name: "cross",
            status: 400,
            code: Some(50035),
        }
    );
    let text = error.to_string();
    assert!(
        text.contains("[EMO-02]") && text.contains("[EMO-04]"),
        "{text}"
    );
    assert_eq!(remote.names(), ["check", "check_white"]);

    http.clear_requests();
    let report = app.publish(PublishScope::Global).await.unwrap();
    assert_eq!(
        report.emojis().unwrap().created(),
        strings(&["cross", "spin", "dot"])
    );
    let posts = emoji_requests(&http)
        .iter()
        .filter(|r| r.starts_with("POST"))
        .count();
    assert_eq!(posts, 3);
    assert_eq!(remote.names().len(), NAMES.len());
}
