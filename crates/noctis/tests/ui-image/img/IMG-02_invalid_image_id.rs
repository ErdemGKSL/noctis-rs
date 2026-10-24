// IMG-02: a static image id must match `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` and be
// at most 64 bytes; in a `const` item `StaticImage::new` rejects it at compile
// time.
use noctis::image::StaticImage;

const L: StaticImage = StaticImage::new("Logo", b"");

fn main() {
    let _ = L;
}
