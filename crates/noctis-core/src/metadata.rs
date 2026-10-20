//! [`CommandMetadata`]: the typed metadata `#[command]`, `#[user_command]` and
//! `#[message_command]` lower their `permissions`, `contexts`,
//! `integration_types`, `nsfw` and `guilds` arguments into (FR-11, AD-26),
//! and [`BaseMeta`], the description and base metadata a Module declares for a
//! base command with `Module::base` (FR-57).

use noctis_rules::command::{CommandMeta, IntegrationType, InteractionContext, Scope};
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

/// The metadata of one Command or context-menu command, as declared.
///
/// Unset fields are `None` (and `guilds` empty): Publish omits them, so
/// Discord applies its own default. The values belong to the **base**
/// command (META-02): one `#[command]` sets them for every path it declares,
/// and handlers sharing a base must agree, unset included, unless a
/// [`BaseMeta`] declares the field (checked at App build). Lists keep their
/// declared order.
///
/// ```
/// use noctis_core::{CommandMetadata, Permissions};
/// use noctis_rules::command::InteractionContext;
///
/// const META: CommandMetadata = CommandMetadata::new()
///     .default_member_permissions(Permissions::MANAGE_GUILD)
///     .contexts(&[InteractionContext::Guild])
///     .nsfw(false);
/// assert_eq!(META.default_member_permissions, Some(Permissions::MANAGE_GUILD));
/// assert_eq!(META.integration_types, None);
/// assert!(!META.is_guild_scoped());
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandMetadata {
    /// The permissions members need by default (META-01).
    pub default_member_permissions: Option<Permissions>,
    /// Where the command can be used (global commands only, META-03).
    pub contexts: Option<&'static [InteractionContext]>,
    /// How the command can be installed (global commands only, META-03).
    pub integration_types: Option<&'static [IntegrationType]>,
    /// Whether the command is age-restricted (CMD-07).
    pub nsfw: Option<bool>,
    /// The guilds the command is published to; empty for a global command.
    pub guilds: &'static [Id<GuildMarker>],
}

impl CommandMetadata {
    /// Metadata with every field unset: a global command.
    pub const fn new() -> Self {
        Self {
            default_member_permissions: None,
            contexts: None,
            integration_types: None,
            nsfw: None,
            guilds: &[],
        }
    }

    /// Sets `default_member_permissions`.
    #[must_use]
    pub const fn default_member_permissions(self, permissions: Permissions) -> Self {
        Self {
            default_member_permissions: Some(permissions),
            ..self
        }
    }

    /// Sets `contexts`.
    #[must_use]
    pub const fn contexts(self, contexts: &'static [InteractionContext]) -> Self {
        Self {
            contexts: Some(contexts),
            ..self
        }
    }

    /// Sets `integration_types`.
    #[must_use]
    pub const fn integration_types(self, types: &'static [IntegrationType]) -> Self {
        Self {
            integration_types: Some(types),
            ..self
        }
    }

    /// Sets `nsfw`.
    #[must_use]
    pub const fn nsfw(self, nsfw: bool) -> Self {
        Self {
            nsfw: Some(nsfw),
            ..self
        }
    }

    /// Sets the guilds the command is published to.
    #[must_use]
    pub const fn guilds(self, guilds: &'static [Id<GuildMarker>]) -> Self {
        Self { guilds, ..self }
    }

    /// Whether the command is published only to [`guilds`](Self::guilds).
    pub fn is_guild_scoped(&self) -> bool {
        !self.guilds.is_empty()
    }

    /// The rules' view of the metadata (raw permission bits).
    pub(crate) fn to_rules(self) -> CommandMeta<'static> {
        let mut meta = CommandMeta::new();
        if let Some(permissions) = self.default_member_permissions {
            meta = meta.default_member_permissions(permissions.bits());
        }
        if let Some(contexts) = self.contexts {
            meta = meta.contexts(contexts);
        }
        if let Some(types) = self.integration_types {
            meta = meta.integration_types(types);
        }
        if let Some(nsfw) = self.nsfw {
            meta = meta.nsfw(nsfw);
        }
        meta
    }

    /// The registration scopes: `[Global]` without guilds, else one
    /// `Guild(id)` per guild, in order.
    pub(crate) fn scopes(&self) -> Vec<Scope> {
        if self.guilds.is_empty() {
            vec![Scope::Global]
        } else {
            self.guilds
                .iter()
                .map(|id| Scope::Guild(id.get()))
                .collect()
        }
    }
}

