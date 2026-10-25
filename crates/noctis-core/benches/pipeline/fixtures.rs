//! The bench's handlers and its pre-parsed items: every `Event` and
//! `Interaction` is built here, before any timing starts.

use std::time::Duration;

use bytes::Bytes;
use noctis::__private::codec::{Parts, encode};
use noctis::events::{EventCtx, MessageDelete};
use noctis::prelude::*;
use noctis_core::__testing::{Inbound, InboundKind, RawJson};
use serde_json::{Value, json};
use tokio::time::Instant;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::Message;
use twilight_model::gateway::ShardId;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming;
use twilight_model::guild::Permissions;
use twilight_model::id::Id;

use crate::recorder::{MAX_GUILDS, recorder};

/// The guild of every item outside the ordered scenario.
pub(crate) const GUILD: u64 = 1;
/// The channel of every item.
const CHANNEL: u64 = 2;
/// The user of every interaction and the author of every message.
const USER: u64 = 3;
/// The testkit's application id.
const APPLICATION_ID: u64 = 1_100_000_000_000_000_000;
/// Interaction ids are `INTERACTION_BASE + seq`.
const INTERACTION_BASE: u64 = 1_000_000_000_000;
/// How long the ordered handler sleeps on the slow guild.
const SLOW_SLEEP: Duration = Duration::from_millis(50);
/// A fixed message timestamp.
const TIMESTAMP: &str = "2026-01-01T00:00:00.000000+00:00";

/// The ordered sub-queue hash of `noctis-core`'s worker (Story 16.1
/// baseline), copied so the slow guild shares guild 1's sub-queue.
const fn subqueue(id: u64) -> u64 {
    id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58
}

/// The smallest guild id above 10 000 in guild 1's ordered sub-queue.
const fn slow_guild() -> u64 {
    let mut id = 10_001;
    while subqueue(id) != subqueue(1) {
        id += 1;
    }
    id
}

/// The ordered scenario's slow guild: never one of the fast guilds
/// `1..=10 000`, always in guild 1's sub-queue.
pub(crate) const SLOW_GUILD: u64 = slow_guild();

/// Checks [`SLOW_GUILD`] before anything is measured.
///
/// # Errors
///
/// Why the slow guild does not hold.
pub(crate) fn self_check() -> Result<(), String> {
    let max = u64::try_from(MAX_GUILDS).map_err(|error| error.to_string())?;
    if SLOW_GUILD <= max || subqueue(SLOW_GUILD) != subqueue(1) {
        return Err(format!("slow guild self-check: {SLOW_GUILD} does not hold"));
    }
    if (max + 1..SLOW_GUILD).any(|id| subqueue(id) == subqueue(1)) {
        return Err(format!(
            "slow guild self-check: {SLOW_GUILD} is not the smallest"
        ));
    }
    Ok(())
}

/// The recorder slot of guild `id`: 0 for the slow guild, the id itself for
/// a fast one.
fn guild_slot(id: Option<Id<twilight_model::id::marker::GuildMarker>>) -> usize {
    match id.map(Id::get) {
        Some(SLOW_GUILD) => 0,
        Some(id) => usize::try_from(id).unwrap_or(usize::MAX),
        None => usize::MAX,
    }
}

/// Defines a concurrent `MESSAGE_DELETE` no-op handler recording as
/// handler `$index`.
macro_rules! delete_handler {
    ($name:ident, $index:literal) => {
        #[derive(EventHandler)]
        pub(crate) struct $name;

        impl $name {
            #[handler]
            async fn handle(self, _ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
                let now = Instant::now();
                recorder().start(ev.id.get() - 1, $index, now);
                Ok(())
            }
        }
    };
}

delete_handler!(Delete0, 0);
delete_handler!(Delete1, 1);
delete_handler!(Delete2, 2);
delete_handler!(Delete3, 3);

/// The ordered `MESSAGE_DELETE` handler of scenario (c): sleeps on the slow
/// guild and checks per-guild start order.
#[derive(EventHandler)]
#[event(ordered)]
pub(crate) struct OrderedDelete;

impl OrderedDelete {
    #[handler]
    async fn handle(self, _ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
        let now = Instant::now();
        let seq = ev.id.get() - 1;
        recorder().start(seq, 0, now);
        let slot = guild_slot(ev.guild_id);
        recorder().ordered(slot, seq);
        if slot == 0 {
            tokio::time::sleep(SLOW_SLEEP).await;
        }
        Ok(())
    }
}

/// The component of the mix: a button carrying the item's seq.
#[derive(Button)]
#[button("tap", defaults(style = Primary, label = "Tap"))]
pub(crate) struct Tap {
    seq: u64,
}

impl Tap {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        let now = Instant::now();
        recorder().start(self.seq, 0, now);
        Ok(r.defer_update().await?.done())
    }
}

/// The command of the mix: `/seq` with the item's seq as its option.
#[derive(SlashCommand)]
#[command("seq", description = "Bench item")]
pub(crate) struct Seq {
    #[option(description = "Item")]
    seq: i64,
}

impl Seq {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let now = Instant::now();
        recorder().start(u64::try_from(self.seq).unwrap_or(u64::MAX), 0, now);
        Ok(r.defer_reply().await?.done())
    }
}

