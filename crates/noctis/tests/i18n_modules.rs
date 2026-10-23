//! Story 11.2 Matrix 1, 3 and 15: Modules that ship their own `i18n!`
//! catalogs through `Module::locales` merge into one App-wide set, and a key
//! two Modules define fails the build naming both Modules, through
//! `TestApp`.

#![cfg(all(feature = "i18n", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::prelude::*;
use noctis::testkit::{TestApp, TestError};
use noctis::{BuildViolation, LocaleOwner};

mod shop_strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/mod_shop");
}

mod admin_strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/mod_admin");
}

mod clash_strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/mod_clash");
}

#[derive(SlashCommand)]
#[command("shop", description = "Open the shop")]
struct ShopCmd;

impl ShopCmd {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text(shop_strings::keys::SHOP_TITLE).await
    }
}

/// Sends a key that only the `shop` Module's catalog defines.
#[derive(SlashCommand)]
#[command("admin", description = "Admin tools")]
struct AdminCmd;

impl AdminCmd {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text(shop_strings::keys::SHOP_TITLE).await
    }
}

/// Sends a key that only the `admin` Module's catalog defines.
#[derive(SlashCommand)]
#[command("done", description = "Mark done")]
struct DoneCmd;

impl DoneCmd {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.reply_text(admin_strings::keys::ADMIN_DONE).await
    }
}

fn shop() -> Module {
    Module::new("shop")
        .add::<ShopCmd>()
        .add::<DoneCmd>()
        .locales(shop_strings::catalog())
}

/// Matrix 1 and 15 (AC 1, 3): the `admin` Module's handler resolves the
/// `shop` Module's key in the response locale, and the `shop` Module's
/// handler resolves the `admin` Module's key.
#[tokio::test(flavor = "multi_thread")]
async fn module_catalogs_resolve_app_wide() {
    let mut builder = App::builder();
    builder.module(shop()).module(
        Module::new("admin")
            .add::<AdminCmd>()
            .locales(admin_strings::catalog()),
    );
    let app = TestApp::build(&mut builder).await.unwrap();
    let tr = app.command("admin").locale("tr").run().await.unwrap();
    assert!(tr.text().contains("Dükkan"), "{}", tr.text());
    let en = app.command("admin").run().await.unwrap();
    assert!(en.text().contains("Shop"), "{}", en.text());
    let own = app.command("shop").locale("tr").run().await.unwrap();
    assert!(own.text().contains("Dükkan"), "{}", own.text());
    let done_tr = app.command("done").locale("tr").run().await.unwrap();
    assert!(done_tr.text().contains("Done"), "{}", done_tr.text());
    let done = app.command("done").run().await.unwrap();
    assert!(done.text().contains("Done"), "{}", done.text());
    app.stop().await.unwrap();
}

/// Matrix 3 (AC 2): two Modules defining `shop.title` fail the build with a
/// violation naming the key, both origins and both Modules.
#[tokio::test(flavor = "multi_thread")]
async fn module_key_clash_names_both_modules() {
    let mut builder = App::builder();
    builder.module(shop()).module(
        Module::new("admin")
            .add::<AdminCmd>()
            .locales(clash_strings::catalog()),
    );
    let Err(TestError::Build(err)) = TestApp::build(&mut builder).await else {
        panic!("the build must fail");
    };
    let shop_origin = shop_strings::catalog().origin();
    let clash_origin = clash_strings::catalog().origin();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicateLocaleKey {
            key: "shop.title",
            first: shop_origin,
            second: clash_origin,
            first_owner: LocaleOwner::Module("shop"),
            second_owner: LocaleOwner::Module("admin"),
        }]
    );
    assert_eq!(
        err.violations()[0].to_string(),
        format!(
            "locale key `shop.title` is defined by two catalogs: `{shop_origin}` (Module `shop`) \
             and `{clash_origin}` (Module `admin`)"
        )
    );
}
