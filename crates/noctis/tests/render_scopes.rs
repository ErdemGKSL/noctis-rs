//! Story 20.1: `ctx.render` from a Lifecycle Hook with the real
//! `ImagePlugin` through `noctis::testkit::TestApp`: an `AppStarted` hook
//! renders a Template and posts it with `ctx.messages().send(..)`.

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::image::{Font, Frame, ImageConfig, Node, Template, TemplateView, theme, view};
use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;
use noctis::testkit::fake::RecordedRequest;
use noctis::testkit::{TestApp, TestError};
use noctis::twilight::model::id::Id;
use noctis::ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
use serde::{Deserialize, Serialize};

const WAIT: Duration = Duration::from_secs(15);
const CHANNEL: &str = "/channels/42/messages";

#[derive(Template, Serialize, Deserialize)]
#[template(id = "welcome", version = 1, size = (64, 32), format = Png)]
struct Welcome {
    name: String,
}

impl TemplateView for Welcome {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="flex w-full h-full bg-brand">{&self.name}</div> }
    }
}

/// The attachment name the hook rendered, once it did.
#[derive(Default)]
struct Rendered(Mutex<Option<String>>);

/// Renders the welcome card when the App starts and posts it.
#[derive(Hook)]
pub struct PostWelcome;

impl PostWelcome {
    #[handler]
    async fn handle(self, ctx: HookCtx, _ev: AppStarted) -> HandlerResult<()> {
        let image = ctx
            .render(Welcome {
                name: "Ada".to_owned(),
            })
            .await?;
        let rendered = ctx.state::<Arc<Rendered>>().expect("registered");
        *rendered.0.lock().unwrap() = Some(image.name().to_owned());
        let mut files = AttachmentSet::new();
        let image = files.add(image);
        let gallery = MediaGallery::new([MediaItem::new(image).alt("Welcome")]);
        let msg = V2Message::new(Components::new().push(gallery)).attachments(files);
        // The testkit's fake answers an unknown channel with 404; the request
        // is what this test checks.
        let _ = ctx.messages().send(Id::new(42), msg).await;
        Ok(())
    }
}

/// The channel-message posts, once there is one.
async fn wait_sent(app: &TestApp) -> RecordedRequest {
    let start = Instant::now();
    loop {
        if let Some(sent) = app
            .http_fake()
            .requests()
            .into_iter()
            .find(|r| r.route_path() == Some(CHANNEL))
        {
            return sent;
        }
        assert!(start.elapsed() < WAIT, "no post to {CHANNEL}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_started_hook_renders_and_posts_a_template() -> Result<(), TestError> {
    let rendered = Arc::new(Rendered::default());
    let mut builder = App::builder();
    builder
        .theme(theme!())
        .image(ImageConfig::new().font(Font::from_static(include_bytes!(
            "../../noctis-image/fonts/InterVariable.woff2"
        ))))
        .state(Arc::clone(&rendered))
        .module(
            Module::new("welcome")
                .template::<Welcome>()
                .add::<PostWelcome>(),
        );
    let app = TestApp::build(&mut builder).await?;
    let sent = wait_sent(&app).await;
    let files = sent.files();
    assert_eq!(files.len(), 1, "one uploaded file");
    let name = rendered.0.lock().unwrap().clone().expect("rendered");
    assert_eq!(files[0].filename, name);
    assert_eq!(
        name.rsplit_once('.').map(|(_, ext)| ext),
        Some("png"),
        "{name}"
    );
    let body = sent.json().expect("a payload_json part");
    assert_eq!(
        body["components"][0]["items"][0]["media"]["url"],
        format!("attachment://{name}")
    );
    let decoder = png::Decoder::new(std::io::Cursor::new(&files[0].bytes[..]));
    let reader = decoder.read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (64, 32));
    app.stop().await
}
