//! `MemoryDiscordCache` caches only its resources, applies twilight's message
//! cap and returns owned `twilight_model` values (Story 3.2, cache matrix).

use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use noctis_ports::DiscordCache;
use noctis_ports::memory::{DiscordCacheResources as R, MemoryDiscordCache};
use serde::de::DeserializeSeed;
use twilight_model::gateway::event::{Event, GatewayEventDeserializer};
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::Guild;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};

const GUILD: Id<GuildMarker> = Id::new(1);
const USER: Id<UserMarker> = Id::new(30);

/// Polls a future that is ready on its first poll.
fn now<F: Future>(future: F) -> F::Output {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("future was not ready"),
    }
}

fn frame(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gateway")
        .join(format!("{name}.json"));
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn parse(json: &str) -> Event {
    let Some(seed) = GatewayEventDeserializer::from_json(json) else {
        panic!("not a gateway frame: {json}");
    };
    let mut de = serde_json::Deserializer::from_str(json);
    match seed.deserialize(&mut de) {
        Ok(event) => Event::from(event),
        Err(err) => panic!("invalid dispatch: {err}"),
    }
}

fn event(name: &str) -> Event {
    parse(&frame(name))
}

/// The `MESSAGE_CREATE` fixture with message id `id`.
fn message_create(id: u64) -> Event {
    parse(&frame("message_create").replace(r#""id":"40""#, &format!(r#""id":"{id}""#)))
}

fn apply(cache: &MemoryDiscordCache, events: &[Event]) {
    for event in events {
        if let Err(err) = now(cache.update(event)) {
            panic!("update failed: {err}");
        }
    }
}

fn ids<T>(raw: &[u64]) -> Vec<Id<T>> {
    raw.iter().map(|&id| Id::new(id)).collect()
}

#[test]
fn empty_cache_has_nothing() {
    let cache = MemoryDiscordCache::new(R::all());
    assert!(now(cache.guild(GUILD)).unwrap().is_none());
    assert!(now(cache.channel(Id::new(10))).unwrap().is_none());
    assert!(now(cache.role(Id::new(20))).unwrap().is_none());
    assert!(now(cache.member(GUILD, USER)).unwrap().is_none());
    assert!(now(cache.user(USER)).unwrap().is_none());
    assert!(now(cache.message(Id::new(40))).unwrap().is_none());
    assert!(now(cache.channel_ids(GUILD)).unwrap().is_none());
    assert!(now(cache.role_ids(GUILD)).unwrap().is_none());
    assert!(now(cache.member_ids(GUILD)).unwrap().is_none());
}

#[test]
fn guild_create_with_every_resource() {
    let cache = MemoryDiscordCache::new(R::all());
    apply(&cache, &[event("guild_create")]);

    let guild = now(cache.guild(GUILD)).unwrap().expect("guild cached");
    assert_eq!(guild.name, "Noctis Test Guild");
    assert!(guild.channels.is_empty() && guild.roles.is_empty() && guild.members.is_empty());
    assert_eq!(guild.approximate_member_count, None);

    let channel = now(cache.channel(Id::new(10)))
        .unwrap()
        .expect("channel cached");
    assert_eq!(channel.name.as_deref(), Some("general"));
    assert_eq!(
        now(cache.role(Id::new(20))).unwrap().expect("role").name,
        "Moderator"
    );

    let member = now(cache.member(GUILD, USER))
        .unwrap()
        .expect("member cached");
    assert_eq!(member.user.id, USER);
    assert_eq!(member.roles, vec![Id::<RoleMarker>::new(20)]);
    assert_eq!(now(cache.user(USER)).unwrap().expect("user").id, USER);

    assert_eq!(
        now(cache.channel_ids(GUILD)).unwrap(),
        Some(ids::<ChannelMarker>(&[10, 11]))
    );
    assert_eq!(
        now(cache.role_ids(GUILD)).unwrap(),
        Some(ids::<RoleMarker>(&[1, 20]))
    );
    assert_eq!(
        now(cache.member_ids(GUILD)).unwrap(),
        Some(ids::<UserMarker>(&[30]))
    );
}

/// The `Guild` the `guild_create` fixture carries.
fn guild_payload() -> Guild {
    match event("guild_create") {
        Event::GuildCreate(create) => match *create {
            GuildCreate::Available(guild) => guild,
            GuildCreate::Unavailable(_) => panic!("fixture guild is unavailable"),
        },
        other => panic!("not GUILD_CREATE: {other:?}"),
    }
}

#[test]
fn cached_guild_and_member_match_the_payload_but_for_lossy_fields() {
    let cache = MemoryDiscordCache::new(R::all());
    apply(&cache, &[event("guild_create")]);
    let mut expected = guild_payload();

    let mut member = expected
        .members
        .iter()
        .find(|m| m.user.id == USER)
        .cloned()
        .expect("payload member 30");
    member.avatar_decoration_data = None;
    member.banner = None;
    assert_eq!(now(cache.member(GUILD, USER)).unwrap(), Some(member));

    expected.channels.clear();
    expected.roles.clear();
    expected.members.clear();
    expected.emojis.clear();
    expected.stickers.clear();
    expected.presences.clear();
    expected.voice_states.clear();
    expected.threads.clear();
    expected.stage_instances.clear();
    expected.approximate_member_count = None;
    expected.approximate_presence_count = None;
    assert_eq!(now(cache.guild(GUILD)).unwrap(), Some(expected));
}

#[test]
fn getters_return_owned_clones() {
    let cache = MemoryDiscordCache::new(R::all());
    apply(&cache, &[event("guild_create")]);
    let mut guild = now(cache.guild(GUILD)).unwrap().expect("guild cached");
    guild.name = "changed".to_owned();
    assert_eq!(
        now(cache.guild(GUILD)).unwrap().expect("guild").name,
        "Noctis Test Guild"
    );
}

#[test]
fn restricted_to_channels() {
    let cache = MemoryDiscordCache::new(R::CHANNEL);
    apply(&cache, &[event("guild_create"), event("message_create")]);
    assert!(now(cache.channel(Id::new(10))).unwrap().is_some());
    assert!(now(cache.guild(GUILD)).unwrap().is_none());
    assert!(now(cache.role(Id::new(20))).unwrap().is_none());
    assert!(now(cache.member(GUILD, USER)).unwrap().is_none());
    assert!(now(cache.user(USER)).unwrap().is_none());
    assert!(now(cache.message(Id::new(40))).unwrap().is_none());
    assert!(now(cache.channel_ids(GUILD)).unwrap().is_some());
    assert!(now(cache.role_ids(GUILD)).unwrap().is_none());
    assert!(now(cache.member_ids(GUILD)).unwrap().is_none());
}

#[test]
fn member_without_user_still_resolves() {
    let cache = MemoryDiscordCache::new(R::GUILD | R::MEMBER);
    apply(&cache, &[event("guild_create")]);
    assert!(now(cache.member(GUILD, USER)).unwrap().is_some());
    assert_eq!(cache.resources(), R::GUILD | R::MEMBER);
    assert!(!cache.resources().contains(R::USER));
}

#[test]
fn message_create_is_full_fidelity() {
    let cache = MemoryDiscordCache::new(R::all());
    let created = event("message_create");
    apply(&cache, std::slice::from_ref(&created));
    let Event::MessageCreate(expected) = created else {
        panic!("not a MESSAGE_CREATE");
    };
    let message = now(cache.message(Id::new(40)))
        .unwrap()
        .expect("message cached");
    assert_eq!(message, expected.0);
    assert_eq!(message.author.id, USER);
    assert_eq!(message.content, "hello noctis");
}

#[test]
fn message_cap_evicts_the_oldest() {
    let cache = MemoryDiscordCache::with_message_cap(R::all(), 2);
    assert_eq!(cache.message_cap(), 2);
    apply(
        &cache,
        &[message_create(40), message_create(41), message_create(42)],
    );
    assert!(now(cache.message(Id::new(40))).unwrap().is_none());
    assert!(now(cache.message(Id::new(41))).unwrap().is_some());
    assert!(now(cache.message(Id::new(42))).unwrap().is_some());
}

#[test]
fn message_cap_zero_disables_messages() {
    let cache = MemoryDiscordCache::with_message_cap(R::all(), 0);
    assert!(!cache.resources().contains(R::MESSAGE));
    apply(&cache, &[message_create(40)]);
    assert!(now(cache.message(Id::new(40))).unwrap().is_none());
}

#[test]
fn updates_are_reflected_in_order() {
    let cache: Arc<dyn DiscordCache> = Arc::new(MemoryDiscordCache::new(R::all()));
    let apply = |name: &str| now(cache.update(&event(name))).unwrap();
    apply("guild_create");
    apply("message_create");

    apply("channel_update");
    let channel = now(cache.channel(Id::new(10))).unwrap().expect("channel");
    assert_eq!(channel.name.as_deref(), Some("renamed"));

    apply("guild_role_delete");
    assert!(now(cache.role(Id::new(20))).unwrap().is_none());

    apply("guild_member_remove");
    assert!(now(cache.member(GUILD, USER)).unwrap().is_none());

    apply("channel_delete");
    assert_eq!(
        now(cache.channel_ids(GUILD)).unwrap(),
        Some(ids::<ChannelMarker>(&[10]))
    );

    apply("message_delete");
    assert!(now(cache.message(Id::new(40))).unwrap().is_none());

    apply("guild_delete");
    assert!(now(cache.guild(GUILD)).unwrap().is_none());
}

#[test]
fn resource_set_ops() {
    let set = R::GUILD | R::ROLE;
    assert!(set.contains(R::GUILD));
    assert!(!set.contains(R::USER));
    assert_eq!(set, R::GUILD.union(R::ROLE));
    for resource in [
        R::GUILD,
        R::CHANNEL,
        R::ROLE,
        R::MEMBER,
        R::USER,
        R::MESSAGE,
    ] {
        assert!(R::all().contains(resource));
        assert!(!resource.is_empty());
    }
    assert!(R::empty().is_empty());
    assert_eq!(R::default(), R::empty());
    assert_eq!(MemoryDiscordCache::DEFAULT_MESSAGE_CAP, 100);
    assert_eq!(MemoryDiscordCache::new(set).message_cap(), 100);
    let debug = format!("{:?}", MemoryDiscordCache::new(set));
    assert!(debug.contains("MemoryDiscordCache"), "{debug}");
}
