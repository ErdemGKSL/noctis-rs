//! Story 9.1 Matrix 1-18, 21 and 27: attachments on the wire (FR-30;
//! ATT-01, ATT-04, ATT-05, MSG-08, INT-13).

use std::collections::HashMap;
use std::sync::Mutex;

use noctis_ui::{
    Attachment, AttachmentRef, AttachmentSet, MediaGallery, MediaItem, Section, Thumbnail,
};

use super::*;
use crate::test_support::Capture;

/// One multipart part: its `name`, `filename` and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) name: String,
    pub(crate) filename: Option<String>,
    pub(crate) body: Vec<u8>,
}

/// Every occurrence of `needle` in `hay`.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    hay.windows(needle.len())
        .enumerate()
        .filter_map(|(i, w)| (w == needle).then_some(i))
        .collect()
}

/// The parts of a `multipart/form-data` request, or `None` for any other
/// content type.
pub(crate) fn parts(seen: &Seen) -> Option<Vec<Part>> {
    let content_type = seen.headers.get("content-type")?.to_str().ok()?;
    let boundary = content_type.strip_prefix("multipart/form-data; boundary=")?;
    let delimiter = format!("--{boundary}");
    let at = find_all(&seen.body, delimiter.as_bytes());
    let mut out = Vec::new();
    for pair in at.windows(2) {
        let segment = &seen.body[pair[0] + delimiter.len()..pair[1]];
        let segment = segment.strip_prefix(b"\r\n").unwrap();
        let split = find_all(segment, b"\r\n\r\n")[0];
        let headers = std::str::from_utf8(&segment[..split]).unwrap();
        let body = segment[split + 4..].strip_suffix(b"\r\n").unwrap().to_vec();
        let field = |key: &str| {
            let start = headers.find(&format!("{key}=\""))? + key.len() + 2;
            let end = headers[start..].find('"')? + start;
            Some(headers[start..end].to_owned())
        };
        out.push(Part {
            name: field("name").unwrap(),
            filename: field("filename"),
            body,
        });
    }
    Some(out)
}

/// The JSON of a request: its body, or its `payload_json` part.
pub(crate) fn payload(seen: &Seen) -> Value {
    match parts(seen) {
        Some(parts) => {
            assert_eq!(parts[0].name, "payload_json");
            serde_json::from_slice(&parts[0].body).unwrap()
        }
        None => serde_json::from_slice(&seen.body).unwrap_or(Value::Null),
    }
}

/// The uploaded parts of a request (`files[n]`), empty for JSON.
pub(crate) fn uploads(seen: &Seen) -> Vec<Part> {
    parts(seen).map_or_else(Vec::new, |p| p.into_iter().skip(1).collect())
}

/// A small Discord: every message answer echoes the request's attachments
/// with ids (kept entries keep theirs, new ones get 21, 22, … in order);
/// a callback answers with its message only `with_response`.
fn discord_files() -> Answer {
    let names: Arc<Mutex<HashMap<String, String>>> = Arc::default();
    let next = Arc::new(Mutex::new(21_u64));
    Arc::new(move |seen: &Seen| {
        let json = payload(seen);
        let message = |body: &Value, id: &str| {
            let list = body.get("attachments").and_then(Value::as_array);
            let attachments: Vec<Value> = list
                .into_iter()
                .flatten()
                .map(|entry| {
                    let mut names = names.lock().unwrap();
                    if let Some(kept) = entry["id"].as_str() {
                        let filename = names.get(kept).cloned().unwrap_or_default();
                        json!({ "id": kept, "filename": filename })
                    } else {
                        let mut next = next.lock().unwrap();
                        let id = next.to_string();
                        *next += 1;
                        let filename = entry["filename"].as_str().unwrap_or_default();
                        names.insert(id.clone(), filename.to_owned());
                        json!({ "id": id, "filename": filename })
                    }
                })
                .collect();
            json!({ "id": id, "channel_id": "9", "attachments": attachments })
        };
        let reply = |value: Value| (StatusCode::OK, Bytes::from(value.to_string()));
        if seen.path.ends_with("/callback") {
            if seen.query.as_deref() != Some("with_response=true") {
                return (StatusCode::NO_CONTENT, Bytes::new());
            }
            let kind = json["type"].clone();
            let mut resource = json!({ "type": kind });
            if matches!(kind.as_u64(), Some(4 | 7)) {
                resource["message"] = message(&json["data"], "77");
            }
            return reply(
                json!({ "interaction": { "id": "100", "type": 3 }, "resource": resource }),
            );
        }
        if seen.method == Method::DELETE {
            return (StatusCode::NO_CONTENT, Bytes::new());
        }
        if seen.method == Method::POST {
            return reply(message(&json, "55"));
        }
        let id = seen.path.rsplit('/').next().unwrap_or_default();
        let id = if id == "@original" { "77" } else { id };
        reply(message(&json, id))
    })
}

