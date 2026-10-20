use noctis::prelude::*;
use noctis::ui::{ModalLabel, SelectOption};

#[derive(Route)]
#[route("t.pick", select)]
struct Pick;
impl noctis::__private::ui::Handled for Pick {}

fn main() {
    let _ = ModalLabel::new("L", Pick.string_select([SelectOption::new("a", "a")]));
}
