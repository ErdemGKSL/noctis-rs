use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d")]
    reason: Option<String>,
}

#[message_command("Report")]
async fn report(_ctx: CommandCtx, _target: noctis::ui::ResolvedMessage, _o: O) -> HandlerResult<()> { // [CTX-02]
    Ok(())
}

fn main() {}
