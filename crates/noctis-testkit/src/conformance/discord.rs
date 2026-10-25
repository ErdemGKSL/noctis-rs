//! The `DiscordCache` suite, over the gateway frames in `fixtures/`.

use std::fmt::Debug;

use noctis_ports::memory::DiscordCacheResources as R;
use noctis_ports::{DiscordCache, PortError};
use serde::de::DeserializeSeed;
use twilight_model::channel::Message;
use twilight_model::gateway::event::{Event, GatewayEventDeserializer};
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};

use super::Case;

const SUITE: &str = "discord_cache";

const GUILD: Id<GuildMarker> = Id::new(1);
const CHANNEL: Id<ChannelMarker> = Id::new(10);
const ROLE: Id<RoleMarker> = Id::new(20);
const USER: Id<UserMarker> = Id::new(30);
const MESSAGE: Id<MessageMarker> = Id::new(40);

/// The gateway frames, by name.
const FRAMES: [(&str, &str); 8] = [
    ("guild_create", include_str!("fixtures/guild_create.json")),
    (
        "message_create",
        include_str!("fixtures/message_create.json"),
    ),
    (
        "channel_update",
        include_str!("fixtures/channel_update.json"),
    ),
    (
        "guild_role_delete",
        include_str!("fixtures/guild_role_delete.json"),
    ),
    (
        "guild_member_remove",
        include_str!("fixtures/guild_member_remove.json"),
    ),
    (
        "channel_delete",
        include_str!("fixtures/channel_delete.json"),
    ),
    (
        "message_delete",
        include_str!("fixtures/message_delete.json"),
    ),
    ("guild_delete", include_str!("fixtures/guild_delete.json")),
];

/// Each single resource, the case that disables it, and what that case
/// turns off: the resource and every resource that needs it. Only `MEMBER`
/// needs another (its user, as `DiscordCacheResources::MEMBER` documents),
/// so `disabled_user` turns off `USER | MEMBER`; every other case turns off
/// its resource alone.
const DISABLED: [(R, &str); 6] = [
    (R::GUILD, "disabled_guild"),
    (R::CHANNEL, "disabled_channel"),
    (R::ROLE, "disabled_role"),
    (R::MEMBER, "disabled_member"),
    (R::USER.union(R::MEMBER), "disabled_user"),
    (R::MESSAGE, "disabled_message"),
];

/// Runs the `DiscordCache` suite: owned reads of every resource after
/// `GUILD_CREATE` and `MESSAGE_CREATE`, updates and deletes in order, and
/// the configured [`DiscordCacheResources`](R) (each one disabled in turn,
/// and channels only).
///
/// The factory receives the resources to cache and must return a fresh,
/// empty cache of exactly those. See the [module docs](super) for the case
/// list.
///
/// # Panics
///
/// On the first failing case, with
/// `` conformance::discord_cache: case `<case>` failed: <detail> ``.
pub async fn discord_cache<C: DiscordCache, F: Fn(R) -> C>(factory: F) {
    empty(&factory(R::all())).await;
    guild_create_all(&factory(R::all())).await;
    unknown_ids(&factory(R::all())).await;
    owned_reads(&factory(R::all())).await;
    updates_in_order(&factory(R::all())).await;
    for (resource, name) in DISABLED {
        disabled(&factory(R::all().without(resource)), resource, name).await;
    }
    channel_only(&factory(R::CHANNEL)).await;
}

/// Parses the fixture frame `name` into an [`Event`].
fn event(case: Case, name: &str) -> Event {
    let Some((_, json)) = FRAMES.iter().find(|(frame, _)| *frame == name) else {
        case.fail(format_args!("no fixture frame `{name}`"));
    };
    let Some(seed) = GatewayEventDeserializer::from_json(json) else {
        case.fail(format_args!("fixture `{name}` is not a gateway frame"));
    };
    let mut de = serde_json::Deserializer::from_str(json);
    match seed.deserialize(&mut de) {
        Ok(event) => Event::from(event),
        Err(err) => case.fail(format_args!("fixture `{name}` did not parse: {err}")),
    }
}

