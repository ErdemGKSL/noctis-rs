//! Folds what a run sent into a [`Response`].

use std::sync::Arc;

use noctis_core::__testing::AppProbe;
use serde_json::Value;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

use super::wire::{self, WireEvent};
use crate::fake::RecordedRequest;
use crate::response::{
    EPHEMERAL_FLAG, HttpCall, ModalForm, Notices, Response, ResponseKind, RunCtx, Source,
    TestMessage, single_text,
};

/// The built-in guard notice keys.
const GUARD_KEYS: [&str; 3] = [
    "noctis.guard.failed",
    "noctis.guard.owner",
    "noctis.guard.permissions",
];
/// The built-in expired notice key.
const EXPIRED_KEY: &str = "noctis.expired";
/// The built-in rate-limit notice key (`{retry}` is a relative
/// timestamp).
const RATE_LIMITED_KEY: &str = "noctis.rate_limited";
/// What stands for `{retry}` while matching the rate-limit notice.
const RETRY_MARK: &str = "\u{1}";
/// The built-in error notice keys.
const ERROR_KEYS: [&str; 2] = ["noctis.error.generic", "noctis.validation.failed"];

/// One run's inputs to the fold.
pub(crate) struct Run<'a> {
    pub(crate) probe: &'a AppProbe,
    pub(crate) ctx: &'a RunCtx,
    pub(crate) source: Option<&'a Arc<Source>>,
}

/// Whether `body` sets the ephemeral flag.
fn flagged_ephemeral(body: &Value) -> bool {
    body.get("flags")
        .and_then(Value::as_u64)
        .is_some_and(|f| f & EPHEMERAL_FLAG != 0)
}

