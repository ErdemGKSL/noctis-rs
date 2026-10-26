//! Components V2 bot messages on the wire (CV2-01, CV2-02, MSG-07).

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use hyper::{Method as HttpMethod, StatusCode};
use noctis_codec::RouteShape;
use noctis_rules::{Limits, RuleId};
use noctis_ui::__macro::{ButtonDefaults, build_button};
use noctis_ui::{
    ActionRow, Attachment, AttachmentSet, ButtonStyle, Components, CreateFlags, EditFlags,
    MediaGallery, MediaItem, TextDisplay, V2Message,
};
use serde_json::{Value, json};
use twilight_model::id::Id;

use super::*;
use crate::edge::test_support::{Answer, Seen};
use crate::responder::tests::Fixture;
use crate::responder::tests::files::payload;
use crate::test_support::{TestRefs, test_route};

test_route!(Stored, "stored", RouteShape::BY_REF, 0);

fn v2(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

/// A small Discord: a new message is 77 in channel 5, an edit answers the
/// edited id; attachments are echoed (new ones get ids 500, 501, … in
/// upload order); a delete answers 204.
fn discord() -> Answer {
    let names: Mutex<Vec<Value>> = Mutex::default();
    Arc::new(move |seen: &Seen| {
        if seen.method == HttpMethod::DELETE {
            return (StatusCode::NO_CONTENT, Bytes::new());
        }
        let json = payload(seen);
        let attachments: Vec<Value> = json
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| {
                let mut names = names.lock().unwrap();
                if let Some(kept) = entry["id"].as_str() {
                    let index = kept.parse::<usize>().unwrap() - 500;
                    json!({"id": kept, "filename": names[index]})
                } else {
                    names.push(entry["filename"].clone());
                    json!({"id": (499 + names.len()).to_string(), "filename": entry["filename"]})
                }
            })
            .collect();
        let id = if seen.method == HttpMethod::PATCH {
            seen.path.rsplit('/').next().unwrap().to_owned()
        } else {
            "77".to_owned()
        };
        let body = json!({"id": id, "channel_id": "5", "attachments": attachments});
        (StatusCode::OK, Bytes::from(body.to_string()))
    })
}

fn png(bytes: &'static [u8]) -> Attachment {
    Attachment::new(bytes, "png").unwrap()
}

fn gallery(file: &Attachment) -> V2Message {
    let mut set = AttachmentSet::new();
    let item = MediaItem::new(set.add(file.clone())).alt("x");
    let body = Components::new()
        .push(TextDisplay::new("t"))
        .push(MediaGallery::try_new([item], &Limits::default()).unwrap());
    V2Message::new(body).attachments(set)
}

/// A send is Create Message with the bot token and the V2 flag.
#[tokio::test]
async fn cv2_01_send_wire() {
    let fx = Fixture::start_with_token(discord()).await;
    let sent = fx.app.messages().send(Id::new(5), v2("hi")).await.unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, HttpMethod::POST);
    assert_eq!(seen[0].path, "/api/v10/channels/5/messages");
    assert_eq!(seen[0].headers["authorization"], "Bot abc");
    assert_eq!(
        payload(&seen[0]),
        json!({
            "flags": 32768,
            "components": [{"type": 10, "id": 1, "content": "hi"}],
            "allowed_mentions": {"parse": []}
        })
    );
    assert_eq!(sent.id(), Id::new(77));
    assert_eq!(sent.channel_id(), Id::new(5));
}

/// MSG-07 flags and the reply reference.
#[tokio::test]
async fn msg_07_flags_and_reply() {
    let fx = Fixture::start(discord()).await;
    fx.app
        .messages()
        .send(Id::new(5), v2("a"))
        .flags(CreateFlags::SUPPRESS_NOTIFICATIONS)
        .reply_to(Id::new(9))
        .await
        .unwrap();
    let body = payload(&fx.server.seen()[0]);
    assert_eq!(body["flags"], 36864);
    assert_eq!(
        body["message_reference"],
        json!({"message_id": "9", "fail_if_not_exists": false})
    );
}

/// Route buttons resolve as in a reply: the Ref is stored.
#[tokio::test]
async fn routes_resolve_as_in_a_reply() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    let button = build_button(
        Stored { item: 1 },
        &ButtonDefaults {
            style: ButtonStyle::Primary,
            label: Some(noctis_ui::__macro::DefaultText::Literal("b")),
            emoji: None,
            disabled: false,
        },
    );
    let msg = V2Message::new(Components::new().push(ActionRow::buttons([button])));
    fx.app.messages().send(Id::new(5), msg).await.unwrap();
    let body = payload(&fx.server.seen()[0]);
    let id = body["components"][0]["components"][0]["custom_id"]
        .as_str()
        .unwrap();
    assert!(id.starts_with("0stored:~"), "{id}");
    assert_eq!(refs.calls_of("put_if_absent").len(), 1);
}

/// The Limits are checked before any request.
#[tokio::test]
async fn limits_before_any_request() {
    let fx = Fixture::start(discord()).await;
    let err = fx
        .app
        .messages()
        .send(Id::new(5), v2(&"a".repeat(4001)))
        .await
        .unwrap_err();
    let MessageError::Invalid(UiError::Limits { violations }) = err else {
        panic!("expected Invalid, got {err:?}");
    };
    assert_eq!(violations[0].rule, RuleId::CV2_04);
    assert!(fx.server.seen().is_empty());
}

/// CV2-02: an edit by id clears the legacy fields; a handle's edit does not
/// and keeps its attachments.
#[tokio::test]
async fn cv2_02_edits() {
    let fx = Fixture::start(discord()).await;
    let messages = fx.app.messages();
    messages
        .edit(Id::new(5), Id::new(40), v2("x"))
        .flags(EditFlags::SUPPRESS_EMBEDS)
        .await
        .unwrap();
    let file = png(b"a");
    let sent = messages.send(Id::new(5), gallery(&file)).await.unwrap();
    sent.edit(gallery(&file)).await.unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen[0].method, HttpMethod::PATCH);
    assert_eq!(seen[0].path, "/api/v10/channels/5/messages/40");
    let first = payload(&seen[0]);
    assert_eq!(first["flags"], 32772);
    assert_eq!(first["content"], Value::Null);
    assert_eq!(first["embeds"], json!([]));
    assert_eq!(seen[2].path, "/api/v10/channels/5/messages/77");
    let kept = payload(&seen[2]);
    assert!(kept.get("content").is_none());
    assert_eq!(kept["attachments"], json!([{"id": "500"}]));
}

/// Deletes by id and through the handle.
#[tokio::test]
async fn deletes() {
    let fx = Fixture::start(discord()).await;
    let messages = fx.app.messages();
    messages.delete(Id::new(5), Id::new(40)).await.unwrap();
    let sent = messages.send(Id::new(5), v2("a")).await.unwrap();
    sent.delete().await.unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen[0].method, HttpMethod::DELETE);
    assert_eq!(seen[0].path, "/api/v10/channels/5/messages/40");
    assert_eq!(seen[2].path, "/api/v10/channels/5/messages/77");
}

/// Error statuses map to typed errors.
#[tokio::test]
async fn rejected() {
    let fx = Fixture::start(Arc::new(|_: &Seen| {
        (
            StatusCode::FORBIDDEN,
            Bytes::from_static(br#"{"code":50013}"#),
        )
    }))
    .await;
    let err = fx.app.messages().send(Id::new(5), v2("a")).await;
    assert!(matches!(
        err,
        Err(MessageError::Rejected {
            status: 403,
            code: Some(50013)
        })
    ));
}
