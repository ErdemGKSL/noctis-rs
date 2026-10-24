//! Seeds: the guilds, channels, roles and users a [`TestApp`](crate::TestApp)
//! knows, and [`TestOption`] values.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use twilight_model::channel::ChannelType;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::{Guild, Permissions};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};

/// A seeded guild: its name, channels, roles and Guild Locale.
///
/// A later channel or role with the same id replaces the earlier one.
///
/// ```
/// use noctis_testkit::TestGuild;
/// use twilight_model::id::Id;
///
/// let guild = TestGuild::new(Id::new(10), "Guild")
///     .text_channel(Id::new(20), "logs")
///     .role(Id::new(30), "Mods")
///     .locale("tr");
/// ```
#[derive(Debug, Clone)]
pub struct TestGuild {
    pub(crate) id: Id<GuildMarker>,
    pub(crate) name: String,
    pub(crate) channels: Vec<(Id<ChannelMarker>, String, ChannelType)>,
    pub(crate) roles: Vec<(Id<RoleMarker>, String)>,
    pub(crate) locale: String,
}

impl TestGuild {
    /// A guild without channels or roles, Guild Locale `en-US`.
    pub fn new(id: Id<GuildMarker>, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            channels: Vec::new(),
            roles: Vec::new(),
            locale: "en-US".to_owned(),
        }
    }

    /// Adds a text channel (type 0).
    #[must_use]
    pub fn text_channel(self, id: Id<ChannelMarker>, name: impl Into<String>) -> Self {
        self.channel(id, name, ChannelType::GuildText)
    }

    /// Adds a channel of `kind`.
    #[must_use]
    pub fn channel(
        mut self,
        id: Id<ChannelMarker>,
        name: impl Into<String>,
        kind: ChannelType,
    ) -> Self {
        self.channels.retain(|(c, _, _)| *c != id);
        self.channels.push((id, name.into(), kind));
        self
    }

    /// Adds a role.
    #[must_use]
    pub fn role(mut self, id: Id<RoleMarker>, name: impl Into<String>) -> Self {
        self.roles.retain(|(r, _)| *r != id);
        self.roles.push((id, name.into()));
        self
    }

    /// Sets the Guild Locale (default `en-US`), sent as `guild_locale`.
    #[must_use]
    pub fn locale(mut self, locale: &str) -> Self {
        locale.clone_into(&mut self.locale);
        self
    }

    /// The seed as an available `GUILD_CREATE`: id, name, Guild Locale,
    /// channels (position 0) and roles (position = seed index, no
    /// permissions), owner 1, every other required field at Discord's zero
    /// value.
    pub(crate) fn create_event(&self) -> Result<Event, serde_json::Error> {
        let guild_id = self.id.to_string();
        let channels: Vec<Value> = self
            .channels
            .iter()
            .map(|(id, name, kind)| {
                json!({
                    "id": id.to_string(),
                    "guild_id": guild_id,
                    "type": u8::from(*kind),
                    "name": name,
                    "position": 0,
                    "permission_overwrites": [],
                })
            })
            .collect();
        let roles: Vec<Value> = self
            .roles
            .iter()
            .enumerate()
            .map(|(position, (id, name))| {
                json!({
                    "id": id.to_string(),
                    "name": name,
                    "color": 0,
                    "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null},
                    "hoist": false,
                    "managed": false,
                    "mentionable": false,
                    "permissions": "0",
                    "position": position,
                    "flags": 0,
                })
            })
            .collect();
        let guild: Guild = serde_json::from_value(json!({
            "id": guild_id,
            "name": self.name,
            "owner_id": "1",
            "preferred_locale": self.locale,
            "afk_timeout": 300,
            "default_message_notifications": 0,
            "explicit_content_filter": 0,
            "features": [],
            "mfa_level": 0,
            "nsfw_level": 0,
            "premium_progress_bar_enabled": false,
            "premium_tier": 0,
            "system_channel_flags": 0,
            "verification_level": 0,
            "emojis": [],
            "stickers": [],
            "channels": channels,
            "roles": roles,
            "threads": [],
            "members": [],
            "voice_states": [],
            "presences": [],
            "stage_instances": [],
            "guild_scheduled_events": [],
        }))?;
        Ok(Event::GuildCreate(Box::new(GuildCreate::Available(guild))))
    }

    pub(crate) fn find_channel(&self, id: Id<ChannelMarker>) -> Option<(&str, ChannelType)> {
        self.channels
            .iter()
            .find(|(c, _, _)| *c == id)
            .map(|(_, name, kind)| (name.as_str(), *kind))
    }

    pub(crate) fn find_role(&self, id: Id<RoleMarker>) -> Option<&str> {
        self.roles
            .iter()
            .find(|(r, _)| *r == id)
            .map(|(_, name)| name.as_str())
    }
}

