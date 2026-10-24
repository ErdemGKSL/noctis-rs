//! The Discord resource cache: its configuration ([`DiscordCacheConfig`]),
//! the handler-facing reader behind `ctx.cache()` ([`DiscordCacheReader`])
//! and the crate-private feed that keeps the `DiscordCache` port current
//! (FR-40, FR-38, FR-6; AD-1, AD-14, AD-21, AD-25; GW-05).
//!
//! Core feeds the backend from the drain loop (each cached event type,
//! before its handlers run) and from each interaction task (before its
//! pipeline starts). A failed or slow update never stops the event or the
//! interaction: it is counted in `noctis_discord_cache_update_errors_total`
//! and logged once per outage.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use noctis_gateway::EventTypeFlags;
use noctis_ports::memory::{DiscordCacheResources, MemoryDiscordCache};
use noctis_ports::{DiscordCache, PortError};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::{Channel, Message};
use twilight_model::gateway::Intents;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::InteractionCreate;
use twilight_model::guild::{Guild, Member, Role};
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use twilight_model::user::User;

use crate::handler::AppShared;

/// How long one cache update may take before it counts as failed and the
/// event or interaction moves on.
pub(crate) const CACHE_UPDATE_TIMEOUT: Duration = Duration::from_secs(1);

/// Which Discord resources the App caches, and how many messages per
/// channel: `AppBuilder::discord_cache(..)`.
///
/// The [`Default`] caches [`DEFAULT_RESOURCES`](Self::DEFAULT_RESOURCES)
/// (`GUILD | CHANNEL | ROLE`): what the always-requested `GUILDS` intent
/// delivers, with no members and no messages. [`none`](Self::none) turns the
/// cache off: the backend is never fed nor read, and every `ctx.cache()`
/// getter returns [`CacheError::Disabled`].
///
/// Each resource adds the gateway event types that keep it current and the
/// non-privileged intents those need; `MEMBER` needs the privileged
/// `GUILD_MEMBERS` intent, which `.intents(..)` must request (GW-05).
///
/// ```
/// use noctis_core::{DiscordCacheConfig, DiscordCacheResources};
///
/// let config = DiscordCacheConfig::default()
///     .with_resources(DiscordCacheResources::USER)
///     .without_resources(DiscordCacheResources::ROLE);
/// assert_eq!(
///     config.resources(),
///     DiscordCacheResources::GUILD | DiscordCacheResources::CHANNEL | DiscordCacheResources::USER
/// );
/// assert!(!DiscordCacheConfig::none().is_enabled());
/// ```
#[derive(Clone, Copy, Debug)]
pub struct DiscordCacheConfig {
    resources: DiscordCacheResources,
    message_cap: usize,
}

impl DiscordCacheConfig {
    /// The default resources: `GUILD | CHANNEL | ROLE`.
    pub const DEFAULT_RESOURCES: DiscordCacheResources = DiscordCacheResources::GUILD
        .union(DiscordCacheResources::CHANNEL)
        .union(DiscordCacheResources::ROLE);

    /// A configuration caching `resources`, at most
    /// [`MemoryDiscordCache::DEFAULT_MESSAGE_CAP`] messages per channel.
    pub const fn new(resources: DiscordCacheResources) -> Self {
        Self {
            resources,
            message_cap: MemoryDiscordCache::DEFAULT_MESSAGE_CAP,
        }
    }

    /// The cache turned off: nothing is fed or read, and no event type or
    /// intent is added for it.
    pub const fn none() -> Self {
        Self::new(DiscordCacheResources::empty())
    }

    /// The same configuration with `resources` added.
    #[must_use]
    pub const fn with_resources(self, resources: DiscordCacheResources) -> Self {
        Self {
            resources: self.resources.union(resources),
            ..self
        }
    }

    /// The same configuration with `resources` removed.
    #[must_use]
    pub const fn without_resources(self, resources: DiscordCacheResources) -> Self {
        Self {
            resources: self.resources.without(resources),
            ..self
        }
    }

    /// The same configuration keeping at most `cap` messages per channel; a
    /// `cap` of 0 removes `MESSAGE` from [`resources`](Self::resources).
    #[must_use]
    pub const fn with_message_cap(self, cap: usize) -> Self {
        Self {
            message_cap: cap,
            ..self
        }
    }

    /// The cached resources: the configured set, without `MESSAGE` when the
    /// message cap is 0.
    pub const fn resources(&self) -> DiscordCacheResources {
        if self.message_cap == 0 {
            self.resources.without(DiscordCacheResources::MESSAGE)
        } else {
            self.resources
        }
    }

