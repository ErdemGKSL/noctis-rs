//! `#[command]` through the facade (Story 6.1, Matrix 1, 2, 4-8, 48 and 49).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;

use noctis::__private::core::__testing::call_command;
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::rules::command::{PathKind, expand};
use noctis::{CommandHandler, CommandPath, UnknownCommandPath};

/// The variants the `config` handler received, in call order.
static RECEIVED: Mutex<Vec<ConfigPath>> = Mutex::new(Vec::new());

#[deny(missing_docs)]
mod cmds {
    //! Generated items are documented (Matrix 48).

    use noctis::prelude::*;

    use super::RECEIVED;

    /// Pings.
    #[command("ping", description = "Check that the bot answers")]
    pub async fn ping(ctx: CommandCtx) -> HandlerResult<()> {
        assert_eq!(ctx.command_path(), "ping");
        Ok(())
    }

    /// Starts the setup.
    #[command("setup start", description = "Start the setup")]
    pub(crate) async fn setup_start(_ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }

    /// Configures the bot.
    #[command("config (set|get) (prefix|welcome)", description = "Configure")]
    pub async fn config(ctx: CommandCtx, path: ConfigPath) -> HandlerResult<()> {
        assert_eq!(ctx.command_path(), path.as_str());
        RECEIVED.lock().unwrap().push(path);
        Ok(())
    }

    /// An optional leading group.
    #[command("(a|b)? c", description = "d")]
    pub async fn opt(_ctx: CommandCtx, _path: OptPath) -> HandlerResult<()> {
        Ok(())
    }
}

use cmds::{ConfigPath, OptPath, config, opt, ping, setup_start};

fn kind(handler: &CommandHandler) -> Option<PathKind> {
    PathKind::of(handler.paths()[0])
}

/// Matrix 1 and 2.
#[test]
fn plain_paths() {
    let handler = ping();
    assert_eq!(handler.paths(), ["ping"]);
    assert_eq!(handler.pattern(), "ping");
    assert_eq!(handler.description(), "Check that the bot answers");
    assert_eq!(handler.name(), format!("{}::cmds::ping", module_path!()));
    assert_eq!(kind(&handler), Some(PathKind::Command));
    let handler = setup_start();
    assert_eq!(handler.paths(), ["setup start"]);
    assert_eq!(kind(&handler), Some(PathKind::Subcommand));
}

/// Matrix 4: the AC pattern and its enum.
#[test]
fn pattern_enum() {
    assert_eq!(
        ConfigPath::ALL,
        [
            ConfigPath::SetPrefix,
            ConfigPath::SetWelcome,
            ConfigPath::GetPrefix,
            ConfigPath::GetWelcome
        ]
    );
    assert_eq!(ConfigPath::SetPrefix.as_str(), "config set prefix");
    let expanded = expand("config (set|get) (prefix|welcome)", &Limits::default()).unwrap();
    assert_eq!(config().paths(), expanded.as_slice());
    assert_eq!(ConfigPath::PATHS, expanded.as_slice());
    let strs: Vec<_> = ConfigPath::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(strs, expanded);
    assert_eq!(config().description(), "Configure");
    assert_eq!(kind(&config()), Some(PathKind::GroupSubcommand));
}

/// Matrix 5: the handler receives the invoked variant.
#[tokio::test]
async fn handler_gets_the_variant() {
    call_command(&config(), "config get welcome").await.unwrap();
    assert!(RECEIVED.lock().unwrap().contains(&ConfigPath::GetWelcome));
}

fn unknown(result: HandlerResult<()>) -> String {
    let err = result.unwrap_err();
    let unknown = err
        .as_error()
        .downcast_ref::<UnknownCommandPath>()
        .expect("an UnknownCommandPath");
    unknown.to_string()
}

/// Matrix 6: an unknown path is an error, not a panic.
#[tokio::test]
async fn unknown_path() {
    let here = module_path!();
    assert_eq!(
        unknown(call_command(&config(), "config nope").await),
        format!("`config nope` is not a path of the Command `{here}::cmds::config`")
    );
    let err = call_command(&ping(), "pong").await;
    assert_eq!(
        unknown(err),
        format!("`pong` is not a path of the Command `{here}::cmds::ping`")
    );
}

/// Matrix 7: `from_path` is an exact byte match.
#[test]
fn from_path() {
    assert_eq!(
        <ConfigPath as CommandPath>::from_path("config set prefix"),
        Some(ConfigPath::SetPrefix)
    );
    for path in ["", "config  set prefix", "Config set prefix"] {
        assert_eq!(
            <ConfigPath as CommandPath>::from_path(path),
            None,
            "{path:?}"
        );
    }
}

