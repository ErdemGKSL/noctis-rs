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
