use noctis::prelude::*;
use noctis::ui::Modal;

fn form() -> Modal {
    unimplemented!()
}

#[command("feedback", description = "d")]
async fn feedback(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
    Ok(r.show_modal(form()).await?.done())
}

fn main() {}
