//! [`MemoryDiscordCache`]: the default [`DiscordCache`], over
//! `twilight-cache-inmemory`.

use std::fmt;
use std::ops::BitOr;

use twilight_cache_inmemory::model::{CachedGuild, CachedMember};
use twilight_cache_inmemory::traits::CacheableMessage;
use twilight_cache_inmemory::{CacheableModels, DefaultCacheModels, InMemoryCache, ResourceType};
use twilight_model::channel::message::Reaction;
use twilight_model::channel::{Channel, Message};
use twilight_model::gateway::event::Event;
use twilight_model::guild::{Guild, Member, Role};
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use twilight_model::user::User;

use crate::{BoxFuture, DiscordCache, PortError};

/// A set of Discord resources for [`MemoryDiscordCache`] to cache.
///
/// Combine the constants with `|` or [`union`](Self::union); the
/// [`Default`] is [`empty`](Self::empty).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DiscordCacheResources(u8);

impl DiscordCacheResources {
    /// Guilds (without their member, channel or role lists).
    pub const GUILD: Self = Self(1);
    /// Guild channels and threads.
    pub const CHANNEL: Self = Self(1 << 1);
    /// Guild roles.
    pub const ROLE: Self = Self(1 << 2);
    /// Guild members (their users are cached with them).
    pub const MEMBER: Self = Self(1 << 3);
    /// Users.
    pub const USER: Self = Self(1 << 4);
    /// Messages, up to the per-channel message cap.
    pub const MESSAGE: Self = Self(1 << 5);

    /// No resource.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Every resource above.
    pub const fn all() -> Self {
        Self(0b11_1111)
    }

    /// The resources in `self` or `other`.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every resource in `other` is in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set has no resource.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    fn resource_types(self) -> ResourceType {
        const MAP: [(DiscordCacheResources, ResourceType); 6] = [
            (DiscordCacheResources::GUILD, ResourceType::GUILD),
            (DiscordCacheResources::CHANNEL, ResourceType::CHANNEL),
            (DiscordCacheResources::ROLE, ResourceType::ROLE),
            // A Member needs its User.
            (
                DiscordCacheResources::MEMBER,
                ResourceType::MEMBER.union(ResourceType::USER),
            ),
            (DiscordCacheResources::USER, ResourceType::USER),
            (DiscordCacheResources::MESSAGE, ResourceType::MESSAGE),
        ];
        MAP.iter()
            .filter(|(resource, _)| self.contains(*resource))
            .fold(ResourceType::empty(), |types, (_, ty)| types | *ty)
    }
}

impl BitOr for DiscordCacheResources {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// A cached message at full fidelity (twilight's `CachedMessage` drops the
/// author and the mentioned users).
#[derive(Clone, Debug, PartialEq)]
struct MessageRecord(Message);

impl From<Message> for MessageRecord {
    fn from(message: Message) -> Self {
        Self(message)
    }
}

impl PartialEq<Message> for MessageRecord {
    fn eq(&self, other: &Message) -> bool {
        self.0 == *other
    }
}

impl CacheableMessage for MessageRecord {
    fn reactions(&self) -> &[Reaction] {
        &self.0.reactions
    }

    fn reactions_mut(&mut self) -> &mut [Reaction] {
        &mut self.0.reactions
    }

    fn retain_reactions(&mut self, f: impl FnMut(&Reaction) -> bool) {
        self.0.reactions.retain(f);
    }

    fn clear_reactions(&mut self) {
        self.0.reactions.clear();
    }

    fn add_reaction(&mut self, reaction: Reaction) {
        self.0.reactions.push(reaction);
    }

