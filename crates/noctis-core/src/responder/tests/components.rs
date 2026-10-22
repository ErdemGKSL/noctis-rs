//! Story 7.2 Matrix 17-34, 39, 41 and 44-48: the component and modal
//! flows of the Responder over a recording upstream.

use noctis_codec::{PayloadKey, RouteShape, SelectRoute};
use noctis_ui::__macro::{ButtonDefaults, SelectDefaults, build_button, build_user_select};
use noctis_ui::{ButtonStyle, Modal, ModalLabel, TextInput, TextInputStyle};

use super::*;
use crate::test_support::test_route;

test_route!(Sig, "sig", RouteShape::INLINE.signed(), 0);
test_route!(Pick, "pick", RouteShape::INLINE.select(), 0);
test_route!(Stored, "stored", RouteShape::BY_REF, 0);
test_route!(Form, "form", RouteShape::INLINE, 0);

impl SelectRoute for Pick {}

const UPDATE_CB: &str = "/api/v10/interactions/100/tok/callback";
const WITH_RESPONSE: &str = "/api/v10/interactions/100/tok/callback?with_response=true";

/// The component message 77 in channel 55: V2, legacy or ephemeral V2.
#[expect(
    clippy::unnecessary_wraps,
    reason = "`Shared::declared` and `from_interaction` take and give an `Option`"
)]
fn source(format: MessageFormat, ephemerality: Ephemerality) -> Option<SourceMessage> {
    Some(SourceMessage {
        id: Id::new(77),
        channel_id: Id::new(55),
        format,
        ephemerality,
        nonces: NonceSet::EMPTY,
        files: KnownFiles::EMPTY,
    })
}

fn v2_source() -> Option<SourceMessage> {
    source(MessageFormat::V2, Ephemerality::Public)
}

fn component(shared: &Arc<Shared>) -> ComponentResponder {
    Responder::new(Arc::clone(shared))
}

fn modal_responder(shared: &Arc<Shared>) -> ModalResponder {
    Responder::new(Arc::clone(shared))
}

/// The type-7 `data` (and edit body) of `v2(text)`.
fn update_data(text: &str) -> Value {
    json!({
        "flags": 32768,
        "components": v2_components(text),
        "allowed_mentions": { "parse": [] },
        "attachments": [],
    })
}

/// No callback or interaction-webhook request carries a bot token, and no
/// query carries `with_components` (Matrix 46).
fn assert_no_bot_token(fx: &Fixture) {
    for seen in fx.server.seen() {
        if seen.path.contains("/interactions/") || seen.path.contains("/webhooks/") {
            assert!(seen.headers.get("authorization").is_none(), "{}", seen.path);
        }
        assert!(
            !seen
                .query
                .as_deref()
                .unwrap_or("")
                .contains("with_components"),
            "{:?}",
            seen.query
        );
    }
}

fn clear(fx: &Fixture) {
    fx.server.log.lock().unwrap().clear();
}

/// Matrix 20, 21 and 23: `update` is one type-7 callback whose data is the
/// edit body of the source's format; it never carries `EPHEMERAL`.
#[tokio::test]
async fn update_is_one_type_7_callback() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component(&shared).update(v2("a")).await.unwrap();
    assert_eq!(shared.state(), REPLIED);
    assert_eq!(
        fx.requests(),
        [(
            Method::POST,
            UPDATE_CB.to_owned(),
            json!({ "type": 7, "data": update_data("a") })
        )]
    );
    // `edit_reply` afterwards edits the same message.
    acked.edit_reply(v2("b")).await.unwrap();
    assert_eq!(
        fx.requests()[1],
        (Method::PATCH, ORIGINAL.to_owned(), update_data("b"))
    );
    let _ = acked.done();
    assert_no_bot_token(&fx);

    clear(&fx);
    let shared = fx.component(
        Declared::Update,
        source(MessageFormat::Legacy, Ephemerality::Public),
    );
    let _ = component(&shared).update(v2("a")).await.unwrap().done();
    assert_eq!(
        fx.requests()[0].2,
        json!({ "type": 7, "data": {
            "flags": 32768, "content": null, "embeds": [], "poll": null, "sticker_ids": [],
            "components": v2_components("a"), "allowed_mentions": { "parse": [] }, "attachments": [],
        } })
    );

    clear(&fx);
    let shared = fx.component(
        Declared::Update,
        source(MessageFormat::V2, Ephemerality::Ephemeral),
    );
    let _ = component(&shared).update(v2("a")).await.unwrap().done();
    assert_eq!(fx.requests()[0].2["data"]["flags"], 32768);
}

/// Matrix 22 (defensive guard): the typed component API never accepts a
/// legacy body (CV2-01, compile time: `ui/cv2/CV2-01_component_update_legacy.rs`);
/// should one reach the shared state, a legacy body over a V2 source is still
/// `V2ToLegacy` with no request.
#[tokio::test]
async fn update_v2_to_legacy_sends_nothing() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let err = shared
        .update(LegacyMessage::new("x").into(), false)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResponderError::Invalid(UiError::V2ToLegacy)),
        "{err}"
    );
    assert!(fx.requests().is_empty());
    assert_eq!(shared.state(), PENDING);
}

/// Matrix 24 and 25: `defer_update` is `{"type":6}`; `defer_update` +
/// `edit_reply` sends the type-7 data as `PATCH @original`.
#[tokio::test]
async fn defer_update_and_its_equivalence() {
    let fx = Fixture::start(discord()).await;
    let a = fx.component(Declared::Update, v2_source());
    let _ = component(&a).update(v2("m")).await.unwrap().done();
    let b = fx.component(Declared::Update, v2_source());
    let acked = component(&b).defer_update().await.unwrap();
    assert_eq!(b.state(), DEFERRED_UPDATE);
    acked.edit_reply(v2("m")).await.unwrap();
    let _ = acked.done();
    let requests = fx.requests();
    assert_eq!(
        requests[1],
        (Method::POST, UPDATE_CB.to_owned(), json!({ "type": 6 }))
    );
    assert_eq!(
        requests[2],
        (
            Method::PATCH,
            ORIGINAL.to_owned(),
            requests[0].2["data"].clone()
        )
    );
    assert_no_bot_token(&fx);
}

