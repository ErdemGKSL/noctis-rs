use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_ports::BoxFuture;
use twilight_model::application::interaction::Interaction;

use super::*;
use crate::test_support::{Capture, guild_create_with, role_create};

use DiscordCacheResources as R;

/// Every single resource, in bit order.
const SINGLE: [DiscordCacheResources; 6] = [
    R::GUILD,
    R::CHANNEL,
    R::ROLE,
    R::MEMBER,
    R::USER,
    R::MESSAGE,
];

fn memory_feed(config: DiscordCacheConfig) -> CacheFeed {
    CacheFeed::new(
        Arc::new(MemoryDiscordCache::with_message_cap(
            config.resources(),
            config.message_cap(),
        )),
        config,
    )
}

// ---- configuration (P1-P9) ----

#[test]
fn p1_default() {
    let config = DiscordCacheConfig::default();
    assert_eq!(config.resources(), R::GUILD | R::CHANNEL | R::ROLE);
    assert_eq!(config.resources(), DiscordCacheConfig::DEFAULT_RESOURCES);
    assert_eq!(config.message_cap(), 100);
    assert!(config.is_enabled());
    assert_eq!(intents(config.resources()), Intents::GUILDS);
    assert!(privileged(config.resources()).is_empty());
    assert!(feeds_interactions(config.resources()));
}

#[test]
fn p2_disabled() {
    let configs = [
        DiscordCacheConfig::none(),
        DiscordCacheConfig::new(R::empty()),
        DiscordCacheConfig::new(R::MESSAGE).with_message_cap(0),
    ];
    for config in configs {
        assert!(!config.is_enabled(), "{config:?}");
        assert!(config.resources().is_empty(), "{config:?}");
        assert!(event_types(config.resources()).is_empty(), "{config:?}");
        assert!(intents(config.resources()).is_empty(), "{config:?}");
        assert!(!feeds_interactions(config.resources()), "{config:?}");
    }
    assert_eq!(
        DiscordCacheConfig::none(),
        DiscordCacheConfig::new(R::empty())
    );
}

/// The table's rows, restated: (resource, events, intents, privileged).
fn expected_rows() -> [(DiscordCacheResources, EventTypeFlags, Intents, Intents); 6] {
    let lifecycle = EventTypeFlags::GUILD_CREATE
        | EventTypeFlags::GUILD_DELETE
        | EventTypeFlags::UNAVAILABLE_GUILD;
    [
        (
            R::GUILD,
            lifecycle | EventTypeFlags::GUILD_UPDATE,
            Intents::GUILDS,
            Intents::empty(),
        ),
        (
            R::CHANNEL,
            lifecycle
                | EventTypeFlags::CHANNEL_CREATE
                | EventTypeFlags::CHANNEL_UPDATE
                | EventTypeFlags::CHANNEL_DELETE
                | EventTypeFlags::CHANNEL_PINS_UPDATE
                | EventTypeFlags::THREAD_CREATE
                | EventTypeFlags::THREAD_UPDATE
                | EventTypeFlags::THREAD_DELETE
                | EventTypeFlags::THREAD_LIST_SYNC,
            Intents::GUILDS,
            Intents::empty(),
        ),
        (
            R::ROLE,
            lifecycle
                | EventTypeFlags::ROLE_CREATE
                | EventTypeFlags::ROLE_UPDATE
                | EventTypeFlags::ROLE_DELETE,
            Intents::GUILDS,
            Intents::empty(),
        ),
        (
            R::MEMBER,
            lifecycle
                | EventTypeFlags::MEMBER_ADD
                | EventTypeFlags::MEMBER_UPDATE
                | EventTypeFlags::MEMBER_REMOVE
                | EventTypeFlags::MEMBER_CHUNK,
            Intents::empty(),
            Intents::GUILD_MEMBERS,
        ),
        (
            R::USER,
            EventTypeFlags::GUILD_CREATE,
            Intents::GUILDS,
            Intents::empty(),
        ),
        (
            R::MESSAGE,
            EventTypeFlags::MESSAGE_CREATE
                | EventTypeFlags::MESSAGE_UPDATE
                | EventTypeFlags::MESSAGE_DELETE
                | EventTypeFlags::MESSAGE_DELETE_BULK,
            Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES,
            Intents::empty(),
        ),
    ]
}

