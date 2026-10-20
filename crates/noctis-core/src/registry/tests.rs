//! Story 5.6 Matrix 1-24: the Registry fold, the derived filter and
//! intents, GW-05 and the new builder bounds.

use std::time::Duration;

use noctis_gateway::{DEFAULT_EVENT_CAPACITY, DEFAULT_OVERFLOW_CAPACITY};

use super::*;
use crate::events::*;
use crate::test_support::{noop, noop_with};
use crate::{App, AppBuilder, BuildError, Plugin, Runtime};

fn builder() -> AppBuilder {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    builder
}

fn names(registry: &Registry) -> Vec<&'static str> {
    registry
        .entries()
        .iter()
        .map(|e| e.handler.name())
        .collect()
}

fn violations(err: &BuildError) -> Vec<BuildViolation> {
    err.violations().to_vec()
}

/// Matrix 1: no modules.
#[tokio::test]
async fn no_modules() {
    let app = builder().build().unwrap();
    assert_eq!(app.intents(), Intents::GUILDS);
    let registry = app.registry();
    assert_eq!(registry.event_types(), EventTypeFlags::INTERACTION_CREATE);
    assert_eq!(registry.droppable(), DROPPABLE_EVENTS);
    assert!(!registry.has_ordered());
    assert_eq!(
        app.workers(crate::worker::Shutdown::default()).subqueues(),
        0
    );
}

/// Matrix 2: an empty module changes nothing.
#[tokio::test]
async fn empty_module() {
    let app = builder().module(Module::new("a/b")).build().unwrap();
    assert_eq!(app.intents(), Intents::GUILDS);
    assert_eq!(
        app.registry().event_types(),
        EventTypeFlags::INTERACTION_CREATE
    );
    assert_eq!(app.registry().droppable(), DROPPABLE_EVENTS);
    assert_eq!(
        app.workers(crate::worker::Shutdown::default()).subqueues(),
        0
    );
}

/// Matrix 3: one handler derives its intent and filter.
#[tokio::test]
async fn one_handler() {
    let app = builder()
        .module(Module::new("m").add(noop::<MessageCreate>("h")))
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MESSAGES);
    assert_eq!(
        app.registry().event_types(),
        EventTypeFlags::INTERACTION_CREATE | EventTypeFlags::MESSAGE_CREATE
    );
}

/// Matrix 4: pre-order fold with ancestor paths.
#[tokio::test]
async fn fold_order_and_ancestors() {
    let g = Module::new("g").add(noop::<MessageDelete>("h4"));
    let c = Module::new("c")
        .add(noop::<MessageDelete>("h2"))
        .add(noop::<MessageCreate>("h3"))
        .module(g);
    let p = Module::new("p").add(noop::<UserUpdate>("h1")).module(c);
    let r = Module::new("r").add(noop::<MessageDelete>("h5"));
    let app = builder().module(p).module(r).build().unwrap();
    let registry = app.registry();
    assert_eq!(names(registry), ["h1", "h2", "h3", "h4", "h5"]);
    let ancestors: Vec<Vec<&str>> = registry
        .entries()
        .iter()
        .map(|e| e.ancestors.to_vec())
        .collect();
    assert_eq!(
        ancestors,
        [vec![], vec!["p"], vec!["p"], vec!["p", "c"], vec![]]
    );
    let modules: Vec<&str> = registry.entries().iter().map(|e| e.module).collect();
    assert_eq!(modules, ["p", "c", "c", "g", "r"]);
    let deletes: Vec<&str> = registry
        .handlers(EventType::MessageDelete)
        .unwrap()
        .concurrent
        .iter()
        .map(|e| e.handler.name())
        .collect();
    assert_eq!(deletes, ["h2", "h4", "h5"]);
}

/// Matrix 5: `module` before `add` still folds the parent first.
#[tokio::test]
async fn call_order_irrelevant() {
    let c = Module::new("c").add(noop::<MessageDelete>("h2"));
    let p = Module::new("p").module(c).add(noop::<MessageDelete>("h1"));
    let app = builder().module(p).build().unwrap();
    assert_eq!(names(app.registry()), ["h1", "h2"]);
}

