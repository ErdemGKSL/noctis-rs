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
    let app = builder().module(Module::new("a")).build().unwrap();
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
        .module(Module::new("m").__add(noop::<MessageCreate>("h")))
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MESSAGES);
    assert_eq!(
        app.registry().event_types(),
        EventTypeFlags::INTERACTION_CREATE | GUILD_LIFECYCLE | EventTypeFlags::MESSAGE_CREATE
    );
}

/// What every App with an event handler parses for the guild locale
/// record (Story 11.3).
const GUILD_LIFECYCLE: EventTypeFlags = EventTypeFlags::GUILD_CREATE
    .union(EventTypeFlags::GUILD_UPDATE)
    .union(EventTypeFlags::GUILD_DELETE);

/// Story 11.3 row 27: the guild lifecycle events are parsed whenever an
/// event handler is registered, and only then.
#[tokio::test]
async fn guild_lifecycle_follows_event_handlers() {
    let with = builder()
        .module(Module::new("m").__add(noop::<MessageCreate>("h")))
        .build()
        .unwrap();
    assert!(with.registry().event_types().contains(GUILD_LIFECYCLE));
    let without = builder().module(Module::new("m")).build().unwrap();
    assert!(!without.registry().event_types().intersects(GUILD_LIFECYCLE));
    // All-droppable handlers never make the record's feed droppable.
    let droppable = builder()
        .module(
            Module::new("m")
                .__add(noop_with::<GuildCreate>("c", false, true))
                .__add(noop_with::<GuildUpdate>("u", false, true))
                .__add(noop_with::<GuildDelete>("d", false, true)),
        )
        .build()
        .unwrap();
    assert!(
        !droppable
            .lane_config()
            .droppable
            .intersects(GUILD_LIFECYCLE)
    );
}

/// Matrix 4: pre-order fold with ancestor paths.
#[tokio::test]
async fn fold_order_and_ancestors() {
    let g = Module::new("p/c/g").__add(noop::<MessageDelete>("h4"));
    let c = Module::new("p/c")
        .__add(noop::<MessageDelete>("h2"))
        .__add(noop::<MessageCreate>("h3"))
        .module(g);
    let p = Module::new("p").__add(noop::<UserUpdate>("h1")).module(c);
    let r = Module::new("r").__add(noop::<MessageDelete>("h5"));
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
        [vec![], vec!["p"], vec!["p"], vec!["p", "p/c"], vec![]]
    );
    let modules: Vec<&str> = registry.entries().iter().map(|e| e.module).collect();
    assert_eq!(modules, ["p", "p/c", "p/c", "p/c/g", "r"]);
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
    let c = Module::new("p/c").__add(noop::<MessageDelete>("h2"));
    let p = Module::new("p")
        .module(c)
        .__add(noop::<MessageDelete>("h1"));
    let app = builder().module(p).build().unwrap();
    assert_eq!(names(app.registry()), ["h1", "h2"]);
}

/// Matrix 6: the same descriptor twice is two entries.
#[tokio::test]
async fn same_descriptor_twice() {
    let h = noop::<MessageDelete>("h");
    let app = builder()
        .module(Module::new("m").__add(h.clone()).__add(h))
        .build()
        .unwrap();
    assert_eq!(names(app.registry()), ["h", "h"]);
}

/// Matrix 7: one tree mounted on two Apps.
#[tokio::test]
async fn tree_on_two_apps() {
    let tree = Module::new("m")
        .__add(noop::<MessageDelete>("h"))
        .module(Module::new("m/c").__add(noop::<MessageCreate>("h2")));
    let a = builder().module(tree.clone()).build().unwrap();
    let b = builder().module(tree).build().unwrap();
    assert_eq!(names(a.registry()), ["h", "h2"]);
    assert_eq!(names(b.registry()), ["h", "h2"]);
    assert!(!Arc::ptr_eq(a.registry(), b.registry()));
}

/// Story 13.1 G1-G19: the Module id grammar.
#[test]
fn id_grammar() {
    use crate::ModuleIdError::{Empty, EmptySegment, InvalidChar, TooLong};
    let a64 = "a".repeat(64);
    let a65 = "a".repeat(65);
    let u65 = "ü".repeat(65);
    let cases: [(&str, Result<(), crate::ModuleIdError>); 22] = [
        ("profile", Ok(())),
        ("a/b/c", Ok(())),
        ("_", Ok(())),
        ("-", Ok(())),
        ("0", Ok(())),
        ("a_b-9", Ok(())),
        ("", Err(Empty)),
        (&a64, Ok(())),
        (&a65, Err(TooLong { len: 65 })),
        (&u65, Err(TooLong { len: 65 })),
        ("Profile", Err(InvalidChar { index: 0, ch: 'P' })),
        ("a b", Err(InvalidChar { index: 1, ch: ' ' })),
        ("shop.admin", Err(InvalidChar { index: 4, ch: '.' })),
        ("a:b", Err(InvalidChar { index: 1, ch: ':' })),
        ("ü/🙂", Err(InvalidChar { index: 0, ch: 'ü' })),
        (
            "a/🙂",
            Err(InvalidChar {
                index: 2, ch: '🙂'
            }),
        ),
        ("/", Err(EmptySegment { index: 0 })),
        ("/a", Err(EmptySegment { index: 0 })),
        ("a/", Err(EmptySegment { index: 2 })),
        ("a//b", Err(EmptySegment { index: 2 })),
        ("a//B", Err(EmptySegment { index: 2 })),
        ("A//b", Err(InvalidChar { index: 0, ch: 'A' })),
    ];
    for (id, expected) in cases {
        assert_eq!(module::check_id(id), expected, "{id:?}");
    }
}

