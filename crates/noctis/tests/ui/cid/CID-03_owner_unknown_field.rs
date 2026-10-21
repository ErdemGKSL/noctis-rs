use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;

#[derive(Button)]
#[button("a", owner = usr, defaults(style = Secondary, label = "Go"))]
struct Owned {
    user: Id<UserMarker>,
    page: u8,
}

impl Owned {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