/// A plain PNG attachment of `bytes`.
fn png(bytes: &'static [u8]) -> Attachment {
    Attachment::new(bytes, "png").unwrap()
}

/// A V2 message showing every file of `files` in one gallery (none when
/// empty), with `extra` set members it does not reference.
fn view(files: &[Attachment], extra: &[Attachment]) -> V2Message {
    let mut set = AttachmentSet::new();
    let refs: Vec<AttachmentRef> = files.iter().map(|f| set.add(f.clone())).collect();
    for f in extra {
        set.add(f.clone());
    }
    let mut body = Components::new().push(TextDisplay::new("t"));
    if !refs.is_empty() {
        let items = refs.into_iter().map(|r| MediaItem::new(r).alt("x"));
        body = body.push(MediaGallery::try_new(items, &Limits::default()).unwrap());
    }
    V2Message::new(body).attachments(set)
}

fn responder(shared: &Arc<Shared>) -> CommandResponder {
    Responder::new(Arc::clone(shared))
}

fn component(shared: &Arc<Shared>) -> ComponentResponder {
    Responder::new(Arc::clone(shared))
}

/// The component message 77 in channel 9, holding `files` (`(name, id)`).
fn source_with(files: &[(&str, u64)]) -> Option<SourceMessage> {
    let attachments: Vec<Value> = files
        .iter()
        .map(|(name, id)| json!({ "id": id.to_string(), "filename": name }))
        .collect();
    let raw = json!({
        "type": 3,
        "message": { "id": "77", "channel_id": "9", "flags": 32768, "attachments": attachments },
    });
    SourceMessage::from_interaction(raw.to_string().as_bytes())
}

/// The last request.
fn last(fx: &Fixture) -> Seen {
    fx.server.seen().pop().unwrap()
}

fn query_path(seen: &Seen) -> String {
    match &seen.query {
        Some(q) => format!("{}?{q}", seen.path),
        None => seen.path.clone(),
    }
}

/// Matrix 1: no file, the JSON body of before, with no `attachments`.
#[tokio::test]
async fn att_04_no_file_reply_is_plain_json() {
    let fx = Fixture::start(discord_files()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared).reply(v2("hi")).await.unwrap().done();
    let seen = last(&fx);
    assert_eq!(seen.headers["content-type"], "application/json");
    assert_eq!(
        seen.body,
        Bytes::from(
            json!({ "type": 4, "data": { "flags": 32768, "components": [{ "type": 10, "id": 1, "content": "hi" }], "allowed_mentions": { "parse": [] } } })
                .to_string()
        )
    );
}