/// Matrix 6: the same descriptor twice is two entries.
#[tokio::test]
async fn same_descriptor_twice() {
    let h = noop::<MessageDelete>("h");
    let app = builder()
        .module(Module::new("m").add(h.clone()).add(h))
        .build()
        .unwrap();
    assert_eq!(names(app.registry()), ["h", "h"]);
}

/// Matrix 7: one tree mounted on two Apps.
#[tokio::test]
async fn tree_on_two_apps() {
    let tree = Module::new("m")
        .add(noop::<MessageDelete>("h"))
        .module(Module::new("m/c").add(noop::<MessageCreate>("h2")));
    let a = builder().module(tree.clone()).build().unwrap();
    let b = builder().module(tree).build().unwrap();
    assert_eq!(names(a.registry()), ["h", "h2"]);
    assert_eq!(names(b.registry()), ["h", "h2"]);
    assert!(!Arc::ptr_eq(a.registry(), b.registry()));
}

/// Matrix 8: ids are not validated yet.
#[tokio::test]
async fn ids_unvalidated() {
    for id in ["", "A B", "ü/🙂"] {
        let app = builder()
            .module(Module::new(id).add(noop::<MessageDelete>("h")))
            .build()
            .unwrap();
        assert_eq!(app.registry().entries()[0].module, id);
    }
}

struct Mounts;

impl Plugin for Mounts {
    fn install(&self, app: &mut AppBuilder) {
        app.module(Module::new("x").add(noop::<MessageDelete>("hx")));
    }
}

/// Matrix 9: a plugin's Module folds after the previously mounted roots.
#[tokio::test]
async fn plugin_mounts() {
    let app = builder()
        .plugin(Mounts)
        .module(Module::new("a").add(noop::<MessageDelete>("ha")))
        .build()
        .unwrap();
    assert_eq!(names(app.registry()), ["ha", "hx"]);
}

/// Matrix 10: a second build has no handlers.
#[tokio::test]
async fn builder_reset() {
    let mut b = builder();
    b.module(Module::new("m").add(noop::<MessageDelete>("h")))
        .event_lane(1, 0)
        .handler_concurrency(3);
    let first = b.build().unwrap();
    assert_eq!(first.registry().len(), 1);
    let second = b.build().unwrap();
    assert_eq!(second.registry().len(), 0);
    assert_eq!(
        second.registry().event_types(),
        EventTypeFlags::INTERACTION_CREATE
    );
    assert_eq!(second.handler_concurrency(), 256);
    assert_eq!(second.lane_config().event_capacity, DEFAULT_EVENT_CAPACITY);
}

/// Matrix 11: a privileged intent that was not requested.
#[tokio::test]
async fn gw_05_missing_members_intent() {
    let err = builder()
        .module(Module::new("welcome").add(noop::<MemberAdd>("bot::greet")))
        .build()
        .unwrap_err();
    assert_eq!(
        violations(&err),
        [BuildViolation::MissingIntent {
            module: "welcome",
            handler: "bot::greet",
            event: "GUILD_MEMBER_ADD",
            intent: Intents::GUILD_MEMBERS,
        }]
    );
    let text = err.violations()[0].to_string();
    assert!(text.starts_with("[GW-05] "), "{text}");
    assert!(text.contains("bot::greet"), "{text}");
    assert!(
        text.contains("`.intents(Intents::GUILD_MEMBERS)`"),
        "{text}"
    );
    assert!(text.contains("Discord Developer Portal"), "{text}");
}

/// Matrix 12: requesting the intent satisfies GW-05.
#[tokio::test]
async fn gw_05_satisfied() {
    let app = builder()
        .intents(Intents::GUILD_MEMBERS)
        .module(Module::new("welcome").add(noop::<MemberAdd>("greet")))
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MEMBERS);
}

