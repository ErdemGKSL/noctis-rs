//! Story 20.4: application emojis under `TestApp`. Every declared emoji
//! resolves to a stable fake id without a request; `app_emojis(names)`
//! limits which exist; `button_by_emoji` finds a button by the declared
//! name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::emoji::{AppEmojis, EmojiDecl, EmojiFormat, EmojiImage};
use noctis::prelude::*;
use noctis::ui::{ActionRow, Button as AnyButton, ButtonStyle, Components, TextDisplay, V2Message};
use noctis_testkit::{TestApp, fake_emoji_id};

const PNG: EmojiImage = EmojiImage::File {
    bytes: b"\x89PNG\r\n\x1a\n",
    format: EmojiFormat::Png,
};

#[derive(Clone, Copy, Debug)]
enum Icons {
    Check,
    Cross,
}

impl AppEmojis for Icons {
    const DECLARATIONS: &'static [EmojiDecl] = &[
        EmojiDecl::__new("check", PNG).__white("check_white", PNG),
        EmojiDecl::__new("cross", PNG),
    ];

    fn declaration(self) -> &'static EmojiDecl {
        &Self::DECLARATIONS[self as usize]
    }
}

#[derive(Button)]
#[button("emo.ok", defaults(style = Success, label = "OK"))]
struct Confirm;

impl Confirm {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
        Ok(Components::new().push(TextDisplay::new("ok")))
    }
}

#[derive(Button)]
#[button("emo.no", defaults(style = Secondary, label = "No"))]
struct No;

impl No {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
        Ok(Components::new().push(TextDisplay::new("no")))
    }
}

#[derive(SlashCommand)]
#[command("status", description = "The status")]
struct Status;

impl Status {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let emojis = ctx.emojis();
        let view = V2Message::new(
            Components::new()
                .push(TextDisplay::new(format!(
                    "{} ready{}",
                    emojis.text(Icons::Check),
                    emojis.text(Icons::Cross)
                )))
                .push(ActionRow::buttons([
                    AnyButton::from(
                        Confirm
                            .build()
                            .emoji(emojis.on(ButtonStyle::Success, Icons::Check)),
                    ),
                    AnyButton::from(No.build().emoji(emojis.get(Icons::Cross))),
                ])),
        );
        ctx.respond(view).await
    }
}

async fn app() -> TestApp {
    let mut builder = App::builder();
    builder.emojis::<Icons>().module(
        Module::new("emo")
            .add::<Status>()
            .add::<Confirm>()
            .add::<No>(),
    );
    TestApp::build(&mut builder).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_declared_emoji_resolves_without_a_request() {
    let app = app().await;
    let reply = app.command("status").run().await.unwrap();
    let check = fake_emoji_id("check");
    assert_eq!(
        reply.text(),
        format!("<:check:{check}> ready<:cross:{}>", fake_emoji_id("cross"))
    );
    // The Success button shows the white variant; the declared name finds it.
    let ok = reply.button_by_emoji("check").unwrap();
    assert_eq!(ok.label(), Some("OK"));
    assert_eq!(ok.emoji(), Some("check_white"));
    assert_eq!(reply.button_by_emoji("cross").unwrap().label(), Some("No"));
    assert!(
        reply
            .http_calls()
            .iter()
            .all(|call| !call.path().contains("/emojis")),
        "{:?}",
        reply.http_calls()
    );
    app.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn app_emojis_limits_which_names_exist() {
    let app = app().await.app_emojis(["check"]);
    let reply = app.command("status").run().await.unwrap();
    assert_eq!(
        reply.text(),
        format!("<:check:{}> ready", fake_emoji_id("check"))
    );
    // `check_white` is missing: the coloured variant on the Success button.
    assert_eq!(
        reply.button_by_emoji("check").unwrap().emoji(),
        Some("check")
    );
    // `cross` is missing: the button has no emoji.
    let buttons = reply.buttons();
    assert_eq!(buttons[1].label(), Some("No"));
    assert_eq!(buttons[1].emoji(), None);
    assert!(reply.button_by_emoji("cross").is_err());
    app.stop().await.unwrap();
}
