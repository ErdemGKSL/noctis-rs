//! Story 9.5: webhooks on the wire (FR-65; WH-01..05, MSG-07).

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use hyper::{Method as HttpMethod, StatusCode};
use noctis_codec::{RouteShape, SelectRoute};
use noctis_rules::{Bound, LimitMode, Limits, RuleId};
use noctis_ui::__macro::{ButtonDefaults, SelectDefaults, build_button, build_user_select};
use noctis_ui::{
    ActionRow, Attachment, AttachmentSet, ButtonStyle, Components, CreateFlags, EditFlags,
    LinkButton, MediaGallery, MediaItem, StaticActionRow, StaticComponents, StaticContainer,
    StaticMessage, TextDisplay, V2Message, WebhookName,
};
use serde_json::{Value, json};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;

use super::*;
use crate::edge::test_support::{Answer, Seen};
use crate::responder::tests::Fixture;
use crate::responder::tests::files::{payload, uploads};
use crate::test_support::{TestRefs, test_route};

test_route!(Stored, "stored", RouteShape::BY_REF, 0);
test_route!(Picked, "picked", RouteShape::BY_REF.select(), 0);
test_route!(Plain, "plain", RouteShape::INLINE, 0);

impl SelectRoute for Picked {}

const EXECUTE: &str = "/api/v10/webhooks/900/tok";
const BOTH: &str = "wait=true&with_components=true";

fn v2(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

fn static_msg(text: &str) -> StaticMessage {
    StaticMessage::new(
        StaticComponents::new()
            .push(StaticContainer::new().push(TextDisplay::new(text.to_owned()))),
    )
}

fn reply(status: StatusCode, value: &Value) -> (StatusCode, Bytes) {
    (status, Bytes::from(value.to_string()))
}

/// A small Discord: Create Webhook answers webhook 900 (token `tok`) in
/// channel 5; a message answer (id 77, or the edited id) is in the
/// `thread_id` of the query, a new post 88 for a `thread_name`, else
/// channel 5, and echoes the request's attachments (new ones get ids 500,
/// 501, … in upload order; kept ones keep their id and filename); a delete
/// answers 204.
fn discord() -> Answer {
    let names: Mutex<Vec<Value>> = Mutex::default();
    Arc::new(move |seen: &Seen| {
        if seen.method == HttpMethod::DELETE {
            return (StatusCode::NO_CONTENT, Bytes::new());
        }
        if seen.path.ends_with("/webhooks") {
            return reply(
                StatusCode::OK,
                &json!({"id": "900", "token": "tok", "channel_id": "5"}),
            );
        }
        let json = payload(seen);
        let thread = seen
            .query
            .as_deref()
            .into_iter()
            .flat_map(|q| q.split('&'))
            .find_map(|kv| kv.strip_prefix("thread_id="))
            .map(str::to_owned);
        let channel = if json.get("thread_name").is_some() {
            "88".to_owned()
        } else {
            thread.unwrap_or_else(|| "5".to_owned())
        };
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
                    json!({
                        "id": (499 + names.len()).to_string(),
                        "filename": entry["filename"],
                    })
                }
            })
            .collect();
        let id = if seen.method == HttpMethod::PATCH {
            seen.path.rsplit('/').next().unwrap().to_owned()
        } else {
            "77".to_owned()
        };
        reply(
            StatusCode::OK,
            &json!({"id": id, "channel_id": channel, "attachments": attachments}),
        )
    })
}

/// Answers every request with `status` and `body`.
fn answer(status: StatusCode, body: &'static str) -> Answer {
    Arc::new(move |_: &Seen| (status, Bytes::from_static(body.as_bytes())))
}

fn owned(fx: &Fixture) -> Webhook<Owned> {
    fx.app.webhooks().owned(Id::new(900), "tok").unwrap()
}

fn foreign(fx: &Fixture) -> Webhook<Foreign> {
    fx.app.webhooks().foreign(Id::new(900), "tok").unwrap()
}