/// Matrix 13: presence updates need `GUILD_PRESENCES`.
#[tokio::test]
async fn gw_05_presence() {
    let err = builder()
        .module(Module::new("m").add(noop::<PresenceUpdate>("p")))
        .build()
        .unwrap_err();
    let [BuildViolation::MissingIntent { intent, event, .. }] = violations(&err)[..] else {
        panic!("{err}");
    };
    assert_eq!(intent, Intents::GUILD_PRESENCES);
    assert_eq!(event, "PRESENCE_UPDATE");
    assert!(err.to_string().contains("GUILD_PRESENCES"));
}

/// Matrix 14: one violation per handler, in fold order.
#[tokio::test]
async fn gw_05_several() {
    let err = builder()
        .module(
            Module::new("m")
                .add(noop::<MemberAdd>("add"))
                .add(noop::<MemberRemove>("remove")),
        )
        .build()
        .unwrap_err();
    let handlers: Vec<&str> = err
        .violations()
        .iter()
        .map(|v| match v {
            BuildViolation::MissingIntent { handler, .. } => *handler,
            other => panic!("{other}"),
        })
        .collect();
    assert_eq!(handlers, ["add", "remove"]);
}

/// Matrix 15: GW-05 joins the other violations in one `BuildError`.
#[tokio::test]
async fn gw_05_mixed_violations() {
    let err = builder()
        .http_timeout(Duration::ZERO)
        .module(Module::new("m").add(noop::<MemberAdd>("add")))
        .build()
        .unwrap_err();
    assert_eq!(err.violations().len(), 2);
    assert!(matches!(
        err.violations()[0],
        BuildViolation::InvalidHttpTimeout { .. }
    ));
    assert!(matches!(
        err.violations()[1],
        BuildViolation::MissingIntent { .. }
    ));
}

