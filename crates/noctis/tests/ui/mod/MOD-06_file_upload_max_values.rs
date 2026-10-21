use noctis::prelude::*;

#[derive(Modal)]
#[modal("t.form", defaults(title = "T"))]
struct F {
    #[file_upload("Files", max_values = 11)]
    files: Vec<noctis::ui::ResolvedAttachment>,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
