use twilight_model::gateway::event::Event;
use twilight_model::id::Id;

use super::sealed::Sealed;
use super::*;

#[test]
fn dispatch_flag_equals_parse_for_every_payload() {
    assert_eq!(TABLE.len(), 64);
    for (name, kind, dispatch, parse, _) in TABLE {
        assert_eq!(EventTypeFlags::from(*dispatch), *parse, "{name}");
        if *name == "UnavailableGuild" {
            assert_eq!(*dispatch, EventType::GuildDelete);
        } else {
            assert_eq!(dispatch, kind, "{name}");
        }
    }
}

fn guild_delete(unavailable: Option<bool>) -> Event {
    Event::GuildDelete(GuildDelete {
        id: Id::new(42),
        unavailable,
    })
}

#[test]
fn unavailable_guild_extracts_only_outages() {
    assert_eq!(
        UnavailableGuild::extract(guild_delete(Some(true))),
        Some(UnavailableGuild { id: Id::new(42) })
    );
    assert_eq!(UnavailableGuild::extract(guild_delete(Some(false))), None);
    assert_eq!(UnavailableGuild::extract(guild_delete(None)), None);
    assert_eq!(
        UnavailableGuild::extract(Event::UnavailableGuild(UnavailableGuild { id: Id::new(7) })),
        Some(UnavailableGuild { id: Id::new(7) })
    );
}

#[test]
fn guild_delete_extracts_every_guild_delete() {
    for unavailable in [Some(true), Some(false), None] {
        assert_eq!(
            GuildDelete::extract(guild_delete(unavailable)),
            Some(GuildDelete {
                id: Id::new(42),
                unavailable
            })
        );
    }
    assert_eq!(
        GuildDelete::extract(Event::UnavailableGuild(UnavailableGuild { id: Id::new(7) })),
        None
    );
}

/// Story 16.3 D2: `accepts(&e)` equals `extract(e.clone()).is_some()` for
/// every payload, on events of several variants (boxed, plain, every
/// `GUILD_DELETE` shape, `UnavailableGuild`) and a gateway-only one.
#[test]
fn accepts_matches_extract_for_every_payload() {
    let events = [
        crate::test_support::message_delete(1, Some(2)),
        crate::test_support::guild_create(9, "tr"),
        crate::test_support::guild_create_unavailable(9),
        crate::test_support::guild_update(9, "de"),
        guild_delete(Some(true)),
        guild_delete(Some(false)),
        guild_delete(None),
        Event::UnavailableGuild(UnavailableGuild { id: Id::new(7) }),
        Event::ReactionRemoveAll(ReactionRemoveAll {
            channel_id: Id::new(2),
            guild_id: Some(Id::new(1)),
            message_id: Id::new(10),
        }),
        Event::GatewayHeartbeatAck,
    ];
    assert_eq!(PROBES.len(), TABLE.len());
    let mut accepted = 0;
    for (name, accepts, extracts) in PROBES {
        for event in &events {
            let expected = extracts(event.clone());
            assert_eq!(accepts(event), expected, "{name} on {:?}", event.kind());
            accepted += usize::from(expected);
        }
    }
    // MessageDelete, GuildCreate x2, GuildUpdate, GuildDelete x3,
    // UnavailableGuild x2 and ReactionRemoveAll.
    assert_eq!(accepted, 10);
}

/// The `UnavailableGuild` filter: outages and the explicit variant only;
/// `GuildDelete` takes every `GUILD_DELETE`.
#[test]
fn unavailable_guild_accepts_only_outages() {
    assert!(UnavailableGuild::accepts(&guild_delete(Some(true))));
    assert!(!UnavailableGuild::accepts(&guild_delete(Some(false))));
    assert!(!UnavailableGuild::accepts(&guild_delete(None)));
    assert!(UnavailableGuild::accepts(&Event::UnavailableGuild(
        UnavailableGuild { id: Id::new(7) }
    )));
    for unavailable in [Some(true), Some(false), None] {
        assert!(GuildDelete::accepts(&guild_delete(unavailable)));
    }
    assert!(!GuildDelete::accepts(&Event::GatewayHeartbeatAck));
}