/// A seeded user: name, guild permissions and roles (used in every seeded
/// guild).
///
/// ```
/// use noctis_testkit::TestUser;
/// use twilight_model::guild::Permissions;
/// use twilight_model::id::Id;
///
/// let admin = TestUser::new(Id::new(40)).name("admin").permissions(Permissions::MANAGE_GUILD);
/// ```
#[derive(Debug, Clone)]
pub struct TestUser {
    pub(crate) id: Id<UserMarker>,
    pub(crate) name: Option<String>,
    pub(crate) permissions: Permissions,
    pub(crate) roles: Vec<Id<RoleMarker>>,
}

impl TestUser {
    /// A user named `user-<id>` without permissions or roles.
    pub fn new(id: Id<UserMarker>) -> Self {
        Self {
            id,
            name: None,
            permissions: Permissions::empty(),
            roles: Vec::new(),
        }
    }

    /// Sets the username (default `user-<id>`).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the member permissions sent as `member.permissions` (default
    /// empty).
    #[must_use]
    pub fn permissions(mut self, permissions: Permissions) -> Self {
        self.permissions = permissions;
        self
    }

    /// Sets the member roles sent as `member.roles`.
    #[must_use]
    pub fn roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        self.roles = roles.into_iter().collect();
        self
    }

    pub(crate) fn username(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("user-{}", self.id))
    }
}

/// A command option value given to `.option(name, value)`.
///
/// ```
/// use noctis_testkit::TestOption;
/// use twilight_model::id::Id;
///
/// let _: TestOption = "text".into();
/// let _: TestOption = 5.into();
/// let _: TestOption = 0.5.into();
/// let _: TestOption = true.into();
/// let _: TestOption = Id::<twilight_model::id::marker::UserMarker>::new(1).into();
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TestOption {
    /// A STRING value.
    String(String),
    /// An INTEGER value (also accepted by NUMBER options).
    Integer(i64),
    /// A NUMBER value.
    Number(f64),
    /// A BOOLEAN value.
    Boolean(bool),
    /// A USER (or MENTIONABLE) value.
    User(Id<UserMarker>),
    /// A CHANNEL value.
    Channel(Id<ChannelMarker>),
    /// A ROLE (or MENTIONABLE) value.
    Role(Id<RoleMarker>),
}

impl TestOption {
    /// The Discord type name of the value.
    pub(crate) const fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "STRING",
            Self::Integer(_) => "INTEGER",
            Self::Number(_) => "NUMBER",
            Self::Boolean(_) => "BOOLEAN",
            Self::User(_) => "USER",
            Self::Channel(_) => "CHANNEL",
            Self::Role(_) => "ROLE",
        }
    }

    /// The value as text, for error messages.
    pub(crate) fn display(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
            Self::Number(f) => f.to_string(),
            Self::Boolean(b) => b.to_string(),
            Self::User(id) => id.to_string(),
            Self::Channel(id) => id.to_string(),
            Self::Role(id) => id.to_string(),
        }
    }
}

impl From<&str> for TestOption {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for TestOption {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i32> for TestOption {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<i64> for TestOption {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for TestOption {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<bool> for TestOption {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<Id<UserMarker>> for TestOption {
    fn from(value: Id<UserMarker>) -> Self {
        Self::User(value)
    }
}

impl From<Id<ChannelMarker>> for TestOption {
    fn from(value: Id<ChannelMarker>) -> Self {
        Self::Channel(value)
    }
}

impl From<Id<RoleMarker>> for TestOption {
    fn from(value: Id<RoleMarker>) -> Self {
        Self::Role(value)
    }
}

/// Every seed of one [`TestApp`](crate::TestApp).
#[derive(Debug, Clone, Default)]
pub(crate) struct Seeds {
    pub(crate) guilds: BTreeMap<Id<GuildMarker>, TestGuild>,
    pub(crate) users: BTreeMap<Id<UserMarker>, TestUser>,
}

impl Seeds {
    pub(crate) fn user(&self, id: Id<UserMarker>) -> Result<&TestUser, crate::TestError> {
        self.users.get(&id).ok_or(crate::TestError::Unknown {
            kind: "user",
            id: id.get(),
        })
    }