/// Applies the fixture frames `names` in order.
async fn apply(case: Case, cache: &impl DiscordCache, names: &[&str]) {
    for name in names {
        let event = event(case, name);
        let result = cache.update(&event).await;
        case.ok(format_args!("update({name})"), result);
    }
}

/// Unwraps a getter's result.
fn read<T>(case: Case, getter: &str, result: Result<T, PortError>) -> T {
    case.ok(getter, result)
}

/// Fails unless the getter returned `None`.
fn none<T: Debug>(case: Case, getter: &str, value: Option<T>, when: &str) {
    if let Some(value) = value {
        case.fail(format_args!(
            "{getter} {when}: expected None, got {value:?}"
        ));
    }
}

/// Fails unless the getter returned `Some`, and returns the value.
fn some<T>(case: Case, getter: &str, value: Option<T>, when: &str) -> T {
    value.unwrap_or_else(|| case.fail(format_args!("{getter} {when}: expected Some, got None")))
}

/// Sorted ids of a list getter, `None` failing the case.
fn sorted<T: Ord>(case: Case, getter: &str, ids: Option<Vec<T>>, when: &str) -> Vec<T> {
    let mut ids = some(case, getter, ids, when);
    ids.sort_unstable();
    ids
}

fn ids<T>(raw: &[u64]) -> Vec<Id<T>> {
    raw.iter().map(|&id| Id::new(id)).collect()
}

/// One id of each kind: guild, channel, role, user, message.
type Ids = (
    Id<GuildMarker>,
    Id<ChannelMarker>,
    Id<RoleMarker>,
    Id<UserMarker>,
    Id<MessageMarker>,
);

/// Every getter on `guild`, `channel`, `role`, `user` and `message` must be
/// `None`.
async fn all_none(case: Case, cache: &impl DiscordCache, ids: Ids, when: &str) {
    let (guild, channel, role, user, message) = ids;
    none(
        case,
        "guild",
        read(case, "guild", cache.guild(guild).await),
        when,
    );
    none(
        case,
        "channel",
        read(case, "channel", cache.channel(channel).await),
        when,
    );
    none(
        case,
        "role",
        read(case, "role", cache.role(role).await),
        when,
    );
    none(
        case,
        "member",
        read(case, "member", cache.member(guild, user).await),
        when,
    );
    none(
        case,
        "member",
        read(case, "member", cache.member(GUILD, user).await),
        when,
    );
    none(
        case,
        "user",
        read(case, "user", cache.user(user).await),
        when,
    );
    none(
        case,
        "message",
        read(case, "message", cache.message(message).await),
        when,
    );
    none(
        case,
        "channel_ids",
        read(case, "channel_ids", cache.channel_ids(guild).await),
        when,
    );
    none(
        case,
        "role_ids",
        read(case, "role_ids", cache.role_ids(guild).await),
        when,
    );
    none(
        case,
        "member_ids",
        read(case, "member_ids", cache.member_ids(guild).await),
        when,
    );
}

async fn empty(cache: &impl DiscordCache) {
    let case = Case::new(SUITE, "empty");
    all_none(
        case,
        cache,
        (GUILD, CHANNEL, ROLE, USER, MESSAGE),
        "on an empty cache",
    )
    .await;
}

/// The message the `message_create` fixture carries.
fn message_payload(case: Case) -> Message {
    match event(case, "message_create") {
        Event::MessageCreate(create) => create.0,
        other => case.fail(format_args!("fixture is not MESSAGE_CREATE: {other:?}")),
    }
}

