use noctis::ui::{ActionRow, LinkButton};

fn main() {
    let b = || LinkButton::new("https://x.y");
    let _ = ActionRow::buttons([b(), b(), b(), b(), b(), b()]);
}
