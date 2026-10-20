use noctis::prelude::*;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

#[derive(Modal)]
#[modal(route = Form, title = "Six")]
struct F {
    #[checkbox("A")]
    a: bool,
    #[checkbox("B")]
    b: bool,
    #[checkbox("C")]
    c: bool,
    #[checkbox("D")]
    d: bool,
    #[checkbox("E")]
    e: bool,
    #[checkbox("F")]
    f: bool,
}

fn main() {}
