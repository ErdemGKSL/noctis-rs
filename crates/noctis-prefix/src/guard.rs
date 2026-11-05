//! Guard emulation for prefix runs (FR-14, AD-32): what Discord enforces
//! before an interaction reaches the bot, applied to a message.
//!
//! - `default_member_permissions` with Discord's permission algorithm
//!   (permissions.mdx, `compute_base_permissions` and `compute_overwrites`:
//!   the owner and ADMINISTRATOR have every permission), from the message's
//!   member roles, the guild's roles and the channel's overwrites (a thread
//!   uses its parent's); `"0"` (no bits) allows administrators only.
//! - The guild's application command permission overrides (Server Settings
//!   → Integrations), read with the bot token and cached per guild until a
//!   `APPLICATION_COMMAND_PERMISSIONS_UPDATE`.
//! - `nsfw`: only in an age-restricted channel (a thread follows its
//!   parent), never in a DM.
//!
//! The pure halves ([`channel_permissions`], [`overridden`], [`allowed`])
//! hold the rules; [`Guards`] reads what they need from the cache, or the
//! REST API when it is not cached.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use noctis_core::__rules::command::Scope;
use noctis_core::events::EventCtx;
use twilight_model::application::command::permissions::{
    CommandPermission, CommandPermissionType, GuildCommandPermissions,
};
use twilight_model::channel::Channel;
use twilight_model::channel::Message;
use twilight_model::channel::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ApplicationMarker, ChannelMarker, CommandMarker, GuildMarker, RoleMarker, UserMarker,
};

use crate::parse::Entry;

/// What a prefix run may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Run the Command.
    Run,
    /// Refuse it with the ephemeral permissions notice.
    Deny,
    /// Ignore the message, as Discord hides the Command there.
    Ignore,
}

/// The author's permissions in a channel: Discord's
/// `compute_base_permissions` then `compute_overwrites` (permissions.mdx).
/// `role` gives a role's permissions (`@everyone` is the role with the
/// guild's id); an unknown role grants nothing.
pub(crate) fn channel_permissions(
    guild: Id<GuildMarker>,
    owner: Id<UserMarker>,
    user: Id<UserMarker>,
    roles: &[Id<RoleMarker>],
    role: impl Fn(Id<RoleMarker>) -> Option<Permissions>,
    overwrites: &[PermissionOverwrite],
) -> Permissions {
    if user == owner {
        return Permissions::all();
    }
    let mut base = role(guild.cast()).unwrap_or_else(Permissions::empty);
    for id in roles {
        base |= role(*id).unwrap_or_else(Permissions::empty);
    }
    if base.contains(Permissions::ADMINISTRATOR) {
        return Permissions::all();
    }
    let mut permissions = base;
    let everyone = guild.cast();
    if let Some(o) = overwrites.iter().find(|o| o.id == everyone) {
        permissions &= !o.deny;
        permissions |= o.allow;
    }
    let (mut allow, mut deny) = (Permissions::empty(), Permissions::empty());
    for o in overwrites.iter().filter(|o| {
        o.kind == PermissionOverwriteType::Role && roles.iter().any(|r| r.cast() == o.id)
    }) {
        allow |= o.allow;
        deny |= o.deny;
    }
    permissions &= !deny;
    permissions |= allow;
    if let Some(o) = overwrites
        .iter()
        .find(|o| o.kind == PermissionOverwriteType::Member && o.id == user.cast())
    {
        permissions &= !o.deny;
        permissions |= o.allow;
    }
    permissions
}

/// The guild's override decision for one Command: `Some(false)` when an
/// override disables it for the author or the channel, `Some(true)` when
/// one enables it for the author, `None` when none applies (the Command's
/// `default_member_permissions` then decide).
///
/// Order: the channel first (the Command's entry for the channel, then for
/// "All Channels" (`guild_id - 1`), then the same two app-wide; a thread
/// counts as its parent too), then the author: the Command's user entry,
/// its role entries (any allow wins over a deny), its `@everyone` entry
/// (id = guild id), then the same three app-wide.
pub(crate) fn overridden(
    command: Option<&[CommandPermission]>,
    app: &[CommandPermission],
    guild: Id<GuildMarker>,
    channels: &[Id<ChannelMarker>],
    user: Id<UserMarker>,
    roles: &[Id<RoleMarker>],
) -> Option<bool> {
    let levels = [command.unwrap_or(&[]), app];
    let all_channels = Id::<ChannelMarker>::new(guild.get().saturating_sub(1).max(1));
    for level in levels {
        let channel = |id: Id<ChannelMarker>| {
            level
                .iter()
                .find(|p| p.id == CommandPermissionType::Channel(id))
                .map(|p| p.permission)
        };
        if let Some(allowed) = channels
            .iter()
            .find_map(|id| channel(*id))
            .or_else(|| channel(all_channels))
        {
            if !allowed {
                return Some(false);
            }
            break;
        }
    }
    for level in levels {
        if let Some(p) = level
            .iter()
            .find(|p| p.id == CommandPermissionType::User(user))
        {
            return Some(p.permission);
        }
        let mut role_entries = level
            .iter()
            .filter(|p| matches!(p.id, CommandPermissionType::Role(id) if roles.contains(&id)));
        let mut any = false;
        let mut allowed = false;
        for p in role_entries.by_ref() {
            any = true;
            allowed |= p.permission;
        }
        if any {
            return Some(allowed);
        }
        if let Some(p) = level
            .iter()
            .find(|p| p.id == CommandPermissionType::Role(guild.cast()))
        {
            return Some(p.permission);
        }
    }
    None
}