    /// The per-channel message cap.
    pub const fn message_cap(&self) -> usize {
        self.message_cap
    }

    /// Whether any resource is cached.
    pub const fn is_enabled(&self) -> bool {
        !self.resources().is_empty()
    }
}

impl Default for DiscordCacheConfig {
    fn default() -> Self {
        Self::new(Self::DEFAULT_RESOURCES)
    }
}

impl PartialEq for DiscordCacheConfig {
    fn eq(&self, other: &Self) -> bool {
        (self.resources(), self.message_cap()) == (other.resources(), other.message_cap())
    }
}

impl Eq for DiscordCacheConfig {}

/// One row of the resource table: the resource, the event types that feed
/// it, the non-privileged intents it adds and the privileged intents it
/// requires.
struct Row {
    resource: DiscordCacheResources,
    events: EventTypeFlags,
    intents: Intents,
    privileged: Intents,
}

/// The guild lifecycle every guild-scoped resource listens to.
const GUILD_LIFECYCLE: EventTypeFlags = EventTypeFlags::GUILD_CREATE
    .union(EventTypeFlags::GUILD_DELETE)
    .union(EventTypeFlags::UNAVAILABLE_GUILD);

/// The resource table (one row per resource).
const ROWS: [Row; 6] = [
    Row {
        resource: DiscordCacheResources::GUILD,
        events: GUILD_LIFECYCLE.union(EventTypeFlags::GUILD_UPDATE),
        intents: Intents::GUILDS,
        privileged: Intents::empty(),
    },
    Row {
        resource: DiscordCacheResources::CHANNEL,
        events: GUILD_LIFECYCLE
            .union(EventTypeFlags::CHANNEL_CREATE)
            .union(EventTypeFlags::CHANNEL_UPDATE)
            .union(EventTypeFlags::CHANNEL_DELETE)
            .union(EventTypeFlags::CHANNEL_PINS_UPDATE)
            .union(EventTypeFlags::THREAD_CREATE)
            .union(EventTypeFlags::THREAD_UPDATE)
            .union(EventTypeFlags::THREAD_DELETE)
            .union(EventTypeFlags::THREAD_LIST_SYNC),
        intents: Intents::GUILDS,
        privileged: Intents::empty(),
    },
    Row {
        resource: DiscordCacheResources::ROLE,
        events: GUILD_LIFECYCLE
            .union(EventTypeFlags::ROLE_CREATE)
            .union(EventTypeFlags::ROLE_UPDATE)
            .union(EventTypeFlags::ROLE_DELETE),
        intents: Intents::GUILDS,
        privileged: Intents::empty(),
    },
    Row {
        resource: DiscordCacheResources::MEMBER,
        events: GUILD_LIFECYCLE
            .union(EventTypeFlags::MEMBER_ADD)
            .union(EventTypeFlags::MEMBER_UPDATE)
            .union(EventTypeFlags::MEMBER_REMOVE)
            .union(EventTypeFlags::MEMBER_CHUNK),
        intents: Intents::empty(),
        privileged: Intents::GUILD_MEMBERS,
    },
    Row {
        resource: DiscordCacheResources::USER,
        events: EventTypeFlags::GUILD_CREATE,
        intents: Intents::GUILDS,
        privileged: Intents::empty(),
    },
    Row {
        resource: DiscordCacheResources::MESSAGE,
        events: EventTypeFlags::MESSAGE_CREATE
            .union(EventTypeFlags::MESSAGE_UPDATE)
            .union(EventTypeFlags::MESSAGE_DELETE)
            .union(EventTypeFlags::MESSAGE_DELETE_BULK),
        intents: Intents::GUILD_MESSAGES.union(Intents::DIRECT_MESSAGES),
        privileged: Intents::empty(),
    },
];

/// The privileged intents a resource can require, one entry per intent.
const PRIVILEGED: [Intents; 1] = [Intents::GUILD_MEMBERS];

/// The event types that feed `resources`.
pub(crate) const fn event_types(resources: DiscordCacheResources) -> EventTypeFlags {
    let mut flags = EventTypeFlags::empty();
    let mut i = 0;
    while i < ROWS.len() {
        if resources.contains(ROWS[i].resource) {
            flags = flags.union(ROWS[i].events);
        }
        i += 1;
    }
    flags
}

/// The non-privileged intents `resources` add.
pub(crate) const fn intents(resources: DiscordCacheResources) -> Intents {
    let mut intents = Intents::empty();
    let mut i = 0;
    while i < ROWS.len() {
        if resources.contains(ROWS[i].resource) {
            intents = intents.union(ROWS[i].intents);
        }
        i += 1;
    }
    intents
}