/// Matrix 8: all groups absent is legal and named `Bare`.
#[test]
fn optional_group() {
    assert_eq!(opt().paths(), ["a c", "b c", "c"]);
    assert_eq!(OptPath::ALL, [OptPath::A, OptPath::B, OptPath::Bare]);
}

/// Matrix 49: `Module::add` takes commands.
#[test]
fn module_add() {
    let module = Module::new("cfg").add(config()).add(ping());
    let debug = format!("{module:?}");
    let here = module_path!();
    assert!(
        debug.contains(&format!(
            "commands: [\"{here}::cmds::config\", \"{here}::cmds::ping\"]"
        )),
        "{debug}"
    );
}

mod metadata {
    //! Command metadata through `metadata()` (Story 6.3, Matrix 1, 2, 3, 12,
    //! 15, 17 and 28).

    use noctis::prelude::*;
    use noctis::twilight::model::id::Id;
    use noctis::{CommandMetadata, IntegrationType, InteractionContext, Permissions};

    #[command(
        "ac",
        description = "d",
        permissions = MANAGE_GUILD | BAN_MEMBERS,
        contexts = [Guild],
        nsfw = true
    )]
    async fn ac(_ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }

    #[command("admin", description = "d", permissions = ADMINISTRATOR)]
    async fn admin(_ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }

    #[command(
        "where",
        description = "d",
        contexts = [Guild, InteractionContext::BotDm, PrivateChannel,],
        integration_types = [UserInstall, GuildInstall],
        nsfw = false
    )]
    async fn r#where(_ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }

    #[command("dev", description = "d", guilds = [123456789012345678, 42u64])]
    async fn dev(_ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }

    #[command("config (set|get)", description = "d", permissions = Permissions::MANAGE_GUILD)]
    async fn config(_ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<()> {
        Ok(())
    }

    #[command("plain", description = "d")]
    async fn plain(_ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }

    /// Matrix 1: the acceptance example.
    #[test]
    fn meta_01_acceptance_example() {
        let handler = ac();
        let meta = handler.metadata();
        assert_eq!(
            meta.default_member_permissions,
            Some(Permissions::MANAGE_GUILD | Permissions::BAN_MEMBERS)
        );
        assert_eq!(meta.contexts, Some(&[InteractionContext::Guild][..]));
        assert_eq!(meta.nsfw, Some(true));
        assert_eq!(meta.integration_types, None);
        assert!(meta.guilds.is_empty());
    }

    /// Matrix 2 and 3.
    #[test]
    fn meta_01_unset_and_single() {
        assert_eq!(plain().metadata(), &CommandMetadata::new());
        assert_eq!(
            admin()
                .metadata()
                .default_member_permissions
                .map(|p| p.bits()),
            Some(8)
        );
    }

    /// Matrix 12 and 15: lists keep their order.
    #[test]
    fn meta_03_lists_in_declared_order() {
        let handler = r#where();
        let meta = handler.metadata();
        assert_eq!(
            meta.contexts,
            Some(
                &[
                    InteractionContext::Guild,
                    InteractionContext::BotDm,
                    InteractionContext::PrivateChannel
                ][..]
            )
        );
        assert_eq!(
            meta.integration_types,
            Some(&[IntegrationType::UserInstall, IntegrationType::GuildInstall][..])
        );
        assert_eq!(meta.nsfw, Some(false));
    }

    /// Matrix 17: guild scope.
    #[test]
    fn meta_03_guilds() {
        let handler = dev();
        let meta = handler.metadata();
        assert_eq!(meta.guilds, [Id::new(123_456_789_012_345_678), Id::new(42)]);
        assert!(meta.is_guild_scoped());
        assert_eq!(meta.default_member_permissions, None);
        assert_eq!(meta.contexts, None);
        assert_eq!(meta.nsfw, None);
    }

    /// Matrix 28: metadata on a pattern applies to the handler.
    #[test]
    fn meta_02_pattern_metadata() {
        let handler = config();
        assert_eq!(handler.paths(), ["config set", "config get"]);
        assert_eq!(
            handler.metadata().default_member_permissions,
            Some(Permissions::MANAGE_GUILD)
        );
        assert!(format!("{handler:?}").contains("metadata: CommandMetadata {"));
    }
}