/// The description and base metadata a Module declares for a base command
/// with `Module::base` (FR-57).
///
/// Every field is optional. A set metadata field is the explicit base value:
/// handlers below the base that leave the field unset inherit it, and those
/// that set it must match (META-01, META-02, CMD-07). Two Modules that
/// declare different values for the same base fail the App build (REG-04 for
/// the description). The description is measured at App build (CMD-04: 1 to
/// 100 characters). There is no `guilds`: the base exists in every scope its
/// handlers are registered in, and the declaration applies to each.
///
/// ```
/// use noctis_core::{BaseMeta, Permissions};
///
/// const CONFIG: BaseMeta = BaseMeta::new()
///     .description("Configure the bot")
///     .default_member_permissions(Permissions::MANAGE_GUILD)
///     .nsfw(false);
/// assert_eq!(CONFIG.description, Some("Configure the bot"));
/// assert_eq!(CONFIG.contexts, None);
/// assert_eq!(BaseMeta::new(), BaseMeta::default());
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BaseMeta {
    /// The base command's description (CMD-04).
    pub description: Option<&'static str>,
    /// The permissions members need by default (META-01).
    pub default_member_permissions: Option<Permissions>,
    /// Where the command can be used (global commands only, META-03).
    pub contexts: Option<&'static [InteractionContext]>,
    /// How the command can be installed (global commands only, META-03).
    pub integration_types: Option<&'static [IntegrationType]>,
    /// Whether the command is age-restricted (CMD-07).
    pub nsfw: Option<bool>,
}

impl BaseMeta {
    /// A declaration with every field unset.
    pub const fn new() -> Self {
        Self {
            description: None,
            default_member_permissions: None,
            contexts: None,
            integration_types: None,
            nsfw: None,
        }
    }

    /// Sets the description.
    #[must_use]
    pub const fn description(self, description: &'static str) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }

    /// Sets `default_member_permissions`.
    #[must_use]
    pub const fn default_member_permissions(self, permissions: Permissions) -> Self {
        Self {
            default_member_permissions: Some(permissions),
            ..self
        }
    }

    /// Sets `contexts`.
    #[must_use]
    pub const fn contexts(self, contexts: &'static [InteractionContext]) -> Self {
        Self {
            contexts: Some(contexts),
            ..self
        }
    }

    /// Sets `integration_types`.
    #[must_use]
    pub const fn integration_types(self, types: &'static [IntegrationType]) -> Self {
        Self {
            integration_types: Some(types),
            ..self
        }
    }

    /// Sets `nsfw`.
    #[must_use]
    pub const fn nsfw(self, nsfw: bool) -> Self {
        Self {
            nsfw: Some(nsfw),
            ..self
        }
    }

    /// The rules' view of the metadata fields (no description, no guilds).
    pub(crate) fn to_rules(self) -> CommandMeta<'static> {
        CommandMetadata {
            default_member_permissions: self.default_member_permissions,
            contexts: self.contexts,
            integration_types: self.integration_types,
            nsfw: self.nsfw,
            guilds: &[],
        }
        .to_rules()
    }
}

#[cfg(test)]
mod tests {
    use noctis_rules::command::{PERMISSIONS, permission};

    use super::*;

    /// Matrix 56: the rules table is twilight's `Permissions`.
    #[test]
    fn meta_01_permission_table_matches_twilight() {
        let mut all = 0;
        for (name, bit) in PERMISSIONS {
            let flag = Permissions::from_name(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(flag.bits(), bit, "{name}");
            assert_eq!(permission(name), Some(bit));
            all |= bit;
        }
        assert_eq!(all, Permissions::all().bits());
    }

    /// Matrix 59: the rules view and the scopes.
    #[test]
    fn meta_03_to_rules_and_scopes() {
        const GUILDS: &[Id<GuildMarker>] = &[Id::new(7), Id::new(8)];
        let unset = CommandMetadata::new();
        assert_eq!(unset, CommandMetadata::default());
        assert_eq!(unset.to_rules(), CommandMeta::new());
        assert_eq!(unset.scopes(), [Scope::Global]);
        assert!(!unset.is_guild_scoped());

        let meta = CommandMetadata::new()
            .default_member_permissions(Permissions::MANAGE_GUILD | Permissions::BAN_MEMBERS)
            .contexts(&[InteractionContext::BotDm, InteractionContext::Guild])
            .integration_types(&[IntegrationType::UserInstall])
            .nsfw(true)
            .guilds(GUILDS);
        assert_eq!(
            meta.to_rules(),
            CommandMeta::new()
                .default_member_permissions(36)
                .contexts(&[InteractionContext::BotDm, InteractionContext::Guild])
                .integration_types(&[IntegrationType::UserInstall])
                .nsfw(true)
        );
        assert_eq!(meta.scopes(), [Scope::Guild(7), Scope::Guild(8)]);
        assert!(meta.is_guild_scoped());
    }

    #[test]
    fn meta_01_base_meta_to_rules() {
        assert_eq!(BaseMeta::new().to_rules(), CommandMeta::new());
        let meta = BaseMeta::new()
            .description("d")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .contexts(&[InteractionContext::Guild])
            .integration_types(&[IntegrationType::GuildInstall])
            .nsfw(true);
        assert_eq!(
            meta.to_rules(),
            CommandMeta::new()
                .default_member_permissions(32)
                .contexts(&[InteractionContext::Guild])
                .integration_types(&[IntegrationType::GuildInstall])
                .nsfw(true)
        );
    }
}
