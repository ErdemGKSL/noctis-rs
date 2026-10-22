//! Story 11.1 Matrix 36 and 64: an App built with `.locales(catalog())`
//! answers a `tr` user with the `tr` text of a `defaults(label = keys::X)`
//! button, and a `noctis.expired` override in `tr.yaml` replaces the
//! built-in notice, through `TestApp`.

#![cfg(all(feature = "i18n", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::prelude::*;
use noctis::testkit::{ResponseKind, TestApp};
use noctis::ui::{ActionRow, Components, TextDisplay, V2Message};

mod strings {
    noctis::i18n!(dir = "tests/fixtures/i18n/app");
}

use strings::keys;

#[derive(Button)]
#[button("shop.save", defaults(style = Primary, label = keys::SHOP_SAVE))]
struct Save {
    item: u64,
}

impl Save {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(view(self.item + 1)).await
    }
}

#[derive(Button)]
#[button("shop.once", by_ref, one_shot, defaults(style = Secondary, label = keys::SHOP_SAVE))]
struct Once {
    item: u64,
}

impl Once {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(view(self.item)).await
    }
}

fn view(item: u64) -> V2Message {
    V2Message::new(
        Components::new()
            .push(TextDisplay::new(format!("Item {item}")))
            .push(ActionRow::buttons([Save { item }.build()]))
            .push(ActionRow::buttons([Once { item }.build()])),
    )
}

#[derive(SlashCommand)]
#[command("shop", description = "Open the shop")]
struct Shop;

impl Shop {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(view(1)).await
    }
}

async fn app() -> TestApp {
    let mut builder = App::builder();
    builder
        .module(
            Module::new("shop")
                .add::<Shop>()
                .add::<Save>()
                .add::<Once>(),
        )
        .locales(strings::catalog());
    TestApp::build(&mut builder).await.unwrap()
}

/// Matrix 64: the label resolves in the response locale.
#[tokio::test(flavor = "multi_thread")]
async fn default_label_keys_resolve_per_user_locale() {
    let app = app().await;
    let en = app.command("shop").run().await.unwrap();
    let labels: Vec<_> = en
        .buttons()
        .iter()
        .map(|b| b.label().map(str::to_owned))
        .collect();
    assert_eq!(labels, [Some("Save".to_owned()), Some("Save".to_owned())]);
    let tr = app
        .click(en.component::<Save>().unwrap())
        .locale("tr")
        .run()
        .await
        .unwrap();
    assert_eq!(tr.kind(), ResponseKind::Update);
    assert!(tr.text().contains("Item 2"), "{}", tr.text());
    assert_eq!(tr.button("Kaydet").unwrap().route(), Some("shop.save"));
    app.stop().await.unwrap();
}

/// Matrix 36: a consumed one-shot Ref answers with the `tr` override of
/// `noctis.expired`; `de` keeps the built-in text.
#[tokio::test(flavor = "multi_thread")]
async fn noctis_overrides_replace_the_builtin_notice() {
    let app = app().await;
    let first = app.command("shop").run().await.unwrap();
    let once = first.component::<Once>().unwrap();
    app.click(once.clone()).run().await.unwrap();
    let tr = app.click(once.clone()).locale("tr").run().await.unwrap();
    assert!(tr.text().contains("Panelin süresi doldu"), "{}", tr.text());
    let de = app.click(once).locale("de").run().await.unwrap();
    assert!(
        de.text().contains("This interaction has expired"),
        "{}",
        de.text()
    );
    app.stop().await.unwrap();
}