#[test]
fn p3_all() {
    let all = DiscordCacheConfig::new(R::all()).resources();
    let union = expected_rows()
        .iter()
        .fold(EventTypeFlags::empty(), |acc, row| acc | row.1);
    assert_eq!(event_types(all), union);
    assert_eq!(
        intents(all),
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES
    );
    assert_eq!(privileged(all), Intents::GUILD_MEMBERS);
    assert!(feeds_interactions(all));
}

#[test]
fn p4_each_single_resource() {
    for (resource, events, adds, needs) in expected_rows() {
        assert_eq!(event_types(resource), events, "{resource}");
        assert_eq!(intents(resource), adds, "{resource}");
        assert_eq!(privileged(resource), needs, "{resource}");
        assert_eq!(
            feeds_interactions(resource),
            [R::MEMBER, R::USER, R::ROLE].contains(&resource),
            "{resource}"
        );
    }
    assert_eq!(SINGLE.len(), expected_rows().len());
}

#[test]
fn p5_cap_zero_removes_messages_both_ways() {
    let a = DiscordCacheConfig::default()
        .with_message_cap(0)
        .with_resources(R::MESSAGE);
    let b = DiscordCacheConfig::default()
        .with_resources(R::MESSAGE)
        .with_message_cap(0);
    for config in [a, b] {
        assert!(!config.resources().contains(R::MESSAGE), "{config:?}");
        assert_eq!(config.resources(), DiscordCacheConfig::DEFAULT_RESOURCES);
    }
    assert_eq!(a, b);
}

#[test]
fn p6_caps_accepted() {
    for cap in [1, usize::MAX] {
        let config = DiscordCacheConfig::new(R::MESSAGE).with_message_cap(cap);
        assert_eq!(config.message_cap(), cap);
        assert!(config.resources().contains(R::MESSAGE));
    }
}

#[test]
fn p7_without_resources() {
    assert_eq!(
        DiscordCacheConfig::default()
            .without_resources(R::ROLE)
            .resources(),
        R::GUILD | R::CHANNEL
    );
    let none = DiscordCacheConfig::default().without_resources(R::all());
    assert!(none.resources().is_empty());
    assert!(!none.is_enabled());
}

#[test]
fn p8_resources_display() {
    assert_eq!(R::empty().to_string(), "none");
    assert_eq!(R::GUILD.to_string(), "GUILD");
    assert_eq!((R::ROLE | R::GUILD).to_string(), "GUILD | ROLE");
    assert_eq!(
        R::all().to_string(),
        "GUILD | CHANNEL | ROLE | MEMBER | USER | MESSAGE"
    );
}

#[test]
fn p9_errors() {
    let disabled = CacheDisabled {
        resource: R::MEMBER,
    };
    let text = disabled.to_string();
    assert!(text.contains("MEMBER"), "{text}");
    assert!(text.contains("AppBuilder::discord_cache"), "{text}");
    let error = CacheError::from(PortError::unavailable("down"));
    let source = std::error::Error::source(&error).expect("a source");
    assert!(source.downcast_ref::<PortError>().is_some());
    assert_eq!(
        source.to_string(),
        PortError::unavailable("down").to_string()
    );
    assert_eq!(CacheError::from(disabled).to_string(), text);
}

#[test]
fn missing_intents_per_privileged_intent() {
    let member = DiscordCacheConfig::new(R::MEMBER | R::GUILD);
    assert_eq!(
        missing_intents(member, Intents::GUILDS),
        [(R::MEMBER, Intents::GUILD_MEMBERS)]
    );
    assert!(missing_intents(member, Intents::GUILDS | Intents::GUILD_MEMBERS).is_empty());
    assert!(missing_intents(DiscordCacheConfig::default(), Intents::GUILDS).is_empty());
}

