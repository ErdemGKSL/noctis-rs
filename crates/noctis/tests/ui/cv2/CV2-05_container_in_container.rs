use noctis::ui::Container;

fn main() {
    let _ = Container::new().push(Container::new());
}