/// Matrix 26: `defer_reply_as(e)` + `edit_reply(m)` ≡ `reply_as(m, e)`, the
/// edit without `EPHEMERAL` (and with the attachments keep-list).
#[tokio::test]
async fn defer_reply_equivalence() {
    for e in [Ephemerality::Public, Ephemerality::Ephemeral] {
        let fx = Fixture::start(discord()).await;
        let a = fx.component(Declared::Update, v2_source());
        let _ = component(&a).reply_as(v2("m"), e).await.unwrap().done();
        let b = fx.component(Declared::Update, v2_source());
        let acked = component(&b).defer_reply_as(e).await.unwrap();
        acked.edit_reply(v2("m")).await.unwrap();
        let _ = acked.done();
        let requests = fx.requests();
        let defer = match e {
            Ephemerality::Public => json!({ "type": 5 }),
            Ephemerality::Ephemeral => json!({ "type": 5, "data": { "flags": 64 } }),
        };
        assert_eq!(requests[1], (Method::POST, UPDATE_CB.to_owned(), defer));
        let mut reply = requests[0].2["data"].clone();
        reply["flags"] = json!(32768);
        let mut edit = requests[2].2.clone();
        assert_eq!(
            edit.as_object_mut().unwrap().remove("attachments"),
            Some(json!([]))
        );
        assert_eq!(
            (requests[2].0.clone(), requests[2].1.as_str()),
            (Method::PATCH, ORIGINAL)
        );
        assert_eq!(edit, reply, "{e:?}");
    }
}

/// Matrix 27 and 28: `reply` and `defer_reply` on a component use the
/// declared response (`Public` under `update`).
#[tokio::test]
async fn reply_and_defer_reply_on_a_component() {
    let fx = Fixture::start(discord()).await;
    let cases = [
        (Declared::Update, None, 32768),
        (Declared::Reply(Ephemerality::Ephemeral), None, 32832),
        (Declared::Update, Some(Ephemerality::Ephemeral), 32832),
    ];
    for (declared, explicit, flags) in cases {
        clear(&fx);
        let shared = fx.component(declared, v2_source());
        let r = component(&shared);
        let acked = match explicit {
            Some(e) => r.reply_as(v2("a"), e).await.unwrap(),
            None => r.reply(v2("a")).await.unwrap(),
        };
        let _ = acked.done();
        let requests = fx.requests();
        assert_eq!(
            (requests[0].1.as_str(), &requests[0].2["type"]),
            (CB_REPLY, &json!(4))
        );
        assert_eq!(requests[0].2["data"]["flags"], flags, "{declared:?}");
    }
    for (declared, body) in [
        (Declared::Update, json!({ "type": 5 })),
        (
            Declared::Reply(Ephemerality::Ephemeral),
            json!({ "type": 5, "data": { "flags": 64 } }),
        ),
    ] {
        clear(&fx);
        let shared = fx.component(declared, v2_source());
        let _ = component(&shared).defer_reply().await.unwrap().done();
        assert_eq!(fx.requests(), [(Method::POST, UPDATE_CB.to_owned(), body)]);
    }
}

/// Matrix 29, 30, 32, 33 and 42: the auto-defer follows the declared
/// response; later calls edit, follow up or send nothing.
#[tokio::test]
async fn auto_defer_by_declared_response() {
    let fx = Fixture::start(discord()).await;
    // `update` on a message: type 6, then `update` edits @original.
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    assert_eq!(shared.state(), DEFERRED_UPDATE);
    let _ = component(&shared).defer_update().await.unwrap().done();
    unanswer(&shared);
    let _ = component(&shared).defer_reply().await.unwrap().done();
    unanswer(&shared);
    let _ = component(&shared).update(v2("u")).await.unwrap().done();
    assert_eq!(
        fx.requests(),
        [
            (Method::POST, UPDATE_CB.to_owned(), json!({ "type": 6 })),
            (Method::PATCH, ORIGINAL.to_owned(), update_data("u")),
        ]
    );
    // A `reply` after type 6 is a public follow-up, never an edit.
    clear(&fx);
    unanswer(&shared);
    let _ = component(&shared).reply(v2("r")).await.unwrap().done();
    let requests = fx.requests();
    assert_eq!(
        (requests[0].0.clone(), requests[0].1.as_str()),
        (Method::POST, FOLLOWUP)
    );
    assert_eq!(requests[0].2["flags"], 32768);

    // `ephemeral` / `public`: type 5, then a returned message edits.
    for (e, defer) in [
        (
            Ephemerality::Ephemeral,
            json!({ "type": 5, "data": { "flags": 64 } }),
        ),
        (Ephemerality::Public, json!({ "type": 5 })),
    ] {
        clear(&fx);
        let shared = fx.component(Declared::Reply(e), v2_source());
        assert!(shared.auto_defer().await);
        shared.finish(v2("m").into()).await.unwrap();
        let requests = fx.requests();
        assert_eq!(requests[0], (Method::POST, UPDATE_CB.to_owned(), defer));
        assert_eq!(
            (requests[1].0.clone(), requests[1].1.as_str()),
            (Method::PATCH, ORIGINAL)
        );
        assert_eq!(requests[1].2["flags"], 32768);
    }

    // A slash-opened modal submit under `update`: type 5 public.
    clear(&fx);
    let shared = fx.component(Declared::Update, None);
    assert!(shared.auto_defer().await);
    assert_eq!(
        fx.requests(),
        [(Method::POST, UPDATE_CB.to_owned(), json!({ "type": 5 }))]
    );
    assert_no_bot_token(&fx);
}

/// Matrix 34 and 43: a returned message while Pending.
#[tokio::test]
async fn returned_messages() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    shared.finish(v2("m").into()).await.unwrap();
    assert_eq!(
        fx.requests(),
        [(
            Method::POST,
            UPDATE_CB.to_owned(),
            json!({ "type": 7, "data": update_data("m") })
        )]
    );
    clear(&fx);
    let shared = fx.component(Declared::Reply(Ephemerality::Ephemeral), v2_source());
    shared.finish(v2("m").into()).await.unwrap();
    assert_eq!(fx.requests()[0].2["data"]["flags"], 32832);
    clear(&fx);
    let shared = fx.component(Declared::Update, None);
    shared.finish(v2("m").into()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(
        (requests[0].1.as_str(), &requests[0].2["type"]),
        (CB_REPLY, &json!(4))
    );
    assert_eq!(requests[0].2["data"]["flags"], 32768);
}

