//! Context-menu boundary passes (CTX-01): names exactly at the limit
//! compile.

use noctis::MenuKind;
use noctis::prelude::*;
use noctis::ui::{ResolvedMessage, ResolvedUser};

#[derive(UserCommand)]
#[user_command("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
struct Ctx0132Chars;

impl Ctx0132Chars {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedUser,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(MessageCommand)]
#[message_command("😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀")]
struct Ctx0116Emoji;

impl Ctx0116Emoji {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedMessage,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[derive(UserCommand)]
#[user_command("x")]
struct Ctx01OneChar;

impl Ctx01OneChar {
    #[handler]
    async fn handle(
        self,
        _ctx: CommandCtx,
        _target: ResolvedUser,
    ) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

#[test]
fn ctx_01_names_at_the_limit() {
    assert_eq!(Ctx0132Chars::descriptor().menu_name().len(), 32);
    assert_eq!(
        Ctx0116Emoji::descriptor()
            .menu_name()
            .encode_utf16()
            .count(),
        32
    );
    assert_eq!(Ctx0116Emoji::descriptor().kind(), MenuKind::Message);
    assert_eq!(Ctx01OneChar::descriptor().menu_name(), "x");
}
