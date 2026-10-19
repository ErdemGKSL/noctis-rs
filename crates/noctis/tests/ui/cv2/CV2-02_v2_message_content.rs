use noctis::ui::{Components, V2Message};

fn main() {
    let _ = V2Message::new(Components::new()).content("hi"); // [CV2-02]
}
