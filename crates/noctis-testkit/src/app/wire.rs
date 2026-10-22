//! The fallback responder `TestApp` installs on its `FakeDiscordHttp`, and
//! the Discord-shaped JSON of seeded entities and messages.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::{Map, Value, json};
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};

use crate::TestError;
use crate::fake::{FakeResponse, Method, RecordedFile, RecordedRequest, StatusCode};
use crate::response::RunCtx;
use crate::seed::{Seeds, TestUser};

/// The application id every simulated interaction carries.
pub(crate) const APPLICATION_ID: u64 = 1_100_000_000_000_000_000;
/// The first message id the testkit assigns.
const FIRST_MESSAGE_ID: u64 = 1_200_000_000_000_000_000;
/// The timestamp of every message and member.
const TIMESTAMP: &str = "2026-01-01T00:00:00.000000+00:00";

/// What one request of the current run did to its messages; every
/// message event carries the attachment objects the message then holds.
#[derive(Debug, Clone)]
pub(crate) enum WireEvent {
    /// An interaction callback; `original` is the id `@original` then
    /// names (a new message for types 4 and 5, the source for 6 and 7).
    Callback {
        kind: u8,
        data: Value,
        original: Id<MessageMarker>,
        attachments: Vec<Value>,
    },
    /// `PATCH` (`Some(body)`) or `DELETE` (`None`) of `@original`.
    Original {
        body: Option<Value>,
        attachments: Vec<Value>,
    },
    /// A follow-up message.
    FollowUp {
        id: Id<MessageMarker>,
        body: Value,
        attachments: Vec<Value>,
    },
    /// `PATCH` (`Some(body)`) or `DELETE` (`None`) of a follow-up.
    Message {
        id: Id<MessageMarker>,
        body: Option<Value>,
        attachments: Vec<Value>,
    },
}

/// The run the responder records for.
#[derive(Debug)]
struct WireRun {
    token: String,
    /// The interaction type, echoed in `with_response` callback answers.
    kind: u8,
    channel: Id<ChannelMarker>,
    /// The source message's id, when the interaction has one.
    source: Option<Id<MessageMarker>>,
    original: Option<Id<MessageMarker>>,
    /// The last body of each message, for `GET`.
    bodies: HashMap<Id<MessageMarker>, Value>,
    /// The attachment objects each message holds (the source's from its
    /// interaction `message`).
    files: HashMap<Id<MessageMarker>, Vec<Value>>,
}

#[derive(Debug, Default)]
struct WireState {
    run: Option<WireRun>,
    events: Vec<WireEvent>,
}

/// The fallback responder: answers interaction callbacks and interaction
/// webhooks like Discord, and records what the current run sent.
#[derive(Debug)]
pub(crate) struct Wire {
    state: Mutex<WireState>,
    next_id: AtomicU64,
}

