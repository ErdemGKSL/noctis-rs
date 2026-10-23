//! Story 11.3 AC 1 and AC 3 (rows 2, 4-6, 29 and 36): User and Guild
//! Locale resolution against an App's locale files, an App
//! `LocaleResolver` and a before-Middleware override, through `TestApp`.

#![cfg(all(feature = "i18n", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::prelude::*;
use noctis::rules::Locale;
use noctis::testkit::{TestApp, TestGuild};
use noctis::twilight::model::id::Id;
use noctis::ui::{Components, TextDisplay, V2Message};
use noctis::{LocaleRequest, Locales};

mod strings {
    noctis::i18n!(default = "tr", dir = "tests/fixtures/i18n/resolve");
}

use strings::keys;

/// The en-US guild of the tests.
const EN_GUILD: u64 = 9;
/// The guild a `LocaleResolver` maps to pt-BR.
const PT_GUILD: u64 = 7;

#[derive(SlashCommand)]
#[command("where", description = "Show the resolved locales")]
struct Where;

impl Where {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let text = format!(
            "{} {} {}",
            ctx.user_locale(),
            ctx.guild_locale(),
            ctx.response_locale()
        );
        ctx.respond(V2Message::new(
            Components::new().push(TextDisplay::new(text)),
        ))
        .await
    }
}

#[derive(SlashCommand)]
#[command("hi", description = "Greet the channel")]
struct Hi;

impl Hi {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text(keys::GREET_HI).public().await
    }
}

/// An App over the `resolve` fixtures (`default = "tr"`, en-US and tr
/// files), with `configure` applied.
async fn app(module: Module, configure: impl FnOnce(&mut AppBuilder)) -> TestApp {
    let mut builder = App::builder();
    builder.module(module).locales(strings::catalog());
    configure(&mut builder);
    let app = TestApp::build(&mut builder).await.unwrap();
    app.guild(TestGuild::new(Id::new(EN_GUILD), "en").locale("en-US"))
        .guild(TestGuild::new(Id::new(PT_GUILD), "pt").locale("en-US"))
}

/// The text `/where` answers `locale` with, in `guild` (a DM for `None`).
async fn where_as(app: &TestApp, locale: &str, guild: Option<u64>) -> String {
    let mut run = app.command("where").locale(locale);
    if let Some(guild) = guild {
        run = run.in_guild(Id::new(guild));
    }
    run.run().await.unwrap().text()
}

/// AC 1 (rows 2, 4, 5 and 6): `en-GB` takes `en-US`'s file, `pt-BR` has no
/// sibling and falls back to the default `tr`, a `tr` user in an `en-US`
/// guild keeps both.
#[tokio::test(flavor = "multi_thread")]
async fn where_resolves_user_guild_and_response() {
    let app = app(Module::new("loc").add::<Where>(), |_| {}).await;
    assert_eq!(where_as(&app, "en-GB", None).await, "en-US en-US en-US");
    assert_eq!(where_as(&app, "pt-BR", None).await, "tr tr tr");
    assert_eq!(where_as(&app, "tr", Some(EN_GUILD)).await, "tr en-US en-US");
    app.stop().await.unwrap();
}

/// AC 3 (row 29): an App `LocaleResolver` maps guild 7 to `pt-BR`; a
/// public reply renders through the catalog's fallback.
#[tokio::test(flavor = "multi_thread")]
async fn resolver_overrides_the_guild_locale() {
    let module = Module::new("loc").add::<Where>().add::<Hi>();
    let app = app(module, |b| {
        b.locale_resolver(|request: &LocaleRequest| match request.guild_id {
            Some(id) if id.get() == PT_GUILD => Locales {
                guild: Locale::PtBr,
                ..request.resolved
            },
            _ => request.resolved,
        });
    })
    .await;
    assert_eq!(where_as(&app, "tr", Some(PT_GUILD)).await, "tr pt-BR pt-BR");
    assert_eq!(where_as(&app, "tr", Some(EN_GUILD)).await, "tr en-US en-US");
    let hi = app
        .command("hi")
        .locale("en-US")
        .in_guild(Id::new(PT_GUILD))
        .run()
        .await
        .unwrap();
    // No pt-BR file: the catalog falls back to its default, tr.
    assert_eq!(hi.text(), "Selam");
    app.stop().await.unwrap();
}

/// The before-Middleware of row 36: every guild's shared content is tr.
async fn turkish_guild(ctx: BeforeCtx) -> HandlerResult<Flow> {
    ctx.set_guild_locale(Locale::Tr);
    Ok(Flow::Continue)
}

/// AC 3 (row 36): a before-Middleware's `set_guild_locale(Tr)` reaches
/// the handler and a public `reply_text(key)`.
#[tokio::test(flavor = "multi_thread")]
async fn middleware_overrides_the_guild_locale() {
    let module = Module::new("loc")
        .before(Middleware::before(turkish_guild))
        .add::<Where>()
        .add::<Hi>();
    let app = app(module, |_| {}).await;
    assert_eq!(where_as(&app, "en-US", Some(EN_GUILD)).await, "en-US tr tr");
    let hi = app
        .command("hi")
        .locale("en-US")
        .in_guild(Id::new(EN_GUILD))
        .run()
        .await
        .unwrap();
    assert_eq!(hi.text(), "Selam");
    app.stop().await.unwrap();
}