/// Matrix 16: every payload alone derives exactly its intent and filter.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one line per payload type")]
async fn full_payload_table() {
    macro_rules! check {
        ($($ty:ident => $flag:ident, $intent:expr;)*) => {{
            let mut count = 0;
            $(
                count += 1;
                let intent: Intents = $intent;
                let privileged = intent.intersects(Intents::GUILD_MEMBERS | Intents::GUILD_PRESENCES);
                let mut b = builder();
                if privileged {
                    b.intents(intent);
                }
                let app = b
                    .module(Module::new("m").add(noop::<$ty>(stringify!($ty))))
                    .build()
                    .unwrap();
                assert_eq!(app.intents(), Intents::GUILDS | intent, stringify!($ty));
                assert_eq!(
                    app.registry().event_types(),
                    EventTypeFlags::INTERACTION_CREATE | EventTypeFlags::$flag,
                    stringify!($ty)
                );
                assert!(<$ty as sealed::Sealed>::KIND.name().is_some());
                if privileged {
                    let err = builder()
                        .module(Module::new("m").add(noop::<$ty>("h")))
                        .build()
                        .unwrap_err();
                    assert_eq!(err.violations().len(), 1, stringify!($ty));
                }
            )*
            count
        }};
    }
    use crate::events::sealed;
    let none = Intents::empty();
    let count = check! {
        AutoModerationRuleCreate => AUTO_MODERATION_RULE_CREATE, Intents::AUTO_MODERATION_CONFIGURATION;
        AutoModerationRuleDelete => AUTO_MODERATION_RULE_DELETE, Intents::AUTO_MODERATION_CONFIGURATION;
        AutoModerationRuleUpdate => AUTO_MODERATION_RULE_UPDATE, Intents::AUTO_MODERATION_CONFIGURATION;
        AutoModerationActionExecution => AUTO_MODERATION_ACTION_EXECUTION, Intents::AUTO_MODERATION_EXECUTION;
        BanAdd => BAN_ADD, Intents::GUILD_MODERATION;
        BanRemove => BAN_REMOVE, Intents::GUILD_MODERATION;
        GuildAuditLogEntryCreate => GUILD_AUDIT_LOG_ENTRY_CREATE, Intents::GUILD_MODERATION;
        ChannelCreate => CHANNEL_CREATE, Intents::GUILDS;
        ChannelDelete => CHANNEL_DELETE, Intents::GUILDS;
        ChannelPinsUpdate => CHANNEL_PINS_UPDATE, Intents::GUILDS;
        ChannelUpdate => CHANNEL_UPDATE, Intents::GUILDS;
        GuildCreate => GUILD_CREATE, Intents::GUILDS;
        GuildDelete => GUILD_DELETE, Intents::GUILDS;
        GuildUpdate => GUILD_UPDATE, Intents::GUILDS;
        UnavailableGuild => GUILD_DELETE, Intents::GUILDS;
        RoleCreate => ROLE_CREATE, Intents::GUILDS;
        RoleDelete => ROLE_DELETE, Intents::GUILDS;
        RoleUpdate => ROLE_UPDATE, Intents::GUILDS;
        StageInstanceCreate => STAGE_INSTANCE_CREATE, Intents::GUILDS;
        StageInstanceDelete => STAGE_INSTANCE_DELETE, Intents::GUILDS;
        StageInstanceUpdate => STAGE_INSTANCE_UPDATE, Intents::GUILDS;
        ThreadCreate => THREAD_CREATE, Intents::GUILDS;
        ThreadDelete => THREAD_DELETE, Intents::GUILDS;
        ThreadListSync => THREAD_LIST_SYNC, Intents::GUILDS;
        ThreadMemberUpdate => THREAD_MEMBER_UPDATE, Intents::GUILDS;
        ThreadMembersUpdate => THREAD_MEMBERS_UPDATE, Intents::GUILDS;
        ThreadUpdate => THREAD_UPDATE, Intents::GUILDS;
        GuildEmojisUpdate => GUILD_EMOJIS_UPDATE, Intents::GUILD_EMOJIS_AND_STICKERS;
        GuildStickersUpdate => GUILD_STICKERS_UPDATE, Intents::GUILD_EMOJIS_AND_STICKERS;
        GuildIntegrationsUpdate => GUILD_INTEGRATIONS_UPDATE, Intents::GUILD_INTEGRATIONS;
        IntegrationCreate => INTEGRATION_CREATE, Intents::GUILD_INTEGRATIONS;
        IntegrationDelete => INTEGRATION_DELETE, Intents::GUILD_INTEGRATIONS;
        IntegrationUpdate => INTEGRATION_UPDATE, Intents::GUILD_INTEGRATIONS;
        InviteCreate => INVITE_CREATE, Intents::GUILD_INVITES;
        InviteDelete => INVITE_DELETE, Intents::GUILD_INVITES;
        WebhooksUpdate => WEBHOOKS_UPDATE, Intents::GUILD_WEBHOOKS;
        VoiceStateUpdate => VOICE_STATE_UPDATE, Intents::GUILD_VOICE_STATES;
        GuildScheduledEventCreate => GUILD_SCHEDULED_EVENT_CREATE, Intents::GUILD_SCHEDULED_EVENTS;
        GuildScheduledEventDelete => GUILD_SCHEDULED_EVENT_DELETE, Intents::GUILD_SCHEDULED_EVENTS;
        GuildScheduledEventUpdate => GUILD_SCHEDULED_EVENT_UPDATE, Intents::GUILD_SCHEDULED_EVENTS;
        GuildScheduledEventUserAdd => GUILD_SCHEDULED_EVENT_USER_ADD, Intents::GUILD_SCHEDULED_EVENTS;
        GuildScheduledEventUserRemove => GUILD_SCHEDULED_EVENT_USER_REMOVE, Intents::GUILD_SCHEDULED_EVENTS;
        MemberAdd => MEMBER_ADD, Intents::GUILD_MEMBERS;
        MemberRemove => MEMBER_REMOVE, Intents::GUILD_MEMBERS;
        MemberUpdate => MEMBER_UPDATE, Intents::GUILD_MEMBERS;
        PresenceUpdate => PRESENCE_UPDATE, Intents::GUILD_PRESENCES;
        MessageCreate => MESSAGE_CREATE, Intents::GUILD_MESSAGES;
        MessageDelete => MESSAGE_DELETE, Intents::GUILD_MESSAGES;
        MessageDeleteBulk => MESSAGE_DELETE_BULK, Intents::GUILD_MESSAGES;
        MessageUpdate => MESSAGE_UPDATE, Intents::GUILD_MESSAGES;
        MessagePollVoteAdd => MESSAGE_POLL_VOTE_ADD, Intents::GUILD_MESSAGE_POLLS;
        MessagePollVoteRemove => MESSAGE_POLL_VOTE_REMOVE, Intents::GUILD_MESSAGE_POLLS;
        ReactionAdd => REACTION_ADD, Intents::GUILD_MESSAGE_REACTIONS;
        ReactionRemove => REACTION_REMOVE, Intents::GUILD_MESSAGE_REACTIONS;
        ReactionRemoveAll => REACTION_REMOVE_ALL, Intents::GUILD_MESSAGE_REACTIONS;
        ReactionRemoveEmoji => REACTION_REMOVE_EMOJI, Intents::GUILD_MESSAGE_REACTIONS;
        TypingStart => TYPING_START, Intents::GUILD_MESSAGE_TYPING;
        CommandPermissionsUpdate => COMMAND_PERMISSIONS_UPDATE, none;
        EntitlementCreate => ENTITLEMENT_CREATE, none;
        EntitlementDelete => ENTITLEMENT_DELETE, none;
        EntitlementUpdate => ENTITLEMENT_UPDATE, none;
        MemberChunk => MEMBER_CHUNK, none;
        UserUpdate => USER_UPDATE, none;
        VoiceServerUpdate => VOICE_SERVER_UPDATE, none;
    };
    assert_eq!(count, 64);
    assert_eq!(TABLE.len(), 64);
    // `PARSE` is the kind's flag, except `UnavailableGuild`.
    for (name, kind, _, parse, _) in TABLE {
        if *name == "UnavailableGuild" {
            assert_eq!(*parse, EventTypeFlags::GUILD_DELETE);
        } else {
            assert_eq!(*parse, EventTypeFlags::from(*kind), "{name}");
        }
    }
}

