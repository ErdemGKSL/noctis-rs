use noctis::ui::{Container, LinkButton};

fn main() {
    let _ = Container::new().push(LinkButton::new("https://x.y"));
}
