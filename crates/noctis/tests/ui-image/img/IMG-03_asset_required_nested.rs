// IMG-03: `#[asset(required)]` takes a remote image field; on a field that
// holds a `#[derive(Assets)]` struct the struct's own fields decide.
use noctis::image::{Assets, Frame, Node, RemoteImage, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Assets, Serialize, Deserialize)]
struct Row {
    #[asset]
    avatar: RemoteImage,
}

#[derive(noctis::image::Template, Serialize, Deserialize)]
#[template(id = "board", version = 1, size = (64, 64), format = Png)]
struct Board {
    #[asset(required)]
    rows: Vec<Row>,
}

impl TemplateView for Board {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="w-full h-full" /> }
    }
}

fn main() {}