/// Matrix 17: `MessageCreate` derives neither content nor DM intents.
#[tokio::test]
async fn no_content_intent() {
    let app = builder()
        .module(Module::new("m").add(noop::<MessageCreate>("h")))
        .build()
        .unwrap();
    assert!(!app.intents().contains(Intents::MESSAGE_CONTENT));
    assert!(!app.intents().contains(Intents::DIRECT_MESSAGES));
}

/// Matrix 18: explicit intents stay.
#[tokio::test]
async fn explicit_extra_intent() {
    let app = builder()
        .intents(Intents::DIRECT_MESSAGES)
        .module(Module::new("m").add(noop::<MessageCreate>("h")))
        .build()
        .unwrap();
    assert_eq!(
        app.intents(),
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES
    );
}

/// Matrix 19: `UnavailableGuild` parses `GUILD_DELETE`.
#[tokio::test]
async fn unavailable_guild_parses_guild_delete() {
    let app = builder()
        .module(Module::new("m").add(noop::<UnavailableGuild>("h")))
        .build()
        .unwrap();
    assert!(
        app.registry()
            .event_types()
            .contains(EventTypeFlags::GUILD_DELETE)
    );
}

/// An `UnavailableGuild` handler sits in the `GUILD_DELETE` slot, merged
/// with `GuildDelete` handlers in fold order.
#[tokio::test]
async fn unavailable_guild_keyed_on_guild_delete() {
    let app = builder()
        .module(Module::new("m").add(noop::<UnavailableGuild>("u")))
        .build()
        .unwrap();
    let registry = app.registry();
    let slot = registry.handlers(EventType::GuildDelete).unwrap();
    assert_eq!(slot.concurrent.len(), 1);
    assert_eq!(slot.concurrent[0].handler.name(), "u");
    assert!(registry.handlers(EventType::UnavailableGuild).is_none());

    let app = builder()
        .module(
            Module::new("m")
                .add(noop::<GuildDelete>("g1"))
                .add(noop::<UnavailableGuild>("u"))
                .add(noop::<GuildDelete>("g2")),
        )
        .build()
        .unwrap();
    let registry = app.registry();
    let slot = registry.handlers(EventType::GuildDelete).unwrap();
    let order: Vec<_> = slot.concurrent.iter().map(|e| e.handler.name()).collect();
    assert_eq!(order, ["g1", "u", "g2"]);
    assert!(registry.handlers(EventType::UnavailableGuild).is_none());
}

