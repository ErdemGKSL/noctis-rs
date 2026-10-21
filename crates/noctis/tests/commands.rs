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
    #[derive(SlashCommand)]
    #[command("ping", description = "Check that the bot answers")]
    pub struct Ping;

    impl Ping {
        #[handler]
        async fn handle(self, ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            assert_eq!(ctx.command_path(), "ping");
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    /// Starts the setup.
    #[derive(SlashCommand)]
    #[command("setup start", description = "Start the setup")]
    pub(crate) struct SetupStart;

    impl SetupStart {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    /// Configures the bot.
    #[derive(SlashCommand)]
    #[command("config (set|get) (prefix|welcome)", description = "Configure")]
    pub struct Config;

    impl Config {
        #[handler]
        async fn handle(
            self,
            ctx: CommandCtx,
            path: ConfigPath,
        ) -> HandlerResult<noctis::ui::LegacyMessage> {
            assert_eq!(ctx.command_path(), path.as_str());
            RECEIVED.lock().unwrap().push(path);
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    /// An optional leading group.
    #[derive(SlashCommand)]
    #[command("(a|b)? c", description = "d")]
    pub struct Opt;

    impl Opt {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            _path: OptPath,
        ) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }
}

use cmds::{Config, ConfigPath, Opt, OptPath, Ping, SetupStart};

fn kind(handler: &CommandHandler) -> Option<PathKind> {
    PathKind::of(handler.paths()[0])
}

/// Matrix 1 and 2.
#[test]
fn plain_paths() {
    let handler = Ping::descriptor();
    assert_eq!(handler.paths(), ["ping"]);
    assert_eq!(handler.pattern(), "ping");
    assert_eq!(handler.description(), "Check that the bot answers");
    assert_eq!(handler.name(), format!("{}::cmds::Ping", module_path!()));
    assert_eq!(kind(&handler), Some(PathKind::Command));
    let handler = SetupStart::descriptor();
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
    assert_eq!(Config::descriptor().paths(), expanded.as_slice());
    assert_eq!(ConfigPath::PATHS, expanded.as_slice());
    let strs: Vec<_> = ConfigPath::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(strs, expanded);
    assert_eq!(Config::descriptor().description(), "Configure");
    assert_eq!(kind(&Config::descriptor()), Some(PathKind::GroupSubcommand));
}

/// Matrix 5: the handler receives the invoked variant.
#[tokio::test]
async fn handler_gets_the_variant() {
    call_command(&Config::descriptor(), "config get welcome")
        .await
        .unwrap();
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
        unknown(call_command(&Config::descriptor(), "config nope").await),
        format!("`config nope` is not a path of the Command `{here}::cmds::Config`")
    );
    let err = call_command(&Ping::descriptor(), "pong").await;
    assert_eq!(
        unknown(err),
        format!("`pong` is not a path of the Command `{here}::cmds::Ping`")
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
    assert_eq!(Opt::descriptor().paths(), ["a c", "b c", "c"]);
    assert_eq!(OptPath::ALL, [OptPath::A, OptPath::B, OptPath::Bare]);
}

/// Matrix 49: `Module::add` takes commands.
#[test]
fn module_add() {
    let module = Module::new("cfg").add::<Config>().add::<Ping>();
    let debug = format!("{module:?}");
    let here = module_path!();
    assert!(
        debug.contains(&format!(
            "commands: [\"{here}::cmds::Config\", \"{here}::cmds::Ping\"]"
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

    #[derive(SlashCommand)]
    #[command(
        "ac",
        description = "d",
        permissions = MANAGE_GUILD | BAN_MEMBERS,
        contexts = [Guild],
        nsfw = true
    )]
    struct Ac;

    impl Ac {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    #[derive(SlashCommand)]
    #[command("admin", description = "d", permissions = ADMINISTRATOR)]
    struct Admin;

    impl Admin {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    #[derive(SlashCommand)]
    #[command(
        "where",
        description = "d",
        contexts = [Guild, InteractionContext::BotDm, PrivateChannel,],
        integration_types = [UserInstall, GuildInstall],
        nsfw = false
    )]
    struct Where;

    impl Where {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    #[derive(SlashCommand)]
    #[command("dev", description = "d", guilds = [123456789012345678, 42u64])]
    struct Dev;

    impl Dev {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    #[derive(SlashCommand)]
    #[command("config (set|get)", description = "d", permissions = Permissions::MANAGE_GUILD)]
    struct Config;

    impl Config {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            _path: ConfigPath,
        ) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    #[derive(SlashCommand)]
    #[command("plain", description = "d")]
    struct Plain;

    impl Plain {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
            Ok(noctis::ui::LegacyMessage::new("ok"))
        }
    }

    /// Matrix 1: the acceptance example.
    #[test]
    fn meta_01_acceptance_example() {
        let handler = Ac::descriptor();
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
        assert_eq!(Plain::descriptor().metadata(), &CommandMetadata::new());
        assert_eq!(
            Admin::descriptor()
                .metadata()
                .default_member_permissions
                .map(|p| p.bits()),
            Some(8)
        );
    }

    /// Matrix 12 and 15: lists keep their order.
    #[test]
    fn meta_03_lists_in_declared_order() {
        let handler = Where::descriptor();
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
        let handler = Dev::descriptor();
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
        let handler = Config::descriptor();
        assert_eq!(handler.paths(), ["config set", "config get"]);
        assert_eq!(
            handler.metadata().default_member_permissions,
            Some(Permissions::MANAGE_GUILD)
        );
        assert!(format!("{handler:?}").contains("metadata: CommandMetadata {"));
    }
}

/// Story 6.5: the Responder through the facade seams (Matrix 30, 41, 52).
mod responder {
    use noctis::__private::core::__testing::{call_command, call_context_menu};
    use noctis::ResponderError;
    use noctis::prelude::*;
    use noctis::ui::{ActionRow, Components, ResolvedUser, SelectOption, TextDisplay};

    #[derive(::noctis::StringSelectMenu)]
    #[string_select("t.pick", defaults(placeholder = "Pick"))]
    struct Pick;

    impl Pick {
        #[::noctis::handler]
        async fn handle(
            self,
            _ctx: ::noctis::ComponentCtx,
        ) -> ::noctis::HandlerResult<::noctis::Responded> {
            Ok(::noctis::__private::core::__testing::responded())
        }
    }

    #[derive(::noctis::Button)]
    #[button("t.form", defaults(style = Secondary, label = "Go"))]
    struct Form;

    impl Form {
        #[::noctis::handler]
        async fn handle(
            self,
            _ctx: ::noctis::ComponentCtx,
        ) -> ::noctis::HandlerResult<::noctis::Responded> {
            Ok(::noctis::__private::core::__testing::responded())
        }
    }

    fn text(t: &str) -> Components {
        Components::new().push(TextDisplay::new(t.to_owned()))
    }

    #[derive(SlashCommand)]
    #[command("answer", description = "d", response = ephemeral)]
    struct Answer;

    impl Answer {
        #[handler]
        async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
            assert_eq!(ctx.ephemerality(), Ephemerality::Ephemeral);
            let acked = r.defer_reply().await?;
            acked.edit_reply(text("done")).await?;
            let handle = acked.follow_up(text("log"), Visibility::Public).await?;
            handle.edit(text("log 2")).await?;
            Ok(acked.done())
        }
    }

    #[derive(SlashCommand)]
    #[command("public", description = "d", response = public)]
    struct Public;

    impl Public {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
            Ok(text("hi"))
        }
    }

    #[derive(SlashCommand)]
    #[command("unresolved", description = "d")]
    struct Unresolved;

    impl Unresolved {
        #[handler]
        async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
            Ok(Components::new().push(ActionRow::select(
                Pick.build().options([SelectOption::new("A", "a")]),
            )))
        }
    }

    #[derive(UserCommand)]
    #[user_command("Wave", response = ephemeral)]
    struct Wave;

    impl Wave {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            target: ResolvedUser,
            r: CommandResponder,
        ) -> HandlerResult<Responded> {
            let text = format!("hi <@{}>", target.user.id);
            Ok(r.reply(text_owned(text)).await?.done())
        }
    }

    fn text_owned(t: String) -> Components {
        Components::new().push(TextDisplay::new(t))
    }

    /// Matrix 41: the declared ephemerality.
    #[test]
    fn ephemeral_argument() {
        assert_eq!(Answer::descriptor().ephemerality(), Ephemerality::Ephemeral);
        assert_eq!(Public::descriptor().ephemerality(), Ephemerality::Public);
        assert_eq!(Wave::descriptor().ephemerality(), Ephemerality::Ephemeral);
    }

    /// Matrix 52: the seams run responder handlers detached.
    #[tokio::test]
    async fn seams_run_responder_handlers() {
        call_command(&Answer::descriptor(), "answer").await.unwrap();
        call_command(&Public::descriptor(), "public").await.unwrap();
        let json = serde_json::json!({
            "type": 2,
            "data": {
                "type": 2, "name": "Wave", "target_id": "11",
                "resolved": { "users": { "11": {
                    "id": "11", "username": "u", "discriminator": "0000",
                    "avatar": null, "global_name": null
                } } }
            }
        });
        call_context_menu(&Wave::descriptor(), json.to_string().as_bytes())
            .await
            .unwrap();
    }

    /// Matrix 30: an unresolved Route `custom_id` is an encode error.
    #[tokio::test]
    async fn unresolved_custom_id() {
        let err = call_command(&Unresolved::descriptor(), "unresolved")
            .await
            .unwrap_err();
        let responder = err
            .as_error()
            .downcast_ref::<ResponderError>()
            .unwrap_or_else(|| panic!("{err}"));
        assert!(
            matches!(responder, ResponderError::Encode { reason } if reason.contains("unresolved")),
            "{responder}"
        );
    }

    /// Story 6.10 boundary pass (INT-05): an `auto_defer = false` handler
    /// compiles with `CommandResponder<Manual>`.
    #[derive(SlashCommand)]
    #[command("feedback", description = "d", auto_defer = false)]
    struct Feedback;

    impl Feedback {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            r: CommandResponder<Manual>,
        ) -> HandlerResult<Responded> {
            let modal = noctis::__private::ui::__macro::build_modal::<Form>(Form, "Feedback")
                .push(TextDisplay::new("Thanks!"));
            Ok(r.show_modal(modal).await?.done())
        }
    }

    /// Story 6.10 boundary pass: `auto_defer = true` with a plain
    /// `CommandResponder`.
    #[derive(SlashCommand)]
    #[command("explicit", description = "d", auto_defer = true)]
    struct Explicit;

    impl Explicit {
        #[handler]
        async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
            Ok(r.reply(text("hi")).await?.done())
        }
    }

    /// Story 6.10 boundary pass: a context menu declared `auto_defer = false`
    /// and `response = ephemeral`.
    #[derive(UserCommand)]
    #[user_command("Report", response = ephemeral, auto_defer = false)]
    struct Report;

    impl Report {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            _target: ResolvedUser,
            r: CommandResponder<Manual>,
        ) -> HandlerResult<Responded> {
            Ok(r.defer_reply().await?.done())
        }
    }

    /// Story 6.6 Matrix 19: a 46-character title (Strict).
    #[derive(SlashCommand)]
    #[command("long", description = "d", auto_defer = false)]
    struct Long;

    impl Long {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            r: CommandResponder<Manual>,
        ) -> HandlerResult<Responded> {
            let modal = noctis::__private::ui::__macro::build_modal::<Form>(Form, "a".repeat(46))
                .push(TextDisplay::new("x"));
            Ok(r.show_modal(modal).await?.done())
        }
    }

    /// Story 6.6 Matrix 20: an unresolved Route id.
    #[derive(SlashCommand)]
    #[command("unresolved_modal", description = "d", auto_defer = false)]
    struct UnresolvedModal;

    impl UnresolvedModal {
        #[handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            r: CommandResponder<Manual>,
        ) -> HandlerResult<Responded> {
            let modal = noctis::__private::ui::__macro::build_modal::<Pick>(Pick, "Pick")
                .push(TextDisplay::new("x"));
            Ok(r.show_modal(modal).await?.done())
        }
    }

    /// Story 6.6 Matrix 17, 19 and 20 over the detached seam.
    #[tokio::test]
    async fn manual_handlers() {
        assert!(!Feedback::descriptor().auto_defer());
        assert!(Public::descriptor().auto_defer());
        assert!(Answer::descriptor().auto_defer());
        assert!(Explicit::descriptor().auto_defer());
        assert!(!Report::descriptor().auto_defer());
        assert_eq!(Report::descriptor().ephemerality(), Ephemerality::Ephemeral);
        call_command(&Feedback::descriptor(), "feedback")
            .await
            .unwrap();
        let responder_error = |err: &HandlerError| {
            format!(
                "{:?}",
                err.as_error()
                    .downcast_ref::<ResponderError>()
                    .unwrap_or_else(|| panic!("{err}"))
            )
        };
        let err = call_command(&Long::descriptor(), "long").await.unwrap_err();
        assert!(responder_error(&err).starts_with("Invalid(Limits"), "{err}");
        let err = call_command(&UnresolvedModal::descriptor(), "unresolved_modal")
            .await
            .unwrap_err();
        assert!(responder_error(&err).starts_with("Encode"), "{err}");
    }
}
