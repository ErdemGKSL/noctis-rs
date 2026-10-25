//! Gateway event payloads an `#[event]` handler can take, with the event
//! type the gateway parses for each and the intent it derives (FR-6,
//! AD-7, GW-05).
//!
//! Every type here implements [`EventPayload`]. Registering a handler for
//! one makes the App parse that event type and request its intent:
//! non-privileged intents are added automatically, and a privileged one
//! (`GUILD_MEMBERS`, `GUILD_PRESENCES`) must be requested with
//! `AppBuilder::intents` or `build()` fails with
//! `BuildViolation::MissingIntent` (GW-05). Interactions are not payloads
//! here: commands and Routes handle them.
//!
//! | Intent | Payloads |
//! | --- | --- |
//! | `AUTO_MODERATION_CONFIGURATION` | [`AutoModerationRuleCreate`], [`AutoModerationRuleDelete`], [`AutoModerationRuleUpdate`] |
//! | `AUTO_MODERATION_EXECUTION` | [`AutoModerationActionExecution`] |
//! | `GUILD_MODERATION` | [`BanAdd`], [`BanRemove`], [`GuildAuditLogEntryCreate`] |
//! | `GUILDS` | [`ChannelCreate`], [`ChannelDelete`], [`ChannelPinsUpdate`], [`ChannelUpdate`], [`GuildCreate`], [`GuildDelete`], [`GuildUpdate`], [`UnavailableGuild`], [`RoleCreate`], [`RoleDelete`], [`RoleUpdate`], [`StageInstanceCreate`], [`StageInstanceDelete`], [`StageInstanceUpdate`], [`ThreadCreate`], [`ThreadDelete`], [`ThreadListSync`], [`ThreadMemberUpdate`], [`ThreadMembersUpdate`], [`ThreadUpdate`] |
//! | `GUILD_EMOJIS_AND_STICKERS` | [`GuildEmojisUpdate`], [`GuildStickersUpdate`] |
//! | `GUILD_INTEGRATIONS` | [`GuildIntegrationsUpdate`], [`IntegrationCreate`], [`IntegrationDelete`], [`IntegrationUpdate`] |
//! | `GUILD_INVITES` | [`InviteCreate`], [`InviteDelete`] |
//! | `GUILD_WEBHOOKS` | [`WebhooksUpdate`] |
//! | `GUILD_VOICE_STATES` | [`VoiceStateUpdate`] |
//! | `GUILD_SCHEDULED_EVENTS` | [`GuildScheduledEventCreate`], [`GuildScheduledEventDelete`], [`GuildScheduledEventUpdate`], [`GuildScheduledEventUserAdd`], [`GuildScheduledEventUserRemove`] |
//! | `GUILD_MEMBERS` (privileged) | [`MemberAdd`], [`MemberRemove`], [`MemberUpdate`] |
//! | `GUILD_PRESENCES` (privileged) | [`PresenceUpdate`] |
//! | `GUILD_MESSAGES` | [`MessageCreate`], [`MessageDelete`], [`MessageDeleteBulk`], [`MessageUpdate`] |
//! | `GUILD_MESSAGE_POLLS` | [`MessagePollVoteAdd`], [`MessagePollVoteRemove`] |
//! | `GUILD_MESSAGE_REACTIONS` | [`ReactionAdd`], [`ReactionRemove`], [`ReactionRemoveAll`], [`ReactionRemoveEmoji`] |
//! | `GUILD_MESSAGE_TYPING` | [`TypingStart`] |
//! | none | [`CommandPermissionsUpdate`], [`EntitlementCreate`], [`EntitlementDelete`], [`EntitlementUpdate`], [`MemberChunk`], [`UserUpdate`], [`VoiceServerUpdate`] |
//!
//! `UnavailableGuild` handlers run on a `GUILD_DELETE` dispatch with
//! `unavailable: true` (an outage); a guild the bot left or was removed from
//! reaches only `GuildDelete` handlers. An `UnavailableGuild` handler
//! therefore makes the gateway parse `GUILD_DELETE`.
//!
//! Only guild intents are derived: `DIRECT_MESSAGES`,
//! `DIRECT_MESSAGE_REACTIONS` and `DIRECT_MESSAGE_TYPING` are never added
//! automatically, so a bot that handles direct messages, DM reactions or DM
//! typing requests them with `AppBuilder::intents`.

