// IMG-03: an `#[asset]` field declares remote images, so it is a
// `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>`; any other type
// is a compile error naming the rule.
use noctis::image::{Frame, Node, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(noctis::image::Template, Serialize, Deserialize)]
#[template(id = "card", version = 1, size = (64, 64), format = Png)]
struct Card {
    #[asset]
    avatar: String,
}

impl TemplateView for Card {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="w-full h-full" /> }
    }
}

fn main() {}
