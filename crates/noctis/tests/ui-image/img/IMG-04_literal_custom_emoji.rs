// IMG-04: only Template data is scanned for custom emoji, so a token in
// literal `view!` text would never be fetched; it is a compile error naming
// the rule.
use noctis::image::{Frame, Node, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(noctis::image::Template, Serialize, Deserialize)]
#[template(id = "card", version = 1, size = (64, 64), format = Png)]
struct Card {}

impl TemplateView for Card {
    fn view(&self, _: Frame) -> Node {
        view! { <p>"hi <:pepe:123456789012345678>"</p> }
    }
}

fn main() {}
