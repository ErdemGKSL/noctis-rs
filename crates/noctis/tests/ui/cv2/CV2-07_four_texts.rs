use noctis::ui::{LinkButton, Section, TextDisplay};

fn main() {
    let t = || TextDisplay::new("a");
    let _ = Section::new([t(), t(), t(), t()], LinkButton::new("https://x.y"));
}