async fn guild_create_all(cache: &impl DiscordCache) {
    let case = Case::new(SUITE, "guild_create_all");
    apply(case, cache, &["guild_create", "message_create"]).await;
    let when = "after GUILD_CREATE and MESSAGE_CREATE";

    let guild = some(
        case,
        "guild(1)",
        read(case, "guild", cache.guild(GUILD).await),
        when,
    );
    case.check(guild.name == "Noctis Test Guild", || {
        format!(
            "guild(1).name: expected \"Noctis Test Guild\", got {:?}",
            guild.name
        )
    });
    let channel = some(
        case,
        "channel(10)",
        read(case, "channel", cache.channel(CHANNEL).await),
        when,
    );
    case.check(channel.name.as_deref() == Some("general"), || {
        format!(
            "channel(10).name: expected Some(\"general\"), got {:?}",
            channel.name
        )
    });
    let role = some(
        case,
        "role(20)",
        read(case, "role", cache.role(ROLE).await),
        when,
    );
    case.check(role.name == "Moderator", || {
        format!("role(20).name: expected \"Moderator\", got {:?}", role.name)
    });
    let member = some(
        case,
        "member(1, 30)",
        read(case, "member", cache.member(GUILD, USER).await),
        when,
    );
    case.check(member.user.id == USER && member.roles == [ROLE], || {
        format!(
            "member(1, 30): expected user 30 with roles [20], got user {} with roles {:?}",
            member.user.id, member.roles
        )
    });
    let user = some(
        case,
        "user(30)",
        read(case, "user", cache.user(USER).await),
        when,
    );
    case.check(user.id == USER, || {
        format!("user(30) returned user {}", user.id)
    });
    let message = some(
        case,
        "message(40)",
        read(case, "message", cache.message(MESSAGE).await),
        when,
    );
    let expected = message_payload(case);
    case.check(message == expected, || {
        format!("message(40): expected the MESSAGE_CREATE payload {expected:?}, got {message:?}")
    });

    let channels = sorted(
        case,
        "channel_ids(1)",
        read(case, "channel_ids", cache.channel_ids(GUILD).await),
        when,
    );
    case.check(channels == ids::<ChannelMarker>(&[10, 11]), || {
        format!("channel_ids(1): expected [10, 11], got {channels:?}")
    });
    let roles = sorted(
        case,
        "role_ids(1)",
        read(case, "role_ids", cache.role_ids(GUILD).await),
        when,
    );
    case.check(roles == ids::<RoleMarker>(&[1, 20]), || {
        format!("role_ids(1): expected [1, 20], got {roles:?}")
    });
    let members = sorted(
        case,
        "member_ids(1)",
        read(case, "member_ids", cache.member_ids(GUILD).await),
        when,
    );
    case.check(members == ids::<UserMarker>(&[30]), || {
        format!("member_ids(1): expected [30], got {members:?}")
    });
}

async fn unknown_ids(cache: &impl DiscordCache) {
    let case = Case::new(SUITE, "unknown_ids");
    apply(case, cache, &["guild_create", "message_create"]).await;
    let unknown = (
        Id::new(999),
        Id::new(999),
        Id::new(999),
        Id::new(999),
        Id::new(999),
    );
    all_none(case, cache, unknown, "for an unknown id (999)").await;
}

async fn owned_reads(cache: &impl DiscordCache) {
    let case = Case::new(SUITE, "owned_reads");
    apply(case, cache, &["guild_create"]).await;
    let when = "after GUILD_CREATE";
    let mut guild = some(
        case,
        "guild(1)",
        read(case, "guild", cache.guild(GUILD).await),
        when,
    );
    "changed".clone_into(&mut guild.name);
    let again = some(
        case,
        "guild(1)",
        read(case, "guild", cache.guild(GUILD).await),
        when,
    );
    case.check(again.name == "Noctis Test Guild", || {
        format!(
            "guild(1).name after changing a returned copy: expected \"Noctis Test Guild\", got {:?} (reads must be owned)",
            again.name
        )
    });
}