// ---- reader (G1-G9) ----

/// A backend that counts calls, can fail every call, and answers from
/// scripted role lists.
#[derive(Default)]
struct Stub {
    calls: AtomicUsize,
    fail: bool,
    /// `role_ids` answer.
    role_ids: Option<Vec<u64>>,
    /// Role ids whose `role` is `None`.
    missing: Vec<u64>,
    /// Role ids whose `role` fails.
    failing: Vec<u64>,
    /// The scripted `update` results, in call order (then `Ok`).
    updates: Mutex<Vec<Result<(), &'static str>>>,
    /// How long each `update` sleeps first.
    delay: Option<Duration>,
}

impl Stub {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn call<T: Send + 'static>(&self, ok: T) -> BoxFuture<'_, Result<T, PortError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = if self.fail {
            Err(PortError::unavailable("stub down"))
        } else {
            Ok(ok)
        };
        Box::pin(async move { result })
    }
}

fn role(id: u64) -> Role {
    serde_json::from_value(crate::test_support::role_json(id, &format!("r{id}"))).unwrap()
}

impl DiscordCache for Stub {
    fn update<'a>(&'a self, _: &'a Event) -> BoxFuture<'a, Result<(), PortError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let next = {
            let mut updates = self.updates.lock().unwrap();
            if updates.is_empty() {
                Ok(())
            } else {
                updates.remove(0)
            }
        };
        let delay = self.delay;
        Box::pin(async move {
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            next.map_err(PortError::unavailable)
        })
    }

    fn guild(&self, _: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>> {
        self.call(None)
    }

    fn channel(&self, _: Id<ChannelMarker>) -> BoxFuture<'_, Result<Option<Channel>, PortError>> {
        self.call(None)
    }

    fn role(&self, id: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>> {
        if self.failing.contains(&id.get()) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            return Box::pin(async { Err(PortError::unavailable("role failed")) });
        }
        let found = (!self.missing.contains(&id.get())).then(|| role(id.get()));
        self.call(found)
    }

    fn member(
        &self,
        _: Id<GuildMarker>,
        _: Id<UserMarker>,
    ) -> BoxFuture<'_, Result<Option<Member>, PortError>> {
        self.call(None)
    }

    fn user(&self, _: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>> {
        self.call(None)
    }

    fn message(&self, _: Id<MessageMarker>) -> BoxFuture<'_, Result<Option<Message>, PortError>> {
        self.call(None)
    }

    fn channel_ids(
        &self,
        _: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>> {
        self.call(None)
    }

    fn role_ids(
        &self,
        _: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>> {
        let ids = self
            .role_ids
            .as_ref()
            .map(|ids| ids.iter().map(|&id| Id::new(id)).collect());
        self.call(ids)
    }

    fn member_ids(
        &self,
        _: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>> {
        self.call(None)
    }
}

const G: Id<GuildMarker> = Id::new(10);

fn disabled_resource<T: fmt::Debug>(result: Result<T, CacheError>) -> DiscordCacheResources {
    match result {
        Err(CacheError::Disabled(CacheDisabled { resource })) => resource,
        other => panic!("not Disabled: {other:?}"),
    }
}

/// Calls every getter and returns the resource each one reports as
/// disabled.
async fn every_getter_disabled(reader: &DiscordCacheReader) -> Vec<DiscordCacheResources> {
    let u = Id::new(40);
    vec![
        disabled_resource(reader.guild(G).await),
        disabled_resource(reader.channel(Id::new(20)).await),
        disabled_resource(reader.role(Id::new(30)).await),
        disabled_resource(reader.member(G, u).await),
        disabled_resource(reader.user(u).await),
        disabled_resource(reader.message(Id::new(50)).await),
        disabled_resource(reader.channel_ids(G).await),
        disabled_resource(reader.role_ids(G).await),
        disabled_resource(reader.member_ids(G).await),
        disabled_resource(reader.channels(G).await),
        disabled_resource(reader.roles(G).await),
        disabled_resource(reader.members(G).await),
    ]
}

const GETTER_RESOURCES: [DiscordCacheResources; 12] = [
    R::GUILD,
    R::CHANNEL,
    R::ROLE,
    R::MEMBER,
    R::USER,
    R::MESSAGE,
    R::CHANNEL,
    R::ROLE,
    R::MEMBER,
    R::CHANNEL,
    R::ROLE,
    R::MEMBER,
];

#[tokio::test]
async fn g1_default_reads_owned_values() {
    let feed = memory_feed(DiscordCacheConfig::default());
    feed.event(
        "default",
        &guild_create_with(
            10,
            "G",
            &[(21, "b"), (20, "a")],
            &[(32, "z"), (30, "x"), (31, "y")],
        ),
    )
    .await;
    let reader = feed.reader();
    assert_eq!(reader.guild(G).await.unwrap().unwrap().name, "G");
    assert_eq!(
        reader
            .channel(Id::new(20))
            .await
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("a")
    );
    assert_eq!(reader.role(Id::new(31)).await.unwrap().unwrap().name, "y");
    assert_eq!(
        reader.role_ids(G).await.unwrap().unwrap(),
        [Id::new(30), Id::new(31), Id::new(32)]
    );
    let roles: Vec<String> = reader
        .roles(G)
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(roles, ["x", "y", "z"]);
    assert_eq!(
        reader.channel_ids(G).await.unwrap().unwrap(),
        [Id::new(20), Id::new(21)]
    );
    let channels: Vec<Id<ChannelMarker>> = reader
        .channels(G)
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(channels, [Id::new(20), Id::new(21)]);
}

#[tokio::test]
async fn g2_default_misses() {
    let reader = memory_feed(DiscordCacheConfig::default()).reader();
    assert!(reader.guild(Id::new(1)).await.unwrap().is_none());
    assert!(reader.roles(Id::new(1)).await.unwrap().is_none());
}

#[tokio::test]
async fn g3_default_disabled_resources_skip_the_backend() {
    let stub = Arc::new(Stub::default());
    let feed = CacheFeed::new(stub.clone(), DiscordCacheConfig::default());
    let reader = feed.reader();
    let u = Id::new(40);
    assert_eq!(disabled_resource(reader.member(G, u).await), R::MEMBER);
    assert_eq!(disabled_resource(reader.members(G).await), R::MEMBER);
    assert_eq!(disabled_resource(reader.member_ids(G).await), R::MEMBER);
    assert_eq!(disabled_resource(reader.user(u).await), R::USER);
    assert_eq!(
        disabled_resource(reader.message(Id::new(1)).await),
        R::MESSAGE
    );
    assert_eq!(stub.calls(), 0);
}

#[tokio::test]
async fn g4_none_disables_every_getter() {
    let stub = Arc::new(Stub::default());
    let feed = CacheFeed::new(stub.clone(), DiscordCacheConfig::none());
    let reader = feed.reader();
    assert!(!reader.is_enabled());
    assert!(reader.resources().is_empty());
    assert_eq!(every_getter_disabled(&reader).await, GETTER_RESOURCES);
    assert_eq!(stub.calls(), 0);
}

#[tokio::test]
async fn g5_detached_reader() {
    let reader = DiscordCacheReader::of(None);
    assert_eq!(disabled_resource(reader.guild(Id::new(1)).await), R::GUILD);
    assert_eq!(every_getter_disabled(&reader).await, GETTER_RESOURCES);
    assert_eq!(
        format!("{reader:?}"),
        "DiscordCacheReader { resources: DiscordCacheResources(0), .. }"
    );
}

#[tokio::test]
async fn g6_listed_but_missing_role_is_skipped() {
    let stub = Arc::new(Stub {
        role_ids: Some(vec![1, 2, 3]),
        missing: vec![2],
        ..Stub::default()
    });
    let reader = CacheFeed::new(stub.clone(), DiscordCacheConfig::default()).reader();
    let ids: Vec<u64> = reader
        .roles(G)
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|r| r.id.get())
        .collect();
    assert_eq!(ids, [1, 3]);
    assert_eq!(stub.calls(), 4);
}

#[tokio::test]
async fn g7_first_role_error_is_returned() {
    let stub = Arc::new(Stub {
        role_ids: Some(vec![1, 2, 3]),
        failing: vec![2],
        ..Stub::default()
    });
    let reader = CacheFeed::new(stub, DiscordCacheConfig::default()).reader();
    assert!(matches!(reader.roles(G).await, Err(CacheError::Port(_))));
}

#[tokio::test]
async fn g8_backend_error_is_port() {
    let stub = Arc::new(Stub {
        fail: true,
        ..Stub::default()
    });
    let reader = CacheFeed::new(stub, DiscordCacheConfig::default()).reader();
    match reader.guild(Id::new(1)).await {
        Err(CacheError::Port(error)) => assert!(error.to_string().contains("stub down"), "{error}"),
        other => panic!("{other:?}"),
    }
}

/// G9: the reader and every getter future are `Send`, the outputs `'static`.
#[test]
fn g9_type_level() {
    fn send_sync_static<T: Clone + Send + Sync + 'static>() {}
    fn send<F: Future<Output = O> + Send, O: 'static>(_: F) {}
    send_sync_static::<DiscordCacheReader>();
    let reader = DiscordCacheReader::detached();
    let u = Id::new(1);
    send(reader.guild(G));
    send(reader.channel(Id::new(1)));
    send(reader.role(Id::new(1)));
    send(reader.member(G, u));
    send(reader.user(u));
    send(reader.message(Id::new(1)));
    send(reader.channel_ids(G));
    send(reader.role_ids(G));
    send(reader.member_ids(G));
    send(reader.channels(G));
    send(reader.roles(G));
    send(reader.members(G));
}

// ---- feed (F1-F6) ----

fn counter(snapshotter: &Snapshotter) -> u64 {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.key().name() == "noctis_discord_cache_update_errors_total")
        .map(|(key, _, _, value)| {
            assert!(
                key.key()
                    .labels()
                    .any(|l| l.key() == "app" && l.value() == "default")
            );
            match value {
                DebugValue::Counter(n) => n,
                other => panic!("{other:?}"),
            }
        })
        .sum()
}

