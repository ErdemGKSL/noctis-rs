use noctis::prelude::*;

#[derive(Route)]
#[route("t.press")]
struct Press {
    n: u8,
}
impl noctis::__private::ui::Handled for Press {}

fn main() {
    let _ = components! { row [ Press { n: 1 }.button(), Press { n: 1 }.button("x") ] };
}
