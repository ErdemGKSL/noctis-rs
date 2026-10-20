use noctis::prelude::*;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

#[derive(Choices, Clone, Copy)]
enum One {
    #[choice("Only")]
    Only,
}

#[derive(Modal)]
#[modal(route = Form, title = "T")]
struct F {
    #[radio_group("Pick")]
    pick: One,
}

fn main() {}