/// Story 13.1: the exact text of each id violation and reason.
#[test]
fn id_violation_texts() {
    use crate::ModuleIdError;
    let invalid = |id, reason| BuildViolation::InvalidModuleId { id, reason }.to_string();
    assert_eq!(
        invalid("", ModuleIdError::Empty),
        "invalid Module id ``: the id is empty: use segments of a-z, 0-9, `_` and `-` joined \
         by `/`, such as `profile/badges`"
    );
    assert_eq!(
        invalid("x", ModuleIdError::TooLong { len: 65 }),
        "invalid Module id `x`: the id has 65 characters: shorten it to at most 64"
    );
    assert_eq!(
        invalid(
            "shop.admin",
            ModuleIdError::InvalidChar { index: 4, ch: '.' }
        ),
        "invalid Module id `shop.admin`: `.` at index 4 is not allowed: use only a-z, 0-9, `_` \
         and `-`, with `/` between segments"
    );
    assert_eq!(
        invalid("a//b", ModuleIdError::EmptySegment { index: 2 }),
        "invalid Module id `a//b`: empty segment at index 2: separate segments with exactly one \
         `/`, with none at the start or end"
    );
    assert_eq!(
        invalid("a/b", ModuleIdError::PathAtRoot { segments: 2 }),
        "invalid Module id `a/b`: it is mounted at the root with 2 segments: a root id is one \
         segment, so nest it with `.module(..)` in its parent Module"
    );
    assert_eq!(
        invalid("c1", ModuleIdError::NotUnderParent { parent: "r1" }),
        "invalid Module id `c1`: it is nested in Module `r1`, so it must be `r1/<name>` with \
         exactly one segment after `r1/`"
    );
    assert_eq!(
        BuildViolation::DuplicateModule { id: "shop" }.to_string(),
        "Module id `shop` is mounted twice: give each Module its own id (a Module tree is \
         mounted at most once per App)"
    );
}

/// Story 13.1 T1-T12: tree position and uniqueness, through the fold.
mod module_ids {
    use super::*;
    use crate::ModuleIdError::{self, Empty, InvalidChar, NotUnderParent, PathAtRoot};

    fn fold(roots: &[Module]) -> Vec<BuildViolation> {
        Registry::fold(roots, Intents::empty(), None, &BootFlags::all()).1
    }

    fn invalid(id: &'static str, reason: ModuleIdError) -> BuildViolation {
        BuildViolation::InvalidModuleId { id, reason }
    }

    fn dup(id: &'static str) -> BuildViolation {
        BuildViolation::DuplicateModule { id }
    }

    fn nest(parent: &'static str, child: &'static str) -> Module {
        Module::new(parent).module(Module::new(child))
    }

    /// T1: a valid 3-level tree folds every level.
    #[test]
    fn valid_tree() {
        let tree = Module::new("ops")
            .__add(noop::<MessageCreate>("h1"))
            .module(
                Module::new("ops/audit")
                    .__add(noop::<MessageCreate>("h2"))
                    .module(Module::new("ops/audit/log").__add(noop::<MessageCreate>("h3"))),
            );
        let (registry, violations) =
            Registry::fold(&[tree], Intents::empty(), None, &BootFlags::all());
        assert_eq!(violations, []);
        let modules: Vec<&str> = registry.entries().iter().map(|e| e.module).collect();
        assert_eq!(modules, ["ops", "ops/audit", "ops/audit/log"]);
    }

    /// T2-T5: tree positions.
    #[test]
    fn positions() {
        assert_eq!(
            fold(&[Module::new("a/b")]),
            [invalid("a/b", PathAtRoot { segments: 2 })]
        );
        assert_eq!(
            fold(&[nest("r1", "c1")]),
            [invalid("c1", NotUnderParent { parent: "r1" })]
        );
        assert_eq!(
            fold(&[nest("a", "a/b/c")]),
            [invalid("a/b/c", NotUnderParent { parent: "a" })]
        );
        assert_eq!(
            fold(&[nest("a", "ab")]),
            [invalid("ab", NotUnderParent { parent: "a" })]
        );
    }

    /// T6-T8: a child equal to its parent, and invalid parents.
    #[test]
    fn invalid_parents() {
        assert_eq!(
            fold(&[nest("a", "a")]),
            [invalid("a", NotUnderParent { parent: "a" }), dup("a")]
        );
        let bad = InvalidChar { index: 0, ch: 'A' };
        assert_eq!(
            fold(&[nest("A", "A/b")]),
            [invalid("A", bad), invalid("A/b", bad)]
        );
        assert_eq!(
            fold(&[nest("A", "a/b")]),
            [
                invalid("A", bad),
                invalid("a/b", NotUnderParent { parent: "A" })
            ]
        );
    }

    /// T9-T12: duplicates, one per later occurrence in fold pre-order.
    #[test]
    fn duplicates() {
        assert_eq!(
            fold(&[Module::new("shop"), Module::new("shop")]),
            [dup("shop")]
        );
        let m = nest("p", "p/c");
        assert_eq!(fold(&[m.clone(), m]), [dup("p"), dup("p/c")]);
        let twins = Module::new("p")
            .module(Module::new("p/x"))
            .module(Module::new("p/x"));
        assert_eq!(fold(&[twins]), [dup("p/x")]);
        assert_eq!(
            fold(&[Module::new(""), Module::new("")]),
            [invalid("", Empty), invalid("", Empty), dup("")]
        );
    }

    /// T15: id violations come first; every other check still runs.
    #[tokio::test]
    async fn id_violations_first() {
        let err = builder()
            .module(Module::new("Ops").__add(noop::<MemberAdd>("greet")))
            .module(Module::new("Ops"))
            .build()
            .unwrap_err();
        let v = violations(&err);
        assert_eq!(
            v[..2],
            [
                invalid("Ops", InvalidChar { index: 0, ch: 'O' }),
                invalid("Ops", InvalidChar { index: 0, ch: 'O' })
            ]
        );
        assert_eq!(v[2], dup("Ops"));
        assert!(
            matches!(v[3], BuildViolation::MissingIntent { module: "Ops", .. }),
            "{v:?}"
        );
        assert_eq!(v.len(), 4, "{v:?}");
    }
}

struct Mounts;

impl Plugin for Mounts {
    fn install(&self, app: &mut AppBuilder) {
        app.module(Module::new("x").__add(noop::<MessageDelete>("hx")));
    }
}

/// Matrix 9: a plugin's Module folds after the previously mounted roots.
#[tokio::test]
async fn plugin_mounts() {
    let app = builder()
        .plugin(Mounts)
        .module(Module::new("a").__add(noop::<MessageDelete>("ha")))
        .build()
        .unwrap();
    assert_eq!(names(app.registry()), ["ha", "hx"]);
}