impl Wire {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(WireState::default()),
            next_id: AtomicU64::new(FIRST_MESSAGE_ID),
        }
    }

    fn lock(&self) -> MutexGuard<'_, WireState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A fresh message id.
    pub(crate) fn fresh_id(&self) -> Id<MessageMarker> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Id::new_checked(id).unwrap_or(Id::new(FIRST_MESSAGE_ID))
    }

    /// Starts recording for a run answering with `token`; `source` is the
    /// source message's id and the attachment objects it holds.
    pub(crate) fn begin(
        &self,
        token: String,
        kind: u8,
        channel: Id<ChannelMarker>,
        source: Option<(Id<MessageMarker>, Vec<Value>)>,
    ) {
        let mut state = self.lock();
        state.events.clear();
        let mut files = HashMap::new();
        let source = source.map(|(id, attachments)| {
            files.insert(id, attachments);
            id
        });
        state.run = Some(WireRun {
            token,
            kind,
            channel,
            source,
            original: None,
            bodies: HashMap::new(),
            files,
        });
    }

    /// The attachment objects a message holds after `body`, sent with the
    /// multipart `uploads`, replaced `previous`: a kept entry `{"id":
    /// "<sf>"}` copies that previous object (an unknown id is dropped), a
    /// new entry `{"id": n, …}` becomes a fresh attachment of the `files[n]`
    /// part. A body without `attachments` keeps `previous`.
    fn echo(
        &self,
        body: &Value,
        uploads: &[RecordedFile],
        previous: &[Value],
        channel: Id<ChannelMarker>,
    ) -> Vec<Value> {
        let Some(entries) = body.get("attachments").and_then(Value::as_array) else {
            return previous.to_vec();
        };
        entries
            .iter()
            .filter_map(|entry| match entry.get("id") {
                Some(Value::String(kept)) => previous
                    .iter()
                    .find(|p| p.get("id").and_then(Value::as_str) == Some(kept.as_str()))
                    .cloned(),
                Some(Value::Number(n)) => {
                    let part = format!("files[{n}]");
                    let file = uploads.iter().find(|f| f.name == part)?;
                    let filename = entry
                        .get("filename")
                        .and_then(Value::as_str)
                        .unwrap_or(&file.filename);
                    let id = self.fresh_id();
                    let url =
                        format!("https://cdn.discordapp.com/attachments/{channel}/{id}/{filename}");
                    let mut object = json!({
                        "id": id.to_string(),
                        "filename": filename,
                        "size": file.bytes.len(),
                        "url": url,
                        "proxy_url": url,
                    });
                    if let Some(description) = entry.get("description") {
                        object["description"] = description.clone();
                    }
                    Some(object)
                }
                _ => None,
            })
            .collect()
    }

    /// Ends the run; returns what it recorded.
    pub(crate) fn end(&self) -> Vec<WireEvent> {
        let mut state = self.lock();
        state.run = None;
        std::mem::take(&mut state.events)
    }

    /// The answer for `request`, or `None` for the fake's `404`.
    pub(crate) fn answer(&self, request: &RecordedRequest) -> Option<FakeResponse> {
        let path = request.route_path()?;
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let method = &request.method;
        match segments.as_slice() {
            ["interactions", id, token, "callback"] if *method == Method::POST => {
                let answered = self.callback(token, request);
                let with_response = request
                    .query
                    .as_deref()
                    .is_some_and(|q| q.split('&').any(|pair| pair == "with_response=true"));
                if !with_response {
                    return Some(FakeResponse::empty(StatusCode::NO_CONTENT));
                }
                let (interaction, kind, original, channel, data, attachments) = answered;
                let mut resource = json!({ "type": kind });
                if matches!(kind, 4 | 7) {
                    resource["message"] = message_json_with(original, channel, &data, &attachments);
                }
                let body = json!({
                    "interaction": { "id": *id, "type": interaction },
                    "resource": resource,
                });
                Some(FakeResponse::json(StatusCode::OK, &body))
            }
            ["webhooks", _, token] if *method == Method::POST => {
                let id = self.fresh_id();
                let body = request.json().unwrap_or(Value::Null);
                let uploads = request.files();
                let (channel, attachments) = self.record(token, |run, events| {
                    let attachments = self.echo(&body, &uploads, &[], run.channel);
                    run.bodies.insert(id, body.clone());
                    run.files.insert(id, attachments.clone());
                    events.push(WireEvent::FollowUp {
                        id,
                        body: body.clone(),
                        attachments: attachments.clone(),
                    });
                    (run.channel, attachments)
                });
                Some(message_response(id, channel, &body, &attachments))
            }
            ["webhooks", _, token, "messages", which] => self.message(token, which, request),
            _ => None,
        }
    }

    /// Records a callback and fixes what `@original` names; returns the
    /// interaction type (0 for a stray token), the callback type, the
    /// message `@original` names, its channel, the callback's `data` and
    /// the attachment objects the message then holds.
    fn callback(&self, token: &str, request: &RecordedRequest) -> Answered {
        let body = request.json().unwrap_or(Value::Null);
        let uploads = request.files();
        let kind = body
            .get("type")
            .and_then(Value::as_u64)
            .and_then(|k| u8::try_from(k).ok())
            .unwrap_or(0);
        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let fresh = self.fresh_id();
        self.record(token, |run, events| {
            let original = match (kind, run.source) {
                (6 | 7, Some(source)) => source,
                _ => fresh,
            };
            if run.original.is_none() {
                run.original = Some(original);
            }
            let previous = run.files.get(&original).cloned().unwrap_or_default();
            let attachments = if matches!(kind, 4 | 7) {
                let attachments = self.echo(&data, &uploads, &previous, run.channel);
                run.bodies.insert(original, data.clone());
                run.files.insert(original, attachments.clone());
                attachments
            } else {
                previous
            };
            events.push(WireEvent::Callback {
                kind,
                data: data.clone(),
                original,
                attachments: attachments.clone(),
            });
            (run.kind, kind, original, run.channel, data, attachments)
        })
    }

    /// `PATCH`, `GET` or `DELETE` of `@original` or a follow-up.
    fn message(&self, token: &str, which: &str, request: &RecordedRequest) -> Option<FakeResponse> {
        let method = request.method.clone();
        if ![Method::PATCH, Method::GET, Method::DELETE].contains(&method) {
            return None;
        }
        let named = (which != "@original")
            .then(|| which.parse::<u64>().ok().and_then(Id::new_checked))
            .flatten();
        if which != "@original" && named.is_none() {
            return None;
        }
        let fresh = self.fresh_id();
        let body = request.json();
        let uploads = request.files();
        let (id, channel, stored, attachments) = self.record(token, |run, events| {
            let id = named.or(run.original).unwrap_or(fresh);
            let previous = run.files.get(&id).cloned().unwrap_or_default();
            let (edit, attachments) = if method == Method::GET {
                return (id, run.channel, run.bodies.get(&id).cloned(), previous);
            } else if method == Method::DELETE {
                run.bodies.remove(&id);
                run.files.remove(&id);
                (None, Vec::new())
            } else {
                let body = body.clone().unwrap_or(Value::Null);
                let attachments = self.echo(&body, &uploads, &previous, run.channel);
                run.bodies.insert(id, body.clone());
                run.files.insert(id, attachments.clone());
                (Some(body), attachments)
            };
            let stored = edit.clone();
            events.push(match named {
                None => WireEvent::Original {
                    body: edit,
                    attachments: attachments.clone(),
                },
                Some(id) => WireEvent::Message {
                    id,
                    body: edit,
                    attachments: attachments.clone(),
                },
            });
            (id, run.channel, stored, attachments)
        });
        if method == Method::DELETE {
            return Some(FakeResponse::empty(StatusCode::NO_CONTENT));
        }
        Some(message_response(
            id,
            channel,
            &stored.or(body).unwrap_or(Value::Null),
            &attachments,
        ))
    }

    /// Runs `f` on the current run when `token` is its token; another
    /// token (a stray request of an earlier run) is answered but not
    /// recorded, `f` then gets a scratch run.
    fn record<T>(&self, token: &str, f: impl FnOnce(&mut WireRun, &mut Vec<WireEvent>) -> T) -> T {
        let mut guard = self.lock();
        let state = &mut *guard;
        match state.run.as_mut() {
            Some(run) if run.token == token => f(run, &mut state.events),
            _ => {
                let mut scratch = WireRun {
                    token: token.to_owned(),
                    kind: 0,
                    channel: Id::new(1),
                    source: None,
                    original: None,
                    bodies: HashMap::new(),
                    files: HashMap::new(),
                };
                f(&mut scratch, &mut Vec::new())
            }
        }
    }
}

