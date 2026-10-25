//! Story 10.9 (IMG-04): emoji end to end through `App::build` on a test
//! Runtime. A Discord custom emoji in a Template's `String` field is fetched
//! from a loopback stand-in for the Discord CDN (`RemoteConfig::discord_cdn`)
//! and drawn; with `image-emoji`, a Unicode emoji draws in Twemoji colours.

#![cfg(feature = "image")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};

use noctis::image::unstable::RenderJob;
use noctis::image::{
    AppBuilderImageExt, Font, Frame, ImageConfig, Node, RemoteConfig, Style, Template,
    TemplateView, theme, view,
};
use noctis::{App, Module, Runtime};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static INTER: &[u8] = include_bytes!("../../noctis-image/fonts/InterVariable.woff2");
static RED_PNG: &[u8] = include_bytes!("fixtures/images/red-2x2.png");
const EMOJI: u64 = 123_456_789_012_345_678;

#[derive(Template, Serialize, Deserialize)]
#[template(id = "line", version = 1, size = (160, 40), format = Png)]
struct Line {
    text: String,
}

impl TemplateView for Line {
    fn view(&self, _: Frame) -> Node {
        view! {
            <div tw="flex w-full h-full items-center" style={Style::new().bg_rgb(0xFF_FFFF)}>
                <p>{&self.text}</p>
            </div>
        }
    }
}

#[derive(Template, Serialize, Deserialize)]
#[template(id = "big", version = 1, size = (96, 96), format = Png)]
struct Big {
    text: String,
}

impl TemplateView for Big {
    fn view(&self, _: Frame) -> Node {
        view! {
            <div tw="flex w-full h-full items-center" style={Style::new().bg_rgb(0xFF_FFFF)}>
                <p tw="text-[64px]">{&self.text}</p>
            </div>
        }
    }
}

/// A loopback server answering `/emojis/…` with a red PNG and anything else
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
                let reply = if target.starts_with("/emojis/") {
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

fn app(origin: &str) -> App {
    let runtime = Runtime::for_test();
    let mut builder = App::builder();
    builder
        .app_id("emoji")
        .runtime(&runtime)
        .theme(theme!())
        .image(
            ImageConfig::new()
                .threads(1)
                .font(Font::from_static(INTER))
                .remote(RemoteConfig::new().discord_cdn(origin)),
        )
        .module(Module::new("cards").template::<Line>().template::<Big>());
    builder.build().unwrap()
}

/// The RGBA pixels of a render of `template`.
async fn pixels<T: Template>(app: &App, template: &T) -> Vec<[u8; 4]> {
    let out = app
        .renderer()
        .expect("a Renderer")
        .render(RenderJob::of(template).unwrap())
        .await
        .unwrap();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&out.bytes[..]));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    buf.chunks_exact(4).map(|p| p.try_into().unwrap()).collect()
}

#[tokio::test]
async fn img_04_a_custom_emoji_in_template_data_is_fetched_and_drawn() {
    let (origin, seen) = cdn().await;
    let app = app(&origin);
    let line = Line {
        text: format!("gg <:pepe:{EMOJI}> wp"),
    };
    let red = pixels(&app, &line)
        .await
        .into_iter()
        .filter(|p| p[0] > 200 && p[1] < 60 && p[2] < 60)
        .count();
    assert!(red > 50, "{red} red pixels");
    assert_eq!(
        *seen.lock().unwrap(),
        [format!("/emojis/{EMOJI}.webp?size=128")]
    );
}

#[cfg(feature = "image-emoji")]
#[tokio::test]
async fn img_04_a_unicode_emoji_draws_in_twemoji_colours() {
    let (origin, _) = cdn().await;
    let app = app(&origin);
    let yellow = pixels(
        &app,
        &Big {
            text: "😀".into()
        },
    )
    .await
    .into_iter()
    .filter(|p| {
        p[..3]
            .iter()
            .zip([0xFF_u8, 0xCC, 0x4D])
            .all(|(&a, b)| a.abs_diff(b) <= 24)
    })
    .count();
    assert!(yellow >= 200, "{yellow} yellow pixels");
}
