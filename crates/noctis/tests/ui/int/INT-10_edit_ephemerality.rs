use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

fn msg() -> Components {
    Components::new().push(TextDisplay::new("hi"))
}

#[command("ping", description = "d")]
async fn ping(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
    let acked = r.reply(msg()).await?;
    acked.edit_reply(msg(), Ephemerality::Ephemeral).await?; // [INT-10]
    Ok(acked.done())
}

fn main() {}