/// The modal of [`Form`] with one text input.
fn form() -> Modal {
    noctis_ui::__macro::build_modal::<Form>(Form { item: 1 }, "Form").push(ModalLabel::new(
        "Bio",
        TextInput::new("bio", TextInputStyle::Short),
    ))
}

/// Matrix 39: `show_modal` after another handle acknowledged (the gate
/// race) is `ModalUnavailable` and sends no type-9 callback.
#[tokio::test]
async fn show_modal_after_an_ack() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let manual: ComponentResponder<Manual> = Responder::new(Arc::clone(&shared));
    let _ = component(&shared).defer_update().await.unwrap().done();
    unanswer(&shared);
    let err = manual.show_modal(form()).await.unwrap_err();
    assert!(matches!(err, ResponderError::ModalUnavailable), "{err}");
    assert_eq!(fx.requests().len(), 1);

    clear(&fx);
    let shared = fx.component(Declared::Update, v2_source());
    let manual: ComponentResponder<Manual> = Responder::new(Arc::clone(&shared));
    let _ = manual.show_modal(form()).await.unwrap().done();
    let requests = fx.requests();
    assert_eq!(requests[0].2["type"], 9);
    assert_eq!(requests[0].2["data"]["custom_id"], "0form:1");
    assert_eq!(requests[0].2["data"]["title"], "Form");
}

/// Matrix 41: a slash-opened modal submit has nothing to update.
#[tokio::test]
async fn update_unavailable_without_a_source() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, None);
    for err in [
        modal_responder(&shared).update(v2("a")).await.unwrap_err(),
        modal_responder(&shared).defer_update().await.unwrap_err(),
        modal_responder(&shared)
            .update_with_response(v2("a"))
            .await
            .unwrap_err(),
        modal_responder(&shared)
            .defer_update_with_response()
            .await
            .unwrap_err(),
    ] {
        assert!(matches!(err, ResponderError::UpdateUnavailable), "{err}");
    }
    assert!(fx.requests().is_empty());
    assert_eq!(shared.state(), PENDING);
    assert_eq!(
        ResponderError::UpdateUnavailable.to_string(),
        "there is no message to update: this modal was not opened from a component message"
    );
    let _ = modal_responder(&shared)
        .reply(v2("a"))
        .await
        .unwrap()
        .done();
    assert_eq!(fx.requests()[0].2["type"], 4);
}

/// Story 7.8 row 14: a modal submit's state refuses `show_modal` before
/// resolving, validating or locking, and sends nothing (INT-04).
#[tokio::test]
async fn submit_refuses_show_modal() {
    let fx = Fixture::start(discord()).await;
    for (declared, source) in [
        (Declared::Update, v2_source()),
        (Declared::Update, None),
        (Declared::Reply(Ephemerality::Ephemeral), None),
        (Declared::Reply(Ephemerality::Public), v2_source()),
    ] {
        let shared = fx.submit(declared, source);
        let err = shared.show_modal(form()).await.unwrap_err();
        assert!(matches!(err, ResponderError::ModalUnavailable), "{err}");
        assert_eq!(shared.state(), PENDING);
    }
    assert!(fx.requests().is_empty());
    // The same state still answers through the modal responder.
    let shared = fx.submit(Declared::Update, v2_source());
    let _ = modal_responder(&shared)
        .update(v2("a"))
        .await
        .unwrap()
        .done();
    assert_eq!(fx.requests()[0].2["type"], 7);
}

/// Discord's answers, echoing the callback type in `resource` when
/// `with_response` is asked for; `bare` answers `{}` instead.
fn discord_resource(bare: bool) -> Answer {
    let fallback = discord();
    Arc::new(move |seen: &Seen| {
        if seen.path.ends_with("/callback") && seen.query.as_deref() == Some("with_response=true") {
            if bare {
                return (StatusCode::OK, Bytes::from_static(b"{}"));
            }
            let body: Value = serde_json::from_slice(&seen.body).unwrap();
            let message: Value = serde_json::from_slice(&message_json("88", "55")).unwrap();
            let answer = json!({ "resource": { "type": body["type"], "message": message } });
            (StatusCode::OK, Bytes::from(answer.to_string()))
        } else {
            fallback(seen)
        }
    })
}

/// Matrix 44 and 45: the `_with_response` variants.
#[tokio::test]
async fn with_response_variants() {
    let fx = Fixture::start(discord_resource(false)).await;
    let check = |resource: Option<CallbackResource>, kind: u8| {
        let resource = resource.expect("a callback was sent");
        assert_eq!(resource.kind(), kind);
        assert_eq!(resource.message().map(|m| m.id), Some(Id::new(88)));
    };
    let shared = fx.component(Declared::Update, v2_source());
    let (acked, resource) = component(&shared)
        .reply_with_response(v2("a"))
        .await
        .unwrap();
    check(resource, 4);
    let _ = acked.done();
    let shared = fx.component(Declared::Update, v2_source());
    let (acked, resource) = component(&shared)
        .defer_reply_with_response()
        .await
        .unwrap();
    check(resource, 5);
    let _ = acked.done();
    let shared = fx.component(Declared::Update, v2_source());
    let (acked, resource) = component(&shared)
        .update_with_response(v2("a"))
        .await
        .unwrap();
    check(resource, 7);
    let _ = acked.done();
    let shared = fx.component(Declared::Update, v2_source());
    let (acked, resource) = component(&shared)
        .defer_update_with_response()
        .await
        .unwrap();
    check(resource, 6);
    let _ = acked.done();
    let requests = fx.requests();
    assert_eq!(requests.len(), 4);
    assert!(
        requests.iter().all(|(_, path, _)| path == WITH_RESPONSE),
        "{requests:?}"
    );

    // Matrix 45: after the auto-defer, an edit and no resource.
    clear(&fx);
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    let (acked, resource) = component(&shared)
        .update_with_response(v2("a"))
        .await
        .unwrap();
    assert!(resource.is_none());
    let _ = acked.done();
    assert_eq!(fx.requests()[1].0, Method::PATCH);
    assert_no_bot_token(&fx);

    // A body without `resource` is `Transport`.
    let fx = Fixture::start(discord_resource(true)).await;
    let shared = fx.component(Declared::Update, v2_source());
    let err = component(&shared)
        .update_with_response(v2("a"))
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::Transport), "{err}");
}

