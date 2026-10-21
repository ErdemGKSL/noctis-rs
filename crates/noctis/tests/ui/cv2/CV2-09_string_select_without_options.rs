use noctis::prelude::*;

#[derive(StringSelectMenu)]
#[string_select("ticket.system", defaults(placeholder = "Pick"))]
struct TicketSystem;

impl TicketSystem {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = components! { row [ TicketSystem.build() ] };
}