/// Matrix 10: a second build has no handlers.
#[tokio::test]
async fn builder_reset() {
    let mut b = builder();
    b.module(Module::new("m").__add(noop::<MessageDelete>("h")))
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
        .module(Module::new("welcome").__add(noop::<MemberAdd>("bot::greet")))
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
        .module(Module::new("welcome").__add(noop::<MemberAdd>("greet")))
        .build()
        .unwrap();
    assert_eq!(app.intents(), Intents::GUILDS | Intents::GUILD_MEMBERS);
}

/// Matrix 13: presence updates need `GUILD_PRESENCES`.
#[tokio::test]
async fn gw_05_presence() {
    let err = builder()
        .module(Module::new("m").__add(noop::<PresenceUpdate>("p")))
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
                .__add(noop::<MemberAdd>("add"))
                .__add(noop::<MemberRemove>("remove")),
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

/// Story 15.4: a handler needing a privileged and a non-privileged
/// intent (the prefix plugin's `MESSAGE_CREATE` + `MESSAGE_CONTENT`): the
/// violation names only the missing privileged bit, and the non-privileged
/// part is still derived.
#[tokio::test]
async fn gw_05_mixed_intent_handler() {
    let handler = || noop::<MessageCreate>("prefix").__with_intents(Intents::MESSAGE_CONTENT);
    let err = builder()
        .module(Module::new("m").__add(handler()))
        .build()
        .unwrap_err();
    assert_eq!(
        violations(&err),
        [BuildViolation::MissingIntent {
            module: "m",
            handler: "prefix",
            event: "MESSAGE_CREATE",
            intent: Intents::MESSAGE_CONTENT,
        }]
    );
    let app = builder()
        .intents(Intents::MESSAGE_CONTENT)
        .module(Module::new("m").__add(handler()))
        .build()
        .unwrap();
    assert_eq!(
        app.intents(),
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT
    );
}

/// Matrix 15: GW-05 joins the other violations in one `BuildError`.
#[tokio::test]
async fn gw_05_mixed_violations() {
    let err = builder()
        .http_timeout(Duration::ZERO)
        .module(Module::new("m").__add(noop::<MemberAdd>("add")))
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
                    .module(Module::new("m").__add(noop::<$ty>(stringify!($ty))))
                    .build()
                    .unwrap();
                assert_eq!(app.intents(), Intents::GUILDS | intent, stringify!($ty));
                assert_eq!(
                    app.registry().event_types(),
                    EventTypeFlags::INTERACTION_CREATE | GUILD_LIFECYCLE | EventTypeFlags::$flag,
                    stringify!($ty)
                );
                assert!(<$ty as sealed::Sealed>::KIND.name().is_some());
                if privileged {
                    let err = builder()
                        .module(Module::new("m").__add(noop::<$ty>("h")))
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
        .module(Module::new("m").__add(noop::<MessageCreate>("h")))
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
        .module(Module::new("m").__add(noop::<MessageCreate>("h")))
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
        .module(Module::new("m").__add(noop::<UnavailableGuild>("h")))
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
        .module(Module::new("m").__add(noop::<UnavailableGuild>("u")))
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
                .__add(noop::<GuildDelete>("g1"))
                .__add(noop::<UnavailableGuild>("u"))
                .__add(noop::<GuildDelete>("g2")),
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
                .__add(noop_with::<UnavailableGuild>("u", false, true))
                .__add(noop_with::<GuildDelete>("g", false, false)),
        )
        .build()
        .unwrap();
    let droppable = app.lane_config().droppable;
    assert!(!droppable.contains(EventTypeFlags::GUILD_DELETE));
    assert!(!droppable.contains(EventTypeFlags::UNAVAILABLE_GUILD));

    let app = builder()
        .module(Module::new("m").__add(noop_with::<UnavailableGuild>("u", false, true)))
        .build()
        .unwrap();
    let droppable = app.lane_config().droppable;
    // An all-droppable slot would add `GUILD_DELETE`, but it feeds the
    // guild locale record (Story 11.3), so it stays non-droppable.
    assert_eq!(droppable, DROPPABLE_EVENTS);
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
                    &[Module::new("m").__add(noop::<$ty>(stringify!($ty)))],
                    Intents::all(),
                    None,
                    &BootFlags::all(),
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
        .module(Module::new("m").__add(noop_with::<MessageDelete>("h", false, true)))
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
                .__add(noop_with::<MessageDelete>("a", false, true))
                .__add(noop_with::<MessageDelete>("b", true, false)),
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
                .__add(noop::<PresenceUpdate>("p"))
                .__add(noop::<TypingStart>("t")),
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
    let module = Module::new("m").__add(noop_with::<MessageDelete>("bot::h", true, false));
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

// --- Story 7.1: the Route map, CID-04 ----------------------------------------

mod routes {
    use super::*;
    use crate::test_support::{TBuy, button, modal};

    fn fold(roots: &[Module]) -> (Registry, Vec<BuildViolation>) {
        Registry::fold(roots, Intents::empty(), None, &BootFlags::all())
    }

    fn duplicate(
        route: &'static str,
        first: (&'static str, &'static str),
        second: (&'static str, &'static str),
    ) -> BuildViolation {
        BuildViolation::DuplicateRoute {
            route,
            first_module: first.0,
            first_handler: first.1,
            second_module: second.0,
            second_handler: second.1,
        }
    }

    fn unmounted(module: &'static str) -> BuildViolation {
        BuildViolation::UnmountedRoute {
            module,
            route: "shop.buy",
            route_type: std::any::type_name::<TBuy>(),
        }
    }

    /// Matrix 2: one Route in two Modules.
    #[tokio::test]
    async fn duplicate_across_modules() {
        let err = builder()
            .module(Module::new("a").__add(button("m::Buy", "shop.buy")))
            .module(Module::new("b").__add(button("m::Buy2", "shop.buy")))
            .build()
            .unwrap_err();
        assert_eq!(
            violations(&err),
            [duplicate("shop.buy", ("a", "m::Buy"), ("b", "m::Buy2"))]
        );
        let text = err.violations()[0].to_string();
        assert!(
            text.starts_with("[CID-04] Route `shop.buy` has two handlers"),
            "{text}"
        );
    }

    /// Matrix 3: the same struct added twice in one Module.
    #[test]
    fn duplicate_in_one_module() {
        let m = Module::new("a")
            .__add(button("m::Buy", "shop.buy"))
            .__add(button("m::Buy", "shop.buy"));
        let (registry, v) = fold(&[m]);
        assert_eq!(v, [duplicate("shop.buy", ("a", "m::Buy"), ("a", "m::Buy"))]);
        assert_eq!(registry.len(), 1);
    }

    /// Matrix 4: a Button and a Modal share an id.
    #[test]
    fn duplicate_across_families() {
        let m = Module::new("a")
            .__add(button("m::X", "x"))
            .__add(modal("m::XForm", "x"));
        let (_, v) = fold(&[m]);
        assert_eq!(v, [duplicate("x", ("a", "m::X"), ("a", "m::XForm"))]);
    }

    /// Matrix 5: three handlers of one id give two violations, each
    /// against the first.
    #[test]
    fn three_handlers() {
        let roots = ["a", "b", "c"].map(|id| Module::new(id).__add(button("m::X", "x")));
        let (_, v) = fold(&roots);
        assert_eq!(
            v,
            [
                duplicate("x", ("a", "m::X"), ("b", "m::X")),
                duplicate("x", ("a", "m::X"), ("c", "m::X")),
            ]
        );
    }

    /// Matrix 6 and 11: distinct ids fold into one map; none is fine.
    #[test]
    fn distinct_routes() {
        let m = Module::new("shop")
            .__add(button("m::Buy", "shop.buy"))
            .module(Module::new("shop/sell").__add(button("m::Sell", "shop.sell")))
            .__add(modal("m::Bio", "bio"));
        let (registry, v) = fold(&[m]);
        assert!(v.is_empty(), "{v:?}");
        let buy = registry.route("shop.buy").unwrap();
        assert_eq!((buy.module, buy.handler.name()), ("shop", "m::Buy"));
        let sell = registry.route("shop.sell").unwrap();
        assert_eq!(
            (sell.module, &*sell.ancestors),
            ("shop/sell", &["shop"][..])
        );
        assert!(matches!(
            registry.route("bio").unwrap().handler,
            RouteHandler::Modal(_)
        ));
        assert!(registry.route("nope").is_none());
        assert_eq!(registry.len(), 3);
        let (empty, v) = fold(&[]);
        assert!(v.is_empty() && empty.route("shop.buy").is_none());
    }

    /// Matrix 7: `uses` satisfied by a sibling or a child.
    #[test]
    fn uses_satisfied() {
        let a = Module::new("a").uses::<TBuy>();
        let b = Module::new("b").__add(button("m::Buy", "shop.buy"));
        assert!(fold(&[a.clone(), b]).1.is_empty());
        let nested = a.module(Module::new("a/c").__add(button("m::Buy", "shop.buy")));
        assert!(fold(&[nested]).1.is_empty());
    }

    /// Matrix 8: `uses` without a mounted handler.
    #[tokio::test]
    async fn uses_unmounted() {
        let err = builder()
            .module(Module::new("a").uses::<TBuy>())
            .build()
            .unwrap_err();
        assert_eq!(violations(&err), [unmounted("a")]);
        let text = err.violations()[0].to_string();
        assert!(
            text.starts_with("[CID-04] Module `a` uses Route `shop.buy` (`")
                && text.ends_with(&format!(
                    "but no mounted Module handles it: add `.add::<{}>()` to a mounted Module",
                    std::any::type_name::<TBuy>()
                )),
            "{text}"
        );
        // An unmounted Module's handler does not count.
        let _unmounted = Module::new("b").__add(button("m::Buy", "shop.buy"));
        let (_, v) = fold(&[Module::new("a").uses::<TBuy>()]);
        assert_eq!(v, [unmounted("a")]);
    }

    /// Matrix 9: duplicate declarations give one violation; declarations
    /// on unmounted Modules none. Duplicates come before unmounted Routes.
    #[test]
    fn uses_deduplicated_and_ordered() {
        let a = Module::new("a").uses::<TBuy>().uses::<TBuy>();
        let (_, v) = fold(&[a]);
        assert_eq!(v, [unmounted("a")]);
        let _never_mounted = Module::new("z").uses::<TBuy>();
        assert!(fold(&[Module::new("y")]).1.is_empty());
        let roots = [
            Module::new("a").uses::<TBuy>(),
            Module::new("b").__add(button("m::X", "x")),
            Module::new("c").__add(button("m::X", "x")),
        ];
        let (_, v) = fold(&roots);
        assert_eq!(
            v,
            [duplicate("x", ("b", "m::X"), ("c", "m::X")), unmounted("a")]
        );
        assert!(format!("{:?}", roots[0]).contains("uses: [\"shop.buy\"]"));
    }
}

// --- Middleware chains and `MissingExtension` --------------------------------

mod extensions {
    use super::*;
    use crate::kind::__ExtType;
    use crate::test_support::{button, command, modal, user_menu};
    use crate::{BeforeCtx, CommandMetadata, Flow, HandlerResult, Middleware};

    struct A;
    struct B;

    const NEEDS_A: &[__ExtType] = &[__ExtType::of::<A>()];
    const NEEDS_B: &[__ExtType] = &[__ExtType::of::<B>()];
    const NEEDS_A_TWICE_B: &[__ExtType] = &[
        __ExtType::of::<A>(),
        __ExtType::of::<A>(),
        __ExtType::of::<B>(),
    ];

    async fn provide(_: BeforeCtx) -> HandlerResult<Flow> {
        Ok(Flow::Continue)
    }

    fn provides_a() -> crate::BeforeMiddleware {
        Middleware::before(provide).provides::<A>()
    }

    fn fold(roots: &[Module]) -> Vec<BuildViolation> {
        Registry::fold(roots, Intents::empty(), None, &BootFlags::all()).1
    }

    fn missing(module: &'static str, handler: &'static str, ty: &'static str) -> BuildViolation {
        BuildViolation::MissingExtension {
            module,
            handler,
            extension: ty,
        }
    }

    fn a() -> &'static str {
        std::any::type_name::<A>()
    }

    fn b() -> &'static str {
        std::any::type_name::<B>()
    }

    /// A Command handler `name` at `/name` requiring `requires`.
    fn cmd(name: &'static str, requires: &'static [__ExtType]) -> crate::CommandHandler {
        let path = crate::test_support::leak_paths(name.rsplit("::").next().unwrap_or(name));
        command(name, path, "d").__with_requires(requires)
    }

    /// Matrix 14, 15 and 18: an own, an ancestor's and no provider for an
    /// optional Extension all build.
    #[test]
    fn provided_on_the_path() {
        let own = Module::new("m")
            .before(provides_a())
            .__add(cmd("bot::own", NEEDS_A));
        assert_eq!(fold(&[own]), []);
        let grandchild = Module::new("root/c/g")
            .__add(noop::<MessageCreate>("bot::ev").__with_requires(NEEDS_A));
        let tree = Module::new("root")
            .before(provides_a())
            .module(Module::new("root/c").module(grandchild));
        assert_eq!(fold(&[tree]), []);
        let optional = Module::new("m").__add(cmd("bot::opt", &[]));
        assert_eq!(fold(&[optional]), []);
    }

    /// Matrix 16: a provider only in a child or a sibling does not count.
    #[test]
    fn child_or_sibling_provider_is_missing() {
        let tree = Module::new("m")
            .__add(cmd("bot::h", NEEDS_A))
            .module(Module::new("m/child").before(provides_a()));
        let sibling = Module::new("s").before(provides_a());
        assert_eq!(fold(&[tree, sibling]), [missing("m", "bot::h", a())]);
    }

    /// Matrix 17: the scope counts.
    #[test]
    fn scope_mismatch() {
        let only_events = Module::new("e")
            .before(Middleware::before(provide).provides::<A>().events_only())
            .__add(cmd("bot::cmd", NEEDS_A))
            .__add(noop::<MessageCreate>("bot::ev").__with_requires(NEEDS_A));
        let skip_events = Module::new("s")
            .before(Middleware::before(provide).provides::<A>().skip_events())
            .__add(noop::<MessageCreate>("bot::ev2").__with_requires(NEEDS_A))
            .__add(cmd("bot::cmd2", NEEDS_A));
        assert_eq!(
            fold(&[only_events, skip_events]),
            [missing("e", "bot::cmd", a()), missing("s", "bot::ev2", a())]
        );
    }

    /// Matrix 19: every violation, in fold order and kind order, one per
    /// (handler, type); the exact text once.
    #[test]
    fn many_in_fold_order() {
        let module = Module::new("m")
            .__add(modal("bot::Modal", "m.modal").__with_requires(NEEDS_B))
            .__add(button("bot::Button", "m.button").__with_requires(NEEDS_A))
            .__add(user_menu("bot::Menu", "Show", CommandMetadata::new()).__with_requires(NEEDS_B))
            .__add(cmd("bot::cmd", NEEDS_A_TWICE_B))
            .__add(noop::<MessageCreate>("bot::ev").__with_requires(NEEDS_A))
            .module(Module::new("m/c").__add(cmd("bot::child", NEEDS_B)));
        let violations = fold(&[module]);
        assert_eq!(
            violations,
            [
                missing("m", "bot::ev", a()),
                missing("m", "bot::cmd", a()),
                missing("m", "bot::cmd", b()),
                missing("m", "bot::Menu", b()),
                missing("m", "bot::Button", a()),
                missing("m", "bot::Modal", b()),
                missing("m/c", "bot::child", b()),
            ]
        );
        assert_eq!(
            violations[0].to_string(),
            format!(
                "handler `bot::ev` (Module `m`) takes `Ext<{0}>`, but no before-Middleware on \
                 its Module path provides it: add `.before(Middleware::before(f).provides::<{0}>())` \
                 to Module `m` or an ancestor, or take `Option<Ext<{0}>>`",
                a()
            )
        );
    }

    /// Matrix 20: one handler in a Module under a providing root and in
    /// one under a bare root fails on the bare path only.
    #[test]
    fn two_mounts() {
        let shared =
            |id| Module::new(id).__add(noop::<MessageCreate>("bot::ev").__with_requires(NEEDS_A));
        let provided = Module::new("p")
            .before(provides_a())
            .module(shared("p/shared"));
        let bare = Module::new("b").module(shared("b/shared"));
        assert_eq!(
            fold(&[provided, bare]),
            [missing("b/shared", "bot::ev", a())]
        );
    }

    /// Matrix 21's chain: parent before child, registration order; the
    /// entries share their Module's chain.
    #[test]
    fn chains_fold_parent_first() {
        async fn r1(_: BeforeCtx) -> HandlerResult<Flow> {
            Ok(Flow::Continue)
        }
        async fn r2(_: BeforeCtx) -> HandlerResult<Flow> {
            Ok(Flow::Continue)
        }
        async fn c1(_: BeforeCtx) -> HandlerResult<Flow> {
            Ok(Flow::Continue)
        }
        let tree = Module::new("root")
            .before(Middleware::before(r1))
            .before(Middleware::before(r2))
            .__add(noop::<MessageCreate>("bot::root"))
            .module(
                Module::new("root/child")
                    .before(Middleware::before(c1))
                    .__add(noop::<MessageCreate>("bot::child")),
            );
        assert!(format!("{tree:?}").contains("before: [\""));
        let (registry, violations) =
            Registry::fold(&[tree], Intents::empty(), None, &BootFlags::all());
        assert_eq!(violations, []);
        let names = |entry: &Entry| -> Vec<&str> {
            entry
                .chain
                .before
                .iter()
                .map(|m| m.name().rsplit("::").next().unwrap_or_default())
                .collect()
        };
        let entries = registry.entries();
        assert_eq!(names(&entries[0]), ["r1", "r2"]);
        assert_eq!(names(&entries[1]), ["r1", "r2", "c1"]);
    }
}