#[tokio::test]
async fn f1_f2_event_filter() {
    let stub = Arc::new(Stub::default());
    let feed = CacheFeed::new(stub.clone(), DiscordCacheConfig::default());
    feed.event("default", &role_create(10, 30, "Mods")).await;
    assert_eq!(stub.calls(), 1);
    feed.event("default", &crate::test_support::message_delete(1, Some(10)))
        .await;
    assert_eq!(stub.calls(), 1, "MESSAGE_DELETE is not fed by default");
}

#[tokio::test]
async fn f3_none_never_feeds() {
    let stub = Arc::new(Stub::default());
    let feed = CacheFeed::new(stub.clone(), DiscordCacheConfig::none());
    feed.event("default", &role_create(10, 30, "Mods")).await;
    feed.event("default", &guild_create_with(10, "G", &[], &[]))
        .await;
    feed.interaction("default", &interaction()).await;
    assert_eq!(stub.calls(), 0);
}

#[tokio::test]
async fn f4_failures_are_counted_and_logged_per_outage() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = ::metrics::set_default_local_recorder(&recorder);
    let capture = Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let stub = Arc::new(Stub {
        updates: Mutex::new(vec![Err("e1"), Err("e2"), Ok(()), Err("e3")]),
        ..Stub::default()
    });
    let feed = CacheFeed::new(stub.clone(), DiscordCacheConfig::default());
    let event = role_create(10, 30, "Mods");
    let warns =
        || capture.lines_with(&["WARN", "noctis::cache", "Discord cache updates are failing"]);
    let infos =
        || capture.lines_with(&["INFO", "noctis::cache", "Discord cache updates recovered"]);

    feed.event("default", &event).await;
    assert_eq!(warns().len(), 1);
    assert!(warns()[0].contains("e1"), "{}", warns()[0]);
    feed.event("default", &event).await;
    assert_eq!(warns().len(), 1);
    assert_eq!(infos().len(), 0);
    feed.event("default", &event).await;
    assert_eq!(infos().len(), 1);
    feed.event("default", &event).await;
    assert_eq!(warns().len(), 2);
    assert_eq!(counter(&snapshotter), 3);
    assert!(
        !capture.text().contains("Mods"),
        "event contents never logged"
    );
}

