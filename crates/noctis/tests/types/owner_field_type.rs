use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{GuildMarker, UserMarker};

#[derive(Button)]
#[button("a", owner = user, defaults(style = Secondary, label = "Go"))]
struct Maybe {
    user: Option<Id<UserMarker>>,
}

impl Maybe {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("b", owner = guild, defaults(style = Secondary, label = "Go"))]
struct Guild {
    guild: Id<GuildMarker>,
}

impl Guild {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("c", owner = user, defaults(style = Secondary, label = "Go"))]
struct Raw {
    user: u64,
}

impl Raw {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