impl Run<'_> {
    /// A message `id` in `channel` with `body` as sent, holding the
    /// attachment objects `attachments`.
    fn message(
        &self,
        id: Id<MessageMarker>,
        channel: Id<ChannelMarker>,
        body: Value,
        ephemeral: bool,
        attachments: &[Value],
    ) -> TestMessage {
        let mut seen = body.clone();
        if ephemeral {
            let flags = seen.get("flags").and_then(Value::as_u64).unwrap_or(0);
            seen["flags"] = Value::from(flags | EPHEMERAL_FLAG);
        }
        let mut ctx = self.ctx.clone();
        ctx.channel = channel;
        TestMessage {
            raw: body,
            source: Arc::new(Source {
                id,
                json: wire::message_json_with(id, channel, &seen, attachments),
                ephemeral,
                ctx,
            }),
        }
    }

    /// The channel of a message the run sent: the source's for an update.
    fn channel(&self, update: bool) -> Id<ChannelMarker> {
        match (update, self.source) {
            (true, Some(source)) => source.ctx.channel,
            _ => self.ctx.channel,
        }
    }

    /// The [`Response`] of `events` and `requests`.
    pub(crate) fn fold(&self, events: Vec<WireEvent>, requests: &[RecordedRequest]) -> Response {
        let source_ephemeral = self.source.is_some_and(|s| s.ephemeral);
        let mut callback: Option<(u8, Value, Id<MessageMarker>)> = None;
        let mut edited = false;
        let mut final_body: Option<Value> = None;
        let mut final_files: Vec<Value> = Vec::new();
        // A follow-up's ephemerality is fixed by its creating body; edits
        // never carry flag 64.
        let mut followups: Vec<(Id<MessageMarker>, Value, bool, Vec<Value>)> = Vec::new();
        for event in events {
            match event {
                WireEvent::Callback {
                    kind,
                    data,
                    original,
                    attachments,
                } => {
                    if callback.is_none() {
                        if matches!(kind, 4 | 7) {
                            final_body = Some(data.clone());
                        }
                        final_files = attachments;
                        callback = Some((kind, data, original));
                    }
                }
                WireEvent::Original { body, attachments } => {
                    edited = true;
                    final_body = body;
                    final_files = attachments;
                }
                WireEvent::FollowUp {
                    id,
                    body,
                    attachments,
                } => {
                    let ephemeral = flagged_ephemeral(&body);
                    followups.push((id, body, ephemeral, attachments));
                }
                WireEvent::Message {
                    id,
                    body,
                    attachments,
                } => match body {
                    Some(body) => {
                        if let Some(entry) = followups.iter_mut().find(|f| f.0 == id) {
                            entry.1 = body;
                            entry.3 = attachments;
                        }
                    }
                    None => followups.retain(|f| f.0 != id),
                },
            }
        }
        let callback_type = callback.as_ref().map(|(kind, _, _)| *kind);
        let has_final = final_body.is_some();
        let kind = response_kind(callback_type, edited && has_final);
        let ephemeral = match &callback {
            Some((4 | 5, data, _)) => flagged_ephemeral(data),
            Some((6 | 7, _, _)) => source_ephemeral,
            _ => false,
        };
        let update = matches!(callback_type, Some(6 | 7));
        let message = match (&callback, final_body) {
            (Some((_, _, original)), Some(body)) => Some(self.message(
                *original,
                self.channel(update),
                body,
                ephemeral,
                &final_files,
            )),
            _ => None,
        };
        let followups: Vec<TestMessage> = followups
            .into_iter()
            .map(|(id, body, ephemeral, files)| {
                self.message(id, self.ctx.channel, body, ephemeral, &files)
            })
            .collect();
        let modal = match &callback {
            Some((9, data, _)) => Some(ModalForm {
                data: data.clone(),
                source: self.source.cloned(),
                ctx: self.ctx.clone(),
                set: Vec::new(),
            }),
            _ => None,
        };
        let suggestions = match &callback {
            Some((8, data, _)) => suggestions(data),
            _ => Vec::new(),
        };
        let notices = self.notices(message.iter().chain(&followups));
        Response {
            kind,
            callback_type,
            ephemeral,
            message,
            followups,
            modal,
            suggestions,
            http_calls: requests.iter().map(http_call).collect(),
            notices,
        }
    }

    /// Which built-in notices `messages` show, in the run's locale.
    fn notices<'m>(&self, messages: impl Iterator<Item = &'m TestMessage>) -> Notices {
        let user = Some(self.ctx.locale.as_str());
        let guild = self.ctx.guild_locale.as_deref();
        let text = |key| self.probe.notice(key, user, guild);
        let guard: Vec<String> = GUARD_KEYS.into_iter().map(text).collect();
        let expired = text(EXPIRED_KEY);
        let error: Vec<String> = ERROR_KEYS.into_iter().map(text).collect();
        let limited =
            self.probe
                .notice_with(RATE_LIMITED_KEY, user, guild, &[("retry", &RETRY_MARK)]);
        let (before, after) = limited.split_once(RETRY_MARK).unwrap_or((&limited, ""));
        let mut notices = Notices::default();
        for message in messages {
            let Some(shown) = single_text(&message.raw) else {
                continue;
            };
            notices.guard |= guard.iter().any(|t| t == shown);
            notices.expired |= expired == shown;
            notices.error |= error.iter().any(|t| t == shown);
            notices.rate_limited |= shown.len() > before.len() + after.len()
                && shown.starts_with(before)
                && shown.ends_with(after)
                && shown[before.len()..shown.len() - after.len()].starts_with("<t:");
        }
        notices
    }
}

/// `(value as a string, label)` of every choice of a type-8 callback.
fn suggestions(data: &Value) -> Vec<(String, String)> {
    data.get("choices")
        .and_then(Value::as_array)
        .map(|choices| {
            choices
                .iter()
                .map(|choice| {
                    let value = match choice.get("value") {
                        Some(Value::String(s)) => s.clone(),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    };
                    let label = choice
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    (value, label)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The [`HttpCall`] of a recorded request.
fn http_call(request: &RecordedRequest) -> HttpCall {
    HttpCall {
        method: request.method.as_str().to_owned(),
        path: request
            .route_path()
            .unwrap_or(request.path.as_str())
            .to_owned(),
        json: request.json(),
        files: request.files(),
    }
}

/// The [`ResponseKind`] of a run whose callback had `callback_type`;
/// `edited` is whether a deferred response was later edited into a message.
fn response_kind(callback_type: Option<u8>, edited: bool) -> ResponseKind {
    match callback_type {
        Some(4) => ResponseKind::Reply,
        Some(5) if edited => ResponseKind::Reply,
        Some(6) if edited => ResponseKind::Update,
        Some(7) => ResponseKind::Update,
        Some(5 | 6) => ResponseKind::Deferred,
        Some(8) => ResponseKind::Autocomplete,
        Some(9) => ResponseKind::Modal,
        _ => ResponseKind::None,
    }
}
