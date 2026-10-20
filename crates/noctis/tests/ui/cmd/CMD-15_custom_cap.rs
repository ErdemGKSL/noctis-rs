use noctis::prelude::*;

#[command("a (b|c) (d|e)", description = "d", max_expansions = 3)]
async fn h(_ctx: CommandCtx, _path: HPath) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
