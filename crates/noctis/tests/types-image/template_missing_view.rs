// A derived Template needs a `TemplateView` impl.
use noctis::image::Template;
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "card", version = 1, size = (10, 10), format = Png)]
struct Card {
    name: String,
}

fn main() {}