// --- Story 10.2: Templates ----------------------------------------------------

mod templates {
    use noctis_ports::{AnimationSpec, ErasedView, Format, Size, Template};
    use serde::{Deserialize, Serialize};

    use super::*;

    macro_rules! template {
        ($name:ident, $id:literal, $version:literal) => {
            #[derive(Serialize, Deserialize)]
            struct $name;

            impl Template for $name {
                const ID: &'static str = $id;
                const VERSION: u32 = $version;
                const SIZE: Size = Size::new(1, 1);
                const FORMAT: Format = Format::Png;
                const ANIMATION: Option<AnimationSpec> = None;

                fn view() -> ErasedView {
                    ErasedView::new(())
                }
            }
        };
    }

    template!(CardA, "card", 1);
    template!(CardB, "card", 2);
    template!(Badge, "badge", 1);

    fn ids(registry: &Registry) -> Vec<(&'static str, u32)> {
        registry
            .templates()
            .iter()
            .map(|t| (t.id, t.version))
            .collect()
    }

    /// Two Modules, one Template id: one violation naming both.
    #[tokio::test]
    async fn duplicate_template_across_modules() {
        let err = builder()
            .module(Module::new("a").template::<CardA>())
            .module(Module::new("b").template::<CardB>())
            .build()
            .unwrap_err();
        let expected = BuildViolation::DuplicateTemplate {
            id: "card",
            first_module: "a",
            first_type: std::any::type_name::<CardA>(),
            second_module: "b",
            second_type: std::any::type_name::<CardB>(),
        };
        assert_eq!(violations(&err), std::slice::from_ref(&expected));
        let text = expected.to_string();
        assert!(
            text.starts_with("Template id `card` is registered twice: `")
                && text.ends_with("(Module `b`): give each Template its own id"),
            "{text}"
        );
    }

