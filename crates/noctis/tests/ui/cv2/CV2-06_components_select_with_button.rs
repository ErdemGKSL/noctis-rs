use noctis::prelude::*;

#[derive(Route)]
#[route("t.press")]
struct Press {
    n: u8,
}
impl noctis::__private::ui::Handled for Press {}

#[derive(Route)]
#[route("t.pick", select)]
struct Pick;
impl noctis::__private::ui::Handled for Pick {}

fn main() {
    let _ = components! { row [ Pick.user_select(), Press { n: 1 }.button() ] };
}
