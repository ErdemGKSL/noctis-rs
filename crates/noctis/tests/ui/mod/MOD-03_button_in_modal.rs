use noctis::prelude::*;
use noctis::ui::LinkButton;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

fn main() {
    let _ = Form.modal("Form").push(LinkButton::new("https://x.y"));
}
