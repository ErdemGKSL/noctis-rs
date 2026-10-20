use noctis::prelude::*;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

#[derive(Modal)]
#[modal(route = Form, title = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
struct F {
    #[checkbox("Agree")]
    agree: bool,
}

fn main() {}
