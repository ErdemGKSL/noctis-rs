//! Context-menu boundary passes (CTX-01): names exactly at the limit
//! compile.

use noctis::MenuKind;
use noctis::prelude::*;
use noctis::ui::{ResolvedMessage, ResolvedUser};

#[user_command("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
async fn ctx_01_32_chars(
    _ctx: CommandCtx,
    _target: ResolvedUser,
) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

#[message_command("😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀")]
async fn ctx_01_16_emoji(
    _ctx: CommandCtx,
    _target: ResolvedMessage,
) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

#[user_command("x")]
async fn ctx_01_one_char(
    _ctx: CommandCtx,
    _target: ResolvedUser,
) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

#[test]
fn ctx_01_names_at_the_limit() {
    assert_eq!(ctx_01_32_chars().menu_name().len(), 32);
    assert_eq!(ctx_01_16_emoji().menu_name().encode_utf16().count(), 32);
    assert_eq!(ctx_01_16_emoji().kind(), MenuKind::Message);
    assert_eq!(ctx_01_one_char().menu_name(), "x");
}
