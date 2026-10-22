// ATT-07: Template output formats are a closed set; an animated format is not
// a still `format`.
use noctis::image::{Frame, Node, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "card", version = 1, size = (10, 10), format = Gif)] // [ATT-07]
struct Card;

impl TemplateView for Card {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div/> }
    }
}

fn main() {}