    pub(crate) fn guild(&self, id: Id<GuildMarker>) -> Result<&TestGuild, crate::TestError> {
        self.guilds.get(&id).ok_or(crate::TestError::Unknown {
            kind: "guild",
            id: id.get(),
        })
    }

    /// The channel in `guild` (a guild run sees only its own guild's); in a
    /// DM run, in any seeded guild.
    pub(crate) fn channel(
        &self,
        guild: Option<Id<GuildMarker>>,
        id: Id<ChannelMarker>,
    ) -> Result<(&str, ChannelType), crate::TestError> {
        match guild {
            Some(g) => self.guilds.get(&g).and_then(|g| g.find_channel(id)),
            None => self.guilds.values().find_map(|g| g.find_channel(id)),
        }
        .ok_or(crate::TestError::Unknown {
            kind: "channel",
            id: id.get(),
        })
    }

    /// The role in `guild` (a guild run sees only its own guild's); in a
    /// DM run, in any seeded guild.
    pub(crate) fn role(
        &self,
        guild: Option<Id<GuildMarker>>,
        id: Id<RoleMarker>,
    ) -> Result<&str, crate::TestError> {
        match guild {
            Some(g) => self.guilds.get(&g).and_then(|g| g.find_role(id)),
            None => self.guilds.values().find_map(|g| g.find_role(id)),
        }
        .ok_or(crate::TestError::Unknown {
            kind: "role",
            id: id.get(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

    use super::*;

    /// A guild seed becomes an available `GUILD_CREATE` with its channels
    /// and roles.
    #[test]
    fn guild_seed_is_a_guild_create() {
        let guild = TestGuild::new(Id::new(10), "G")
            .text_channel(Id::new(20), "logs")
            .role(Id::new(30), "Mods")
            .role(Id::new(31), "Admins")
            .locale("tr");
        let Event::GuildCreate(create) = guild.create_event().unwrap() else {
            panic!("not a GUILD_CREATE");
        };
        let GuildCreate::Available(guild) = *create else {
            panic!("unavailable");
        };
        assert_eq!(guild.name, "G");
        assert_eq!(guild.preferred_locale, "tr");
        assert_eq!(guild.channels[0].guild_id, Some(Id::new(10)));
        assert_eq!(guild.channels[0].name.as_deref(), Some("logs"));
        assert_eq!(guild.roles[1].name, "Admins");
        assert_eq!(guild.roles[1].position, 1);
    }

    /// A guild run never resolves another guild's channels or roles.
    #[test]
    fn guild_run_resolves_only_its_own_guild() {
        let mut seeds = Seeds::default();
        let a = TestGuild::new(Id::new(10), "A")
            .text_channel(Id::new(20), "a-logs")
            .role(Id::new(30), "a-mods");
        let b = TestGuild::new(Id::new(11), "B")
            .text_channel(Id::new(21), "b-logs")
            .role(Id::new(31), "b-mods");
        seeds.guilds.insert(a.id, a);
        seeds.guilds.insert(b.id, b);
        let guild = Some(Id::new(10));
        assert_eq!(seeds.channel(guild, Id::new(20)).unwrap().0, "a-logs");
        assert_eq!(seeds.role(guild, Id::new(30)).unwrap(), "a-mods");
        assert!(matches!(
            seeds.channel(guild, Id::new(21)),
            Err(crate::TestError::Unknown {
                kind: "channel",
                id: 21
            })
        ));
        assert!(matches!(
            seeds.role(guild, Id::new(31)),
            Err(crate::TestError::Unknown {
                kind: "role",
                id: 31
            })
        ));
        assert_eq!(seeds.channel(None, Id::new(21)).unwrap().0, "b-logs");
        assert_eq!(seeds.role(None, Id::new(31)).unwrap(), "b-mods");
    }
}