#[tokio::test(start_paused = true)]
async fn f5_slow_update_times_out_after_one_second() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = ::metrics::set_default_local_recorder(&recorder);
    let capture = Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let stub = Arc::new(Stub {
        delay: Some(Duration::from_secs(5)),
        ..Stub::default()
    });
    let feed = CacheFeed::new(stub, DiscordCacheConfig::default());
    let start = tokio::time::Instant::now();
    feed.event("default", &role_create(10, 30, "Mods")).await;
    assert_eq!(start.elapsed(), CACHE_UPDATE_TIMEOUT);
    assert_eq!(counter(&snapshotter), 1);
    let warns = capture.lines_with(&["WARN", "Discord cache updates are failing"]);
    assert_eq!(warns.len(), 1);
    assert!(warns[0].contains("timed out"), "{}", warns[0]);
}

/// A guild command interaction by user 40 in guild 10.
fn interaction() -> Interaction {
    serde_json::from_value(serde_json::json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "guild_id": "10",
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": { "id": "40", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null },
        "data": { "id": "9", "name": "ping", "type": 1, "options": [] },
    }))
    .unwrap()
}

/// A backend recording the kinds of the events it is fed.
#[derive(Default)]
struct Kinds(Mutex<Vec<twilight_model::gateway::event::EventType>>);

