//! Story 12.5 Build matrix B1-B8, F3 and F7: the Discord cache
//! configuration, its intents (GW-05), gateway filter, droppable set and
//! default backend.

use std::sync::Arc;

use noctis_gateway::EventTypeFlags;
use noctis_ports::DiscordCache;
use noctis_ports::memory::{DiscordCacheResources as R, MemoryDiscordCache};
use twilight_model::gateway::Intents;
use twilight_model::id::Id;

use crate::events::{MemberAdd, RoleCreate};
use crate::ports::PortSlot;
use crate::test_support::{Capture, guild_create_with, noop, noop_with};
use crate::{App, AppBuilder, BuildViolation, DiscordCacheConfig, Module, Runtime};

fn builder() -> AppBuilder {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    builder
}

#[tokio::test]
async fn b1_default() {
    let app = builder().build().unwrap();
    assert_eq!(app.intents(), Intents::GUILDS);
    assert_eq!(app.discord_cache(), DiscordCacheConfig::default());
    let filter = app.event_types();
    for flag in [
        EventTypeFlags::GUILD_CREATE,
        EventTypeFlags::ROLE_CREATE,
        EventTypeFlags::CHANNEL_CREATE,
        EventTypeFlags::THREAD_LIST_SYNC,
        EventTypeFlags::INTERACTION_CREATE,
    ] {
        assert!(filter.contains(flag), "{flag:?}");
    }
    assert!(!filter.contains(EventTypeFlags::MESSAGE_CREATE));
}

#[tokio::test]
async fn b2_none_keeps_the_handler_filter() {
    let app = builder()
        .discord_cache(DiscordCacheConfig::none())
        .build()
        .unwrap();
    assert_eq!(app.event_types(), app.registry().event_types());
    assert_eq!(app.event_types(), EventTypeFlags::INTERACTION_CREATE);
    assert_eq!(app.intents(), Intents::GUILDS);
}

#[tokio::test]
async fn b3_gw_05_member_without_guild_members() {
    let err = builder()
        .discord_cache(DiscordCacheConfig::new(R::MEMBER))
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::MissingCacheIntent {
            resource: R::MEMBER,
            intent: Intents::GUILD_MEMBERS,
        }]
    );
    let text = err.violations()[0].to_string();
    for needle in ["[GW-05]", "MEMBER", "GUILD_MEMBERS", ".discord_cache("] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    assert!(
        text.contains("`.intents(Intents::GUILD_MEMBERS)`"),
        "{text}"
    );
}

#[tokio::test]
async fn b4_gw_05_member_with_guild_members() {
    let app = builder()
        .discord_cache(DiscordCacheConfig::new(R::MEMBER))
        .intents(Intents::GUILD_MEMBERS)
        .build()
        .unwrap();
    assert!(app.intents().contains(Intents::GUILD_MEMBERS));
    assert!(app.event_types().contains(EventTypeFlags::MEMBER_ADD));
    assert!(app.event_types().contains(EventTypeFlags::MEMBER_CHUNK));
}

#[tokio::test]
async fn b5_messages_add_their_intents() {
    let app = builder()
        .discord_cache(DiscordCacheConfig::new(R::MESSAGE))
        .build()
        .unwrap();
    assert_eq!(
        app.intents(),
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES
    );
}

#[tokio::test]
async fn b6_gw_05_handler_violation_first() {
    let err = builder()
        .discord_cache(DiscordCacheConfig::new(R::MEMBER))
        .module(Module::new("welcome").__add(noop::<MemberAdd>("greet")))
        .build()
        .unwrap_err();
    let violations = err.violations();
    assert_eq!(violations.len(), 2, "{err}");
    assert!(matches!(
        violations[0],
        BuildViolation::MissingIntent { .. }
    ));
    assert!(matches!(
        violations[1],
        BuildViolation::MissingCacheIntent {
            resource: R::MEMBER,
            ..
        }
    ));
}

#[tokio::test]
async fn b7_last_call_wins() {
    let app = builder()
        .discord_cache(DiscordCacheConfig::new(R::MEMBER))
        .discord_cache(DiscordCacheConfig::new(R::USER).with_message_cap(5))
        .build()
        .unwrap();
    assert_eq!(
        app.discord_cache(),
        DiscordCacheConfig::new(R::USER).with_message_cap(5)
    );
}

#[tokio::test]
async fn b8_default_backend_follows_the_config() {
    let app = builder().build().unwrap();
    assert!(app.ports().is_default(PortSlot::DiscordCache));
    let event = guild_create_with(10, "G", &[(20, "c")], &[(30, "r")]);
    app.shared().discord_cache.event("default", &event).await;
    let backend = &app.ports().discord_cache_backend;
    assert_eq!(backend.guild(Id::new(10)).await.unwrap().unwrap().name, "G");
    assert!(backend.role(Id::new(30)).await.unwrap().is_some());
    assert_eq!(
        format!(
            "{:?}",
            app.ports()
                .discord_cache_backend
                .role_ids(Id::new(10))
                .await
                .unwrap()
        ),
        "Some([Id<RoleMarker>(30)])"
    );
    let reader = crate::DiscordCacheReader::of(Some(app.shared()));
    assert_eq!(reader.resources(), DiscordCacheConfig::DEFAULT_RESOURCES);

    // A USER-only config gets a USER-only backend: GUILD_CREATE feeds it,
    // but the guild itself is not kept.
    let app = builder()
        .discord_cache(DiscordCacheConfig::new(R::USER))
        .build()
        .unwrap();
    app.shared().discord_cache.event("default", &event).await;
    let backend = &app.ports().discord_cache_backend;
    assert!(backend.guild(Id::new(10)).await.unwrap().is_none());
    assert!(backend.role(Id::new(30)).await.unwrap().is_none());
}

#[tokio::test]
async fn f3_none_with_custom_backend_warns_once_at_build() {
    let capture = Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let shared = Arc::new(MemoryDiscordCache::new(R::all()));
    let app = builder()
        .discord_cache(DiscordCacheConfig::none())
        .discord_cache_backend(Arc::clone(&shared))
        .build()
        .unwrap();
    let lines = capture.lines_with(&["WARN", "noctis::app", "DiscordCacheConfig::none()"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    // The backend is never fed.
    let event = guild_create_with(10, "G", &[], &[]);
    app.shared().discord_cache.event("default", &event).await;
    assert!(shared.guild(Id::new(10)).await.unwrap().is_none());

    // A custom backend with an enabled cache does not warn.
    let capture = Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    builder()
        .discord_cache_backend(Arc::clone(&shared))
        .build()
        .unwrap();
    assert!(
        capture
            .lines_with(&["DiscordCacheConfig::none()"])
            .is_empty()
    );
}

#[tokio::test]
async fn f7_cache_fed_types_are_never_dropped() {
    let app = builder()
        .module(Module::new("m").__add(noop_with::<RoleCreate>("r", false, true)))
        .build()
        .unwrap();
    assert!(
        app.registry()
            .droppable()
            .contains(EventTypeFlags::ROLE_CREATE)
    );
    assert!(
        !app.lane_config()
            .droppable
            .contains(EventTypeFlags::ROLE_CREATE)
    );

    let app = builder()
        .discord_cache(DiscordCacheConfig::none())
        .module(Module::new("m").__add(noop_with::<RoleCreate>("r", false, true)))
        .build()
        .unwrap();
    assert!(
        app.lane_config()
            .droppable
            .contains(EventTypeFlags::ROLE_CREATE)
    );
}
