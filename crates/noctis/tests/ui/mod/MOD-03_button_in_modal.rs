use noctis::prelude::*;
use noctis::ui::LinkButton;

#[derive(Button)]
#[button("t.form", defaults(style = Secondary, label = "Go"))]
struct Form;

impl Form {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = noctis::__private::ui::__macro::build_modal::<Form>(Form, "Form").push(LinkButton::new("https://x.y"));
}