/// Matrix 47 and 48: follow-ups after an `update`, and the token expiry.
#[tokio::test]
async fn follow_ups_and_expiry_after_update() {
    let fx = Fixture::start_with_token(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component(&shared).update(v2("a")).await.unwrap();
    let _ = acked
        .follow_up(v2("f"), Visibility::Ephemeral)
        .await
        .unwrap();
    let requests = fx.requests();
    assert_eq!(
        (requests[1].0.clone(), requests[1].1.as_str()),
        (Method::POST, FOLLOWUP)
    );
    assert_eq!(requests[1].2["flags"], 32832);
    skip(Duration::from_mins(15)).await;
    acked.edit_reply(v2("b")).await.unwrap();
    assert_eq!(
        fx.requests()[2],
        (
            Method::PATCH,
            "/api/v10/channels/55/messages/77".to_owned(),
            update_data("b")
        )
    );
    // S19: the bot-token channel edit carries `Bot …` (INT-02).
    assert_bot_auth(&fx.server.seen()[2]);
    let err = acked
        .follow_up(v2("f"), Visibility::Public)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    let _ = acked.done();

    clear(&fx);
    let shared = fx.component(
        Declared::Update,
        source(MessageFormat::V2, Ephemerality::Ephemeral),
    );
    let acked = component(&shared).defer_update().await.unwrap();
    skip(Duration::from_mins(15)).await;
    let err = acked.edit_reply(v2("b")).await.unwrap_err();
    assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    let _ = acked.done();
    assert_eq!(fx.requests().len(), 1);
}

/// The notice after a type-6 acknowledgement never deletes the clicked
/// message: an ephemeral follow-up.
#[tokio::test]
async fn notice_after_defer_update_is_a_follow_up() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    shared.notice("oops".to_owned()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        (requests[1].0.clone(), requests[1].1.as_str()),
        (Method::POST, FOLLOWUP)
    );
    assert_eq!(requests[1].2["flags"], 32832);
}

/// A key ring whose current key is `[0xab; 32]`.
fn key() -> PayloadKey {
    PayloadKey::from_bytes([0xab; 32])
}

fn button<R: noctis_ui::__macro::ComponentValue>(route: R) -> noctis_ui::RouteButton<R> {
    build_button(
        route,
        &ButtonDefaults {
            style: ButtonStyle::Primary,
            label: Some("b"),
            emoji: None,
            disabled: false,
        },
    )
}

const PICK: SelectDefaults = SelectDefaults {
    placeholder: Some("p"),
    min_values: None,
    max_values: None,
    disabled: false,
};

/// Matrix 17: a `signed` Route's id is signed with the App's key.
#[tokio::test]
async fn send_side_signing() {
    let fx = Fixture::start_keyed(discord(), key()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let msg = V2Message::new(Components::new().push(ActionRow::buttons([button(Sig { item: 5 })])));
    let _ = component(&shared).update(msg).await.unwrap().done();
    let id = fx.requests()[0].2["data"]["components"][0]["components"][0]["custom_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let decoded = noctis_codec::decode(&id).unwrap();
    let keys = noctis_codec::PayloadKeys::new(key());
    assert!(matches!(
        decoded.verified_payload::<Sig>(&keys),
        Ok(noctis_codec::PayloadSource::Inline(Sig { item: 5 }))
    ));
    // Without a key (a detached responder) it cannot be sent.
    let shared = Shared::new(Transport::Detached, Ephemerality::Public, Limits::default());
    let msg = V2Message::new(Components::new().push(ActionRow::buttons([button(Sig { item: 5 })])));
    let err = Responder::<Command>::new(shared)
        .reply(msg)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ResponderError::Encode { reason } if reason.contains("`sig`")),
        "{err}"
    );
}

