use noctis::prelude::*;

#[command("config (set|get)", description = "d", permissions(set = MANAGE_GUILD))]
async fn config(_ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
