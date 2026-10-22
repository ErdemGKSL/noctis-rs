use noctis::prelude::*;
use noctis::ui::StaticActionRow;

#[derive(Button)]
#[button("log.ack", defaults(style = Primary, label = "Ack"))]
struct Ack;

impl Ack {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = StaticActionRow::buttons([Ack.build()]);
}