/// Matrix 18: one nonce per send, shared by every select of the message.
#[tokio::test]
async fn one_select_nonce_per_send() {
    let fx = Fixture::start(discord()).await;
    let mut nonces = Vec::new();
    for _ in 0..3 {
        let shared = fx.component(Declared::Update, v2_source());
        let msg = V2Message::new(
            Components::new()
                .push(ActionRow::select(build_user_select(
                    Pick { item: 1 },
                    &PICK,
                )))
                .push(ActionRow::select(build_user_select(
                    Pick { item: 2 },
                    &PICK,
                ))),
        );
        let _ = component(&shared).update(msg).await.unwrap().done();
        let data = fx.requests().last().unwrap().2["data"].clone();
        let ids: Vec<String> = (0..2)
            .map(|i| {
                data["components"][i]["components"][0]["custom_id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        // `4pick:<nonce><item>`: the nonce is the two characters after `:`.
        let nonce = |id: &str| id.split_once(':').unwrap().1[..2].to_owned();
        assert_eq!(nonce(&ids[0]), nonce(&ids[1]), "{ids:?}");
        nonces.push(nonce(&ids[0]));
    }
    assert!(nonces.iter().any(|n| *n != nonces[0]), "{nonces:?}");
}

/// Matrix 19 (Story 7.5): a Ref slot is now minted and stored before the
/// callback (the full matrix is in `refs.rs`).
#[tokio::test]
async fn ref_slot_is_minted() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let msg =
        V2Message::new(Components::new().push(ActionRow::buttons([button(Stored { item: 1 })])));
    let _ = component(&shared).update(msg).await.unwrap().done();
    let id = fx.requests()[0].2["data"]["components"][0]["components"][0]["custom_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(id.starts_with("0stored:~"), "{id}");
    assert_eq!(id.len(), "0stored:~".len() + 11, "{id}");
}

/// The source message is read from the raw interaction JSON.
#[test]
fn source_message_from_raw_json() {
    let read = |message: Value| {
        SourceMessage::from_interaction(
            json!({ "type": 3, "message": message })
                .to_string()
                .as_bytes(),
        )
    };
    assert_eq!(
        read(
            json!({ "id": "77", "channel_id": "55", "flags": 32768, "components": [{ "type": 17 }] })
        ),
        v2_source()
    );
    assert_eq!(
        read(json!({ "id": "77", "channel_id": "55", "flags": 32832 })),
        source(MessageFormat::V2, Ephemerality::Ephemeral)
    );
    assert_eq!(
        read(json!({ "id": "77", "channel_id": "55" })),
        source(MessageFormat::Legacy, Ephemerality::Public)
    );
    assert_eq!(read(json!({ "id": "x", "channel_id": "55" })), None);
    // The select nonces of the whole interaction are collected.
    let raw = json!({
        "type": 3,
        "data": { "custom_id": "0form:1" },
        "message": { "id": "77", "channel_id": "55", "components": [
            { "type": 1, "components": [{ "type": 5, "custom_id": "4pick:cd1" }] },
        ] },
    });
    let found = SourceMessage::from_interaction(raw.to_string().as_bytes()).unwrap();
    let nonces: Vec<String> = found.nonces.iter().map(|n| n.to_string()).collect();
    assert_eq!(nonces, ["cd"]);
    assert_eq!(SourceMessage::from_interaction(b"{\"type\":5}"), None);
    assert_eq!(SourceMessage::from_interaction(b"not json"), None);
}

/// A message of one User Select of `Pick { item }`.
fn select_msg(item: u64) -> V2Message {
    V2Message::new(
        Components::new().push(ActionRow::select(build_user_select(Pick { item }, &PICK))),
    )
}

/// The nonce of every select id (`4pick:<nonce><item>`) anywhere in `body`.
fn select_nonces(body: &Value) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(id) = map.get("custom_id").and_then(Value::as_str)
                    && let Some(rest) = id.strip_prefix("4pick:")
                {
                    out.push(rest[..2].to_owned());
                }
                map.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(body, &mut out);
    out
}

/// The component message 77 as read from a raw select click whose ids carry
/// nonce `ab` (in `data` and in the message's select).
fn source_with_nonce_ab() -> Option<SourceMessage> {
    let raw = json!({
        "type": 3,
        "data": { "custom_id": "4pick:ab1", "component_type": 5, "values": ["1"] },
        "message": {
            "id": "77", "channel_id": "55", "flags": 32768,
            "components": [{ "type": 1, "components": [{ "type": 5, "custom_id": "4pick:ab1" }] }],
        },
    });
    let source = SourceMessage::from_interaction(raw.to_string().as_bytes());
    assert_eq!(
        source
            .as_ref()
            .map(|s| s.nonces.iter().map(|n| n.to_string()).collect::<Vec<_>>()),
        Some(vec!["ab".to_owned()])
    );
    source
}

/// FR-25 (R1, R4): `update` never reuses the nonce the clicked message
/// carries, as type 7 and as `PATCH @original` after the auto-defer.
#[tokio::test]
async fn fr_25_update_avoids_the_source_nonce() {
    let fx = Fixture::start(discord()).await;
    for _ in 0..64 {
        clear(&fx);
        let shared = fx.component(Declared::Update, source_with_nonce_ab());
        let _ = component(&shared)
            .update(select_msg(1))
            .await
            .unwrap()
            .done();
        let requests = fx.requests();
        assert_eq!(requests[0].2["type"], 7);
        let nonces = select_nonces(&requests[0].2);
        assert_eq!(nonces.len(), 1);
        assert_ne!(nonces[0], "ab");

        clear(&fx);
        let shared = fx.component(Declared::Update, source_with_nonce_ab());
        assert!(shared.auto_defer().await);
        let _ = component(&shared)
            .update(select_msg(1))
            .await
            .unwrap()
            .done();
        let requests = fx.requests();
        assert_eq!(
            (requests[1].0.clone(), requests[1].1.as_str()),
            (Method::PATCH, ORIGINAL)
        );
        let nonces = select_nonces(&requests[1].2);
        assert_eq!(nonces.len(), 1);
        assert_ne!(nonces[0], "ab");
    }
}

/// FR-25 (R3): every send of one run draws a nonce no earlier send drew.
#[tokio::test]
async fn fr_25_each_send_of_a_run_gets_a_new_nonce() {
    let fx = Fixture::start(discord()).await;
    for _ in 0..16 {
        clear(&fx);
        let shared = fx.component(Declared::Update, source_with_nonce_ab());
        let acked = component(&shared).update(select_msg(1)).await.unwrap();
        acked.edit_reply(select_msg(1)).await.unwrap();
        let _ = acked
            .follow_up(select_msg(1), Visibility::Public)
            .await
            .unwrap();
        let _ = acked.done();
        let mut seen: Vec<String> = fx
            .requests()
            .iter()
            .flat_map(|(_, _, body)| select_nonces(body))
            .collect();
        assert_eq!(seen.len(), 3);
        seen.push("ab".to_owned());
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 4, "{seen:?}");
    }
}

/// FR-25 (R5): a message without selects draws no nonce, even when its
/// ids are unresolved (signed, by Ref).
#[tokio::test]
async fn fr_25_no_select_no_nonce() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, source_with_nonce_ab());
    let msg = V2Message::new(
        Components::new()
            .push(ActionRow::buttons([button(Form { item: 1 })]))
            .push(ActionRow::buttons([button(Stored { item: 1 })])),
    );
    let _ = component(&shared).update(msg).await.unwrap().done();
    assert_eq!(*shared.drawn.lock().unwrap(), NonceSet::EMPTY);
    assert!(select_nonces(&fx.requests()[0].2).is_empty());
}

/// The nonce `ab` as the codec reads it.
fn nonce_ab() -> SelectNonce {
    noctis_codec::decode("4pick:ab1").unwrap().nonce().unwrap()
}

/// A scripted fill yielding `values` in order.
fn scripted(values: Vec<u16>) -> ScriptedFill {
    let mut it = values.into_iter();
    Box::new(move |bytes| {
        *bytes = it.next().unwrap().to_le_bytes();
        Ok(())
    })
}

/// FR-25: the avoid side holds the interaction's nonce from the start and
/// every sent nonce after the send.
#[tokio::test]
async fn fr_25_avoid_side_holds_source_and_sent_nonces() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, source_with_nonce_ab());
    assert!(shared.avoids(nonce_ab()));
    let _ = component(&shared)
        .update(select_msg(1))
        .await
        .unwrap()
        .done();
    let sent = select_nonces(&fx.requests()[0].2);
    assert_eq!(sent.len(), 1);
    assert!(shared.avoids(nonce_ab()));
    let drawn: Vec<String> = shared
        .drawn
        .lock()
        .unwrap()
        .iter()
        .map(|n| n.to_string())
        .collect();
    assert_eq!(drawn, sent);
}

/// FR-25: a draw that first yields the source nonce `ab` sends the next one.
#[tokio::test]
async fn fr_25_scripted_draw_skips_the_source_nonce() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, source_with_nonce_ab());
    let ab = nonce_ab();
    let x = SelectNonce::new((ab.get() + 1) % (SelectNonce::MAX + 1)).unwrap();
    shared.script_fill(scripted(vec![ab.get(), x.get()]));
    let _ = component(&shared)
        .update(select_msg(1))
        .await
        .unwrap()
        .done();
    assert_eq!(select_nonces(&fx.requests()[0].2), [x.to_string()]);
}

