// `Gif`/`Apng` animated output needs the `image-gif` feature; animated WebP
// (the default `format`) needs only `image`.
use noctis::image::{Frame, Node, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "a", version = 1, size = (2, 2), format = Png,
           animation(duration_ms = 500, fps = 10, format = Gif))]
struct Blink;

impl TemplateView for Blink {
    fn view(&self, _: Frame) -> Node {
        view! { <div /> }
    }
}

#[derive(Template, Serialize, Deserialize)]
#[template(id = "b", version = 1, size = (2, 2), format = Png,
           animation(duration_ms = 500, fps = 10))]
struct Spin;

impl TemplateView for Spin {
    fn view(&self, _: Frame) -> Node {
        view! { <div /> }
    }
}

fn main() {}
