use noctis::prelude::*;
use noctis::ui::{ModalLabel, SelectOption};

#[derive(StringSelectMenu)]
#[string_select("t.pick", defaults(placeholder = "Pick"))]
struct Pick;

impl Pick {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = ModalLabel::new("L", Pick.build().options([SelectOption::new("a", "a")]));
}