/// FR-25: more draws than the ring holds never evict the interaction's
/// nonce.
#[tokio::test]
async fn fr_25_many_draws_keep_avoiding_the_source_nonce() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, source_with_nonce_ab());
    let ab = nonce_ab();
    let others: Vec<u16> = (0..=SelectNonce::MAX)
        .filter(|&v| v != ab.get())
        .take(12)
        .collect();
    shared.script_fill(scripted(others.clone()));
    for _ in &others {
        shared.draw_nonce().unwrap();
    }
    assert!(shared.avoids(ab));
    let x = others[0];
    shared.script_fill(scripted(vec![ab.get(), x]));
    assert_eq!(shared.draw_nonce().unwrap().get(), x);
}

/// C7: two equal select Routes resolve to one id (one nonce per send), so
/// the send is CID-02 `Invalid` with the resolved id and sends nothing.
#[tokio::test]
async fn cid_02_duplicate_after_resolution_sends_nothing() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let msg = V2Message::new(
        Components::new()
            .push(ActionRow::select(build_user_select(
                Pick { item: 1 },
                &PICK,
            )))
            .push(ActionRow::select(build_user_select(
                Pick { item: 1 },
                &PICK,
            ))),
    );
    let err = component(&shared).update(msg).await.unwrap_err();
    let ResponderError::Invalid(UiError::Limits { violations }) = &err else {
        panic!("expected Invalid, got {err}");
    };
    assert_eq!(violations.len(), 1, "{violations:?}");
    let x = &violations[0];
    assert_eq!(x.rule, RuleId::CID_02);
    assert_eq!(x.path, "row[1].select[0].custom_id");
    assert!(
        matches!(&x.actual, noctis_rules::Actual::Text(id) if id.starts_with("4pick:")),
        "{:?}",
        x.actual
    );
    assert!(x.help.is_some());
    assert!(fx.requests().is_empty());
}

/// A request the bot token authorizes: `authorization: Bot …`.
fn assert_bot_auth(seen: &Seen) {
    let auth = seen
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(auth.starts_with("Bot "), "{} has no bot token", seen.path);
}

/// The last request: method, path with query, and whether it carried an
/// `authorization` header.
fn last(fx: &Fixture) -> (Method, String, bool) {
    let seen = fx.server.seen();
    let s = seen.last().expect("a request was sent");
    let path = match &s.query {
        Some(q) => format!("{}?{q}", s.path),
        None => s.path.clone(),
    };
    (
        s.method.clone(),
        path,
        s.headers.get("authorization").is_some(),
    )
}

const SOURCE_CHANNEL_MESSAGE: &str = "/api/v10/channels/55/messages/77";

/// S9-S12: after `update` or `defer_update`, `fetch_reply` and
/// `delete_reply` act on the clicked message over the interaction token.
#[tokio::test]
async fn fetch_and_delete_after_update() {
    let fx = Fixture::start_with_token(discord_fetch()).await;

    // S9: update, then fetch.
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component(&shared).update(v2("a")).await.unwrap();
    assert_eq!(fx.requests()[0].2["type"], 7);
    let message = acked.fetch_reply().await.unwrap();
    assert_eq!(message.id.get(), 77);
    assert_eq!(last(&fx), (Method::GET, ORIGINAL.to_owned(), false));
    // S12: update, then delete.
    acked.delete_reply().await.unwrap();
    assert_eq!(last(&fx), (Method::DELETE, ORIGINAL.to_owned(), false));
    let _ = acked.done();

    // S10 and S11: defer_update, then fetch and delete.
    clear(&fx);
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component(&shared).defer_update().await.unwrap();
    assert_eq!(
        fx.requests(),
        [(Method::POST, UPDATE_CB.to_owned(), json!({ "type": 6 }))]
    );
    assert_eq!(acked.fetch_reply().await.unwrap().id.get(), 77);
    assert_eq!(last(&fx), (Method::GET, ORIGINAL.to_owned(), false));
    acked.delete_reply().await.unwrap();
    assert_eq!(last(&fx), (Method::DELETE, ORIGINAL.to_owned(), false));
    let _ = acked.done();
    assert_eq!(fx.requests().len(), 3);
    assert_no_bot_token(&fx);
}

/// S13-S15 (INT-02): after 15 minutes a public source is fetched and
/// deleted with the bot token; an ephemeral source is `TokenExpired` with
/// no request.
#[tokio::test]
async fn fetch_and_delete_after_the_token_expired() {
    const T: Duration = Duration::from_mins(15);
    let fx = Fixture::start_with_token(discord_fetch()).await;

    // S13: delete after `update` on a public V2 source.
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component(&shared).update(v2("a")).await.unwrap();
    skip(T).await;
    acked.delete_reply().await.unwrap();
    assert_eq!(
        last(&fx),
        (Method::DELETE, SOURCE_CHANNEL_MESSAGE.to_owned(), true)
    );
    assert_bot_auth(fx.server.seen().last().unwrap());
    let _ = acked.done();

    // S15: fetch after `defer_update` on a public source.
    clear(&fx);
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component(&shared).defer_update().await.unwrap();
    skip(T).await;
    assert_eq!(acked.fetch_reply().await.unwrap().id.get(), 77);
    assert_eq!(
        last(&fx),
        (Method::GET, SOURCE_CHANNEL_MESSAGE.to_owned(), true)
    );
    assert_bot_auth(fx.server.seen().last().unwrap());
    let _ = acked.done();

    // S14: an ephemeral source after 15 minutes.
    clear(&fx);
    let shared = fx.component(
        Declared::Update,
        source(MessageFormat::V2, Ephemerality::Ephemeral),
    );
    let acked = component(&shared).update(v2("a")).await.unwrap();
    skip(T).await;
    for err in [
        acked.delete_reply().await.unwrap_err(),
        acked.fetch_reply().await.unwrap_err(),
        acked.edit_reply(v2("b")).await.unwrap_err(),
    ] {
        assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    }
    let _ = acked.done();
    assert_eq!(fx.requests().len(), 1, "only the type-7 callback");
}

