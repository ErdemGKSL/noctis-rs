use noctis::prelude::*;

#[derive(Choices, Clone, Copy)]
enum Severity {
    #[choice("Low")]
    Low,
}

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "d", autocomplete)]
    a: Severity,
}

impl O {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
