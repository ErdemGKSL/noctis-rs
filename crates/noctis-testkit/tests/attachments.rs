//! Story 9.1: attachments end to end through `TestApp` (FR-30; ATT-04,
//! MSG-08) and the multipart view of a `RecordedRequest` (matrix 45).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use bytes::Bytes;
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Attachment, AttachmentSet, Components, MediaGallery, MediaItem, Section,
    TextDisplay, Thumbnail, V2Message,
};
use noctis_testkit::fake::{HeaderMap, HeaderValue, Method, RecordedRequest};
use noctis_testkit::{HttpCall, Response, TestApp, TestError};
use serde_json::{Value, json};

/// The card image: `0` and `1` are two different renders.
fn card(version: u8) -> Attachment {
    Attachment::rendered(
        Bytes::from(vec![version; 16]),
        [version.wrapping_add(0xA0); 32],
        "png",
    )
    .expect("valid ext")
}

/// The View: the card (version `version`, or none) shown twice in a gallery
/// and once as a Thumbnail, then the buttons.
fn render(version: Option<u8>) -> V2Message {
    let mut set = AttachmentSet::new();
    let mut body = Components::new().push(TextDisplay::new("Profile"));
    if let Some(v) = version {
        let r = set.add(card(v));
        body = body
            .push(MediaGallery::new([
                MediaItem::new(r.clone()).alt("Card"),
                MediaItem::new(r.clone()).alt("Card"),
            ]))
            .push(Section::new(
                [TextDisplay::new("Card")],
                Thumbnail::new(r).alt("Card"),
            ));
    }
    let row = ActionRow::buttons([
        Step { mode: 0 }.build(),
        Step { mode: 1 }.build().label("Change"),
        Step { mode: 2 }.build().label("Remove"),
    ]);
    V2Message::new(body.push(row)).attachments(set)
}

#[derive(SlashCommand)]
#[command("card", description = "Show the card")]
struct Show;

impl Show {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render(Some(0))).await
    }
}

#[derive(SlashCommand)]
#[command("later", description = "Defer, then edit the card in")]
struct Later;

impl Later {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.defer_reply().await?;
        acked.edit_reply(render(Some(0))).await?;
        Ok(acked.done())
    }
}

#[derive(SlashCommand)]
#[command("post", description = "Reply, then post the card as a follow-up")]
struct Post;

impl Post {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.reply(render(None)).await?;
        let _ = acked.follow_up(render(Some(0)), Visibility::Public).await?;
        Ok(acked.done())
    }
}

/// `mode` 0 re-renders the same card, 1 a changed one, 2 none.
#[derive(Button, Debug, Clone, Copy)]
#[button("card.step", defaults(style = Secondary, label = "Same"))]
struct Step {
    mode: u8,
}

impl Step {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let version = match self.mode {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        };
        ctx.update(render(version)).await
    }
}

async fn start() -> TestApp {
    let mut builder = App::builder();
    builder.module(
        Module::new("card")
            .add::<Show>()
            .add::<Later>()
            .add::<Post>()
            .add::<Step>(),
    );
    TestApp::build(&mut builder).await.unwrap()
}

/// The one callback of a run.
fn callback(response: &Response) -> &HttpCall {
    let calls: Vec<&HttpCall> = response
        .http_calls()
        .iter()
        .filter(|c| c.path().ends_with("/callback"))
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", response.http_calls());
    calls[0]
}