/// S16: a modal opened from a component message fetches and deletes that
/// message after `update`; a slash-opened modal after its type-5
/// auto-defer acts on the deferred reply's `@original`.
#[tokio::test]
async fn modal_fetch_and_delete() {
    let fx = Fixture::start_with_token(discord_fetch()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let acked = modal_responder(&shared).update(v2("a")).await.unwrap();
    assert_eq!(fx.requests()[0].2["type"], 7);
    assert_eq!(acked.fetch_reply().await.unwrap().id.get(), 77);
    assert_eq!(last(&fx), (Method::GET, ORIGINAL.to_owned(), false));
    acked.delete_reply().await.unwrap();
    assert_eq!(last(&fx), (Method::DELETE, ORIGINAL.to_owned(), false));
    let _ = acked.done();

    clear(&fx);
    let shared = fx.component(Declared::Update, None);
    assert!(shared.auto_defer().await);
    assert_eq!(
        fx.requests(),
        [(Method::POST, UPDATE_CB.to_owned(), json!({ "type": 5 }))]
    );
    let acked: ModalResponder<AutoDefer> = Responder::new(Arc::clone(&shared));
    let acked = acked.acked();
    assert_eq!(acked.fetch_reply().await.unwrap().id.get(), 77);
    assert_eq!(last(&fx), (Method::GET, ORIGINAL.to_owned(), false));
    acked.delete_reply().await.unwrap();
    assert_eq!(last(&fx), (Method::DELETE, ORIGINAL.to_owned(), false));
    let _ = acked.done();
    assert_no_bot_token(&fx);
}

/// Story 9.3: `shop.buy` is `Al` in Turkish, `Kaufen` in German and `Buy`
/// in en-US; `shop.long` is 81 characters; anything else is missing.
struct Shop;

impl Translate for Shop {
    fn translate(
        &self,
        locale: noctis_rules::Locale,
        key: MessageKey,
        _: &[(&str, &dyn core::fmt::Display)],
    ) -> Option<String> {
        match (key.as_str(), format!("{locale:?}").as_str()) {
            ("shop.buy", "Tr") => Some("Al".to_owned()),
            ("shop.buy", "De") => Some("Kaufen".to_owned()),
            ("shop.buy", _) => Some("Buy".to_owned()),
            ("shop.long", _) => Some("l".repeat(81)),
            _ => None,
        }
    }
}

/// A V2 message with one Route button labelled by the key `key`.
fn keyed(key: &'static str) -> V2Message {
    V2Message::new(Components::new().push(ActionRow::buttons([
        button(Form { item: 1 }).label(MessageKey::new(key)),
    ])))
}

/// The label of the first button of a sent message body.
fn sent_label(body: &Value) -> Value {
    body["data"]["components"][0]["components"][0]["label"].clone()
}

fn with_locales(shared: Arc<Shared>, user: Option<&str>, guild: Option<&str>) -> Arc<Shared> {
    shared.set_i18n(
        Arc::new(Shop),
        user.map(str::to_owned),
        guild.map(str::to_owned),
    );
    shared
}

/// T3: keys resolve in the response locale: User first for an ephemeral
/// send or a modal, Guild first for a public send, en-US without a
/// parsable locale.
#[tokio::test]
async fn t3_keys_resolve_in_the_response_locale() {
    let fx = Fixture::start(discord()).await;
    let cases = [
        (
            Declared::Reply(Ephemerality::Ephemeral),
            v2_source(),
            true,
            "Al",
        ),
        (
            Declared::Reply(Ephemerality::Public),
            v2_source(),
            true,
            "Kaufen",
        ),
        (
            Declared::Update,
            source(MessageFormat::V2, Ephemerality::Ephemeral),
            false,
            "Al",
        ),
        (Declared::Update, v2_source(), false, "Kaufen"),
    ];
    for (declared, src, reply, expected) in cases {
        clear(&fx);
        let shared = with_locales(fx.component(declared, src), Some("tr"), Some("de"));
        let r = component(&shared);
        let _ = if reply {
            r.reply(keyed("shop.buy")).await.unwrap().done()
        } else {
            r.update(keyed("shop.buy")).await.unwrap().done()
        };
        let requests = fx.requests();
        assert_eq!(sent_label(&requests[0].2), expected, "{declared:?}");
    }

    clear(&fx);
    let shared = with_locales(
        fx.component(Declared::Reply(Ephemerality::Public), v2_source()),
        Some("tr"),
        Some("de"),
    );
    let manual: ComponentResponder<Manual> = Responder::new(Arc::clone(&shared));
    let modal =
        noctis_ui::__macro::build_modal::<Form>(Form { item: 1 }, MessageKey::new("shop.buy"))
            .push(ModalLabel::new(
                "Bio",
                TextInput::new("bio", TextInputStyle::Short),
            ));
    let _ = manual.show_modal(modal).await.unwrap().done();
    assert_eq!(fx.requests()[0].2["data"]["title"], "Al");

    clear(&fx);
    let shared = with_locales(
        fx.component(Declared::Reply(Ephemerality::Ephemeral), v2_source()),
        Some("xx"),
        None,
    );
    let _ = component(&shared)
        .reply(keyed("shop.buy"))
        .await
        .unwrap()
        .done();
    assert_eq!(sent_label(&fx.requests()[0].2), "Buy");

    t3_later_sends(&fx).await;
}

/// The label of the first button of a webhook (follow-up or edit) body.
fn body_label(body: &Value) -> Value {
    body["components"][0]["components"][0]["label"].clone()
}

/// T3 for the sends after the first response: follow-ups, edits of a
/// deferred response, an update after a type-6 defer and a follow-up edit.
async fn t3_later_sends(fx: &Fixture) {
    let tr_de = |shared| with_locales(shared, Some("tr"), Some("de"));

    // After a public reply: an ephemeral and a public follow-up, then an
    // edit of the ephemeral one.
    clear(fx);
    let shared = tr_de(fx.component(Declared::Reply(Ephemerality::Public), v2_source()));
    let acked = component(&shared).reply(v2("a")).await.unwrap();
    let private = acked
        .follow_up(keyed("shop.buy"), Visibility::Ephemeral)
        .await
        .unwrap();
    let _public = acked
        .follow_up(keyed("shop.buy"), Visibility::Public)
        .await
        .unwrap();
    private.edit(keyed("shop.buy")).await.unwrap();
    let _ = acked.done();
    let requests = fx.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_eq!(body_label(&requests[1].2), "Al", "ephemeral follow-up");
    assert_eq!(body_label(&requests[2].2), "Kaufen", "public follow-up");
    assert_eq!(body_label(&requests[3].2), "Al", "follow-up edit");

    // An ephemeral type-5 defer, then `reply` or `ctx.respond`: an edit.
    for respond in [false, true] {
        clear(fx);
        let shared = tr_de(fx.component(Declared::Reply(Ephemerality::Ephemeral), v2_source()));
        assert!(shared.auto_defer().await);
        if respond {
            shared.ctx_respond(keyed("shop.buy").into()).await.unwrap();
        } else {
            let _ = component(&shared)
                .reply(keyed("shop.buy"))
                .await
                .unwrap()
                .done();
        }
        let requests = fx.requests();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(requests[1].0, Method::PATCH);
        assert_eq!(body_label(&requests[1].2), "Al", "respond: {respond}");
    }

    // A type-6 defer of an ephemeral source, then `update`: an edit.
    clear(fx);
    let shared = tr_de(fx.component(
        Declared::Update,
        source(MessageFormat::V2, Ephemerality::Ephemeral),
    ));
    assert!(shared.auto_defer().await);
    let _ = component(&shared)
        .update(keyed("shop.buy"))
        .await
        .unwrap()
        .done();
    let requests = fx.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[1].0, Method::PATCH);
    assert_eq!(body_label(&requests[1].2), "Al", "update after type 6");
}