fn invalid(error: WebhookError) -> Vec<(RuleId, String, Bound)> {
    match error {
        WebhookError::Invalid(UiError::Limits { violations }) => violations
            .into_iter()
            .map(|v| (v.rule, v.path, v.limit))
            .collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn rules(error: WebhookError) -> Vec<RuleId> {
    invalid(error).into_iter().map(|(r, ..)| r).collect()
}

fn png(bytes: &'static [u8]) -> Attachment {
    Attachment::new(bytes, "png").unwrap()
}

/// A message showing every file of `files` in one gallery, with the
/// unreferenced set members `extra`.
fn view(files: &[Attachment], extra: &[Attachment]) -> V2Message {
    let mut set = AttachmentSet::new();
    let refs: Vec<_> = files.iter().map(|f| set.add(f.clone())).collect();
    for f in extra {
        set.add(f.clone());
    }
    let items = refs.into_iter().map(|r| MediaItem::new(r).alt("x"));
    let body = Components::new()
        .push(TextDisplay::new("t"))
        .push(MediaGallery::try_new(items, &Limits::default()).unwrap());
    V2Message::new(body).attachments(set)
}

/// E1, WH-01, MSG-07: an owned execute.
#[tokio::test]
async fn wh_01_owned_execute_wire() {
    let fx = Fixture::start_with_token(discord()).await;
    let sent = owned(&fx).send(v2("hi")).await.unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, HttpMethod::POST);
    assert_eq!(seen[0].path, EXECUTE);
    assert_eq!(seen[0].query.as_deref(), Some(BOTH));
    assert!(seen[0].headers.get("authorization").is_none());
    assert_eq!(
        payload(&seen[0]),
        json!({
            "flags": 32768,
            "components": [{"type": 10, "id": 1, "content": "hi"}],
            "allowed_mentions": {"parse": []}
        })
    );
    assert_eq!(sent.id(), Id::new(77));
}

/// E2, MSG-07: the execute flags.
#[tokio::test]
async fn msg_07_execute_flags() {
    let fx = Fixture::start(discord()).await;
    let hook = owned(&fx);
    hook.send(v2("a"))
        .flags(CreateFlags::SUPPRESS_NOTIFICATIONS)
        .await
        .unwrap();
    hook.send(v2("b"))
        .flags(CreateFlags::SUPPRESS_EMBEDS.union(CreateFlags::SUPPRESS_NOTIFICATIONS))
        .await
        .unwrap();
    let seen = fx.server.seen();
    assert_eq!(payload(&seen[0])["flags"], 36864);
    assert_eq!(payload(&seen[1])["flags"], 36868);
}

/// N4: the per-message persona.
#[tokio::test]
async fn wh_04_persona() {
    let fx = Fixture::start(discord()).await;
    owned(&fx)
        .send(v2("a"))
        .username(WebhookName::try_from("Mod log").unwrap())
        .avatar_url("https://a/b.png")
        .await
        .unwrap();
    let body = payload(&fx.server.seen()[0]);
    assert_eq!(body["username"], "Mod log");
    assert_eq!(body["avatar_url"], "https://a/b.png");
}

/// E3: an owned webhook sends interactive components, resolved as in a
/// reply: the Ref is stored in the App's `RefStore`, the select carries a
/// nonce.
#[tokio::test]
async fn wh_02_owned_sends_interactive_components() {
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
    let select = build_user_select(
        Picked { item: 2 },
        &SelectDefaults {
            placeholder: Some(noctis_ui::__macro::DefaultText::Literal("p")),
            min_values: None,
            max_values: None,
            disabled: false,
        },
    );
    let msg = V2Message::new(
        Components::new()
            .push(ActionRow::buttons([button]))
            .push(ActionRow::select(select)),
    );
    owned(&fx).send(msg).await.unwrap();
    let body = payload(&fx.server.seen()[0]);
    let button_id = body["components"][0]["components"][0]["custom_id"]
        .as_str()
        .unwrap();
    let select_id = body["components"][1]["components"][0]["custom_id"]
        .as_str()
        .unwrap();
    assert!(button_id.starts_with("0stored:~"), "{button_id}");
    assert!(
        select_id.starts_with("4picked:") && select_id.contains('~'),
        "{select_id}"
    );
    assert_eq!(refs.calls_of("put_if_absent").len(), 2);
}

/// E4: the Limits are checked before any request.
#[tokio::test]
async fn limits_before_any_request() {
    let fx = Fixture::start(discord()).await;
    let hook = owned(&fx);
    let many = (0..41).fold(Components::new(), |c, _| c.push(TextDisplay::new("a")));
    let err = hook.send(V2Message::new(many)).await.unwrap_err();
    assert_eq!(rules(err)[0], RuleId::CV2_03);
    let err = hook.send(v2(&"a".repeat(4001))).await.unwrap_err();
    assert_eq!(rules(err), [RuleId::CV2_04]);
    assert!(fx.server.seen().is_empty());
}

/// E5: only referenced files are uploaded; the Upload Budget holds.
#[tokio::test]
async fn files_upload_and_budget() {
    let fx = Fixture::start(discord()).await;
    let hook = owned(&fx);
    let a = png(b"a");
    let b = png(b"b");
    hook.send(view(std::slice::from_ref(&a), &[b]))
        .await
        .unwrap();
    let seen = fx.server.seen();
    let body = payload(&seen[0]);
    assert_eq!(body["attachments"][0]["id"], 0);
    assert_eq!(body["attachments"][0]["filename"], a.name());
    assert_eq!(body["attachments"].as_array().unwrap().len(), 1);
    let parts = uploads(&seen[0]);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "files[0]");
    let big: &'static [u8] = Box::leak(vec![1_u8; 20 * 1024 * 1024 + 1].into_boxed_slice());
    let err = hook.send(view(&[png(big)], &[])).await.unwrap_err();
    assert_eq!(rules(err), [RuleId::ATT_01]);
    assert_eq!(fx.server.seen().len(), 1);
}