/// What [`Wire::callback`] answers: the interaction type, the callback
/// type, the message `@original` names, its channel, the `data` and the
/// message's attachment objects.
type Answered = (
    u8,
    u8,
    Id<MessageMarker>,
    Id<ChannelMarker>,
    Value,
    Vec<Value>,
);

/// A `200` with the complete message `id` built from `body`, holding
/// `attachments`.
fn message_response(
    id: Id<MessageMarker>,
    channel: Id<ChannelMarker>,
    body: &Value,
    attachments: &[Value],
) -> FakeResponse {
    FakeResponse::json(
        StatusCode::OK,
        &message_json_with(id, channel, body, attachments),
    )
}

/// A complete Discord message object for `id` in `channel`, with the
/// `content`, `flags` and `components` of `body`, authored by the App,
/// without attachments.
pub(crate) fn message_json(
    id: Id<MessageMarker>,
    channel: Id<ChannelMarker>,
    body: &Value,
) -> Value {
    message_json_with(id, channel, body, &[])
}

/// [`message_json`] holding the attachment objects `attachments`.
pub(crate) fn message_json_with(
    id: Id<MessageMarker>,
    channel: Id<ChannelMarker>,
    body: &Value,
    attachments: &[Value],
) -> Value {
    json!({
        "id": id.to_string(),
        "channel_id": channel.to_string(),
        "application_id": APPLICATION_ID.to_string(),
        "type": 0,
        "content": body.get("content").and_then(Value::as_str).unwrap_or_default(),
        "tts": false,
        "author": {
            "id": APPLICATION_ID.to_string(),
            "username": "noctis-test-app",
            "discriminator": "0",
            "avatar": null,
            "global_name": null,
            "bot": true,
        },
        "timestamp": TIMESTAMP,
        "edited_timestamp": null,
        "mention_everyone": false,
        "mentions": [],
        "mention_roles": [],
        "attachments": attachments,
        "embeds": [],
        "pinned": false,
        "flags": body.get("flags").and_then(Value::as_u64).unwrap_or(0),
        "components": body.get("components").cloned().unwrap_or_else(|| json!([])),
    })
}

