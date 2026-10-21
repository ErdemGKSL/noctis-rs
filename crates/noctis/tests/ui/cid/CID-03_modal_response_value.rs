use noctis::prelude::*;

#[derive(Modal)]
#[modal("a", response = edit, defaults(title = "Bio"))]
struct Edit {
    #[text_input("Bio")]
    bio: String,
}

impl Edit {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Modal)]
#[modal("b", auto_defer = 0, defaults(title = "Bio"))]
struct Zero {
    #[text_input("Bio")]
    bio: String,
}

impl Zero {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Modal)]
#[modal("c", response = update, response = public, defaults(title = "Bio"))]
struct Twice {
    #[text_input("Bio")]
    bio: String,
}

impl Twice {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
