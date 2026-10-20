use noctis::prelude::*;
use noctis::ui::CallbackFlags;

#[command("ping", description = "d")]
async fn ping(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
    Ok(r.defer_reply_as(CallbackFlags::EPHEMERAL).await?.done()) // [INT-08]
}

fn main() {}
