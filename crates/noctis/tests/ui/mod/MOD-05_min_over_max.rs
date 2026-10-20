use noctis::prelude::*;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

#[derive(Modal)]
#[modal(route = Form, title = "T")]
struct F {
    #[text_input("Reason", min_length = 10, max_length = 5)]
    reason: String,
}

fn main() {}