    /// The same type twice in one Module.
    #[test]
    fn same_template_twice() {
        let (registry, violations) = Registry::fold(
            &[Module::new("a").template::<CardA>().template::<CardA>()],
            Intents::empty(),
            None,
            &BootFlags::all(),
        );
        let name = std::any::type_name::<CardA>();
        assert_eq!(
            violations,
            [BuildViolation::DuplicateTemplate {
                id: "card",
                first_module: "a",
                first_type: name,
                second_module: "a",
                second_type: name,
            }]
        );
        assert_eq!(ids(&registry), [("card", 1)]);
    }

    /// Distinct ids across a nested child: kept in fold order.
    #[tokio::test]
    async fn templates_in_fold_order() {
        let app = builder()
            .module(
                Module::new("root")
                    .template::<Badge>()
                    .module(Module::new("root/child").template::<CardB>()),
            )
            .build()
            .unwrap();
        assert_eq!(ids(app.registry()), [("badge", 1), ("card", 2)]);
        let (registry, violations) = Registry::fold(
            &[
                Module::new("x").module(Module::new("x/y").template::<CardA>()),
                Module::new("z").template::<Badge>(),
            ],
            Intents::empty(),
            None,
            &BootFlags::all(),
        );
        assert!(violations.is_empty());
        assert_eq!(ids(&registry), [("card", 1), ("badge", 1)]);
    }

