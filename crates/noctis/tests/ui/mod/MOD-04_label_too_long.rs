use noctis::prelude::*;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

#[derive(Modal)]
#[modal(route = Form, title = "T")]
struct F {
    #[checkbox("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
    agree: bool,
}

fn main() {}