use noctis_gateway::EventTypeFlags;
use twilight_model::gateway::Intents;
use twilight_model::gateway::event::{Event, EventType};

pub use crate::handler::{EventCtx, EventHandler};

pub(crate) mod sealed {
    use noctis_gateway::EventTypeFlags;
    use twilight_model::gateway::Intents;
    use twilight_model::gateway::event::{Event, EventType};

    /// The per-payload data the Registry and the workers read.
    pub trait Sealed: Sized {
        /// The event type the payload is named after (logs, GW-05).
        const KIND: EventType;
        /// The event type the gateway dispatches it as: the single type
        /// whose flag is `PARSE`. Equals `KIND` except for
        /// `UnavailableGuild`, which arrives as `GUILD_DELETE`.
        const DISPATCH: EventType;
        /// The event types the gateway must parse to produce it.
        const PARSE: EventTypeFlags;
        /// The intent it needs; empty for none.
        const INTENT: Intents;

        /// The payload of `event`, unboxed; `None` when `event` does not
        /// carry it.
        fn extract(event: Event) -> Option<Self>;

        /// Whether `event` carries the payload: `extract(event.clone())`
        /// is `Some` exactly when this is `true`, without the clone.
        fn accepts(event: &Event) -> bool;
    }
}

/// A gateway event payload an `#[event]` handler can take: one of the
/// types in [`noctis_core::events`](self). Sealed.
///
/// ```
/// use noctis_core::events::{EventPayload, MessageCreate};
///
/// fn takes<P: EventPayload>() {}
/// takes::<MessageCreate>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a gateway event payload",
    note = "use a type from `noctis::events`; interactions are handled by commands and Routes"
)]
pub trait EventPayload: sealed::Sealed + Send + 'static {}

/// Implements [`EventPayload`] for each payload:
/// `Type(boxed|plain|custom) [dispatch Other] parse FLAG intent INTENT|none`.
///
/// `boxed`/`plain` extract the `Event::Type` variant (unboxing it);
/// `custom` calls the hand-written `extract_<type>` and `accepts_<type>`
/// functions below.
macro_rules! payloads {
    (@extract boxed $ty:ident $event:ident) => {
        match $event {
            Event::$ty(e) => Some(*e),
            _ => None,
        }
    };
    (@extract plain $ty:ident $event:ident) => {
        match $event {
            Event::$ty(e) => Some(e),
            _ => None,
        }
    };
    (@extract custom UnavailableGuild $event:ident) => {
        extract_unavailable_guild($event)
    };
    (@accepts boxed $ty:ident $event:ident) => {
        matches!($event, Event::$ty(_))
    };
    (@accepts plain $ty:ident $event:ident) => {
        matches!($event, Event::$ty(_))
    };
    (@accepts custom UnavailableGuild $event:ident) => {
        accepts_unavailable_guild($event)
    };
    (@dispatch $ty:ident) => { EventType::$ty };
    (@dispatch $ty:ident $dispatch:ident) => { EventType::$dispatch };
    (@intent none) => { Intents::empty() };
    (@intent $intent:ident) => { Intents::$intent };
    ($($ty:ident($how:ident) $(dispatch $dispatch:ident)? parse $flag:ident intent $intent:ident;)*) => {
        $(
            pub use twilight_model::gateway::payload::incoming::$ty;

            impl sealed::Sealed for $ty {
                const KIND: EventType = EventType::$ty;
                const DISPATCH: EventType = payloads!(@dispatch $ty $($dispatch)?);
                const PARSE: EventTypeFlags = EventTypeFlags::$flag;
                const INTENT: Intents = payloads!(@intent $intent);

                fn extract(event: Event) -> Option<Self> {
                    payloads!(@extract $how $ty event)
                }

                fn accepts(event: &Event) -> bool {
                    payloads!(@accepts $how $ty event)
                }
            }

            impl EventPayload for $ty {}
        )*

        /// Every payload's `(name, KIND, DISPATCH, PARSE, INTENT)`, in table
        /// order.
        #[cfg(test)]
        pub(crate) const TABLE: &[(&str, EventType, EventType, EventTypeFlags, Intents)] = &[
            $((
                stringify!($ty),
                <$ty as sealed::Sealed>::KIND,
                <$ty as sealed::Sealed>::DISPATCH,
                <$ty as sealed::Sealed>::PARSE,
                <$ty as sealed::Sealed>::INTENT,
            ),)*
        ];

        /// Every payload's `(name, accepts, extract(..).is_some())`, in
        /// table order.
        #[cfg(test)]
        pub(crate) const PROBES: &[Probe] = &[
            $((
                stringify!($ty),
                <$ty as sealed::Sealed>::accepts,
                |event| <$ty as sealed::Sealed>::extract(event).is_some(),
            ),)*
        ];
    };
}