/// A user object.
pub(crate) fn user_json(user: &TestUser) -> Value {
    json!({
        "id": user.id.to_string(),
        "username": user.username(),
        "discriminator": "0",
        "avatar": null,
        "global_name": null,
        "bot": false,
    })
}

/// A guild member object of `user`; `with_user` adds `user` (the
/// interaction's `member`; resolved members carry none).
pub(crate) fn member_json(user: &TestUser, with_user: bool) -> Value {
    let roles: Vec<String> = user.roles.iter().map(ToString::to_string).collect();
    let mut member = json!({
        "roles": roles,
        "permissions": user.permissions.bits().to_string(),
        "joined_at": TIMESTAMP,
        "deaf": false,
        "mute": false,
        "flags": 0,
        "nick": null,
        "avatar": null,
        "pending": false,
        "premium_since": null,
        "communication_disabled_until": null,
    });
    if with_user {
        member["user"] = user_json(user);
    }
    member
}

/// A role object.
fn role_json(id: Id<RoleMarker>, name: &str) -> Value {
    json!({
        "id": id.to_string(),
        "name": name,
        "color": 0,
        "colors": { "primary_color": 0, "secondary_color": null, "tertiary_color": null },
        "hoist": false,
        "icon": null,
        "unicode_emoji": null,
        "position": 1,
        "permissions": "0",
        "managed": false,
        "mentionable": false,
        "flags": 0,
    })
}

/// A resolved channel object.
fn channel_json(id: Id<ChannelMarker>, name: &str, kind: ChannelType) -> Value {
    json!({
        "id": id.to_string(),
        "name": name,
        "type": u8::from(kind),
        "permissions": Permissions::all().bits().to_string(),
    })
}

/// The `resolved` data of one interaction, built from the seeds.
#[derive(Debug, Default)]
pub(crate) struct Resolved {
    users: Map<String, Value>,
    members: Map<String, Value>,
    roles: Map<String, Value>,
    channels: Map<String, Value>,
    messages: Map<String, Value>,
}

impl Resolved {
    /// Adds a seeded user (and its member in a guild run).
    pub(crate) fn user(
        &mut self,
        seeds: &Seeds,
        ctx: &RunCtx,
        id: Id<UserMarker>,
    ) -> Result<(), TestError> {
        let user = seeds.user(id)?;
        self.users.insert(id.to_string(), user_json(user));
        if ctx.guild.is_some() {
            self.members
                .insert(id.to_string(), member_json(user, false));
        }
        Ok(())
    }

    /// Adds a seeded role.
    pub(crate) fn role(
        &mut self,
        seeds: &Seeds,
        ctx: &RunCtx,
        id: Id<RoleMarker>,
    ) -> Result<(), TestError> {
        let name = seeds.role(ctx.guild, id)?;
        self.roles.insert(id.to_string(), role_json(id, name));
        Ok(())
    }

    /// Adds a seeded channel.
    pub(crate) fn channel(
        &mut self,
        seeds: &Seeds,
        ctx: &RunCtx,
        id: Id<ChannelMarker>,
    ) -> Result<(), TestError> {
        let (name, kind) = seeds.channel(ctx.guild, id)?;
        self.channels
            .insert(id.to_string(), channel_json(id, name, kind));
        Ok(())
    }

    /// Adds a message.
    pub(crate) fn message(&mut self, id: Id<MessageMarker>, json: Value) {
        self.messages.insert(id.to_string(), json);
    }

    /// The `resolved` object, or `None` when nothing was added.
    pub(crate) fn into_value(self) -> Option<Value> {
        let mut out = Map::new();
        for (key, map) in [
            ("users", self.users),
            ("members", self.members),
            ("roles", self.roles),
            ("channels", self.channels),
            ("messages", self.messages),
        ] {
            if !map.is_empty() {
                out.insert(key.to_owned(), Value::Object(map));
            }
        }
        (!out.is_empty()).then_some(Value::Object(out))
    }
}