/// Whether the author may run the Command: the owner and administrators
/// always, else a disabling override refuses and an enabling one allows,
/// else `required` (`default_member_permissions`; no bits: administrators
/// only) against `permissions`.
pub(crate) fn allowed(
    permissions: Permissions,
    required: Option<Permissions>,
    overridden: Option<bool>,
) -> bool {
    if permissions.contains(Permissions::ADMINISTRATOR) {
        return true;
    }
    match (overridden, required) {
        (Some(decided), _) => decided,
        (None, None) => true,
        (None, Some(required)) if required.is_empty() => false,
        (None, Some(required)) => permissions.contains(required),
    }
}

/// One guild's command permission overrides.
#[derive(Debug, Default)]
struct GuildOverrides {
    /// The app-wide entries (id = the application id).
    app: Vec<CommandPermission>,
    /// The entries of each Command, by Command id.
    commands: HashMap<Id<CommandMarker>, Vec<CommandPermission>>,
    /// The guild's own Commands, by name.
    guild_ids: HashMap<String, Id<CommandMarker>>,
}

impl GuildOverrides {
    fn new(
        application: Id<ApplicationMarker>,
        list: Vec<GuildCommandPermissions>,
        guild_ids: HashMap<String, Id<CommandMarker>>,
    ) -> Self {
        let mut out = Self {
            guild_ids,
            ..Self::default()
        };
        for entry in list {
            if entry.id.cast() == application {
                out.app = entry.permissions;
            } else {
                out.commands.insert(entry.id, entry.permissions);
            }
        }
        out
    }
}

/// Command ids by top-level name.
type CommandIds = Arc<HashMap<String, Id<CommandMarker>>>;

/// The plugin's guard state: the application id, the global Command ids
/// and each guild's overrides, read once and kept until an update.
#[derive(Debug, Default)]
pub(crate) struct Guards {
    application: OnceLock<Id<ApplicationMarker>>,
    global_ids: Mutex<Option<CommandIds>>,
    overrides: Mutex<HashMap<Id<GuildMarker>, Arc<GuildOverrides>>>, // keyed-map: one entry per guild the bot is in, removed on update
}

impl Guards {
    /// Forgets the overrides of `guild` (an
    /// `APPLICATION_COMMAND_PERMISSIONS_UPDATE`) and the global Command
    /// ids, so the next run reads them again.
    pub(crate) fn invalidate(&self, guild: Id<GuildMarker>) {
        self.overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&guild);
        *self
            .global_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// What the prefix run of `entry` from `message` may do.
    pub(crate) async fn check(&self, ctx: &EventCtx, entry: &Entry, message: &Message) -> Verdict {
        let nsfw = entry.metadata().nsfw == Some(true);
        let Some(guild) = message.guild_id else {
            // Discord never shows an age-restricted Command in a DM unless
            // the user opted in, which a bot cannot see.
            return if nsfw { Verdict::Ignore } else { Verdict::Run };
        };
        let required = entry.metadata().default_member_permissions;
        let overrides = self.overrides(ctx, guild).await;
        let command = overrides.as_deref().and_then(|o| {
            let base = entry.path().split(' ').next().unwrap_or_default();
            let id = match entry.scope() {
                Scope::Guild(_) => o.guild_ids.get(base).copied(),
                Scope::Global => self.global_id(base),
            }?;
            o.commands.get(&id).map(Vec::as_slice)
        });
        let has_overrides = overrides
            .as_deref()
            .is_some_and(|o| !o.app.is_empty() || command.is_some());
        if !nsfw && required.is_none() && !has_overrides {
            return Verdict::Run;
        }
        let Some(channel) = channel(ctx, message.channel_id).await else {
            tracing::warn!(
                path = entry.path(),
                "prefix command refused: its channel could not be read"
            );
            return Verdict::Ignore;
        };
        let parent = if channel.kind.is_thread() {
            match channel.parent_id {
                Some(id) => self::channel(ctx, id).await,
                None => None,
            }
        } else {
            None
        };
        let place = parent.as_ref().unwrap_or(&channel);
        if nsfw && place.nsfw != Some(true) {
            return Verdict::Ignore;
        }
        let roles: &[Id<RoleMarker>] = message.member.as_ref().map_or(&[], |m| &m.roles);
        let user = message.author.id;
        let channels: Vec<Id<ChannelMarker>> = std::iter::once(channel.id)
            .chain(parent.as_ref().map(|p| p.id))
            .collect();
        let decided = overrides
            .as_deref()
            .and_then(|o| overridden(command, &o.app, guild, &channels, user, roles));
        if required.is_none() && decided != Some(false) {
            return Verdict::Run;
        }
        let Some(facts) = guild_facts(ctx, guild, roles).await else {
            tracing::warn!(
                path = entry.path(),
                "prefix command refused: the guild's roles could not be read"
            );
            return Verdict::Deny;
        };
        let overwrites = place.permission_overwrites.as_deref().unwrap_or(&[]);
        let permissions = channel_permissions(
            guild,
            facts.owner,
            user,
            roles,
            |id| facts.roles.get(&id).copied(),
            overwrites,
        );
        if allowed(permissions, required, decided) {
            Verdict::Run
        } else {
            Verdict::Deny
        }
    }

