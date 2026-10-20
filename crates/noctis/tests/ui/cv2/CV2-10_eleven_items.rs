use noctis::ui::{Media, MediaGallery, MediaItem};

fn main() {
    let i = || MediaItem::new(Media::url("https://x.y/a.png"));
    let _ = MediaGallery::new([i(), i(), i(), i(), i(), i(), i(), i(), i(), i(), i()]);
}