/// The privileged intents `resources` require.
pub(crate) const fn privileged(resources: DiscordCacheResources) -> Intents {
    let mut intents = Intents::empty();
    let mut i = 0;
    while i < ROWS.len() {
        if resources.contains(ROWS[i].resource) {
            intents = intents.union(ROWS[i].privileged);
        }
        i += 1;
    }
    intents
}

/// The resources of `resources` that require the privileged `intent`.
const fn requiring(resources: DiscordCacheResources, intent: Intents) -> DiscordCacheResources {
    let mut out = DiscordCacheResources::empty();
    let mut i = 0;
    while i < ROWS.len() {
        if resources.contains(ROWS[i].resource) && ROWS[i].privileged.contains(intent) {
            out = out.union(ROWS[i].resource);
        }
        i += 1;
    }
    out
}

/// Whether interactions feed `resources` (what twilight's
/// `InteractionCreate` update reads: members, users and resolved roles).
pub(crate) const fn feeds_interactions(resources: DiscordCacheResources) -> bool {
    const READ: DiscordCacheResources = DiscordCacheResources::MEMBER
        .union(DiscordCacheResources::USER)
        .union(DiscordCacheResources::ROLE);
    // `resources` minus its part outside READ: its intersection with READ.
    !resources.without(resources.without(READ)).is_empty()
}

/// Every `(resources, intent)` pair of a privileged intent `config` needs
/// that `requested` lacks, in [`PRIVILEGED`] order (GW-05).
pub(crate) fn missing_intents(
    config: DiscordCacheConfig,
    requested: Intents,
) -> Vec<(DiscordCacheResources, Intents)> {
    let resources = config.resources();
    PRIVILEGED
        .iter()
        .filter(|intent| privileged(resources).contains(**intent) && !requested.contains(**intent))
        .map(|intent| (requiring(resources, *intent), *intent))
        .collect()
}

/// A `ctx.cache()` getter's resource is not configured: the App runs with
/// [`DiscordCacheConfig::none`] or without that resource.
///
/// ```
/// use noctis_core::{CacheDisabled, DiscordCacheResources};
///
/// fn explain(error: &CacheDisabled) -> String {
///     format!("{error} ({})", error.resource)
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error(
    "Discord cache resource {resource} is not enabled: add it with \
     AppBuilder::discord_cache(DiscordCacheConfig::default().with_resources(..))"
)]
pub struct CacheDisabled {
    /// The resource the getter reads.
    pub resource: DiscordCacheResources,
}

/// The error of a `ctx.cache()` getter. `?` turns it into a
/// [`HandlerError`](crate::HandlerError).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// The getter's resource is not configured (no backend call was made).
    #[error(transparent)]
    Disabled(#[from] CacheDisabled),
    /// The backend failed.
    #[error("Discord cache backend failed")]
    Port(
        #[source]
        #[from]
        PortError,
    ),
}

/// Read access to the App's Discord resource cache, from `ctx.cache()`.
///
/// Every getter returns an owned `twilight_model` value (never a guard or
/// a reference, so nothing is held across an `.await`): `Ok(None)` when the
/// value is not cached, [`CacheError::Disabled`] when its resource is not
/// configured (checked before any backend call), and [`CacheError::Port`]
/// when the backend fails. Each getter makes one backend call; the list
/// getters ([`channels`](Self::channels), [`roles`](Self::roles),
/// [`members`](Self::members)) read the ids and then each value.
///
/// Cheap to clone. `Debug` prints only the cached resources.
///
/// ```
/// use noctis_core::{CommandCtx, HandlerResult};
///
/// async fn guild_name(ctx: CommandCtx) -> HandlerResult<Option<String>> {
///     let Some(guild_id) = ctx.guild_id() else { return Ok(None) };
///     let guild = ctx.cache().guild(guild_id).await?;
///     Ok(guild.map(|guild| guild.name))
/// }
/// ```
#[derive(Clone)]
pub struct DiscordCacheReader {
    /// The App's backend; `None` without an App or with the cache off.
    backend: Option<Arc<dyn DiscordCache>>,
    resources: DiscordCacheResources,
}

impl DiscordCacheReader {
    /// A reader without a backend: every getter is
    /// [`CacheError::Disabled`].
    pub(crate) const fn detached() -> Self {
        Self {
            backend: None,
            resources: DiscordCacheResources::empty(),
        }
    }

