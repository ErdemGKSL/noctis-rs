use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[derive(Button)]
#[button("profile.edit", auto_defer = false, defaults(style = Secondary, label = "Edit"))]
struct EditBio;

impl EditBio {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
        Ok(Components::new().push(TextDisplay::new("hi")))
    }
}

fn main() {}
