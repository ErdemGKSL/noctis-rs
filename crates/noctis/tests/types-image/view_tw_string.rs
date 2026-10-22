// `tw = {expr}` takes `Classes`, never a `String` (user data is text only).
use noctis::image::{Node, view};

fn main() {
    let name = String::from("flex");
    let _: Node = view! { <div tw={name}></div> };
}
