// `plays = 0` does not compile: an animation plays at least once
// (`Plays::Times` holds a `NonZeroU16`).
use noctis::image::{Frame, Node, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "a", version = 1, size = (2, 2), format = Png,
           animation(duration_ms = 500, fps = 10, plays = 0))]
struct Never;

impl TemplateView for Never {
    fn view(&self, _: Frame) -> Node {
        view! { <div /> }
    }
}

fn main() {}