async fn updates_in_order(cache: &impl DiscordCache) {
    let case = Case::new(SUITE, "updates_in_order");
    apply(
        case,
        cache,
        &["guild_create", "message_create", "channel_update"],
    )
    .await;
    let channel = some(
        case,
        "channel(10)",
        read(case, "channel", cache.channel(CHANNEL).await),
        "after CHANNEL_UPDATE",
    );
    case.check(channel.name.as_deref() == Some("renamed"), || {
        format!(
            "channel(10).name after CHANNEL_UPDATE: expected Some(\"renamed\"), got {:?}",
            channel.name
        )
    });

    apply(case, cache, &["guild_role_delete"]).await;
    none(
        case,
        "role(20)",
        read(case, "role", cache.role(ROLE).await),
        "after GUILD_ROLE_DELETE",
    );

    apply(case, cache, &["guild_member_remove"]).await;
    none(
        case,
        "member(1, 30)",
        read(case, "member", cache.member(GUILD, USER).await),
        "after GUILD_MEMBER_REMOVE",
    );

    apply(case, cache, &["channel_delete"]).await;
    let channels = sorted(
        case,
        "channel_ids(1)",
        read(case, "channel_ids", cache.channel_ids(GUILD).await),
        "after CHANNEL_DELETE",
    );
    case.check(channels == ids::<ChannelMarker>(&[10]), || {
        format!("channel_ids(1) after CHANNEL_DELETE of 11: expected [10], got {channels:?}")
    });

    apply(case, cache, &["message_delete"]).await;
    none(
        case,
        "message(40)",
        read(case, "message", cache.message(MESSAGE).await),
        "after MESSAGE_DELETE",
    );

    apply(case, cache, &["guild_delete"]).await;
    none(
        case,
        "guild(1)",
        read(case, "guild", cache.guild(GUILD).await),
        "after GUILD_DELETE",
    );
}

/// Whether each primary getter (guild, channel, role, member, user,
/// message) returns `Some` after `GUILD_CREATE` and `MESSAGE_CREATE`.
async fn presence(case: Case, cache: &impl DiscordCache) -> [(R, &'static str, bool); 6] {
    [
        (
            R::GUILD,
            "guild(1)",
            read(case, "guild", cache.guild(GUILD).await).is_some(),
        ),
        (
            R::CHANNEL,
            "channel(10)",
            read(case, "channel", cache.channel(CHANNEL).await).is_some(),
        ),
        (
            R::ROLE,
            "role(20)",
            read(case, "role", cache.role(ROLE).await).is_some(),
        ),
        (
            R::MEMBER,
            "member(1, 30)",
            read(case, "member", cache.member(GUILD, USER).await).is_some(),
        ),
        (
            R::USER,
            "user(30)",
            read(case, "user", cache.user(USER).await).is_some(),
        ),
        (
            R::MESSAGE,
            "message(40)",
            read(case, "message", cache.message(MESSAGE).await).is_some(),
        ),
    ]
}

async fn disabled(cache: &impl DiscordCache, off: R, name: &'static str) {
    let case = Case::new(SUITE, name);
    apply(case, cache, &["guild_create", "message_create"]).await;
    for (resource, getter, cached) in presence(case, cache).await {
        let want = !off.contains(resource);
        case.check(cached == want, || {
            let got = if cached { "Some" } else { "None" };
            let expected = if want { "Some" } else { "None" };
            format!(
                "{getter} with resources {}: expected {expected}, got {got}",
                R::all().without(off)
            )
        });
    }
}

async fn channel_only(cache: &impl DiscordCache) {
    let case = Case::new(SUITE, "channel_only");
    apply(case, cache, &["guild_create", "message_create"]).await;
    for (resource, getter, cached) in presence(case, cache).await {
        let want = resource == R::CHANNEL;
        case.check(cached == want, || {
            let got = if cached { "Some" } else { "None" };
            let expected = if want { "Some" } else { "None" };
            format!("{getter} with resources CHANNEL: expected {expected}, got {got}")
        });
    }
}
