//! Story 11.4: command localization end to end. `#[command]` reads its
//! descriptions from the default-locale file, Publish sends the merged
//! catalog's localizations (`AppProbe::commands`, Publish's exact JSON),
//! `App::build` checks them, and dispatch routes on default names only.

#![cfg(all(feature = "i18n", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use noctis::__private::core::__testing::AppProbe;
use noctis::prelude::*;
use noctis::rules::{Locale, RuleId};
use noctis::testkit::{TestApp, TestError};
use noctis::ui::{ResolvedMessage, ResolvedUser};
use noctis::{AppBuilder, BuildViolation, Runtime};
use serde_json::{Value, json};

mod strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/cmd");
}

mod gb_strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/cmd_gb");
}

mod clash_strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/cmd_clash");
}

#[derive(Choices, Debug, Clone, Copy, PartialEq)]
enum BanColor {
    #[choice("Red")]
    Red,
    #[choice("Blue")]
    Blue,
}

/// Every description comes from `tests/fixtures/i18n/cmd/en-US.yaml`.
#[derive(SlashCommand)]
#[command("mod ban", locales = "tests/fixtures/i18n/cmd")]
struct Ban {
    user: Option<ResolvedUser>,
    color: Option<BanColor>,
}

impl Ban {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let _ = (self.user, self.color);
        ctx.reply_text("banned").await
    }
}

/// The base `setup` takes `cmd.setup.description` from the file.
#[derive(SlashCommand)]
#[command("setup start", locales = "tests/fixtures/i18n/cmd")]
struct SetupStart;

impl SetupStart {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text("started").await
    }
}

/// A Pattern: each expansion takes its own `cmd.cfg.<word>.description`.
#[derive(SlashCommand)]
#[command("cfg (set|get)", locales = "tests/fixtures/i18n/cmd")]
struct Cfg;

impl Cfg {
    #[handler]
    async fn handle(self, ctx: CommandCtx, path: CfgPath) -> HandlerResult<Responded> {
        ctx.reply_text(path.as_str()).await
    }
}

#[derive(UserCommand)]
#[user_command("Show profile", key = "show_profile")]
struct ShowProfile;

impl ShowProfile {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Responded> {
        let _ = target;
        ctx.reply_text("profile").await
    }
}

#[derive(MessageCommand)]
#[message_command("Quote message", key = "quote_message")]
struct QuoteMessage;

impl QuoteMessage {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedMessage) -> HandlerResult<Responded> {
        let _ = target;
        ctx.reply_text("quoted").await
    }
}

#[derive(SlashCommand)]
#[command("ping", locales = "tests/fixtures/i18n/cmd_gb")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text("pong").await
    }
}

#[derive(SlashCommand)]
#[command("clash", locales = "tests/fixtures/i18n/cmd_clash")]
struct Clash {
    #[option(description = "Who")]
    user: Option<ResolvedUser>,
    reason: Option<String>,
}

impl Clash {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let _ = (self.user, self.reason);
        ctx.reply_text("clash").await
    }
}

fn module() -> Module {
    Module::new("mod")
        .add::<Ban>()
        .add::<SetupStart>()
        .add::<ShowProfile>()
        .locales(strings::catalog())
}

fn builder(module: Module) -> AppBuilder {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test()).module(module);
    builder
}

fn commands(module: Module) -> Vec<Value> {
    let app = builder(module).build().unwrap();
    AppProbe::of(&app)
        .commands()
        .into_iter()
        .map(|(_, command)| command)
        .collect()
}

