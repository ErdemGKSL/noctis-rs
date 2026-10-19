use noctis::ui::{Components, V2Message};

fn main() {
    let _ = V2Message::new(Components::new()).mentions(None); // [MSG-05]
}