/// Matrix 2 and 3: a type-4 reply showing one image twice uploads it once,
/// as multipart, over the interaction route without the bot token.
#[tokio::test]
async fn att_04_reply_uploads_one_part() {
    let fx = Fixture::start_with_token(discord_files()).await;
    let a = png(b"a");
    let mut set = AttachmentSet::new();
    let r = set.add(a.clone());
    let body = Components::new()
        .push(MediaGallery::new([
            MediaItem::new(r.clone()).alt("A"),
            MediaItem::new(r.clone()).alt("A"),
        ]))
        .push(Section::new(
            [TextDisplay::new("t")],
            Thumbnail::new(r).alt("A"),
        ));
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared)
        .reply(V2Message::new(body).attachments(set))
        .await
        .unwrap()
        .done();
    let seen = fx.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(query_path(&seen[0]), CB_REPLY);
    assert!(seen[0].headers.get("authorization").is_none());
    assert_eq!(
        payload(&seen[0])["data"]["attachments"],
        json!([{ "id": 0, "filename": a.name() }])
    );
    assert_eq!(
        uploads(&seen[0]),
        [Part {
            name: "files[0]".to_owned(),
            filename: Some(a.name().to_owned()),
            body: b"a".to_vec(),
        }]
    );
    // The new id is learned from the resource.
    assert_eq!(shared.original().files.get(a.name()), Some(Id::new(21)));
}

/// Matrix 4 and 5: an unreferenced member is never sent (one debug line
/// names it); a file's alt is its `description`.
#[tokio::test]
async fn att_05_unreferenced_member_is_not_sent() {
    let fx = Fixture::start(discord_files()).await;
    let a = png(b"a").alt("file alt");
    let b = png(b"b");
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared)
        .reply(view(std::slice::from_ref(&a), std::slice::from_ref(&b)))
        .await
        .unwrap()
        .done();
    let seen = last(&fx);
    assert_eq!(
        payload(&seen)["data"]["attachments"],
        json!([{ "id": 0, "filename": a.name(), "description": "file alt" }])
    );
    assert_eq!(uploads(&seen).len(), 1);
    let lines = capture.lines_with(&["noctis::responder", b.name()]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
}

/// Matrix 6: a hand-written `attachment://` URL of a member is uploaded.
#[tokio::test]
async fn att_05_hand_written_url_is_uploaded() {
    let fx = Fixture::start(discord_files()).await;
    let a = png(b"a");
    let mut set = AttachmentSet::new();
    set.add(a.clone());
    let url = noctis_ui::Media::url(format!("attachment://{}", a.name()));
    let msg =
        V2Message::new(Components::new().push(MediaGallery::new([MediaItem::new(url).alt("A")])))
            .attachments(set);
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared).reply(msg).await.unwrap().done();
    assert_eq!(uploads(&last(&fx))[0].filename.as_deref(), Some(a.name()));
}