/// twilight-model deserializes every `GUILD_DELETE` dispatch into
/// `Event::GuildDelete`; an outage is the one with `unavailable: true`.
fn extract_unavailable_guild(event: Event) -> Option<UnavailableGuild> {
    match event {
        Event::GuildDelete(GuildDelete {
            id,
            unavailable: Some(true),
        }) => Some(UnavailableGuild { id }),
        Event::UnavailableGuild(unavailable) => Some(unavailable),
        _ => None,
    }
}

/// Whether [`extract_unavailable_guild`] takes `event`.
fn accepts_unavailable_guild(event: &Event) -> bool {
    matches!(
        event,
        Event::GuildDelete(GuildDelete {
            unavailable: Some(true),
            ..
        }) | Event::UnavailableGuild(_)
    )
}

/// Every payload's `accepts`, in table order (the accepts/extract
/// equivalence test).
#[cfg(test)]
pub(crate) type Probe = (&'static str, fn(&Event) -> bool, fn(Event) -> bool);

payloads! {
    AutoModerationRuleCreate(plain) parse AUTO_MODERATION_RULE_CREATE intent AUTO_MODERATION_CONFIGURATION;
    AutoModerationRuleDelete(plain) parse AUTO_MODERATION_RULE_DELETE intent AUTO_MODERATION_CONFIGURATION;
    AutoModerationRuleUpdate(plain) parse AUTO_MODERATION_RULE_UPDATE intent AUTO_MODERATION_CONFIGURATION;
    AutoModerationActionExecution(plain) parse AUTO_MODERATION_ACTION_EXECUTION intent AUTO_MODERATION_EXECUTION;
    BanAdd(plain) parse BAN_ADD intent GUILD_MODERATION;
    BanRemove(plain) parse BAN_REMOVE intent GUILD_MODERATION;
    GuildAuditLogEntryCreate(boxed) parse GUILD_AUDIT_LOG_ENTRY_CREATE intent GUILD_MODERATION;
    ChannelCreate(boxed) parse CHANNEL_CREATE intent GUILDS;
    ChannelDelete(boxed) parse CHANNEL_DELETE intent GUILDS;
    ChannelPinsUpdate(plain) parse CHANNEL_PINS_UPDATE intent GUILDS;
    ChannelUpdate(boxed) parse CHANNEL_UPDATE intent GUILDS;
    GuildCreate(boxed) parse GUILD_CREATE intent GUILDS;
    GuildDelete(plain) parse GUILD_DELETE intent GUILDS;
    GuildUpdate(boxed) parse GUILD_UPDATE intent GUILDS;
    UnavailableGuild(custom) dispatch GuildDelete parse GUILD_DELETE intent GUILDS;
    RoleCreate(plain) parse ROLE_CREATE intent GUILDS;
    RoleDelete(plain) parse ROLE_DELETE intent GUILDS;
    RoleUpdate(plain) parse ROLE_UPDATE intent GUILDS;
    StageInstanceCreate(plain) parse STAGE_INSTANCE_CREATE intent GUILDS;
    StageInstanceDelete(plain) parse STAGE_INSTANCE_DELETE intent GUILDS;
    StageInstanceUpdate(plain) parse STAGE_INSTANCE_UPDATE intent GUILDS;
    ThreadCreate(boxed) parse THREAD_CREATE intent GUILDS;
    ThreadDelete(plain) parse THREAD_DELETE intent GUILDS;
    ThreadListSync(plain) parse THREAD_LIST_SYNC intent GUILDS;
    ThreadMemberUpdate(boxed) parse THREAD_MEMBER_UPDATE intent GUILDS;
    ThreadMembersUpdate(plain) parse THREAD_MEMBERS_UPDATE intent GUILDS;
    ThreadUpdate(boxed) parse THREAD_UPDATE intent GUILDS;
    GuildEmojisUpdate(plain) parse GUILD_EMOJIS_UPDATE intent GUILD_EMOJIS_AND_STICKERS;
    GuildStickersUpdate(plain) parse GUILD_STICKERS_UPDATE intent GUILD_EMOJIS_AND_STICKERS;
    GuildIntegrationsUpdate(plain) parse GUILD_INTEGRATIONS_UPDATE intent GUILD_INTEGRATIONS;
    IntegrationCreate(boxed) parse INTEGRATION_CREATE intent GUILD_INTEGRATIONS;
    IntegrationDelete(plain) parse INTEGRATION_DELETE intent GUILD_INTEGRATIONS;
    IntegrationUpdate(boxed) parse INTEGRATION_UPDATE intent GUILD_INTEGRATIONS;
    InviteCreate(boxed) parse INVITE_CREATE intent GUILD_INVITES;
    InviteDelete(plain) parse INVITE_DELETE intent GUILD_INVITES;
    WebhooksUpdate(plain) parse WEBHOOKS_UPDATE intent GUILD_WEBHOOKS;
    VoiceStateUpdate(boxed) parse VOICE_STATE_UPDATE intent GUILD_VOICE_STATES;
    GuildScheduledEventCreate(boxed) parse GUILD_SCHEDULED_EVENT_CREATE intent GUILD_SCHEDULED_EVENTS;
    GuildScheduledEventDelete(boxed) parse GUILD_SCHEDULED_EVENT_DELETE intent GUILD_SCHEDULED_EVENTS;
    GuildScheduledEventUpdate(boxed) parse GUILD_SCHEDULED_EVENT_UPDATE intent GUILD_SCHEDULED_EVENTS;
    GuildScheduledEventUserAdd(plain) parse GUILD_SCHEDULED_EVENT_USER_ADD intent GUILD_SCHEDULED_EVENTS;
    GuildScheduledEventUserRemove(plain) parse GUILD_SCHEDULED_EVENT_USER_REMOVE intent GUILD_SCHEDULED_EVENTS;
    MemberAdd(boxed) parse MEMBER_ADD intent GUILD_MEMBERS;
    MemberRemove(plain) parse MEMBER_REMOVE intent GUILD_MEMBERS;
    MemberUpdate(boxed) parse MEMBER_UPDATE intent GUILD_MEMBERS;
    PresenceUpdate(boxed) parse PRESENCE_UPDATE intent GUILD_PRESENCES;
    MessageCreate(boxed) parse MESSAGE_CREATE intent GUILD_MESSAGES;
    MessageDelete(plain) parse MESSAGE_DELETE intent GUILD_MESSAGES;
    MessageDeleteBulk(plain) parse MESSAGE_DELETE_BULK intent GUILD_MESSAGES;
    MessageUpdate(boxed) parse MESSAGE_UPDATE intent GUILD_MESSAGES;
    MessagePollVoteAdd(plain) parse MESSAGE_POLL_VOTE_ADD intent GUILD_MESSAGE_POLLS;
    MessagePollVoteRemove(plain) parse MESSAGE_POLL_VOTE_REMOVE intent GUILD_MESSAGE_POLLS;
    ReactionAdd(boxed) parse REACTION_ADD intent GUILD_MESSAGE_REACTIONS;
    ReactionRemove(boxed) parse REACTION_REMOVE intent GUILD_MESSAGE_REACTIONS;
    ReactionRemoveAll(plain) parse REACTION_REMOVE_ALL intent GUILD_MESSAGE_REACTIONS;
    ReactionRemoveEmoji(plain) parse REACTION_REMOVE_EMOJI intent GUILD_MESSAGE_REACTIONS;
    TypingStart(boxed) parse TYPING_START intent GUILD_MESSAGE_TYPING;
    CommandPermissionsUpdate(plain) parse COMMAND_PERMISSIONS_UPDATE intent none;
    EntitlementCreate(plain) parse ENTITLEMENT_CREATE intent none;
    EntitlementDelete(plain) parse ENTITLEMENT_DELETE intent none;
    EntitlementUpdate(plain) parse ENTITLEMENT_UPDATE intent none;
    MemberChunk(plain) parse MEMBER_CHUNK intent none;
    UserUpdate(plain) parse USER_UPDATE intent none;
    VoiceServerUpdate(plain) parse VOICE_SERVER_UPDATE intent none;
}

#[cfg(test)]
mod tests;
