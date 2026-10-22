use noctis::prelude::*;
use noctis::ui::{ActionRow, StaticComponents};

#[derive(UserSelectMenu)]
#[user_select("log.pick", defaults(placeholder = "Pick"))]
struct Pick;

impl Pick {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = StaticComponents::new().push(ActionRow::select(Pick.build()));
}
