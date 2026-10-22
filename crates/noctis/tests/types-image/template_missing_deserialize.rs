// `Template` requires `Deserialize`: the Renderer decodes the value it gets.
use noctis::image::{Frame, Node, Template, TemplateView, view};
use serde::Serialize;

#[derive(Template, Serialize)]
#[template(id = "card", version = 1, size = (10, 10), format = Png)]
struct Card {
    name: String,
}

impl TemplateView for Card {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div/> }
    }
}

fn main() {}