    fn remove_reaction(&mut self, idx: usize) {
        self.0.reactions.remove(idx);
    }
}

/// twilight's default models, except messages, which stay full `Message`s.
#[derive(Clone, Debug)]
struct NoctisModels;

type Twilight = DefaultCacheModels;

impl CacheableModels for NoctisModels {
    type Channel = <Twilight as CacheableModels>::Channel;
    type CurrentUser = <Twilight as CacheableModels>::CurrentUser;
    type Emoji = <Twilight as CacheableModels>::Emoji;
    type Guild = <Twilight as CacheableModels>::Guild;
    type GuildIntegration = <Twilight as CacheableModels>::GuildIntegration;
    type GuildScheduledEvent = <Twilight as CacheableModels>::GuildScheduledEvent;
    type Member = <Twilight as CacheableModels>::Member;
    type Message = MessageRecord;
    type Presence = <Twilight as CacheableModels>::Presence;
    type Role = <Twilight as CacheableModels>::Role;
    type StageInstance = <Twilight as CacheableModels>::StageInstance;
    type Sticker = <Twilight as CacheableModels>::Sticker;
    type User = <Twilight as CacheableModels>::User;
    type VoiceState = <Twilight as CacheableModels>::VoiceState;
}

/// The default [`DiscordCache`]: an in-process cache of the chosen
/// [`DiscordCacheResources`], bounded by them and by a per-channel message
/// cap.
///
/// Every getter returns an owned clone; no lock is held past the call.
///
/// Lossy fields (what the cache does not keep):
/// - [`Guild`]: `channels`, `roles`, `members`, `emojis`, `stickers`,
///   `presences`, `voice_states`, `threads` and `stage_instances` are empty
///   (read them with [`channel_ids`](DiscordCache::channel_ids),
///   [`role_ids`](DiscordCache::role_ids) and
///   [`member_ids`](DiscordCache::member_ids)); `approximate_member_count`
///   and `approximate_presence_count` are `None`.
/// - [`Member`]: `avatar_decoration_data` and `banner` are `None`; `deaf` and
///   `mute` are `false` when unknown (a member seen only in a message). A
///   member whose user is not cached is `None`.
/// - [`Channel`], [`Role`], [`User`] and [`Message`] are stored whole.
pub struct MemoryDiscordCache {
    cache: InMemoryCache<NoctisModels>,
    resources: DiscordCacheResources,
    message_cap: usize,
}

impl MemoryDiscordCache {
    /// The default per-channel message cap (twilight's own default).
    pub const DEFAULT_MESSAGE_CAP: usize = 100;

    /// A cache of `resources`, keeping at most
    /// [`DEFAULT_MESSAGE_CAP`](Self::DEFAULT_MESSAGE_CAP) messages per
    /// channel.
    pub fn new(resources: DiscordCacheResources) -> Self {
        Self::with_message_cap(resources, Self::DEFAULT_MESSAGE_CAP)
    }

    /// A cache of `resources`, keeping at most `cap` messages per channel; a
    /// `cap` of 0 removes [`MESSAGE`](DiscordCacheResources::MESSAGE).
    pub fn with_message_cap(resources: DiscordCacheResources, cap: usize) -> Self {
        let resources = if cap == 0 {
            resources.without(DiscordCacheResources::MESSAGE)
        } else {
            resources
        };
        let cache = InMemoryCache::<NoctisModels>::builder()
            .resource_types(resources.resource_types())
            .message_cache_size(cap.max(1))
            .build();
        Self {
            cache,
            resources,
            message_cap: cap,
        }
    }

    /// The cached resources, as requested.
    pub const fn resources(&self) -> DiscordCacheResources {
        self.resources
    }

    /// The per-channel message cap.
    pub const fn message_cap(&self) -> usize {
        self.message_cap
    }

    fn sorted<T: Ord + Copy>(ids: &std::collections::HashSet<T>) -> Vec<T> {
        let mut ids: Vec<T> = ids.iter().copied().collect();
        ids.sort_unstable();
        ids
    }
}

impl fmt::Debug for MemoryDiscordCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryDiscordCache")
            .field("resources", &self.resources)
            .field("message_cap", &self.message_cap)
            .finish_non_exhaustive()
    }
}