/// Matrix 7-9 and 14: edits keep known files by id, upload new ones and
/// drop the rest; a second edit keeps what the first uploaded.
#[tokio::test]
async fn msg_08_edits_keep_upload_and_drop() {
    let fx = Fixture::start(discord_files()).await;
    let (a, b) = (png(b"a"), png(b"b"));
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    // Matrix 7 and 14: every file known, JSON.
    acked
        .edit_reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(seen.headers["content-type"], "application/json");
    assert_eq!(payload(&seen)["attachments"], json!([{ "id": "21" }]));
    // Matrix 8: one kept, one new; names in order.
    let files = [a.clone(), b.clone()];
    acked.edit_reply(view(&files, &[])).await.unwrap();
    let seen = last(&fx);
    let mut want = [
        (a.name(), json!({ "id": "21" })),
        (b.name(), json!({ "id": 0, "filename": b.name() })),
    ];
    want.sort_by_key(|(name, _)| *name);
    let want: Vec<Value> = want.into_iter().map(|(_, v)| v).collect();
    assert_eq!(payload(&seen)["attachments"], Value::Array(want));
    assert_eq!(uploads(&seen).len(), 1);
    assert_eq!(uploads(&seen)[0].filename.as_deref(), Some(b.name()));
    // Matrix 8 (MSG-08): `a` no longer used is left out.
    acked
        .edit_reply(view(std::slice::from_ref(&b), &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(payload(&seen)["attachments"], json!([{ "id": "22" }]));
    // Matrix 9: no file, an empty list.
    acked.edit_reply(v2("none")).await.unwrap();
    assert_eq!(payload(&last(&fx))["attachments"], json!([]));
    let _ = acked.done();
}

/// Matrix 10: `update` keeps the clicked message's files by id, as JSON,
/// without `with_response`.
#[tokio::test]
async fn msg_08_update_keeps_source_files() {
    let fx = Fixture::start(discord_files()).await;
    let a = png(b"a");
    let shared = fx.component(Declared::Update, source_with(&[(a.name(), 11)]));
    let _ = component(&shared)
        .update(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap()
        .done();
    let seen = last(&fx);
    assert_eq!(query_path(&seen), CB);
    assert_eq!(seen.headers["content-type"], "application/json");
    let json = payload(&seen);
    assert_eq!(json["type"], 7);
    assert_eq!(json["data"]["attachments"], json!([{ "id": "11" }]));
}

/// Matrix 11: an `update` that uploads is multipart `with_response`, and
/// the new ids are learned.
#[tokio::test]
async fn att_04_update_uploads_with_response() {
    let fx = Fixture::start(discord_files()).await;
    let b = png(b"b");
    let shared = fx.component(Declared::Update, source_with(&[("old.png", 11)]));
    let acked = component(&shared)
        .update(view(std::slice::from_ref(&b), &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(query_path(&seen), CB_REPLY);
    assert_eq!(
        payload(&seen)["data"]["attachments"],
        json!([{ "id": 0, "filename": b.name() }])
    );
    assert_eq!(uploads(&seen)[0].body, b"b");
    assert_eq!(shared.original().files.get(b.name()), Some(Id::new(21)));
    acked
        .edit_reply(view(std::slice::from_ref(&b), &[]))
        .await
        .unwrap();
    assert_eq!(payload(&last(&fx))["attachments"], json!([{ "id": "21" }]));
    let _ = acked.done();
}

/// Matrix 12 and 13: after a type-5 defer nothing is known; after a type-6
/// defer the clicked message's files are.
#[tokio::test]
async fn msg_08_edit_after_defers() {
    let fx = Fixture::start(discord_files()).await;
    let a = png(b"a");
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).defer_reply().await.unwrap();
    acked
        .edit_reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    assert_eq!(
        payload(&last(&fx))["attachments"],
        json!([{ "id": 0, "filename": a.name() }])
    );
    let _ = acked.done();

    let shared = fx.component(Declared::Update, source_with(&[(a.name(), 11)]));
    let acked = component(&shared).defer_update().await.unwrap();
    acked
        .edit_reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(payload(&seen)["attachments"], json!([{ "id": "11" }]));
    assert!(uploads(&seen).is_empty());
    let _ = acked.done();
}

/// Matrix 15: a response that names no message leaves only the kept
/// entries known, so a new upload is uploaded again.
#[tokio::test]
async fn msg_08_unknown_ids_upload_again() {
    let fx = Fixture::start(discord()).await;
    let a = png(b"a");
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    acked
        .edit_reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(
        payload(&seen)["attachments"],
        json!([{ "id": 0, "filename": a.name() }])
    );
    assert_eq!(uploads(&seen).len(), 1);
    let _ = acked.done();

    // Detached: the budget still runs, nothing is learned.
    let detached = Shared::new(Transport::Detached, Ephemerality::Public, Limits::default());
    let acked = responder(&detached)
        .reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    assert_eq!(detached.original().files, KnownFiles::EMPTY);
    let _ = acked.done();
}

/// Matrix 16: a follow-up learns its ids; its edit keeps them.
#[tokio::test]
async fn msg_08_follow_up_edit_keeps_its_files() {
    let fx = Fixture::start(discord_files()).await;
    let a = png(b"a");
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("r")).await.unwrap();
    let handle = acked
        .follow_up(view(std::slice::from_ref(&a), &[]), Visibility::Public)
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(query_path(&seen), FOLLOWUP);
    assert!(seen.headers.get("authorization").is_none());
    assert_eq!(uploads(&seen).len(), 1);
    handle
        .edit(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(seen.path, "/api/v10/webhooks/7/tok/messages/55");
    assert_eq!(payload(&seen)["attachments"], json!([{ "id": "21" }]));
    assert!(uploads(&seen).is_empty());
    let _ = acked.done();
}

/// Matrix 17: after the token expired, the bot-token channel edit carries
/// the same keep-list and multipart.
#[tokio::test]
async fn msg_08_expired_token_channel_edit() {
    let fx = Fixture::start_with_token(discord_files()).await;
    let (a, b) = (png(b"a"), png(b"b"));
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .reply(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    skip(Duration::from_mins(15)).await;
    acked
        .edit_reply(view(&[a.clone(), b.clone()], &[]))
        .await
        .unwrap();
    let seen = last(&fx);
    assert_eq!(seen.path, "/api/v10/channels/9/messages/77");
    assert!(seen.headers.get("authorization").is_some());
    let entries = payload(&seen)["attachments"].clone();
    assert!(entries.as_array().unwrap().contains(&json!({ "id": "21" })));
    assert_eq!(uploads(&seen)[0].filename.as_deref(), Some(b.name()));
    let _ = acked.done();
}

/// Matrix 18: a legacy message keeps nothing on an edit.
#[tokio::test]
async fn msg_08_legacy_edit_sends_an_empty_list() {
    let fx = Fixture::start(discord_files()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .reply(LegacyMessage::new("a"))
        .await
        .unwrap();
    acked.edit_reply(LegacyMessage::new("b")).await.unwrap();
    assert_eq!(payload(&last(&fx))["attachments"], json!([]));
    let _ = acked.done();
}

/// Matrix 21, 22 and 27: over the Upload Budget no request is sent, the
/// state is unchanged and a valid reply still goes out as type 4.
#[tokio::test]
async fn att_01_over_budget_sends_nothing() {
    let fx = Fixture::start(discord_files()).await;
    let big = Attachment::new(vec![0_u8; 20_971_521], "png").unwrap();
    let shared = fx.interaction("tok", Ephemerality::Public);
    let err = responder(&shared)
        .reply(view(std::slice::from_ref(&big), &[]))
        .await
        .unwrap_err();
    let ResponderError::Invalid(UiError::Limits { violations }) = err else {
        panic!("{err}");
    };
    assert_eq!(violations[0].rule, RuleId::ATT_01);
    assert_eq!(violations[0].path, "attachments[0].size");
    assert_eq!(violations[0].notes, [format!("file `{}`", big.name())]);
    assert!(fx.server.seen().is_empty());
    assert_eq!(shared.state(), PENDING);
    let _ = responder(&shared).reply(v2("ok")).await.unwrap().done();
    let seen = fx.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(payload(&seen[0])["type"], 4);

    // Matrix 22: the interaction's lower limit.
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
        channel_id: Some(Id::new(9)),
        file_limit: Some(10_485_760),
    });
    let shared = Shared::new(transport, Ephemerality::Public, Limits::default());
    let mid = Attachment::new(vec![0_u8; 10_485_761], "png").unwrap();
    let err = responder(&shared)
        .reply(view(std::slice::from_ref(&mid), &[]))
        .await
        .unwrap_err();
    let ResponderError::Invalid(UiError::Limits { violations }) = err else {
        panic!("{err}");
    };
    assert_eq!(
        (violations[0].rule, &violations[0].limit),
        (RuleId::ATT_01, &noctis_rules::Bound::AtMost(10_485_760))
    );
    assert_eq!(fx.server.seen().len(), 1);
}

/// Matrix 2, 7 and 12 over `Transport::Message`: a prefix reply and a
/// deferred reply finished by `edit_reply` post multipart with the
/// `message_reference`; a following edit keeps the learned id as JSON.
#[tokio::test]
async fn message_transport_attachments() {
    let fx = Fixture::start(discord_files()).await;
    let a = png(b"a");
    for deferred in [false, true] {
        fx.server.log.lock().unwrap().clear();
        let shared = fx.message(Ephemerality::Public);
        let acked = if deferred {
            let acked = responder(&shared)
                .defer_reply_as(Ephemerality::Public)
                .await
                .unwrap();
            acked
                .edit_reply(view(std::slice::from_ref(&a), &[]))
                .await
                .unwrap();
            acked
        } else {
            responder(&shared)
                .reply(view(std::slice::from_ref(&a), &[]))
                .await
                .unwrap()
        };
        let seen = last(&fx);
        assert_eq!(
            (seen.method.clone(), seen.path.as_str()),
            (Method::POST, "/api/v10/channels/5/messages"),
            "deferred = {deferred}"
        );
        let json = payload(&seen);
        assert_eq!(
            json["message_reference"],
            json!({ "message_id": "6", "fail_if_not_exists": false })
        );
        assert_eq!(
            json["attachments"],
            json!([{ "id": 0, "filename": a.name() }])
        );
        let parts = uploads(&seen);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "files[0]");
        assert_eq!(parts[0].body, b"a");

        acked
            .edit_reply(view(std::slice::from_ref(&a), &[]))
            .await
            .unwrap();
        let seen = last(&fx);
        assert_eq!(
            (seen.method.clone(), seen.path.as_str()),
            (Method::PATCH, "/api/v10/channels/5/messages/55")
        );
        assert_eq!(seen.headers["content-type"], "application/json");
        let learned = if deferred { "22" } else { "21" };
        assert_eq!(payload(&seen)["attachments"], json!([{ "id": learned }]));
        assert!(uploads(&seen).is_empty());
        let _ = acked.done();
    }
}

/// The `Invalid(Limits)` violation of `err`, which must name `rule` at
/// `path`.
fn assert_limit(err: &ResponderError, rule: RuleId, path: &str) {
    let ResponderError::Invalid(UiError::Limits { violations }) = err else {
        panic!("{err}");
    };
    assert_eq!(
        (violations[0].rule, violations[0].path.as_str()),
        (rule, path),
        "{violations:?}"
    );
}

/// Matrix 26 through the Responder: 11 used files fail ATT-03 before any
/// request.
#[tokio::test]
async fn att_03_eleven_files_send_nothing() {
    let fx = Fixture::start(discord_files()).await;
    let files: Vec<Attachment> = (0_u8..11)
        .map(|i| Attachment::new(vec![i], "png").unwrap())
        .collect();
    let mut set = AttachmentSet::new();
    let refs: Vec<AttachmentRef> = files.iter().map(|f| set.add(f.clone())).collect();
    let gallery = |refs: &[AttachmentRef]| {
        MediaGallery::try_new(
            refs.iter().map(|r| MediaItem::new(r.clone()).alt("x")),
            &Limits::default(),
        )
        .unwrap()
    };
    let msg = V2Message::new(
        Components::new()
            .push(gallery(&refs[..10]))
            .push(gallery(&refs[10..])),
    )
    .attachments(set);
    let shared = fx.interaction("tok", Ephemerality::Public);
    let err = responder(&shared).reply(msg).await.unwrap_err();
    assert_limit(&err, RuleId::ATT_03, "attachments");
    assert!(fx.server.seen().is_empty());
}

/// Matrix 25 through the Responder: a request over 25 MiB fails ATT-02
/// before any request.
#[tokio::test]
async fn att_02_request_over_budget_sends_nothing() {
    let fx = Fixture::start(discord_files()).await;
    let a = Attachment::new(vec![0_u8; 13_107_200], "png").unwrap();
    let b = Attachment::new(vec![1_u8; 13_107_200], "png").unwrap();
    let shared = fx.interaction("tok", Ephemerality::Public);
    let err = responder(&shared)
        .reply(view(&[a, b], &[]))
        .await
        .unwrap_err();
    assert_limit(&err, RuleId::ATT_02, "request");
    assert!(fx.server.seen().is_empty());
}