/// Droppability folds by dispatch type: `GUILD_DELETE` covers both
/// payloads, and `UNAVAILABLE_GUILD` is never added.
#[tokio::test]
async fn droppable_unavailable_guild() {
    let app = builder()
        .module(
            Module::new("m")
                .add(noop_with::<UnavailableGuild>("u", false, true))
                .add(noop_with::<GuildDelete>("g", false, false)),
        )
        .build()
        .unwrap();
    let droppable = app.lane_config().droppable;
    assert!(!droppable.contains(EventTypeFlags::GUILD_DELETE));
    assert!(!droppable.contains(EventTypeFlags::UNAVAILABLE_GUILD));

    let app = builder()
        .module(Module::new("m").add(noop_with::<UnavailableGuild>("u", false, true)))
        .build()
        .unwrap();
    let droppable = app.lane_config().droppable;
    assert_eq!(droppable, DROPPABLE_EVENTS | EventTypeFlags::GUILD_DELETE);
    assert!(!droppable.contains(EventTypeFlags::UNAVAILABLE_GUILD));
}

/// Every parsed type has a handler slot: each payload alone is found under
/// the single event type of its `PARSE`.
#[test]
fn every_parsed_type_has_a_slot() {
    macro_rules! check {
        ($($ty:ident),* $(,)?) => {{
            let mut count = 0;
            $(
                count += 1;
                let parse = <$ty as crate::events::sealed::Sealed>::PARSE;
                let (registry, _) = Registry::fold(
                    &[Module::new("m").add(noop::<$ty>(stringify!($ty)))],
                    Intents::all(),
                );
                let slots: Vec<EventType> = TABLE
                    .iter()
                    .map(|(_, kind, _, _, _)| *kind)
                    .filter(|kind| EventTypeFlags::from(*kind) == parse)
                    .collect();
                assert_eq!(slots.len(), 1, stringify!($ty));
                let slot = registry.handlers(slots[0]);
                assert!(
                    slot.is_some_and(|h| h.ordered.len() + h.concurrent.len() == 1),
                    stringify!($ty)
                );
            )*
            assert_eq!(count, 64);
        }};
    }
    check!(
        AutoModerationRuleCreate,
        AutoModerationRuleDelete,
        AutoModerationRuleUpdate,
        AutoModerationActionExecution,
        BanAdd,
        BanRemove,
        GuildAuditLogEntryCreate,
        ChannelCreate,
        ChannelDelete,
        ChannelPinsUpdate,
        ChannelUpdate,
        GuildCreate,
        GuildDelete,
        GuildUpdate,
        UnavailableGuild,
        RoleCreate,
        RoleDelete,
        RoleUpdate,
        StageInstanceCreate,
        StageInstanceDelete,
        StageInstanceUpdate,
        ThreadCreate,
        ThreadDelete,
        ThreadListSync,
        ThreadMemberUpdate,
        ThreadMembersUpdate,
        ThreadUpdate,
        GuildEmojisUpdate,
        GuildStickersUpdate,
        GuildIntegrationsUpdate,
        IntegrationCreate,
        IntegrationDelete,
        IntegrationUpdate,
        InviteCreate,
        InviteDelete,
        WebhooksUpdate,
        VoiceStateUpdate,
        GuildScheduledEventCreate,
        GuildScheduledEventDelete,
        GuildScheduledEventUpdate,
        GuildScheduledEventUserAdd,
        GuildScheduledEventUserRemove,
        MemberAdd,
        MemberRemove,
        MemberUpdate,
        PresenceUpdate,
        MessageCreate,
        MessageDelete,
        MessageDeleteBulk,
        MessageUpdate,
        MessagePollVoteAdd,
        MessagePollVoteRemove,
        ReactionAdd,
        ReactionRemove,
        ReactionRemoveAll,
        ReactionRemoveEmoji,
        TypingStart,
        CommandPermissionsUpdate,
        EntitlementCreate,
        EntitlementDelete,
        EntitlementUpdate,
        MemberChunk,
        UserUpdate,
        VoiceServerUpdate,
    );
}