/// Rows 1-2, 6, 11-12, 15, 20: the Publish JSON carries the file's texts
/// and the localizations.
#[tokio::test]
async fn publish_json_is_localized() {
    let json = commands(module());
    assert_eq!(
        json,
        [
            json!({
                "type": 1, "name": "mod",
                "description": "Moderation tools",
                "description_localizations": {"en-GB": "Moderation tools", "en-US": "Moderation tools"},
                "options": [{
                    "type": 1, "name": "ban", "name_localizations": {"tr": "engelle"},
                    "description": "Ban a member",
                    "description_localizations": {
                        "en-GB": "Ban a member", "en-US": "Ban a member", "tr": "Bir üyeyi yasakla"
                    },
                    "options": [
                        {"type": 6, "name": "user", "name_localizations": {"tr": "kullanici"},
                         "description": "Who to ban",
                         "description_localizations": {
                            "en-GB": "Who to ban", "en-US": "Who to ban", "tr": "Kim yasaklanacak"
                         }},
                        {"type": 3, "name": "color", "description": "The ban color",
                         "description_localizations": {"en-GB": "The ban color", "en-US": "The ban color"},
                         "choices": [
                            {"name": "Red", "name_localizations": {"tr": "Kırmızı"}, "value": "Red"},
                            {"name": "Blue", "value": "Blue"},
                         ]},
                    ],
                }],
            }),
            json!({
                "type": 1, "name": "setup",
                "description": "Set the bot up",
                "description_localizations": {"en-GB": "Set the bot up", "en-US": "Set the bot up"},
                "options": [{
                    "type": 1, "name": "start", "description": "Start the setup",
                    "description_localizations": {"en-GB": "Start the setup", "en-US": "Start the setup"},
                    "options": [],
                }],
            }),
            json!({
                "type": 2, "name": "Show profile",
                "name_localizations": {
                    "en-GB": "Show profile", "en-US": "Show profile", "tr": "Profili göster"
                },
            }),
        ]
    );
}

/// Row 15: a keyed `#[message_command]` takes `ctxcmd.<key>.name`.
#[tokio::test]
async fn keyed_message_menu_is_localized() {
    let json = commands(
        Module::new("quote")
            .add::<QuoteMessage>()
            .locales(strings::catalog()),
    );
    assert_eq!(
        json,
        [json!({
            "type": 3, "name": "Quote message",
            "name_localizations": {
                "en-GB": "Quote message", "en-US": "Quote message", "tr": "Mesajı alıntıla"
            },
        })]
    );
}

/// Matrix M6: a Pattern's expansions publish their own file descriptions.
#[tokio::test]
async fn pattern_paths_take_their_own_descriptions() {
    let json = commands(Module::new("cfg").add::<Cfg>().locales(strings::catalog()));
    assert_eq!(json[0]["description"], "Configuration");
    let subs: Vec<(&str, &str)> = json[0]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["name"].as_str().unwrap(),
                o["description"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(subs, [("set", "Set a value"), ("get", "Get a value")]);
}

/// Row 7: an en-GB file serves en-GB; en-US keeps its own text.
#[tokio::test]
async fn en_gb_file_overrides_its_sibling() {
    let json = commands(
        Module::new("ping")
            .add::<Ping>()
            .locales(gb_strings::catalog()),
    );
    assert_eq!(json[0]["description"], "Pong");
    assert_eq!(
        json[0]["description_localizations"],
        json!({"en-GB": "Pong (GB)", "en-US": "Pong"})
    );
}

/// Row 34: a localized option name equal to another option's default
/// fails the build as LOC-03.
#[tokio::test]
async fn colliding_option_name_fails_the_build() {
    let module = Module::new("clash")
        .add::<Clash>()
        .locales(clash_strings::catalog());
    let violations = builder(module).build().unwrap_err().into_violations();
    let origin = clash_strings::catalog().origin();
    assert!(
        matches!(
            violations.as_slice(),
            [BuildViolation::CommandLocalization {
                rule: RuleId::LOC_03,
                locale: Locale::Tr,
                key: "cmd.clash.opt.reason.name",
                origin: o,
                ..
            }] if *o == origin
        ),
        "{violations:?}"
    );
}

/// Row 42 (LOC-06): dispatch routes on default names; a localized name is
/// no command.
#[tokio::test(flavor = "multi_thread")]
async fn dispatch_uses_default_names() {
    let mut builder = builder(module());
    let app = TestApp::build(&mut builder).await.unwrap();
    let ran = app.command("mod ban").locale("tr").run().await.unwrap();
    assert!(ran.text().contains("banned"), "{}", ran.text());
    assert!(matches!(
        app.command("mod engelle").run().await,
        Err(TestError::UnknownCommand { .. })
    ));
    assert!(matches!(
        app.user_command("Profili göster").run().await,
        Err(TestError::UnknownCommand { .. })
    ));
    app.stop().await.unwrap();
}