    #[test]
    fn module_debug_names_templates() {
        let module = Module::new("m").template::<Badge>();
        assert!(format!("{module:?}").contains("templates: [\"badge\"]"));
    }

    /// Story 13.2: a pruned Module's Templates stay registered (App-wide).
    #[test]
    fn pruned_module_keeps_templates() {
        let (registry, violations) = Registry::fold(
            &[
                Module::new("a").flags(["beta"]).template::<CardA>(),
                Module::new("b").template::<Badge>(),
            ],
            Intents::empty(),
            None,
            &BootFlags::none(),
        );
        assert_eq!(violations, []);
        assert_eq!(ids(&registry), [("card", 1), ("badge", 1)]);
    }
}

// --- Story 13.2: Boot Flags -------------------------------------------------

mod boot_flags {
    use super::*;
    use crate::BootFlagError;
    use crate::metadata::BaseMeta;
    use crate::test_support::{TBuy, button, command, modal, user_menu};

    fn only(flags: &[&str]) -> BootFlags {
        BootFlags::only(flags).expect("valid flags")
    }

    fn fold(roots: &[Module], selection: &BootFlags) -> (Registry, Vec<BuildViolation>) {
        Registry::fold(roots, Intents::empty(), None, selection)
    }

    /// The Module ids of the folded event handlers.
    fn modules(registry: &Registry) -> Vec<&'static str> {
        registry.entries().iter().map(|e| e.module).collect()
    }

    fn eco() -> Module {
        Module::new("eco")
            .flags(["economy"])
            .__add(noop::<MessageCreate>("eco"))
    }

    /// B1: a flagged Module under {economy}, {beta}, all and none.
    #[test]
    fn module_selection() {
        let roots = [eco(), Module::new("beta").flags(["beta"])];
        let cases = [
            (only(&["economy"]), vec!["eco"]),
            (only(&["beta"]), vec![]),
            (BootFlags::all(), vec!["eco"]),
            (BootFlags::none(), vec![]),
        ];
        for (selection, expected) in cases {
            let (registry, violations) = fold(&roots, &selection);
            assert_eq!(violations, [], "{selection}");
            assert_eq!(modules(&registry), expected, "{selection}");
        }
    }

    /// B2: any of a Module's flags keeps it.
    #[test]
    fn any_of() {
        let m = Module::new("m")
            .flags(["a", "b"])
            .__add(noop::<MessageCreate>("h"));
        let (registry, violations) = fold(&[m], &only(&["b"]));
        assert_eq!(violations, []);
        assert_eq!(modules(&registry), ["m"]);
    }

    /// B3, B4: nesting is AND.
    #[test]
    fn nesting_is_and() {
        let tree = || {
            Module::new("p")
                .flags(["a"])
                .__add(noop::<MessageCreate>("p"))
                .module(
                    Module::new("p/c")
                        .__add(noop::<MessageCreate>("c"))
                        .module(Module::new("p/c/g").__add(noop::<MessageCreate>("g"))),
                )
                .module(
                    Module::new("p/d")
                        .flags(["b"])
                        .__add(noop::<MessageCreate>("d"))
                        .module(Module::new("p/d/e").__add(noop::<MessageCreate>("e"))),
                )
        };
        let (registry, violations) = fold(&[tree()], &only(&["b"]));
        assert_eq!(violations, []);
        assert!(modules(&registry).is_empty());
        let (registry, violations) = fold(&[tree()], &only(&["a"]));
        assert_eq!(violations, []);
        assert_eq!(modules(&registry), ["p", "p/c", "p/c/g"]);
        let (registry, _) = fold(&[tree()], &only(&["a", "b"]));
        assert_eq!(modules(&registry), ["p", "p/c", "p/c/g", "p/d", "p/d/e"]);
    }

    /// B5, B6, B7: flagged registrations in an unflagged Module.
    #[test]
    fn flagged_registrations() {
        let core = Module::new("core")
            .__add(command("ping", &["ping"], "Ping"))
            .__add_flagged(&["beta"], command("stats", &["beta stats"], "Stats"))
            .__add_flagged(&["beta"], noop::<MessageDelete>("gone"))
            .__add_flagged(&["beta"], button("buy", "shop.buy"))
            .__add_flagged(
                &["beta"],
                user_menu("inspect", "Inspect", crate::CommandMetadata::new()),
            );
        let (registry, violations) = fold(std::slice::from_ref(&core), &BootFlags::none());
        assert_eq!(violations, []);
        let tree = registry.commands().render();
        assert!(tree.contains("/ping"), "{tree}");
        assert!(!tree.contains("beta"), "{tree}");
        assert!(!tree.contains("Inspect"), "{tree}");
        assert!(registry.entries().is_empty());
        assert!(registry.handlers(EventType::MessageDelete).is_none());
        assert!(!registry.intents().contains(Intents::GUILD_MESSAGES));
        assert!(registry.route("shop.buy").is_none());

        let (registry, violations) = fold(&[core], &only(&["beta"]));
        assert_eq!(violations, []);
        let tree = registry.commands().render();
        assert!(tree.contains("/beta"), "{tree}");
        assert!(tree.contains("Inspect"), "{tree}");
        assert!(registry.handlers(EventType::MessageDelete).is_some());
        assert!(registry.intents().contains(Intents::GUILD_MESSAGES));
        assert!(registry.route("shop.buy").is_some());
    }

    /// A flag declared only on a component, modal or menu registration
    /// counts as declared: selecting it is no `UnknownBootFlag`.
    #[test]
    fn flags_declared_only_on_a_route_or_menu() {
        let selection = only(&["shop"]);
        let core = Module::new("core").__add_flagged(&["shop"], button("buy", "shop.buy"));
        let (registry, violations) = fold(&[core], &selection);
        assert_eq!(violations, []);
        assert!(registry.route("shop.buy").is_some());

        let core = Module::new("core").__add_flagged(&["shop"], modal("pay", "shop.pay"));
        let (registry, violations) = fold(&[core], &selection);
        assert_eq!(violations, []);
        assert!(registry.route("shop.pay").is_some());

        let core = Module::new("core").__add_flagged(
            &["shop"],
            user_menu("inspect", "Inspect", crate::CommandMetadata::new()),
        );
        let (registry, violations) = fold(&[core], &selection);
        assert_eq!(violations, []);
        let tree = registry.commands().render();
        assert!(tree.contains("Inspect"), "{tree}");
    }

    /// B7: a kept Module that `uses` a pruned Route is CID-04.
    #[test]
    fn uses_of_a_pruned_route() {
        let roots = [
            Module::new("shop")
                .flags(["shop"])
                .__add(modal("buy", "shop.buy")),
            Module::new("dash").uses::<TBuy>(),
        ];
        let (_, violations) = fold(&roots, &BootFlags::none());
        assert_eq!(
            violations,
            [BuildViolation::UnmountedRoute {
                module: "dash",
                route: "shop.buy",
                route_type: std::any::type_name::<TBuy>(),
            }]
        );
        let (_, violations) = fold(&roots, &only(&["shop"]));
        assert_eq!(violations, []);
        // A pruned Module's own `uses` goes with it.
        let roots = [Module::new("dash").flags(["dash"]).uses::<TBuy>()];
        let (_, violations) = fold(&roots, &BootFlags::none());
        assert_eq!(violations, []);
    }

    /// B8, B9: an emptied `base` is dropped; one that never had a Command
    /// is still REG-04.
    #[test]
    fn emptied_base_is_dropped() {
        let core = Module::new("core")
            .base("beta", BaseMeta::new().description("Beta tools"))
            .group("beta x", "Group")
            .__add(command("ping", &["ping"], "Ping"))
            .__add_flagged(&["beta"], command("stats", &["beta stats"], "Stats"))
            .__add_flagged(&["beta"], command("x", &["beta x y"], "Y"));
        let (registry, violations) = fold(std::slice::from_ref(&core), &BootFlags::none());
        assert_eq!(violations, []);
        assert!(!registry.commands().render().contains("beta"));
        let (registry, violations) = fold(&[core], &BootFlags::all());
        assert_eq!(violations, []);
        assert!(registry.commands().render().contains("Beta tools"));

        let ghost = Module::new("core")
            .flags(["core"])
            .base("ghost", BaseMeta::new().description("Nothing"))
            .__add(command("ping", &["ping"], "Ping"));
        let (_, violations) = fold(&[ghost], &only(&["core"]));
        assert!(
            matches!(
                violations.as_slice(),
                [BuildViolation::Command(v)] if v.rule == noctis_rules::RuleId::REG_04
            ),
            "{violations:?}"
        );
    }

    /// B10, B11, B12: `Module::flags` grammar and repeats.
    #[test]
    fn module_flag_grammar() {
        let invalid = |module, flag, reason| BuildViolation::InvalidBootFlag {
            module,
            flag,
            reason,
        };
        let (_, violations) = fold(&[Module::new("m").flags(["Beta"])], &BootFlags::all());
        assert_eq!(
            violations,
            [invalid(
                "m",
                "Beta",
                BootFlagError::InvalidChar { index: 0, ch: 'B' }
            )]
        );
        for m in [
            Module::new("m").flags(["a", "a"]),
            Module::new("m").flags(["a"]).flags(["a"]),
            Module::new("m").flags(["a", "a", "a"]),
        ] {
            let (_, violations) = fold(&[m], &BootFlags::all());
            assert_eq!(violations, [invalid("m", "a", BootFlagError::Duplicate)]);
        }
        let long: &'static str = "x".repeat(33).leak();
        let (_, violations) = fold(
            &[Module::new("m")
                .flags(["", "all"])
                .module(Module::new("m/c").flags([long]))],
            &BootFlags::all(),
        );
        assert_eq!(
            violations,
            [
                invalid("m", "", BootFlagError::Empty),
                invalid("m", "all", BootFlagError::Reserved { flag: "all" }),
                invalid("m/c", long, BootFlagError::TooLong { len: 33 }),
            ]
        );
        // B12: no flags is unflagged.
        let m = Module::new("m").flags([]).__add(noop::<MessageCreate>("h"));
        let (registry, violations) = fold(&[m], &BootFlags::none());
        assert_eq!(violations, []);
        assert_eq!(modules(&registry), ["m"]);
    }

    /// B13, B14, B15: the typo guard over the full trees.
    #[test]
    fn unknown_selected_flags() {
        let roots = [
            eco(),
            Module::new("core").__add_flagged(&["beta"], command("s", &["stats"], "S")),
        ];
        let (_, violations) = fold(&roots, &only(&["ecnomy", "beta", "zzz"]));
        assert_eq!(
            violations,
            [
                BuildViolation::UnknownBootFlag {
                    flag: "ecnomy".to_owned(),
                    declared: vec!["beta", "economy"],
                },
                BuildViolation::UnknownBootFlag {
                    flag: "zzz".to_owned(),
                    declared: vec!["beta", "economy"],
                },
            ]
        );
        let (_, violations) = fold(&[Module::new("m")], &only(&["x"]));
        assert_eq!(
            violations,
            [BuildViolation::UnknownBootFlag {
                flag: "x".to_owned(),
                declared: vec![],
            }]
        );
        // B15: declared only inside a pruned subtree still counts.
        let tree = Module::new("p")
            .flags(["a"])
            .module(Module::new("p/c").flags(["deep"]));
        let (registry, violations) = fold(&[tree], &only(&["deep"]));
        assert_eq!(violations, []);
        assert_eq!(registry.len(), 0);
        // An invalid Module flag is not listed as declared.
        let (_, violations) = fold(&[Module::new("m").flags(["Beta", "ok"])], &only(&["x"]));
        assert!(matches!(
            violations.as_slice(),
            [
                BuildViolation::InvalidBootFlag { module: "m", flag: "Beta", .. },
                BuildViolation::UnknownBootFlag { flag, declared },
            ] if flag == "x" && *declared == ["ok"]
        ));
    }

    /// B16: Module ids are checked on the full trees.
    #[test]
    fn duplicate_module_in_pruned_subtree() {
        let roots = [
            Module::new("a").flags(["x"]).module(Module::new("a/b")),
            Module::new("c").module(Module::new("c/d").flags(["x"]).module(Module::new("a"))),
        ];
        let (_, violations) = fold(&roots, &BootFlags::none());
        assert!(violations.contains(&BuildViolation::DuplicateModule { id: "a" }));
    }

    /// B17, B18: flagged alternatives of one Command.
    #[test]
    fn alternative_commands() {
        let roots = [
            Module::new("shop-a")
                .flags(["a"])
                .__add(command("shop_a", &["shop"], "Shop")),
            Module::new("shop-b")
                .flags(["b"])
                .__add(command("shop_b", &["shop"], "Shop")),
        ];
        let (registry, violations) = fold(&roots, &only(&["a"]));
        assert_eq!(violations, []);
        let tree = registry.commands().render();
        assert_eq!(tree.matches("/shop").count(), 1, "{tree}");
        assert!(tree.contains("shop_a"), "{tree}");
        let (_, violations) = fold(&roots, &BootFlags::all());
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, BuildViolation::Command(_))),
            "{violations:?}"
        );
    }

    /// Selected registrations keep their fold order.
    #[test]
    fn order_is_kept() {
        let m = Module::new("m")
            .__add(noop::<MessageCreate>("1"))
            .__add_flagged(&["x"], noop::<MessageCreate>("2"))
            .__add(noop::<MessageCreate>("3"))
            .__add_flagged(&["y"], noop::<MessageCreate>("4"))
            .__add_flagged(&["x", "y"], noop::<MessageCreate>("5"));
        let (registry, violations) = fold(&[m], &only(&["y"]));
        assert_eq!(violations, []);
        let names: Vec<&str> = registry
            .entries()
            .iter()
            .map(|e| e.handler.name())
            .collect();
        assert_eq!(names, ["1", "3", "4", "5"]);
    }

    #[test]
    fn module_debug_shows_flags() {
        let m = Module::new("m")
            .flags(["eco"])
            .__add_flagged(&["beta"], noop::<MessageCreate>("h"));
        let text = format!("{m:?}");
        assert!(text.contains("flags: [\"eco\"]"), "{text}");
        assert!(text.contains("flagged: [(\"h\", [\"beta\"])]"), "{text}");
    }
}