/// T4: a key with no translation is sent as the key, with one `warn` on
/// `noctis::i18n`.
#[tokio::test]
async fn t4_missing_key_sends_the_key() {
    let fx = Fixture::start(discord()).await;
    let shared = with_locales(
        fx.component(Declared::Reply(Ephemerality::Public), v2_source()),
        Some("tr"),
        None,
    );
    let capture = crate::test_support::Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let _ = component(&shared)
        .reply(keyed("shop.missing"))
        .await
        .unwrap()
        .done();
    assert_eq!(sent_label(&fx.requests()[0].2), "shop.missing");
    let lines = capture.lines_with(&["WARN", "noctis::i18n", "shop.missing"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
}

/// T5: a translation over a limit is validated after resolution: Strict
/// is `Invalid` with no request, Lenient truncates to 80 ending `…`.
#[tokio::test]
async fn t5_resolved_text_is_validated() {
    let fx = Fixture::start(discord()).await;
    let shared = with_locales(
        fx.component(Declared::Reply(Ephemerality::Public), v2_source()),
        None,
        None,
    );
    let err = component(&shared)
        .reply(keyed("shop.long"))
        .await
        .unwrap_err();
    let ResponderError::Invalid(UiError::Limits { violations }) = &err else {
        panic!("{err}");
    };
    assert_eq!(violations[0].rule, RuleId::CV2_08);
    assert!(fx.requests().is_empty());

    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
        channel_id: Some(Id::new(9)),
        file_limit: None,
    });
    let mut limits = fx.app.limits().clone();
    limits.mode = LimitMode::Lenient;
    let shared = with_locales(
        Shared::declared(
            transport,
            Declared::Reply(Ephemerality::Public),
            v2_source(),
            limits,
            None,
        ),
        None,
        None,
    );
    let _ = component(&shared)
        .reply(keyed("shop.long"))
        .await
        .unwrap()
        .done();
    let label = sent_label(&fx.requests()[0].2);
    let label = label.as_str().unwrap();
    assert_eq!(label.chars().count(), 80);
    assert!(label.ends_with('…'), "{label}");
}

/// A component Responder's state on the fixture in limit mode `mode`.
fn shared_in(fx: &Fixture, mode: LimitMode, declared: Declared) -> Arc<Shared> {
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
        channel_id: Some(Id::new(9)),
        file_limit: None,
    });
    let mut limits = fx.app.limits().clone();
    limits.mode = mode;
    Shared::declared(transport, declared, v2_source(), limits, None)
}

/// M17, M20: `show_modal` takes `impl Into<Modal>` and validates before
/// sending, in every mode: a value over 4000 or its `max_length` (MOD-05),
/// a Checkbox Group over `max_values` or a Radio Group with two defaults
/// (MOD-07) and a modal select over `max_values` (CV2-09) are `Invalid`
/// with no callback.
#[tokio::test]
async fn m17_show_modal_validates_before_sending() {
    use noctis_ui::{CheckboxGroup, ModalInput, ModalOption, ModalUserSelect, RadioGroup};

    fn modal(input: impl ModalInput) -> Modal {
        noctis_ui::__macro::build_modal::<Form>(Form { item: 1 }, "Form")
            .push(ModalLabel::new("Input", input))
    }
    fn options() -> [ModalOption; 3] {
        ["a", "b", "c"].map(|v| ModalOption::new(v.to_uppercase(), v).default(true))
    }
    let cases: [(fn() -> Modal, RuleId); 5] = [
        (
            || modal(TextInput::new("bio", TextInputStyle::Paragraph).value("x".repeat(4001))),
            RuleId::MOD_05,
        ),
        (
            || {
                modal(
                    TextInput::new("bio", TextInputStyle::Short)
                        .max_length(5)
                        .value("toolong"),
                )
            },
            RuleId::MOD_05,
        ),
        (
            || modal(CheckboxGroup::new("pick", options()).max_values(2)),
            RuleId::MOD_07,
        ),
        (
            || {
                modal(RadioGroup::new(
                    "one",
                    [
                        ModalOption::new("A", "a").default(true),
                        ModalOption::new("B", "b").default(true),
                    ],
                ))
            },
            RuleId::MOD_07,
        ),
        (
            || {
                modal(
                    ModalUserSelect::new("who")
                        .max_values(3)
                        .default_users([1, 2, 3, 4].map(Id::new)),
                )
            },
            RuleId::CV2_09,
        ),
    ];
    let fx = Fixture::start(discord()).await;
    for mode in [LimitMode::Strict, LimitMode::Lenient] {
        for (i, (build, rule)) in cases.iter().enumerate() {
            clear(&fx);
            let shared = shared_in(&fx, mode, Declared::Update);
            let manual: ComponentResponder<Manual> = Responder::new(shared);
            let err = manual.show_modal(build()).await.unwrap_err();
            let ResponderError::Invalid(UiError::Limits { violations }) = &err else {
                panic!("{mode:?} case {i}: {err}");
            };
            assert_eq!(violations[0].rule, *rule, "{mode:?} case {i}");
            assert!(fx.requests().is_empty(), "{mode:?} case {i}");
        }
    }
}