impl Kinds {
    fn seen(&self) -> Vec<twilight_model::gateway::event::EventType> {
        self.0.lock().unwrap().clone()
    }
}

macro_rules! no_reads {
    ($($name:ident($($arg:ty),*) -> $out:ty;)*) => {
        $(fn $name(&self, $(_: $arg),*) -> BoxFuture<'_, Result<$out, PortError>> {
            Box::pin(async { Ok(None) })
        })*
    };
}

impl DiscordCache for Kinds {
    fn update<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.lock().unwrap().push(event.kind());
        Box::pin(async { Ok(()) })
    }

    no_reads! {
        guild(Id<GuildMarker>) -> Option<Guild>;
        channel(Id<ChannelMarker>) -> Option<Channel>;
        role(Id<RoleMarker>) -> Option<Role>;
        member(Id<GuildMarker>, Id<UserMarker>) -> Option<Member>;
        user(Id<UserMarker>) -> Option<User>;
        message(Id<MessageMarker>) -> Option<Message>;
        channel_ids(Id<GuildMarker>) -> Option<Vec<Id<ChannelMarker>>>;
        role_ids(Id<GuildMarker>) -> Option<Vec<Id<RoleMarker>>>;
        member_ids(Id<GuildMarker>) -> Option<Vec<Id<UserMarker>>>;
    }
}

#[tokio::test]
async fn f6_interaction_feed() {
    let kinds = Arc::new(Kinds::default());
    let feed = CacheFeed::new(kinds.clone(), DiscordCacheConfig::default());
    feed.interaction("default", &interaction()).await;
    assert_eq!(
        kinds.seen(),
        [twilight_model::gateway::event::EventType::InteractionCreate]
    );

    let kinds = Arc::new(Kinds::default());
    let feed = CacheFeed::new(
        kinds.clone(),
        DiscordCacheConfig::new(R::GUILD | R::CHANNEL),
    );
    feed.interaction("default", &interaction()).await;
    assert!(kinds.seen().is_empty());

    // The user reaches a USER cache through the interaction.
    let feed = memory_feed(DiscordCacheConfig::default().with_resources(R::USER));
    feed.interaction("default", &interaction()).await;
    assert!(feed.reader().user(Id::new(40)).await.unwrap().is_some());
}
