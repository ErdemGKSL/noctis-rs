use noctis::ui::{Components, TextDisplay, V2Message};
use noctis::webhook::ForeignWebhook;

fn mirror(hook: &ForeignWebhook) {
    let msg = V2Message::new(Components::new().push(TextDisplay::new("x")));
    let _ = hook.send(msg);
}

fn main() {
    let _ = mirror;
}
