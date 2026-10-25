//! Story 10.8 (IMG-03): remote images end to end through `App::build` on a
//! test Runtime. A loopback HTTP/1.1 server stands in for the Discord CDN
//! (`RemoteConfig::discord_cdn`): an avatar is fetched once and cached in
//! the App's namespaced `AssetStore`, a URL that is not allow-listed draws
//! the fallback without a request, and a required image that fails is
//! `RenderError::RemoteImage`.

#![cfg(feature = "image")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use noctis::image::unstable::RenderJob;
use noctis::image::{
    AppBuilderImageExt, Font, Frame, ImageConfig, Node, RemoteConfig, RemoteImage, RenderError,
    Template, TemplateView, theme, view,
};
use noctis::ports::memory::MemoryAssetStore;
use noctis::ports::{AssetStore, BoxFuture, PortError};
use noctis::{App, Module, Runtime};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use twilight_model::id::Id;
use twilight_model::user::User;

static INTER: &[u8] = include_bytes!("../../noctis-image/fonts/InterVariable.woff2");
static RED_PNG: &[u8] = include_bytes!("fixtures/images/red-2x2.png");
const RED: [u8; 4] = [255, 0, 0, 255];
const SLATE: [u8; 4] = [0x47, 0x55, 0x69, 0xFF];
const HASH: &str = "1269e74af4df7417b13759eae50c83dc";

#[derive(Template, Serialize, Deserialize)]
#[template(id = "avatar", version = 1, size = (4, 4), format = Png)]
struct Avatar {
    #[asset]
    avatar: RemoteImage,
}

impl TemplateView for Avatar {
    fn view(&self, _: Frame) -> Node {
        view! {
            <div tw="flex w-full h-full">
                <img tw="w-full h-full" src={&self.avatar} />
            </div>
        }
    }
}

#[derive(Template, Serialize, Deserialize)]
#[template(id = "strict", version = 1, size = (4, 4), format = Png)]
struct Strict {
    #[asset(required)]
    avatar: RemoteImage,
}

impl TemplateView for Strict {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="flex w-full h-full"><img tw="w-full h-full" src={&self.avatar} /></div> }
    }
}

/// A loopback server answering `/avatars/…` with a red PNG and anything else
/// with 404; it records every request target.
async fn cdn() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let target = head.split(' ').nth(1).unwrap_or_default().to_owned();
                let reply = if target.starts_with("/avatars/") {
                    let mut reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        RED_PNG.len()
                    )
                    .into_bytes();
                    reply.extend_from_slice(RED_PNG);
                    reply
                } else {
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_vec()
                };
                log.lock().unwrap().push(target);
                let _ = stream.write_all(&reply).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (origin, seen)
}

/// An `AssetStore` recording every `put`.
#[derive(Default)]
struct Recording {
    inner: MemoryAssetStore,
    puts: Mutex<Vec<(String, Option<Duration>)>>,
}

impl AssetStore for Recording {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.inner.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.puts.lock().unwrap().push((key.to_owned(), ttl));
        self.inner.put(key, value, ttl)
    }
}

fn user(avatar: Option<&str>) -> User {
    User {
        accent_color: None,
        avatar: avatar.map(|hash| hash.parse().unwrap()),
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator: 0,
        email: None,
        flags: None,
        global_name: None,
        id: Id::new(80_351_110_224_678_912),
        locale: None,
        mfa_enabled: None,
        name: "ada".to_owned(),
        premium_type: None,
        primary_guild: None,
        public_flags: None,
        system: None,
        verified: None,
    }
}

fn app(origin: &str, store: Arc<Recording>) -> App {
    let runtime = Runtime::for_test();
    let mut builder = App::builder();
    builder
        .app_id("remote")
        .runtime(&runtime)
        .asset_store(store)
        .theme(theme!())
        .image(
            ImageConfig::new()
                .threads(1)
                .font(Font::from_static(INTER))
                .remote(RemoteConfig::new().discord_cdn(origin)),
        )
        .module(
            Module::new("cards")
                .template::<Avatar>()
                .template::<Strict>(),
        );
    builder.build().unwrap()
}

async fn render<T: Template>(app: &App, template: &T) -> Result<[u8; 4], RenderError> {
    let out = app
        .renderer()
        .expect("a Renderer")
        .render(RenderJob::of(template).unwrap())
        .await?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&out.bytes[..]));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    Ok(buf[..4].try_into().unwrap())
}

#[tokio::test]
async fn img_03_an_avatar_is_fetched_once_and_cached_in_the_namespaced_store() {
    let (origin, seen) = cdn().await;
    let store = Arc::new(Recording::default());
    let app = app(&origin, Arc::clone(&store));
    let avatar = Avatar {
        avatar: RemoteImage::avatar(&user(Some(HASH)), 256),
    };
    assert_eq!(render(&app, &avatar).await.unwrap(), RED);
    assert_eq!(render(&app, &avatar).await.unwrap(), RED);
    assert_eq!(
        *seen.lock().unwrap(),
        [format!("/avatars/80351110224678912/{HASH}.webp?size=256")],
        "the second render is served from the AssetStore"
    );
    assert_eq!(
        *store.puts.lock().unwrap(),
        [(
            format!("remote:asset:img:v1:avatar:80351110224678912:{HASH}:256:s"),
            Some(Duration::from_hours(24))
        )]
    );
}

#[tokio::test]
async fn img_03_a_url_that_is_not_allow_listed_draws_the_fallback() {
    let (origin, seen) = cdn().await;
    let app = app(&origin, Arc::new(Recording::default()));
    let other = Avatar {
        avatar: RemoteImage::url("https://example.com/x.png").unwrap(),
    };
    assert_eq!(render(&app, &other).await.unwrap(), SLATE);
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn img_03_a_required_image_that_fails_is_a_render_error() {
    let (origin, seen) = cdn().await;
    let app = app(&origin, Arc::new(Recording::default()));
    let strict = Strict {
        avatar: RemoteImage::url(&format!("{origin}/missing.png?token=1")).unwrap(),
    };
    let error = render(&app, &strict).await.unwrap_err();
    assert!(
        matches!(&error, RenderError::RemoteImage { id, reason, .. } if id == "strict" && reason == "HTTP 404"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        format!(
            "[IMG-03] template `strict` requires remote image `{origin}/missing.png`, which could \
             not be used: HTTP 404"
        )
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
    // The placeholder never fails, even when required.
    let placeholder = Strict {
        avatar: RemoteImage::placeholder(),
    };
    assert_eq!(render(&app, &placeholder).await.unwrap(), SLATE);
}
