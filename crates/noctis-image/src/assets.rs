//! Static images (IMG-02): [`StaticImage`] handles over `&'static` PNG,
//! JPEG or WebP bytes, registered per App with `ImageConfig::image` and
//! drawn with `<img src={..} />`; [`StaticImages`] decodes them once at
//! install.

use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;

use takumi::prelude::ImageSource;

use crate::fonts::digest_prefix;

/// The longest static image id, in bytes.
const MAX_ID_BYTES: usize = 64;
/// The most pixels a static image may have: 16 MP (4096 x 4096).
pub(crate) const MAX_PIXELS: u64 = 16_777_216;

/// Whether `id` matches `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` and is at most 64
/// bytes.
pub(crate) const fn valid_image_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_ID_BYTES {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let edge = i == 0 || i == bytes.len() - 1;
        let alnum = b.is_ascii_lowercase() || b.is_ascii_digit();
        if !(alnum || (!edge && matches!(b, b'.' | b'_' | b'-'))) {
            return false;
        }
        i += 1;
    }
    true
}

/// A static image for image Templates: an id plus `&'static` PNG, JPEG or
/// WebP bytes (usually `include_bytes!`), drawn with `<img src={..} />`
/// once registered with `ImageConfig::image`.
///
/// [`new`](Self::new) is a `const fn`: in a `const` item an invalid id is a
/// compile error (IMG-02). The bytes are checked when the App builds. Two
/// handles are equal when their ids and bytes are. `Debug` never prints
/// the bytes, only their length and digest prefix.
///
/// ```
/// use noctis_image::StaticImage;
///
/// const LOGO: StaticImage = StaticImage::new("logo.v2", b"\x89PNG\r\n\x1a\n");
/// assert_eq!(LOGO.id(), "logo.v2");
/// ```
///
/// An id must match `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` and be at most 64
/// bytes:
///
/// ```compile_fail
/// use noctis_image::StaticImage;
///
/// const LOGO: StaticImage = StaticImage::new("Logo", b"");
/// ```
#[derive(Clone, Copy)]
pub struct StaticImage {
    id: &'static str,
    bytes: &'static [u8],
}

impl StaticImage {
    /// A static image `id` over `bytes`.
    ///
    /// # Panics
    ///
    /// When `id` does not match `[a-z0-9]([a-z0-9._-]*[a-z0-9])?` or is over
    /// 64 bytes; in a `const` item that is a compile error.
    pub const fn new(id: &'static str, bytes: &'static [u8]) -> Self {
        assert!(
            valid_image_id(id),
            "[IMG-02] static image id must match [a-z0-9]([a-z0-9._-]*[a-z0-9])? and be at most \
             64 bytes"
        );
        Self { id, bytes }
    }

    /// The image's id.
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Whether `other` holds the same bytes (by address first).
    fn same_bytes(&self, other: &[u8]) -> bool {
        std::ptr::eq(self.bytes, other) || self.bytes == other
    }
}

impl PartialEq for StaticImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.same_bytes(other.bytes)
    }
}

impl Eq for StaticImage {}

impl fmt::Debug for StaticImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StaticImage")
            .field("id", &self.id)
            .field("len", &self.bytes.len())
            .field(
                "blake3",
                &digest_prefix(blake3::hash(self.bytes).as_bytes()),
            )
            .finish()
    }
}

/// A static image format, sniffed by its magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

/// The format `bytes` start with; `None` for anything but PNG, JPEG and
/// WebP (GIF, SVG, BMP, ICO and empty input included).
pub(crate) fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

/// Whether a `width` x `height` image is within the 16 MP cap.
pub(crate) fn check_pixels(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) <= MAX_PIXELS
}

/// The decoded static images of one App, by id.
#[derive(Default)]
pub(crate) struct StaticImages {
    by_id: HashMap<&'static str, (StaticImage, ImageSource)>,
}

impl StaticImages {
    /// Decodes every image of `images` once; a repeated handle is
    /// idempotent.
    ///
    /// # Errors
    ///
    /// The `[IMG-02]` message of the first image that is unsupported,
    /// corrupt, over 16 MP, or whose id is registered twice with different
    /// bytes.
    pub(crate) fn decode(images: &[StaticImage]) -> Result<Self, String> {
        let mut by_id = HashMap::with_capacity(images.len());
        for image in images {
            if let Some((known, _)) = by_id.get(image.id) {
                if known == image {
                    continue;
                }
                return Err(format!(
                    "[IMG-02] static image id `{}` is registered twice with different bytes",
                    image.id
                ));
            }
            by_id.insert(image.id, (*image, decode(image)?));
        }
        Ok(Self { by_id })
    }

    /// The decoded source of `image`; `None` when its id is not registered
    /// or was registered with other bytes.
    pub(crate) fn get(&self, image: &StaticImage) -> Option<&ImageSource> {
        self.by_id
            .get(image.id)
            .filter(|(known, _)| known.same_bytes(image.bytes))
            .map(|(_, source)| source)
    }
}

impl fmt::Debug for StaticImages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ids: Vec<&str> = self.by_id.keys().copied().collect();
        ids.sort_unstable();
        f.debug_struct("StaticImages").field("ids", &ids).finish()
    }
}

/// Sniffs, size-checks and decodes one image.
pub(crate) fn decode(image: &StaticImage) -> Result<ImageSource, String> {
    let id = image.id;
    if sniff(image.bytes).is_none() {
        return Err(format!(
            "[IMG-02] static image `{id}`: unsupported format: use PNG, JPEG or WebP"
        ));
    }
    let corrupt = |why: &dyn fmt::Display| {
        format!("[IMG-02] static image `{id}` could not be decoded: {why}")
    };
    let (width, height) = image::ImageReader::new(Cursor::new(image.bytes))
        .with_guessed_format()
        .map_err(|err| corrupt(&err))?
        .into_dimensions()
        .map_err(|err| corrupt(&err))?;
    if !check_pixels(width, height) {
        return Err(format!(
            "[IMG-02] static image `{id}` is {width}x{height}: at most {MAX_PIXELS} pixels"
        ));
    }
    ImageSource::from_bytes(image.bytes).map_err(|err| corrupt(&err))
}

#[cfg(test)]
mod tests;