    /// The reader of the App behind `app`, or a detached one.
    pub(crate) fn of(app: Option<&Arc<AppShared>>) -> Self {
        app.map_or_else(Self::detached, |app| app.discord_cache.reader())
    }

    /// The cached resources (empty when the cache is off).
    pub const fn resources(&self) -> DiscordCacheResources {
        self.resources
    }

    /// Whether any resource is cached.
    pub const fn is_enabled(&self) -> bool {
        !self.resources.is_empty()
    }

    /// The backend, when `resource` is cached.
    fn backend_for(
        &self,
        resource: DiscordCacheResources,
    ) -> Result<&Arc<dyn DiscordCache>, CacheDisabled> {
        self.backend
            .as_ref()
            .filter(|_| self.resources.contains(resource))
            .ok_or(CacheDisabled { resource })
    }

    /// The cached guild (its channel, role and member lists are read with
    /// [`channel_ids`](Self::channel_ids), [`role_ids`](Self::role_ids) and
    /// [`member_ids`](Self::member_ids)). Resource `GUILD`.
    ///
    /// # Errors
    ///
    /// [`CacheError::Disabled`] when `GUILD` is not cached, else the
    /// backend's error as [`CacheError::Port`].
    pub async fn guild(&self, guild: Id<GuildMarker>) -> Result<Option<Guild>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::GUILD)?;
        Ok(backend.guild(guild).await?)
    }

    /// The cached channel or thread. Resource `CHANNEL`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `CHANNEL`.
    pub async fn channel(&self, channel: Id<ChannelMarker>) -> Result<Option<Channel>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::CHANNEL)?;
        Ok(backend.channel(channel).await?)
    }

    /// The cached role. Resource `ROLE`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `ROLE`.
    pub async fn role(&self, role: Id<RoleMarker>) -> Result<Option<Role>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::ROLE)?;
        Ok(backend.role(role).await?)
    }

    /// The cached member `user` of `guild`. Resource `MEMBER` (which needs
    /// the privileged `GUILD_MEMBERS` intent).
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `MEMBER`.
    pub async fn member(
        &self,
        guild: Id<GuildMarker>,
        user: Id<UserMarker>,
    ) -> Result<Option<Member>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::MEMBER)?;
        Ok(backend.member(guild, user).await?)
    }

    /// The cached user. Resource `USER`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `USER`.
    pub async fn user(&self, user: Id<UserMarker>) -> Result<Option<User>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::USER)?;
        Ok(backend.user(user).await?)
    }

    /// The cached message. Resource `MESSAGE`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `MESSAGE`.
    pub async fn message(&self, message: Id<MessageMarker>) -> Result<Option<Message>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::MESSAGE)?;
        Ok(backend.message(message).await?)
    }

    /// The ids of the guild's cached channels; `None` when the guild is not
    /// cached. Resource `CHANNEL`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `CHANNEL`.
    pub async fn channel_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> Result<Option<Vec<Id<ChannelMarker>>>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::CHANNEL)?;
        Ok(backend.channel_ids(guild).await?)
    }

    /// The ids of the guild's cached roles; `None` when the guild is not
    /// cached. Resource `ROLE`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `ROLE`.
    pub async fn role_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> Result<Option<Vec<Id<RoleMarker>>>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::ROLE)?;
        Ok(backend.role_ids(guild).await?)
    }

    /// The user ids of the guild's cached members; `None` when the guild is
    /// not cached. Resource `MEMBER`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `MEMBER`.
    pub async fn member_ids(
        &self,
        guild: Id<GuildMarker>,
    ) -> Result<Option<Vec<Id<UserMarker>>>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::MEMBER)?;
        Ok(backend.member_ids(guild).await?)
    }

    /// The guild's cached channels in id order (a channel listed but no
    /// longer cached is skipped); `None` when the guild is not cached.
    /// Resource `CHANNEL`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `CHANNEL`; the first backend error.
    pub async fn channels(
        &self,
        guild: Id<GuildMarker>,
    ) -> Result<Option<Vec<Channel>>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::CHANNEL)?;
        let Some(mut ids) = backend.channel_ids(guild).await? else {
            return Ok(None);
        };
        ids.sort_unstable();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.extend(backend.channel(id).await?);
        }
        Ok(Some(out))
    }

    /// The guild's cached roles in id order (a role listed but no longer
    /// cached is skipped); `None` when the guild is not cached. Resource
    /// `ROLE`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `ROLE`; the first backend error.
    pub async fn roles(&self, guild: Id<GuildMarker>) -> Result<Option<Vec<Role>>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::ROLE)?;
        let Some(mut ids) = backend.role_ids(guild).await? else {
            return Ok(None);
        };
        ids.sort_unstable();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.extend(backend.role(id).await?);
        }
        Ok(Some(out))
    }

    /// The guild's cached members in user-id order (a member listed but no
    /// longer cached is skipped); `None` when the guild is not cached.
    /// Resource `MEMBER`.
    ///
    /// # Errors
    ///
    /// As [`guild`](Self::guild), for `MEMBER`; the first backend error.
    pub async fn members(&self, guild: Id<GuildMarker>) -> Result<Option<Vec<Member>>, CacheError> {
        let backend = self.backend_for(DiscordCacheResources::MEMBER)?;
        let Some(mut ids) = backend.member_ids(guild).await? else {
            return Ok(None);
        };
        ids.sort_unstable();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.extend(backend.member(guild, id).await?);
        }
        Ok(Some(out))
    }
}