/// Matrix 20: a type whose handlers are all droppable is droppable.
#[tokio::test]
async fn droppable_all() {
    let app = builder()
        .module(Module::new("m").add(noop_with::<MessageDelete>("h", false, true)))
        .build()
        .unwrap();
    assert_eq!(
        app.lane_config().droppable,
        DROPPABLE_EVENTS | EventTypeFlags::MESSAGE_DELETE
    );
}

/// Matrix 21: one non-droppable handler keeps the type non-droppable.
#[tokio::test]
async fn droppable_mixed() {
    let app = builder()
        .module(
            Module::new("m")
                .add(noop_with::<MessageDelete>("a", false, true))
                .add(noop_with::<MessageDelete>("b", true, false)),
        )
        .build()
        .unwrap();
    assert!(
        !app.lane_config()
            .droppable
            .contains(EventTypeFlags::MESSAGE_DELETE)
    );
    assert_eq!(app.lane_config().droppable, DROPPABLE_EVENTS);
}

/// Matrix 22: the default droppable types stay droppable.
#[tokio::test]
async fn droppable_defaults_stay() {
    let app = builder()
        .intents(Intents::GUILD_PRESENCES)
        .module(
            Module::new("m")
                .add(noop::<PresenceUpdate>("p"))
                .add(noop::<TypingStart>("t")),
        )
        .build()
        .unwrap();
    assert_eq!(app.lane_config().droppable, DROPPABLE_EVENTS);
}

/// Matrix 23: event lane bounds.
#[tokio::test]
async fn event_lane_bounds() {
    for (capacity, overflow) in [(1, 0), (1_048_576, 4_194_304)] {
        let app = builder().event_lane(capacity, overflow).build().unwrap();
        let config = app.lane_config();
        assert_eq!(config.event_capacity.get(), capacity);
        assert_eq!(config.overflow_capacity, overflow);
    }
    for (capacity, overflow) in [(0, 0), (1_048_577, 0), (1, 4_194_305)] {
        let err = builder()
            .event_lane(capacity, overflow)
            .build()
            .unwrap_err();
        assert_eq!(
            violations(&err),
            [BuildViolation::InvalidEventLane { capacity, overflow }]
        );
        let text = err.to_string();
        assert!(text.contains(&capacity.to_string()), "{text}");
        assert!(text.contains(&overflow.to_string()), "{text}");
    }
    let config = builder().build().unwrap().lane_config();
    assert_eq!(config.event_capacity, DEFAULT_EVENT_CAPACITY);
    assert_eq!(config.overflow_capacity, DEFAULT_OVERFLOW_CAPACITY);
    assert_eq!(config.event_capacity.get(), 4096);
    assert_eq!(config.overflow_capacity, 16384);
    assert_eq!(&*config.app, "default");
}

/// Matrix 24: handler concurrency bounds.
#[tokio::test]
async fn handler_concurrency_bounds() {
    for limit in [1, 65_536] {
        let app = builder().handler_concurrency(limit).build().unwrap();
        assert_eq!(app.handler_concurrency(), limit);
    }
    for actual in [0, 65_537] {
        let err = builder().handler_concurrency(actual).build().unwrap_err();
        assert_eq!(
            violations(&err),
            [BuildViolation::InvalidHandlerConcurrency { actual }]
        );
    }
    assert_eq!(builder().build().unwrap().handler_concurrency(), 256);
}

/// Debug output names handlers and Modules, never payloads.
#[tokio::test]
async fn debug_output() {
    let module = Module::new("m").add(noop_with::<MessageDelete>("bot::h", true, false));
    let text = format!("{module:?}");
    assert!(text.contains("bot::h") && text.contains("\"m\""), "{text}");
    let handler = format!("{:?}", noop_with::<MessageDelete>("bot::h", true, false));
    assert_eq!(
        handler,
        "EventHandler { handler: \"bot::h\", event: \"MESSAGE_DELETE\", ordered: true, droppable: false }"
    );
    let app = builder().module(module).build().unwrap();
    assert!(format!("{app:?}").contains("handlers: 1"));
}
