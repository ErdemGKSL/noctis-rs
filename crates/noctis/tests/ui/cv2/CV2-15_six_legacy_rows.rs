use noctis::ui::{ActionRow, LegacyMessage, LinkButton};

fn main() {
    let row = ActionRow::buttons([LinkButton::new("https://a.b")]);
    let _ = LegacyMessage::new("x").rows([
        row.clone(),
        row.clone(),
        row.clone(),
        row.clone(),
        row.clone(),
        row,
    ]);
}
