use noctis::ui::{Components, Container, Media, Thumbnail};

fn main() {
    let thumbnail = || Thumbnail::new(Media::url("https://i/x.png"));
    let _ = Container::new().push(thumbnail());
    let _ = Components::new().push(thumbnail());
}
