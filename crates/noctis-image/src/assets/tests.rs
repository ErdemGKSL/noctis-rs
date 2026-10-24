//! `StaticImage`, `sniff`, `check_pixels` and `StaticImages` (IMG-02).

use takumi::prelude::{Bitmap, OutputFormat, Quality};

use super::*;

/// A `width` x `height` red image encoded as `format`, leaked.
fn encoded(width: u32, height: u32, format: OutputFormat) -> &'static [u8] {
    let pixels = [255, 0, 0, 255].repeat(usize::try_from(width * height).unwrap());
    let bitmap = Bitmap::from_raw(width, height, pixels).unwrap();
    let mut out = Vec::new();
    takumi::write_image(&bitmap, &mut out, format).unwrap();
    out.leak()
}

fn png() -> &'static [u8] {
    encoded(2, 2, OutputFormat::Png)
}

fn decode_err(images: &[StaticImage]) -> String {
    StaticImages::decode(images).map(|_| ()).unwrap_err()
}

#[test]
fn img_02_valid_ids() {
    const LOGO: StaticImage = StaticImage::new("logo.v2_x-1", b"");
    for id in ["a", "logo.v2_x-1", "0", &"a".repeat(64)] {
        assert!(valid_image_id(id), "{id}");
    }
    assert_eq!(LOGO.id(), "logo.v2_x-1");
}

#[test]
fn img_02_invalid_ids() {
    for id in [
        "",
        &"a".repeat(65),
        "Logo",
        "-a",
        "a-",
        ".a",
        "a_",
        "a b",
        "ä",
    ] {
        assert!(!valid_image_id(id), "{id:?}");
    }
}

#[test]
#[should_panic(
    expected = "[IMG-02] static image id must match [a-z0-9]([a-z0-9._-]*[a-z0-9])? and be at most 64 bytes"
)]
fn img_02_invalid_id_panics_at_run_time() {
    let id = String::from("Logo").leak();
    let _ = StaticImage::new(id, b"");
}

#[test]
fn img_02_sniff_accepts_png_jpeg_and_webp_only() {
    assert_eq!(sniff(png()), Some(ImageFormat::Png));
    let jpeg = encoded(
        2,
        2,
        OutputFormat::Jpeg {
            quality: Quality::new(90),
        },
    );
    assert_eq!(sniff(jpeg), Some(ImageFormat::Jpeg));
    let webp = encoded(2, 2, OutputFormat::WebPLossless);
    assert_eq!(sniff(webp), Some(ImageFormat::Webp));
    for bytes in [
        &b"GIF89a\x01\0\x01\0"[..],
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        b"BM\0\0\0\0",
        b"\0\0\x01\0\x01\0",
        b"",
        b"RIFF\0\0\0\0WAVE",
    ] {
        assert_eq!(sniff(bytes), None, "{bytes:?}");
    }
}

#[test]
fn img_02_formats_decode() {
    for format in [
        OutputFormat::Png,
        OutputFormat::Jpeg {
            quality: Quality::new(90),
        },
        OutputFormat::WebP {
            quality: Quality::new(90),
        },
        OutputFormat::WebPLossless,
    ] {
        let image = StaticImage::new("logo", encoded(3, 2, format));
        let images = StaticImages::decode(&[image]).unwrap();
        assert!(images.get(&image).is_some());
    }
}

#[test]
fn img_02_unsupported_formats_are_rejected() {
    for bytes in [
        &b"GIF89a\x01\0\x01\0\0\0\0;"[..],
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>",
        b"BM\0\0\0\0\0\0\0\0",
        b"",
    ] {
        assert_eq!(
            decode_err(&[StaticImage::new("logo", bytes)]),
            "[IMG-02] static image `logo`: unsupported format: use PNG, JPEG or WebP"
        );
    }
}

#[test]
fn img_02_corrupt_image_is_rejected() {
    let full = png();
    let truncated: &'static [u8] = full[..full.len() / 2].to_vec().leak();
    let short: &'static [u8] = &full[..10];
    for bytes in [truncated, short] {
        let error = decode_err(&[StaticImage::new("logo", bytes)]);
        assert!(
            error.starts_with("[IMG-02] static image `logo` could not be decoded: "),
            "{error}"
        );
    }
}

#[test]
fn img_02_pixel_cap() {
    assert!(check_pixels(4096, 4096));
    assert!(check_pixels(16_777_216, 1));
    assert!(!check_pixels(4097, 4096));
    assert!(!check_pixels(16_777_217, 1));
    assert!(!check_pixels(u32::MAX, u32::MAX));
    assert!(check_pixels(0, u32::MAX));
}

#[test]
fn img_02_oversized_image_is_rejected_before_decoding() {
    // A 1x1 PNG whose header claims 4097x4096 pixels.
    let mut header = encoded(1, 1, OutputFormat::Png).to_vec();
    header[16..20].copy_from_slice(&4097u32.to_be_bytes());
    header[20..24].copy_from_slice(&4096u32.to_be_bytes());
    let crc = png_crc(&header[12..29]);
    header[29..33].copy_from_slice(&crc.to_be_bytes());
    let bytes: &'static [u8] = header.leak();
    assert_eq!(
        decode_err(&[StaticImage::new("huge", bytes)]),
        "[IMG-02] static image `huge` is 4097x4096: at most 16777216 pixels"
    );
}

/// The CRC-32 a PNG chunk carries over its type and data.
fn png_crc(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn img_02_duplicates() {
    let logo = StaticImage::new("logo", png());
    let images = StaticImages::decode(&[logo, logo]).unwrap();
    assert!(images.get(&logo).is_some());
    let copy = StaticImage::new("logo", png().to_vec().leak());
    assert!(StaticImages::decode(&[logo, copy]).is_ok());
    let other = StaticImage::new("logo", encoded(3, 3, OutputFormat::Png));
    assert_eq!(
        decode_err(&[logo, other]),
        "[IMG-02] static image id `logo` is registered twice with different bytes"
    );
}

#[test]
fn img_02_lookup_checks_the_bytes() {
    let logo = StaticImage::new("logo", png());
    let images = StaticImages::decode(&[logo]).unwrap();
    assert!(images.get(&StaticImage::new("other", png())).is_none());
    let other = StaticImage::new("logo", encoded(3, 3, OutputFormat::Png));
    assert!(images.get(&other).is_none());
    assert_ne!(logo, other);
}

#[test]
fn img_02_debug_never_prints_bytes() {
    let image = StaticImage::new("logo", b"secret");
    let digest = blake3::hash(b"secret").to_hex();
    assert_eq!(
        format!("{image:?}"),
        format!(
            "StaticImage {{ id: \"logo\", len: 6, blake3: \"{}\" }}",
            &digest[..8]
        )
    );
}

#[test]
fn fr_36_versions_are_the_blake3_of_each_image() {
    let logo = StaticImage::new("logo", png());
    let other = StaticImage::new("other", encoded(3, 3, OutputFormat::Png));
    let images = StaticImages::decode(&[logo, other]).unwrap();
    let mut versions: Vec<(&str, [u8; 32])> = images.versions().collect();
    versions.sort_unstable();
    assert_eq!(
        versions,
        [
            ("logo", *blake3::hash(logo.bytes()).as_bytes()),
            ("other", *blake3::hash(other.bytes()).as_bytes()),
        ]
    );
}
