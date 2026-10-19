use noctis::ui::File;

fn main() {
    let _ = File::new("attachment://x.png"); // [CV2-12]
}