/// What one item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `MESSAGE_CREATE`, which no handler takes.
    Create,
    /// `MESSAGE_DELETE` in `guild`.
    Delete { guild: u64 },
    /// A `Tap` click.
    Tap,
    /// A `/seq` command.
    Seq,
}

impl Kind {
    /// A handled `MESSAGE_DELETE` in [`GUILD`].
    pub(crate) const DELETE: Self = Self::Delete { guild: GUILD };

    pub(crate) fn is_interaction(self) -> bool {
        matches!(self, Self::Tap | Self::Seq)
    }

    pub(crate) fn is_slow(self) -> bool {
        self == Self::Delete { guild: SLOW_GUILD }
    }
}

/// The templates every item is built from.
pub(crate) struct Fixtures {
    message: Message,
    interaction: Value,
}

impl Fixtures {
    /// Parses the templates.
    ///
    /// # Errors
    ///
    /// The message template does not deserialize.
    pub(crate) fn new() -> Result<Self, String> {
        let message = serde_json::from_value(message_json())
            .map_err(|error| format!("the MESSAGE_CREATE fixture is invalid: {error}"))?;
        Ok(Self {
            message,
            interaction: interaction_json(),
        })
    }

    /// Item `seq` of kind `kind` on shard `shard`, stamped with a
    /// placeholder `received_at` the producer replaces.
    ///
    /// # Errors
    ///
    /// An interaction fixture that does not deserialize.
    pub(crate) fn inbound(&self, seq: u64, kind: Kind, shard: ShardId) -> Result<Inbound, String> {
        let kind = match kind {
            Kind::Create => InboundKind::Event(Box::new(Event::MessageCreate(Box::new(
                incoming::MessageCreate(self.message.clone()),
            )))),
            Kind::Delete { guild } => {
                InboundKind::Event(Box::new(Event::MessageDelete(MessageDelete {
                    id: Id::new(seq + 1),
                    channel_id: Id::new(CHANNEL),
                    guild_id: Some(Id::new(guild)),
                })))
            }
            Kind::Tap => {
                let custom_id = encode::<Tap>(&Tap { seq }, &Parts::new())
                    .map_err(|error| format!("cannot encode Tap {{ seq: {seq} }}: {error:?}"))?
                    .to_string();
                let data = json!({ "custom_id": custom_id, "component_type": 2 });
                self.interaction(seq, 3, data, true)?
            }
            Kind::Seq => {
                let data = json!({
                    "id": "900",
                    "name": "seq",
                    "type": 1,
                    "options": [{ "name": "seq", "type": 4, "value": seq }],
                });
                self.interaction(seq, 2, data, false)?
            }
        };
        Ok(Inbound {
            shard,
            received_at: Instant::now(),
            kind,
        })
    }

    /// An interaction of type `kind` with `data`, parsed from the same bytes
    /// its `RawJson` carries.
    fn interaction(
        &self,
        seq: u64,
        kind: u8,
        data: Value,
        with_message: bool,
    ) -> Result<InboundKind, String> {
        let mut d = self.interaction.clone();
        d["id"] = json!((INTERACTION_BASE + seq).to_string());
        d["token"] = json!(format!("bench-{seq}"));
        d["type"] = json!(kind);
        d["data"] = data;
        if with_message {
            d["message"] = message_json();
        }
        let bytes = serde_json::to_vec(&d).map_err(|error| error.to_string())?;
        let interaction: Interaction = serde_json::from_slice(&bytes).map_err(|error| {
            format!("the interaction fixture (type {kind}) is invalid: {error}")
        })?;
        Ok(InboundKind::Interaction(
            Box::new(interaction),
            RawJson::new(Bytes::from(bytes)),
        ))
    }
}

/// The bench's message: in [`GUILD`], by [`USER`], no content.
fn message_json() -> Value {
    json!({
        "id": "10",
        "channel_id": CHANNEL.to_string(),
        "guild_id": GUILD.to_string(),
        "author": { "id": USER.to_string(), "username": "bench", "discriminator": "0", "avatar": null },
        "content": "",
        "timestamp": TIMESTAMP,
        "edited_timestamp": null,
        "tts": false,
        "mention_everyone": false,
        "mentions": [],
        "mention_roles": [],
        "attachments": [],
        "embeds": [],
        "pinned": false,
        "type": 0,
        "flags": 0,
    })
}

/// The guild interaction base (the testkit wire's shape): everything but
/// `id`, `token`, `type`, `data` and `message`.
fn interaction_json() -> Value {
    json!({
        "application_id": APPLICATION_ID.to_string(),
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "locale": "en-US",
        "app_permissions": Permissions::all().bits().to_string(),
        "channel": { "id": CHANNEL.to_string(), "type": 0 },
        "channel_id": CHANNEL.to_string(),
        "guild_id": GUILD.to_string(),
        "guild_locale": "en-US",
        "member": {
            "user": {
                "id": USER.to_string(),
                "username": "bench",
                "discriminator": "0",
                "avatar": null,
                "global_name": null,
                "bot": false,
            },
            "roles": [],
            "permissions": Permissions::all().bits().to_string(),
            "joined_at": TIMESTAMP,
            "deaf": false,
            "mute": false,
            "flags": 0,
            "nick": null,
            "avatar": null,
            "pending": false,
            "premium_since": null,
            "communication_disabled_until": null,
        },
        "context": 0,
    })
}