/// Story 13.3: the Module index lists the kept Modules in pre-order with
/// root-first paths; a Boot-Flag-pruned Module is absent.
#[test]
fn module_index_is_pre_order_of_the_kept_trees() {
    let roots = [
        Module::new("p")
            .module(Module::new("p/c").module(Module::new("p/c/d")))
            .module(Module::new("p/e").flags(["fun"])),
        Module::new("q"),
    ];
    let (registry, violations) = Registry::fold(&roots, Intents::empty(), None, &BootFlags::all());
    assert!(violations.is_empty(), "{violations:?}");
    let index = registry.modules();
    assert_eq!(index.ids(), ["p", "p/c", "p/c/d", "p/e", "q"]);
    assert_eq!(index.path("p/c/d"), Some(&[0, 1, 2][..]));
    assert_eq!(index.path("p/e"), Some(&[0, 3][..]));
    assert_eq!(index.path("q"), Some(&[4][..]));
    let only = BootFlags::only(["other"]).unwrap();
    let roots = [
        Module::new("p").module(Module::new("p/e").flags(["fun"])),
        Module::new("q").flags(["other"]),
    ];
    let (registry, violations) = Registry::fold(&roots, Intents::empty(), None, &only);
    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(registry.modules().ids(), ["p", "q"]);
    assert_eq!(registry.modules().path("p/e"), None);
}
