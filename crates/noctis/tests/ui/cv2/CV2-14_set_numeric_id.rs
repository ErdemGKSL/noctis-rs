use noctis::ui::TextDisplay;

fn main() {
    let _ = TextDisplay::new("a").id(5); // [CV2-14]
}