/// The base interaction object of a run by `ctx`: everything but `id`,
/// `token`, `data` and `message`.
pub(crate) fn interaction_json(kind: u8, seeds: &Seeds, ctx: &RunCtx) -> Result<Value, TestError> {
    let user = seeds.user(ctx.user)?;
    let mut d = json!({
        "application_id": APPLICATION_ID.to_string(),
        "type": kind,
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "locale": ctx.locale,
        "app_permissions": Permissions::all().bits().to_string(),
        "channel": { "id": ctx.channel.to_string(), "type": ctx.channel_type },
        "channel_id": ctx.channel.to_string(),
    });
    if let Some(guild) = ctx.guild {
        d["guild_id"] = json!(guild.to_string());
        d["guild_locale"] = json!(ctx.guild_locale.as_deref().unwrap_or("en-US"));
        d["member"] = member_json(user, true);
        d["context"] = json!(0);
    } else {
        d["user"] = user_json(user);
        d["context"] = json!(1);
    }
    Ok(d)
}

/// The channel and type of a run in `guild` (or a DM with `user`).
pub(crate) fn run_channel(
    seeds: &Seeds,
    guild: Option<Id<GuildMarker>>,
    user: Id<UserMarker>,
    channel: Option<Id<ChannelMarker>>,
) -> Result<(Id<ChannelMarker>, u8), TestError> {
    match (guild, channel) {
        (Some(guild), Some(channel)) => {
            let (_, kind) =
                seeds
                    .guild(guild)?
                    .find_channel(channel)
                    .ok_or(TestError::Unknown {
                        kind: "channel",
                        id: channel.get(),
                    })?;
            Ok((channel, u8::from(kind)))
        }
        (Some(guild), None) => {
            let seeded = seeds.guild(guild)?;
            Ok(seeded
                .channels
                .iter()
                .find(|(_, _, kind)| *kind == ChannelType::GuildText)
                .map_or((guild.cast(), 0), |(id, _, _)| (*id, 0)))
        }
        (None, Some(channel)) => {
            let (_, kind) = seeds.channel(None, channel)?;
            Ok((channel, u8::from(kind)))
        }
        (None, None) => Ok((user.cast(), 1)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

    use super::*;
    use crate::seed::{TestGuild, TestUser};

    fn seeds() -> Seeds {
        let mut seeds = Seeds::default();
        let a = TestUser::new(Id::new(40))
            .permissions(Permissions::MANAGE_GUILD)
            .roles([Id::new(30)]);
        seeds.users.insert(a.id, a);
        let g = TestGuild::new(Id::new(10), "Guild")
            .text_channel(Id::new(20), "logs")
            .role(Id::new(30), "Mods");
        seeds.guilds.insert(g.id, g);
        let empty = TestGuild::new(Id::new(11), "Empty");
        seeds.guilds.insert(empty.id, empty);
        seeds
    }

    fn ctx(guild: Option<u64>, seeds: &Seeds) -> RunCtx {
        let guild = guild.map(Id::new);
        let (channel, channel_type) = run_channel(seeds, guild, Id::new(40), None).unwrap();
        RunCtx {
            user: Id::new(40),
            guild,
            channel,
            channel_type,
            locale: "en-US".to_owned(),
            guild_locale: guild.map(|_| "en-US".to_owned()),
        }
    }

    /// S1: a guild run carries the member's permissions and roles.
    #[test]
    fn guild_run_carries_the_member() {
        let seeds = seeds();
        let d = interaction_json(2, &seeds, &ctx(Some(10), &seeds)).unwrap();
        assert_eq!(d["guild_id"], "10");
        assert_eq!(
            d["member"]["permissions"],
            Permissions::MANAGE_GUILD.bits().to_string()
        );
        assert_eq!(d["member"]["roles"], json!(["30"]));
        assert_eq!(d["member"]["user"]["id"], "40");
        assert_eq!(d["channel"], json!({ "id": "20", "type": 0 }));
        assert_eq!(d["context"], 0);
        assert!(d.get("user").is_none());
    }

    /// S5: a guild without channels runs in a text channel with its id.
    #[test]
    fn guild_without_channels_uses_its_id() {
        let seeds = seeds();
        let d = interaction_json(2, &seeds, &ctx(Some(11), &seeds)).unwrap();
        assert_eq!(d["channel"], json!({ "id": "11", "type": 0 }));
    }

    /// S6: a DM run carries `user`, no `member`, the DM channel.
    #[test]
    fn dm_run_carries_the_user() {
        let seeds = seeds();
        let d = interaction_json(2, &seeds, &ctx(None, &seeds)).unwrap();
        assert_eq!(d["user"]["id"], "40");
        assert!(d.get("member").is_none());
        assert_eq!(d["channel"], json!({ "id": "40", "type": 1 }));
        assert_eq!(d["context"], 1);
    }

    #[test]
    fn unseeded_ids_are_unknown() {
        let seeds = seeds();
        assert!(matches!(
            run_channel(&seeds, Some(Id::new(98)), Id::new(40), None),
            Err(TestError::Unknown {
                kind: "guild",
                id: 98
            })
        ));
        assert!(matches!(
            run_channel(&seeds, Some(Id::new(10)), Id::new(40), Some(Id::new(97))),
            Err(TestError::Unknown {
                kind: "channel",
                id: 97
            })
        ));
        let mut resolved = Resolved::default();
        let c = ctx(Some(10), &seeds);
        assert!(matches!(
            resolved.role(&seeds, &c, Id::new(99)),
            Err(TestError::Unknown {
                kind: "role",
                id: 99
            })
        ));
        resolved.user(&seeds, &c, Id::new(40)).unwrap();
        let value = resolved.into_value().unwrap();
        assert_eq!(value["users"]["40"]["id"], "40");
        assert!(value["members"]["40"].get("user").is_none());
    }

    /// Story 9.1: kept entries copy the previous object, unknown ids are
    /// dropped, new ones get a fresh id, size and CDN URLs; the message
    /// parses as twilight's `Message`.
    #[test]
    fn attachments_echo_like_discord() {
        use twilight_model::channel::Message;

        let wire = Wire::new();
        let channel = Id::new(20);
        let previous = [json!({ "id": "5", "filename": "old.png", "size": 1,
            "url": "https://cdn.discordapp.com/attachments/20/5/old.png",
            "proxy_url": "https://cdn.discordapp.com/attachments/20/5/old.png" })];
        let uploads = [RecordedFile {
            name: "files[0]".to_owned(),
            filename: "new.png".to_owned(),
            bytes: bytes::Bytes::from_static(b"abc"),
        }];
        let body = json!({ "attachments": [
            { "id": "5" },
            { "id": "6" },
            { "id": 0, "filename": "new.png", "description": "alt" },
            { "id": 1, "filename": "missing.png" },
        ] });
        let echoed = wire.echo(&body, &uploads, &previous, channel);
        assert_eq!(echoed.len(), 2);
        assert_eq!(echoed[0], previous[0]);
        assert_eq!(echoed[1]["filename"], "new.png");
        assert_eq!(echoed[1]["size"], 3);
        assert_eq!(echoed[1]["description"], "alt");
        let id = echoed[1]["id"].as_str().unwrap();
        assert_eq!(
            echoed[1]["url"],
            format!("https://cdn.discordapp.com/attachments/20/{id}/new.png")
        );
        assert_eq!(wire.echo(&json!({}), &[], &previous, channel), previous);
        let message = message_json_with(Id::new(7), channel, &json!({}), &echoed);
        let parsed = serde_json::from_value::<Message>(message);
        assert!(parsed.is_ok(), "{parsed:?}");
        assert_eq!(parsed.unwrap().attachments.len(), 2);
    }

    /// Every shape the testkit sends parses as twilight's `Interaction`, so
    /// the shard never drops a run (which would surface as a timeout).
    #[test]
    fn interactions_parse_as_twilight_interactions() {
        use twilight_model::application::interaction::Interaction;

        let seeds = seeds();
        for guild in [Some(10), Some(11), None] {
            let c = ctx(guild, &seeds);
            let mut resolved = Resolved::default();
            resolved.user(&seeds, &c, Id::new(40)).unwrap();
            if guild == Some(10) {
                resolved.role(&seeds, &c, Id::new(30)).unwrap();
                resolved.channel(&seeds, &c, Id::new(20)).unwrap();
            }
            resolved.message(
                Id::new(555),
                message_json(Id::new(555), c.channel, &json!({ "content": "" })),
            );
            let mut d = interaction_json(2, &seeds, &c).unwrap();
            d["id"] = json!("1");
            d["token"] = json!("t");
            d["data"] = json!({ "id": "2", "name": "x", "type": 1 });
            d["data"]["resolved"] = resolved.into_value().unwrap();
            let parsed = serde_json::from_value::<Interaction>(d.clone());
            assert!(parsed.is_ok(), "{parsed:?}\n{d}");
        }
    }
}
