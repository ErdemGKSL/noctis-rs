use noctis::prelude::*;
use noctis::ui::Modal;

fn form() -> Modal {
    unimplemented!()
}

#[command("feedback", description = "d")]
async fn feedback(_ctx: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded> {
    Ok(r.modal(form()).await?.done())
}

fn main() {}