    /// The global Command id of `name`, when the ids were read.
    fn global_id(&self, name: &str) -> Option<Id<CommandMarker>> {
        self.global_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()?
            .get(name)
            .copied()
    }

    /// The overrides of `guild`, read once; `None` (and a `warn`) when they
    /// could not be read, so only `default_member_permissions` decide.
    async fn overrides(
        &self,
        ctx: &EventCtx,
        guild: Id<GuildMarker>,
    ) -> Option<Arc<GuildOverrides>> {
        if let Some(cached) = self
            .overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&guild)
        {
            return Some(Arc::clone(cached));
        }
        let read = self.read_overrides(ctx, guild).await;
        let Some(read) = read else {
            tracing::warn!(
                guild_id = guild.get(),
                "the guild's command permission overrides could not be read: \
                 only default_member_permissions apply"
            );
            return None;
        };
        let read = Arc::new(read);
        self.overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(guild, Arc::clone(&read));
        Some(read)
    }

    async fn read_overrides(
        &self,
        ctx: &EventCtx,
        guild: Id<GuildMarker>,
    ) -> Option<GuildOverrides> {
        let application = self.application(ctx).await?;
        let http = ctx.http().interaction(application);
        let list = http
            .guild_command_permissions(guild)
            .await
            .ok()?
            .model()
            .await
            .ok()?;
        let mut guild_ids = HashMap::new();
        if list.iter().any(|entry| entry.id.cast() != application) {
            let need_global = self
                .global_ids
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_none();
            if need_global {
                let body = http.global_commands().await.ok()?.bytes().await.ok()?;
                let ids = command_ids(&body)?;
                *self
                    .global_ids
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(ids));
            }
            let body = http.guild_commands(guild).await.ok()?.bytes().await.ok()?;
            guild_ids = command_ids(&body)?;
        }
        Some(GuildOverrides::new(application, list, guild_ids))
    }

    async fn application(&self, ctx: &EventCtx) -> Option<Id<ApplicationMarker>> {
        if let Some(id) = self.application.get() {
            return Some(*id);
        }
        let body = ctx
            .http()
            .current_user_application()
            .await
            .ok()?
            .bytes()
            .await
            .ok()?;
        let value: serde_json::Value = serde_json::from_slice(&body).ok()?;
        let id = Id::new_checked(value.get("id")?.as_str()?.parse().ok()?)?;
        Some(*self.application.get_or_init(|| id))
    }
}

/// The ids of a Get Commands answer, by name (read leniently: only `id`
/// and `name` matter here).
fn command_ids(body: &[u8]) -> Option<HashMap<String, Id<CommandMarker>>> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|command| {
                let name = command.get("name")?.as_str()?.to_owned();
                let id = command.get("id")?.as_str()?.parse().ok()?;
                Some((name, Id::new_checked(id)?))
            })
            .collect(),
    )
}

/// What the permission algorithm needs from the guild.
struct GuildFacts {
    owner: Id<UserMarker>,
    roles: HashMap<Id<RoleMarker>, Permissions>,
}

/// The channel `id`, from the cache or the REST API.
async fn channel(ctx: &EventCtx, id: Id<ChannelMarker>) -> Option<Channel> {
    if let Ok(Some(channel)) = ctx.cache().channel(id).await {
        return Some(channel);
    }
    ctx.http().channel(id).await.ok()?.model().await.ok()
}

/// The guild's owner and the permissions of `@everyone` and `roles`, from
/// the cache when it holds all of them, else from the REST API.
async fn guild_facts(
    ctx: &EventCtx,
    guild: Id<GuildMarker>,
    roles: &[Id<RoleMarker>],
) -> Option<GuildFacts> {
    let cache = ctx.cache();
    if let Ok(Some(cached)) = cache.guild(guild).await {
        let mut facts = GuildFacts {
            owner: cached.owner_id,
            roles: HashMap::new(),
        };
        let mut complete = true;
        for id in std::iter::once(guild.cast()).chain(roles.iter().copied()) {
            let Ok(Some(role)) = cache.role(id).await else {
                complete = false;
                break;
            };
            facts.roles.insert(id, role.permissions);
        }
        if complete {
            return Some(facts);
        }
    }
    let fetched = ctx.http().guild(guild).await.ok()?.model().await.ok()?;
    Some(GuildFacts {
        owner: fetched.owner_id,
        roles: fetched
            .roles
            .into_iter()
            .map(|role| (role.id, role.permissions))
            .collect(),
    })
}

#[cfg(test)]
mod tests;