/// The callback's `data.attachments`.
fn entries(call: &HttpCall) -> Value {
    call.json().unwrap()["data"]["attachments"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn att_04_reply_click_update_keeps_changes_and_drops() -> Result<(), TestError> {
    let app = start().await;
    let name0 = card(0).name().to_owned();
    let name1 = card(1).name().to_owned();

    // The reply uploads the card once, though the tree shows it three times.
    let reply = app.command("card").run().await?;
    let call = callback(&reply);
    assert_eq!(call.method(), Method::POST.as_str());
    assert_eq!(entries(call), json!([{ "id": 0, "filename": name0 }]));
    let files = call.files();
    assert_eq!(files.len(), 1);
    assert_eq!(
        (files[0].name.as_str(), files[0].filename.as_str()),
        ("files[0]", name0.as_str())
    );
    assert_eq!(&files[0].bytes[..], &[0_u8; 16][..]);

    // The same View again: kept by id, nothing uploaded.
    let same = app.click(reply.button("Same")?).run().await?;
    let call = callback(&same);
    assert!(call.files().is_empty());
    let kept = entries(call);
    let id = kept[0]["id"]
        .as_str()
        .expect("a kept id is a snowflake string");
    assert!(id.parse::<u64>().unwrap() > 0);
    assert_eq!(kept, json!([{ "id": id }]));

    // A changed card: the old one is dropped, the new one uploaded.
    let changed = app.click(same.button("Change")?).run().await?;
    let call = callback(&changed);
    assert_eq!(entries(call), json!([{ "id": 0, "filename": name1 }]));
    assert_eq!(call.files().len(), 1);
    assert_eq!(&call.files()[0].bytes[..], &[1_u8; 16][..]);

    // No card: an empty keep-list.
    let removed = app.click(same.button("Remove")?).run().await?;
    let call = callback(&removed);
    assert_eq!(entries(call), json!([]));
    assert!(call.files().is_empty());
    app.stop().await
}

fn request(content_type: Option<&'static str>, body: &'static str) -> RecordedRequest {
    let mut headers = HeaderMap::new();
    if let Some(ct) = content_type {
        headers.insert("content-type", HeaderValue::from_static(ct));
    }
    RecordedRequest {
        method: Method::POST,
        path: "/api/v10/interactions/1/t/callback".into(),
        query: None,
        headers,
        body: body.into(),
        connection: 0,
    }
}

/// Matrix 45: `json()` reads `payload_json` and `files()` lists the parts
/// of a multipart request; a JSON request has no files.
#[test]
fn att_04_recorded_request_multipart() {
    let multipart = request(
        Some("multipart/form-data; boundary=XYZ"),
        "--XYZ\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{\"type\":4}\r\n--XYZ\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"a.png\"\r\n\r\nAA\r\n--XYZ\r\nContent-Disposition: form-data; name=\"files[1]\"; filename=\"b.png\"\r\n\r\nB\r\nB\r\n--XYZ--",
    );
    assert_eq!(multipart.json(), Some(json!({ "type": 4 })));
    let files = multipart.files();
    assert_eq!(files.len(), 2);
    assert_eq!(
        (
            files[0].name.as_str(),
            files[0].filename.as_str(),
            &files[0].bytes[..]
        ),
        ("files[0]", "a.png", &b"AA"[..])
    );
    assert_eq!(
        (
            files[1].name.as_str(),
            files[1].filename.as_str(),
            &files[1].bytes[..]
        ),
        ("files[1]", "b.png", &b"B\r\nB"[..])
    );

    let plain = request(Some("application/json"), "{\"type\":5}");
    assert_eq!(plain.json(), Some(json!({ "type": 5 })));
    assert!(plain.files().is_empty());
    assert!(request(None, "").files().is_empty());
}

/// The update of a click re-rendering the same card keeps it by its
/// snowflake id and uploads nothing.
fn assert_kept(update: &Response) {
    let call = callback(update);
    assert!(call.files().is_empty());
    let kept = entries(call);
    let id = kept[0]["id"]
        .as_str()
        .expect("a kept id is a snowflake string");
    assert!(id.parse::<u64>().unwrap() > 0);
    assert_eq!(kept, json!([{ "id": id }]));
}

/// The testkit echoes the attachments of a deferred reply finished by an
/// edit (`@original`) and of a follow-up, so a click on either keeps them.
#[tokio::test(flavor = "multi_thread")]
async fn att_04_deferred_edit_and_follow_up_are_echoed() -> Result<(), TestError> {
    let app = start().await;

    let later = app.command("later").run().await?;
    let same = app.click(later.button("Same")?).run().await?;
    assert_kept(&same);

    let post = app.command("post").run().await?;
    let followup = post.followups().first().expect("one follow-up");
    let same = app.click(followup.button("Same")?).run().await?;
    assert_kept(&same);
    app.stop().await
}
