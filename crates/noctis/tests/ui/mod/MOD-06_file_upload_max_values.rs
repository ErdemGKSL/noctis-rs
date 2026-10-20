use noctis::prelude::*;

#[derive(Route)]
#[route("t.form")]
struct Form;
impl noctis::__private::ui::Handled for Form {}

#[derive(Modal)]
#[modal(route = Form, title = "T")]
struct F {
    #[file_upload("Files", max_values = 11)]
    files: Vec<noctis::ui::ResolvedAttachment>,
}

fn main() {}