/// E6: the handle names the message, its channel and no thread.
#[tokio::test]
async fn response_handle() {
    let fx = Fixture::start(discord()).await;
    let sent = owned(&fx).send(v2("a")).await.unwrap();
    assert_eq!(sent.id(), Id::new(77));
    assert_eq!(sent.channel_id(), Id::new(5));
    assert_eq!(sent.thread_id(), None);
}

/// E7: unusable answers and error statuses.
#[tokio::test]
async fn bad_responses() {
    let fx = Fixture::start(answer(StatusCode::OK, "{}")).await;
    assert!(matches!(
        owned(&fx).send(v2("a")).await,
        Err(WebhookError::Transport)
    ));
    let fx = Fixture::start(answer(StatusCode::INTERNAL_SERVER_ERROR, "{}")).await;
    assert!(matches!(
        owned(&fx).send(v2("a")).await,
        Err(WebhookError::Rejected {
            status: 500,
            code: None
        })
    ));
    let fx = Fixture::start(answer(StatusCode::NOT_FOUND, r#"{"code":10015}"#)).await;
    assert!(matches!(
        owned(&fx).send(v2("a")).await,
        Err(WebhookError::Rejected {
            status: 404,
            code: Some(10015)
        })
    ));
}

/// E8: a bad token is refused before anything is sent.
#[tokio::test]
async fn token_checked_before_sending() {
    let fx = Fixture::start(discord()).await;
    let hooks = fx.app.webhooks();
    for bad in [
        String::new(),
        "a".repeat(513),
        "a/b".to_owned(),
        "a b".to_owned(),
        "tök".to_owned(),
    ] {
        assert!(matches!(
            hooks.owned(Id::new(1), bad.clone()),
            Err(WebhookError::InvalidToken)
        ));
        assert!(matches!(
            hooks.foreign(Id::new(1), bad),
            Err(WebhookError::InvalidToken)
        ));
    }
    let ok = "aZ09._-".repeat(10);
    let ok = &ok[..68];
    assert_eq!(hooks.owned(Id::new(1), ok).unwrap().token(), ok);
    assert!(fx.server.seen().is_empty());
}

/// E9: a detached context sends nothing.
#[tokio::test]
async fn detached_sends_nothing() {
    let hooks = crate::ComponentCtx::detached().webhooks();
    let hook = hooks.owned(Id::new(1), "tok").unwrap();
    assert!(matches!(
        hook.send(v2("a")).await,
        Err(WebhookError::Detached)
    ));
    assert!(matches!(
        hook.edit(Id::new(2), v2("a")).await,
        Err(WebhookError::Detached)
    ));
    assert!(matches!(
        hook.delete(Id::new(2)).await,
        Err(WebhookError::Detached)
    ));
    let channel = WebhookChannel::new(Id::new(5), ChannelType::GuildText);
    assert!(matches!(
        hooks
            .create(channel, WebhookName::try_from("Log").unwrap())
            .await,
        Err(WebhookError::Detached)
    ));
}

/// E10: no `Debug` prints the token.
#[tokio::test]
async fn token_is_redacted() {
    let fx = Fixture::start(answer(StatusCode::NOT_FOUND, r#"{"code":10015}"#)).await;
    let secret = "s3cr3tT0ken";
    let hook = fx.app.webhooks().owned(Id::new(900), secret).unwrap();
    assert!(!format!("{hook:?}").contains(secret));
    assert!(!format!("{:?}", hook.send(v2("a"))).contains(secret));
    assert!(!format!("{:?}", hook.edit(Id::new(1), v2("a"))).contains(secret));
    assert!(!format!("{:?}", hook.delete(Id::new(1))).contains(secret));
    let err = hook.send(v2("a")).await.unwrap_err();
    assert!(!format!("{err:?} {err}").contains(secret));
    assert!(!format!("{:?}", fx.app.webhooks()).contains(secret));
}

/// S5: a foreign execute has the same wire.
#[tokio::test]
async fn wh_02_foreign_execute_wire() {
    let fx = Fixture::start(discord()).await;
    foreign(&fx).send(static_msg("x")).await.unwrap();
    foreign(&fx)
        .send(
            StaticComponents::new()
                .push(StaticActionRow::buttons([LinkButton::new("https://a.b")])),
        )
        .await
        .unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen[0].path, EXECUTE);
    assert_eq!(seen[0].query.as_deref(), Some(BOTH));
    assert!(seen[0].headers.get("authorization").is_none());
    assert_eq!(payload(&seen[0])["flags"], 32768);
    assert_eq!(payload(&seen[0])["components"][0]["type"], 17);
    assert_eq!(
        payload(&seen[1])["components"][0]["components"][0]["style"],
        5
    );
}

/// S6: an owned webhook takes a `StaticMessage` too.
#[tokio::test]
async fn owned_takes_a_static_message() {
    let fx = Fixture::start(discord()).await;
    owned(&fx).send(static_msg("x")).await.unwrap();
    let foreign_fx = Fixture::start(discord()).await;
    foreign(&foreign_fx).send(static_msg("x")).await.unwrap();
    assert_eq!(
        payload(&fx.server.seen()[0]),
        payload(&foreign_fx.server.seen()[0])
    );
}

/// S3 at send time: a run-time tree with a button is refused by
/// `StaticMessage::try_from` before a foreign send.
#[tokio::test]
async fn wh_02_try_from_refuses_interactive() {
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
    let err = StaticMessage::try_from(msg).unwrap_err();
    assert!(matches!(&err, UiError::Limits { violations } if violations[0].rule == RuleId::WH_02));
}

fn forum(fx: &Fixture, kind: ChannelType) -> Webhook<Owned> {
    owned(fx).in_channel(kind)
}

fn wh05_thread_id() -> (RuleId, String, Bound) {
    (
        RuleId::WH_05,
        "thread_id".to_owned(),
        Bound::Describe("a thread_id or a thread_name in a forum or media channel"),
    )
}

/// T1: a forum or media webhook without a thread sends nothing.
#[tokio::test]
async fn wh_05_forum_without_thread() {
    let fx = Fixture::start_with_token(discord()).await;
    let channel = WebhookChannel::new(Id::new(5), ChannelType::GuildForum);
    let hook = fx
        .app
        .webhooks()
        .create(channel, WebhookName::try_from("Log").unwrap())
        .await
        .unwrap();
    let err = hook.send(v2("a")).await.unwrap_err();
    assert_eq!(invalid(err), [wh05_thread_id()]);
    let media = forum(&fx, ChannelType::GuildMedia);
    assert_eq!(
        invalid(media.send(v2("a")).await.unwrap_err()),
        [wh05_thread_id()]
    );
    assert_eq!(fx.server.seen().len(), 1, "only the create");
}

/// T2: an existing thread is a query, a new post a `thread_name`.
#[tokio::test]
async fn wh_05_forum_threads() {
    let fx = Fixture::start(discord()).await;
    let hook = forum(&fx, ChannelType::GuildForum);
    let sent = hook
        .send(v2("a"))
        .thread(WebhookThread::Existing(Id::new(9)))
        .await
        .unwrap();
    assert_eq!(sent.thread_id(), Some(Id::new(9)));
    let posted = hook
        .send(v2("b"))
        .thread(WebhookThread::Create("Daily log".into()))
        .await
        .unwrap();
    assert_eq!(posted.thread_id(), Some(Id::new(88)));
    let seen = fx.server.seen();
    assert_eq!(
        seen[0].query.as_deref(),
        Some("thread_id=9&wait=true&with_components=true")
    );
    assert_eq!(seen[1].query.as_deref(), Some(BOTH));
    assert_eq!(payload(&seen[1])["thread_name"], "Daily log");
    assert!(payload(&seen[0]).get("thread_name").is_none());
}

/// T3: a `thread_name` has 1 to 100 characters.
#[tokio::test]
async fn wh_05_thread_name_bounds() {
    let fx = Fixture::start(discord()).await;
    let hook = forum(&fx, ChannelType::GuildForum);
    let between = |count| {
        (
            RuleId::WH_05,
            "thread_name".to_owned(),
            Bound::Between { min: 1, max: 100 },
            count,
        )
    };
    let check = |err: WebhookError| -> Vec<(RuleId, String, Bound, usize)> {
        match err {
            WebhookError::Invalid(UiError::Limits { violations }) => violations
                .into_iter()
                .map(|v| {
                    let Actual::Count(n) = v.actual else {
                        panic!("a count")
                    };
                    (v.rule, v.path, v.limit, n)
                })
                .collect(),
            other => panic!("{other:?}"),
        }
    };
    let err = hook
        .send(v2("a"))
        .thread(WebhookThread::Create("".into()))
        .await
        .unwrap_err();
    assert_eq!(check(err), [between(0)]);
    hook.send(v2("a"))
        .thread(WebhookThread::Create("a".repeat(100).into()))
        .await
        .unwrap();
    let err = hook
        .send(v2("a"))
        .thread(WebhookThread::Create("a".repeat(101).into()))
        .await
        .unwrap_err();
    assert_eq!(check(err), [between(101)]);
    assert_eq!(fx.server.seen().len(), 1);
    let unknown = owned(&fx);
    let err = unknown
        .send(v2("a"))
        .thread(WebhookThread::Create("".into()))
        .await
        .unwrap_err();
    assert_eq!(check(err), [between(0)]);
    let err = unknown
        .send(v2("a"))
        .thread(WebhookThread::Create("a".repeat(101).into()))
        .await
        .unwrap_err();
    assert_eq!(check(err), [between(101)]);
    assert_eq!(
        fx.server.seen().len(),
        1,
        "an unknown kind checks the length"
    );
    hook.send(v2("a"))
        .thread(WebhookThread::Create("\u{1f600}".repeat(50).into()))
        .await
        .unwrap();
    let err = hook
        .send(v2("a"))
        .thread(WebhookThread::Create("\u{1f600}".repeat(51).into()))
        .await
        .unwrap_err();
    assert_eq!(check(err), [between(102)]);
    assert_eq!(fx.server.seen().len(), 2);
}

/// T4: a `thread_name` only in a forum or media channel; an unknown kind
/// is not prechecked.
#[tokio::test]
async fn wh_05_thread_name_off_forum() {
    let fx = Fixture::start(discord()).await;
    let text = forum(&fx, ChannelType::GuildText);
    let err = text
        .send(v2("a"))
        .thread(WebhookThread::Create("x".into()))
        .await
        .unwrap_err();
    assert_eq!(
        invalid(err),
        [(
            RuleId::WH_05,
            "thread_name".to_owned(),
            Bound::Describe("a thread_name only in a forum or media channel"),
        )]
    );
    text.send(v2("a")).thread(Id::new(9)).await.unwrap();
    owned(&fx)
        .send(v2("a"))
        .thread(WebhookThread::Create("x".into()))
        .await
        .unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0].query.as_deref(),
        Some("thread_id=9&wait=true&with_components=true")
    );
}

/// T5: Discord's forum answers become the typed WH-05 error.
#[tokio::test]
async fn wh_05_discord_forum_errors() {
    let fx = Fixture::start(answer(StatusCode::BAD_REQUEST, r#"{"code":220001}"#)).await;
    assert_eq!(
        rules(owned(&fx).send(v2("a")).await.unwrap_err()),
        [RuleId::WH_05]
    );
    assert_eq!(
        rules(owned(&fx).edit(Id::new(77), v2("a")).await.unwrap_err()),
        [RuleId::WH_05]
    );
    assert_eq!(
        rules(owned(&fx).delete(Id::new(77)).await.unwrap_err()),
        [RuleId::WH_05]
    );
    let fx = Fixture::start(answer(StatusCode::BAD_REQUEST, r#"{"code":220003}"#)).await;
    let err = owned(&fx).send(v2("a")).await.unwrap_err();
    assert_eq!(invalid(err)[0].1, "thread_name");
    let fx = Fixture::start(answer(StatusCode::BAD_REQUEST, r#"{"code":50035}"#)).await;
    assert!(matches!(
        owned(&fx).send(v2("a")).await,
        Err(WebhookError::Rejected {
            status: 400,
            code: Some(50035)
        })
    ));
}

/// T6: an edit or delete by id in a forum names its thread.
#[tokio::test]
async fn wh_05_edit_and_delete_by_id_in_a_forum() {
    let fx = Fixture::start(discord()).await;
    let hook = forum(&fx, ChannelType::GuildForum);
    let err = hook.edit(Id::new(77), v2("a")).await.unwrap_err();
    assert_eq!(invalid(err), [wh05_thread_id()]);
    let err = hook.delete(Id::new(77)).await.unwrap_err();
    assert_eq!(invalid(err), [wh05_thread_id()]);
    hook.edit(Id::new(77), v2("a"))
        .thread(Id::new(9))
        .await
        .unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, HttpMethod::PATCH);
    assert_eq!(
        seen[0].query.as_deref(),
        Some("with_components=true&thread_id=9")
    );
}

/// D1, WH-03: the edit wire clears the legacy fields.
#[tokio::test]
async fn wh_03_edit_wire() {
    let fx = Fixture::start_with_token(discord()).await;
    owned(&fx)
        .edit(Id::new(77), v2("hi"))
        .flags(EditFlags::SUPPRESS_EMBEDS)
        .await
        .unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen[0].method, HttpMethod::PATCH);
    assert_eq!(seen[0].path, "/api/v10/webhooks/900/tok/messages/77");
    assert_eq!(seen[0].query.as_deref(), Some("with_components=true"));
    assert!(seen[0].headers.get("authorization").is_none());
    assert_eq!(
        payload(&seen[0]),
        json!({
            "flags": 32772,
            "content": null,
            "embeds": [],
            "poll": null,
            "components": [{"type": 10, "id": 1, "content": "hi"}],
            "allowed_mentions": {"parse": []},
            "attachments": []
        })
    );
}

/// D2: a handle's edit keeps what the message holds and uploads the rest.
#[tokio::test]
async fn handle_edit_diffs_attachments() {
    let fx = Fixture::start(discord()).await;
    let a = png(b"a");
    let b = png(b"b");
    let sent = owned(&fx)
        .send(view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    sent.edit(view(&[a.clone(), b.clone()], &[])).await.unwrap();
    let seen = fx.server.seen();
    let entries = payload(&seen[1])["attachments"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&json!({"id": "500"})), "{entries:?}");
    assert!(
        entries
            .iter()
            .any(|e| e["id"] == 0 && e["filename"] == b.name()),
        "{entries:?}"
    );
    let parts = uploads(&seen[1]);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].filename.as_deref(), Some(b.name()));
    assert_eq!(parts[0].body, b"b");
    sent.edit(view(&[a, b], &[])).await.unwrap();
    let seen = fx.server.seen();
    let entries = payload(&seen[2])["attachments"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries.contains(&json!({"id": "500"})), "{entries:?}");
    assert!(entries.contains(&json!({"id": "501"})), "{entries:?}");
    assert!(uploads(&seen[2]).is_empty());
}

/// D2 in a thread: a handle's edit and delete keep the send's thread.
#[tokio::test]
async fn handle_edit_in_thread() {
    let fx = Fixture::start_with_token(discord()).await;
    let hook = owned(&fx);
    let plain = hook.send(v2("a")).await.unwrap();
    plain.edit(v2("b")).await.unwrap();
    let threaded = hook.send(v2("a")).thread(Id::new(9)).await.unwrap();
    threaded.edit(v2("b")).await.unwrap();
    let post = forum(&fx, ChannelType::GuildForum)
        .send(v2("a"))
        .thread(WebhookThread::Create("Daily".into()))
        .await
        .unwrap();
    post.edit(v2("b")).await.unwrap();
    post.delete().await.unwrap();
    let seen = fx.server.seen();
    let edits = [
        (1, "with_components=true"),
        (3, "with_components=true&thread_id=9"),
        (5, "with_components=true&thread_id=88"),
    ];
    for (i, query) in edits {
        assert_eq!(seen[i].method, HttpMethod::PATCH);
        assert_eq!(seen[i].path, "/api/v10/webhooks/900/tok/messages/77");
        assert_eq!(seen[i].query.as_deref(), Some(query));
        assert!(seen[i].headers.get("authorization").is_none());
        let body = payload(&seen[i]);
        assert_eq!(body["content"], Value::Null);
        assert!(body.as_object().unwrap().contains_key("content"));
        assert_eq!(body["embeds"], json!([]));
        assert_eq!(body["flags"].as_u64().unwrap() & 32768, 32768);
    }
    assert_eq!(seen[6].method, HttpMethod::DELETE);
    assert_eq!(seen[6].query.as_deref(), Some("thread_id=88"));
    assert!(seen[6].headers.get("authorization").is_none());
}

/// The App's strings resolve a webhook message's locale keys (en-US).
#[tokio::test]
async fn webhook_resolves_locale_keys() {
    struct Shop;
    impl noctis_rules::Translate for Shop {
        fn translate(
            &self,
            locale: noctis_rules::Locale,
            key: noctis_rules::MessageKey,
            _: &[(&str, &dyn fmt::Display)],
        ) -> Option<String> {
            match (key.as_str(), locale) {
                ("shop.buy", noctis_rules::Locale::EnUs) => Some("Buy".to_owned()),
                _ => None,
            }
        }
    }
    let mut fx = Fixture::start(discord()).await;
    fx.app.set_strings(Arc::new(Shop));
    let button = build_button(
        Plain { item: 1 },
        &ButtonDefaults {
            style: ButtonStyle::Primary,
            label: Some(noctis_ui::__macro::DefaultText::Literal("b")),
            emoji: None,
            disabled: false,
        },
    )
    .label(noctis_rules::MessageKey::new("shop.buy"));
    let msg = V2Message::new(Components::new().push(ActionRow::buttons([button])));
    owned(&fx).send(msg).await.unwrap();
    let body = payload(&fx.server.seen()[0]);
    assert_eq!(body["components"][0]["components"][0]["label"], "Buy");
}

/// D3: an edit by id uploads every used file again.
#[tokio::test]
async fn edit_by_id_uploads_again() {
    let fx = Fixture::start(discord()).await;
    let a = png(b"a");
    owned(&fx)
        .edit(Id::new(77), view(std::slice::from_ref(&a), &[]))
        .await
        .unwrap();
    let parts = uploads(&fx.server.seen()[0]);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "files[0]");
    assert_eq!(parts[0].filename.as_deref(), Some(a.name()));
}

/// D4: deletes, in a thread and by id.
#[tokio::test]
async fn deletes() {
    let fx = Fixture::start_with_token(discord()).await;
    let hook = owned(&fx);
    let sent = hook.send(v2("a")).thread(Id::new(9)).await.unwrap();
    sent.delete().await.unwrap();
    hook.delete(Id::new(77)).await.unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen[1].method, HttpMethod::DELETE);
    assert_eq!(seen[1].path, "/api/v10/webhooks/900/tok/messages/77");
    assert_eq!(seen[1].query.as_deref(), Some("thread_id=9"));
    assert_eq!(seen[2].path, "/api/v10/webhooks/900/tok/messages/77");
    assert_eq!(seen[2].query, None);
    assert!(seen[2].headers.get("authorization").is_none());
}

/// C1: Create Webhook uses the bot token.
#[tokio::test]
async fn create_webhook() {
    let fx = Fixture::start_with_token(discord()).await;
    let channel = WebhookChannel::new(Id::new(5), ChannelType::GuildText);
    let hook = fx
        .app
        .webhooks()
        .create(channel, WebhookName::try_from("Noctis log").unwrap())
        .await
        .unwrap();
    let seen = fx.server.seen();
    assert_eq!(seen[0].method, HttpMethod::POST);
    assert_eq!(seen[0].path, "/api/v10/channels/5/webhooks");
    assert_eq!(seen[0].headers.get("authorization").unwrap(), "Bot abc");
    assert_eq!(payload(&seen[0]), json!({"name": "Noctis log"}));
    assert_eq!(hook.id(), Id::new(900));
    assert_eq!(hook.token(), "tok");
    assert_eq!(hook.channel_kind(), Some(ChannelType::GuildText));
}

/// C2: a bad Create Webhook answer.
#[tokio::test]
async fn create_webhook_errors() {
    let channel = WebhookChannel::new(Id::new(5), ChannelType::GuildText);
    let name = || WebhookName::try_from("Noctis log").unwrap();
    let fx = Fixture::start(answer(StatusCode::OK, r#"{"id":"900","channel_id":"5"}"#)).await;
    assert!(matches!(
        fx.app.webhooks().create(channel, name()).await,
        Err(WebhookError::Transport)
    ));
    let fx = Fixture::start(answer(StatusCode::FORBIDDEN, r#"{"code":50013}"#)).await;
    assert!(matches!(
        fx.app.webhooks().create(channel, name()).await,
        Err(WebhookError::Rejected {
            status: 403,
            code: Some(50013)
        })
    ));
}

/// Lenient mode never truncates a thread name.
#[tokio::test]
async fn wh_05_lenient_never_truncates() {
    let mut limits = Limits::default();
    limits.mode = LimitMode::Lenient;
    let fx = Fixture::start_with(discord(), limits).await;
    let err = forum(&fx, ChannelType::GuildForum)
        .send(v2("a"))
        .thread(WebhookThread::Create("a".repeat(101).into()))
        .await
        .unwrap_err();
    assert_eq!(rules(err), [RuleId::WH_05]);
}

#[test]
fn webhook_channel_from_interaction_channel() {
    let channel: InteractionChannel = serde_json::from_value(json!({
        "id": "5", "type": 15, "name": "f", "permissions": "0"
    }))
    .unwrap();
    let ch = WebhookChannel::from(&channel);
    assert_eq!((ch.id(), ch.kind()), (Id::new(5), ChannelType::GuildForum));
}