impl fmt::Debug for DiscordCacheReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscordCacheReader")
            .field("resources", &self.resources)
            .finish_non_exhaustive()
    }
}

/// The App's cache feed: its backend, its configuration and whether the
/// last update succeeded.
pub(crate) struct CacheFeed {
    backend: Arc<dyn DiscordCache>,
    config: DiscordCacheConfig,
    /// The event types that feed the configured resources.
    events: EventTypeFlags,
    /// Whether interactions feed the configured resources.
    interactions: bool,
    /// `false` after a failed update, until one succeeds.
    healthy: AtomicBool,
}

impl CacheFeed {
    /// The feed of `backend` under `config`.
    pub(crate) fn new(backend: Arc<dyn DiscordCache>, config: DiscordCacheConfig) -> Self {
        let resources = config.resources();
        Self {
            backend,
            config,
            events: event_types(resources),
            interactions: feeds_interactions(resources),
            healthy: AtomicBool::new(true),
        }
    }

    /// The configuration.
    pub(crate) fn config(&self) -> DiscordCacheConfig {
        self.config
    }

    /// A reader over the backend, limited to the configured resources.
    pub(crate) fn reader(&self) -> DiscordCacheReader {
        let resources = self.config.resources();
        if resources.is_empty() {
            return DiscordCacheReader::detached();
        }
        DiscordCacheReader {
            backend: Some(Arc::clone(&self.backend)),
            resources,
        }
    }

    /// Whether `event` feeds the configured resources.
    pub(crate) fn wants(&self, event: &Event) -> bool {
        let kind = EventTypeFlags::from(event.kind());
        !kind.is_empty() && self.events.contains(kind)
    }

    /// Applies `event` when its type feeds the configured resources;
    /// otherwise does nothing.
    pub(crate) async fn event(&self, app: &str, event: &Event) {
        if self.wants(event) {
            self.apply(app, event).await;
        }
    }

    /// Applies `interaction` (as an `INTERACTION_CREATE`) when interactions
    /// feed the configured resources; otherwise does nothing.
    pub(crate) async fn interaction(&self, app: &str, interaction: &Interaction) {
        if self.interactions {
            let event = Event::InteractionCreate(Box::new(InteractionCreate(interaction.clone())));
            self.apply(app, &event).await;
        }
    }

    /// One bounded update; a failure is counted and logged on the first
    /// failure of an outage, a success after one logs the recovery.
    async fn apply(&self, app: &str, event: &Event) {
        let result =
            match tokio::time::timeout(CACHE_UPDATE_TIMEOUT, self.backend.update(event)).await {
                Ok(result) => result,
                Err(_) => Err(PortError::unavailable(format!(
                    "Discord cache update timed out after {CACHE_UPDATE_TIMEOUT:?}"
                ))),
            };
        match result {
            Ok(()) => {
                if !self.healthy.swap(true, Ordering::AcqRel) {
                    tracing::info!(target: "noctis::cache", app, "Discord cache updates recovered");
                }
            }
            Err(error) => {
                metrics::counter!(
                    "noctis_discord_cache_update_errors_total",
                    "app" => app.to_owned()
                )
                .increment(1);
                if self.healthy.swap(false, Ordering::AcqRel) {
                    tracing::warn!(
                        target: "noctis::cache",
                        app,
                        error = %crate::app::error_chain(&error),
                        "Discord cache updates are failing; events still reach their handlers"
                    );
                }
            }
        }
    }
}

impl fmt::Debug for CacheFeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheFeed")
            .field("config", &self.config)
            .field("healthy", &self.healthy.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