fn guild_from_cached(guild: &CachedGuild) -> Guild {
    Guild {
        afk_channel_id: guild.afk_channel_id(),
        afk_timeout: guild.afk_timeout(),
        application_id: guild.application_id(),
        approximate_member_count: None,
        approximate_presence_count: None,
        banner: guild.banner().copied(),
        channels: Vec::new(),
        default_message_notifications: guild.default_message_notifications(),
        description: guild.description().map(str::to_owned),
        discovery_splash: guild.discovery_splash().copied(),
        emojis: Vec::new(),
        explicit_content_filter: guild.explicit_content_filter(),
        features: guild.features().cloned().collect(),
        guild_scheduled_events: guild.guild_scheduled_events().to_vec(),
        icon: guild.icon().copied(),
        id: guild.id(),
        joined_at: guild.joined_at(),
        large: guild.large(),
        max_members: guild.max_members(),
        max_presences: guild.max_presences(),
        max_stage_video_channel_users: guild.max_stage_video_channel_users(),
        max_video_channel_users: guild.max_video_channel_users(),
        member_count: guild.member_count(),
        members: Vec::new(),
        mfa_level: guild.mfa_level(),
        name: guild.name().to_owned(),
        nsfw_level: guild.nsfw_level(),
        owner_id: guild.owner_id(),
        owner: guild.owner(),
        permissions: guild.permissions(),
        preferred_locale: guild.preferred_locale().to_owned(),
        premium_progress_bar_enabled: guild.premium_progress_bar_enabled(),
        premium_subscription_count: guild.premium_subscription_count(),
        premium_tier: guild.premium_tier(),
        presences: Vec::new(),
        public_updates_channel_id: guild.public_updates_channel_id(),
        roles: Vec::new(),
        rules_channel_id: guild.rules_channel_id(),
        safety_alerts_channel_id: guild.safety_alerts_channel_id(),
        splash: guild.splash().copied(),
        stage_instances: Vec::new(),
        stickers: Vec::new(),
        system_channel_flags: guild.system_channel_flags(),
        system_channel_id: guild.system_channel_id(),
        threads: Vec::new(),
        unavailable: guild.unavailable(),
        vanity_url_code: guild.vanity_url_code().map(str::to_owned),
        verification_level: guild.verification_level(),
        voice_states: Vec::new(),
        widget_channel_id: guild.widget_channel_id(),
        widget_enabled: guild.widget_enabled(),
    }
}

fn member_from_cached(member: &CachedMember, user: User) -> Member {
    Member {
        avatar: member.avatar(),
        avatar_decoration_data: None,
        banner: None,
        communication_disabled_until: member.communication_disabled_until(),
        deaf: member.deaf().unwrap_or(false),
        flags: member.flags(),
        joined_at: member.joined_at(),
        mute: member.mute().unwrap_or(false),
        nick: member.nick().map(str::to_owned),
        pending: member.pending(),
        premium_since: member.premium_since(),
        roles: member.roles().to_vec(),
        user,
    }
}

impl DiscordCache for MemoryDiscordCache {
    fn update<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            self.cache.update(event);
            Ok(())
        })
    }

    fn guild(&self, guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>> {
        Box::pin(async move {
            Ok(self
                .cache
                .guild(guild)
                .map(|cached| guild_from_cached(cached.value())))
        })
    }

    fn channel(
        &self,
        channel: Id<ChannelMarker>,
    ) -> BoxFuture<'_, Result<Option<Channel>, PortError>> {
        Box::pin(async move { Ok(self.cache.channel(channel).map(|c| c.value().clone())) })
    }

    fn role(&self, role: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>> {
        Box::pin(async move {
            Ok(self
                .cache
                .role(role)
                .map(|cached| cached.value().resource().clone()))
        })
    }

    fn member(
        &self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
    ) -> BoxFuture<'_, Result<Option<Member>, PortError>> {
        Box::pin(async move {
            let cached = self.cache.member(guild, user).map(|m| m.value().clone());
            let Some(cached) = cached else {
                return Ok(None);
            };
            let user = self.cache.user(user).map(|u| u.value().clone());
            Ok(user.map(|user| member_from_cached(&cached, user)))
        })
    }

    fn user(&self, user: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>> {
        Box::pin(async move { Ok(self.cache.user(user).map(|u| u.value().clone())) })
    }

    fn message(
        &self,
        message: Id<MessageMarker>,
    ) -> BoxFuture<'_, Result<Option<Message>, PortError>> {
        Box::pin(async move { Ok(self.cache.message(message).map(|m| m.value().0.clone())) })
    }

    fn channel_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>> {
        Box::pin(async move {
            Ok(self
                .cache
                .guild_channels(guild)
                .map(|ids| Self::sorted(ids.value())))
        })
    }

    fn role_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>> {
        Box::pin(async move {
            Ok(self
                .cache
                .guild_roles(guild)
                .map(|ids| Self::sorted(ids.value())))
        })
    }

    fn member_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>> {
        Box::pin(async move {
            Ok(self
                .cache
                .guild_members(guild)
                .map(|ids| Self::sorted(ids.value())))
        })
    }
}
