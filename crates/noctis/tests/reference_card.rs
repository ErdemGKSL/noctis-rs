//! Story 10.11: the reference card (a copy of the Project Template's
//! profile card, shared with `noctis-image`'s `reference_card` bench) as a
//! pixel snapshot through `TestApp` (NFR-8: a takumi upgrade that changes
//! pixels fails here), its class strings, and the card in a Media Gallery
//! (FR-49).

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

#[path = "../../noctis-image/benches/reference_card/card.rs"]
mod card;

use noctis::prelude::*;
use noctis::testkit::{Response, TestApp, assert_render_snapshot};
use noctis::ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
use noctis_image::tw;
use serde_json::Value;

#[test]
fn reference_card_classes_are_valid() {
    let theme = card::theme().unwrap();
    for classes in card::CLASS_STRINGS {
        assert!(tw::validate(classes, &theme).is_ok(), "{classes}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reference_card_matches_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let app = TestApp::build(card::configure(&mut App::builder())?).await?;
    assert_render_snapshot!(
        app.renderer(),
        "reference_card",
        card::sample(42),
        tolerance = 0.01
    );
    app.stop().await?;
    Ok(())
}

#[derive(SlashCommand)]
#[command("card", description = "Show the reference card")]
struct ShowCard;

impl ShowCard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let image = ctx.render(card::sample(42)).await?;
        let mut files = AttachmentSet::new();
        let image = files.add(image);
        let gallery = MediaGallery::new([MediaItem::new(image).alt("Deniz's profile card")]);
        ctx.respond(V2Message::new(Components::new().push(gallery)).attachments(files))
            .await
    }
}

/// Every `"url"` string in `value`.
fn urls(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                match value {
                    Value::String(url) if key == "url" => out.push(url.clone()),
                    _ => urls(value, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| urls(item, out)),
        _ => {}
    }
}

/// The run's one uploaded file.
fn uploaded(response: &Response) -> (String, Vec<u8>) {
    let files: Vec<_> = response
        .http_calls()
        .iter()
        .flat_map(|call| call.files().iter())
        .collect();
    assert_eq!(files.len(), 1, "one upload");
    (files[0].filename.clone(), files[0].bytes.to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn reference_card_in_a_media_gallery() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = App::builder();
    card::configure(&mut builder)?.module(Module::new("cards").add::<ShowCard>());
    let app = TestApp::build(&mut builder).await?;
    let response = app.command("card").run().await?;
    let (name, bytes) = uploaded(&response);
    let (hex, ext) = name.split_once('.').unwrap();
    assert_eq!(ext, "png");
    assert_eq!(hex.len(), 16, "{name}");
    assert!(
        hex.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{name}"
    );
    let reader = png::Decoder::new(std::io::Cursor::new(&bytes[..])).read_info()?;
    assert_eq!((reader.info().width, reader.info().height), (934, 282));
    let mut found = Vec::new();
    urls(&response.message_json(), &mut found);
    assert_eq!(found, [format!("attachment://{name}")]);
    app.stop().await?;
    Ok(())
}
