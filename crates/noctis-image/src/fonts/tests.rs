//! `Font`, `validate` and the `FontRegistry` Resource (IMG-01).

use noctis_core::Resource;

use super::*;

static INTER: &[u8] = include_bytes!("../../fonts/InterVariable.woff2");
static JP400: &[u8] =
    include_bytes!("../../tests/fixtures/fonts/noto-sans-jp-119-400-normal.woff2");
static JP700: &[u8] =
    include_bytes!("../../tests/fixtures/fonts/noto-sans-jp-119-700-normal.woff2");

/// A minimal font file of one empty glyph: `tag` is the sfnt version
/// (`0x00010000` TrueType or `OTTO`), `family` the name table's family
/// (`None`: no name table), `base` the offset the file starts at inside a
/// collection.
#[expect(
    clippy::too_many_lines,
    reason = "one table after another, in file order"
)]
fn sfnt(tag: [u8; 4], family: Option<&str>, base: u32) -> Vec<u8> {
    fn be16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    fn be32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let truetype = tag != *b"OTTO";
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    // cmap: one format 4 subtable holding only the 0xFFFF end segment.
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        be16(&mut cmap, v);
    }
    be32(&mut cmap, 12);
    for v in [4u16, 24, 0, 2, 2, 0, 0, 0xFFFF, 0, 0xFFFF, 1, 0] {
        be16(&mut cmap, v);
    }
    tables.push((*b"cmap", cmap));
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0);
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0);
    be16(&mut head, 1000);
    head.extend_from_slice(&[0; 16]);
    for v in [0u16, 0, 500, 800, 0, 8, 2, 0, 0] {
        be16(&mut head, v);
    }
    tables.push((*b"head", head));
    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    for v in [800u16, 0xFF38, 0, 500, 0, 0, 500, 1, 0, 0, 0, 0, 0, 0, 0, 1] {
        be16(&mut hhea, v);
    }
    tables.push((*b"hhea", hhea));
    tables.push((*b"hmtx", vec![0x01, 0xF4, 0, 0]));
    let mut maxp = Vec::new();
    if truetype {
        be32(&mut maxp, 0x0001_0000);
        be16(&mut maxp, 1);
        maxp.extend_from_slice(&[0; 26]);
    } else {
        be32(&mut maxp, 0x0000_5000);
        be16(&mut maxp, 1);
    }
    tables.push((*b"maxp", maxp));
    if let Some(family) = family {
        let strings: Vec<(u16, Vec<u8>)> = [(1u16, family), (2, "Regular"), (4, family)]
            .into_iter()
            .map(|(id, s)| (id, s.encode_utf16().flat_map(u16::to_be_bytes).collect()))
            .collect();
        let mut name = Vec::new();
        let count = u16::try_from(strings.len()).unwrap();
        be16(&mut name, 0);
        be16(&mut name, count);
        be16(&mut name, 6 + 12 * count);
        let mut offset = 0u16;
        for (id, s) in &strings {
            let len = u16::try_from(s.len()).unwrap();
            for v in [3u16, 1, 0x409, *id, len, offset] {
                be16(&mut name, v);
            }
            offset += len;
        }
        for (_, s) in &strings {
            name.extend_from_slice(s);
        }
        tables.push((*b"name", name));
    }
    let mut post = Vec::new();
    be32(&mut post, 0x0003_0000);
    post.extend_from_slice(&[0; 28]);
    tables.push((*b"post", post));
    if truetype {
        tables.push((*b"glyf", Vec::new()));
        tables.push((*b"loca", vec![0, 0, 0, 0]));
    }
    tables.sort_by_key(|(tag, _)| *tag);
    let count = u16::try_from(tables.len()).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&tag);
    for v in [count, 128, 3, count * 16 - 128] {
        be16(&mut out, v);
    }
    let mut offset = 12 + 16 * u32::from(count);
    let mut body = Vec::new();
    for (tag, data) in &tables {
        let len = u32::try_from(data.len()).unwrap();
        let mut padded = data.clone();
        padded.resize(data.len().next_multiple_of(4), 0);
        let sum = padded.chunks_exact(4).fold(0u32, |s, c| {
            s.wrapping_add(u32::from_be_bytes(c.try_into().unwrap()))
        });
        out.extend_from_slice(tag);
        be32(&mut out, sum);
        be32(&mut out, base + offset);
        be32(&mut out, len);
        offset += u32::try_from(padded.len()).unwrap();
        body.extend_from_slice(&padded);
    }
    out.extend_from_slice(&body);
    out
}

/// A collection of one [`sfnt`] font.
fn ttc(family: &str) -> Vec<u8> {
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&16u32.to_be_bytes());
    out.extend_from_slice(&sfnt([0, 1, 0, 0], Some(family), 16));
    out
}

fn registry(fonts: Vec<Font>) -> FontRegistry {
    FontRegistry::init(&FontsConfig {
        fonts,
        bundled: false,
        emoji: false,
    })
}

#[test]
fn img_01_ranges_single_full_and_none_are_accepted() {
    let single = Font::from_static(INTER).unicode_range('a'..='a');
    let full = Font::from_static(INTER).unicode_range('\0'..=char::MAX);
    let none = Font::from_static(INTER);
    assert!(none.ranges.is_empty());
    for font in [single, full, none] {
        assert_eq!(validate(std::slice::from_ref(&font)), Vec::<String>::new());
        let registry = registry(vec![font]);
        assert_eq!(registry.build_error(), None);
        assert!(!registry.is_empty());
    }
}

#[test]
fn img_01_inverted_range_is_a_violation() {
    let fonts = [
        Font::from_static(INTER),
        Font::from_static(INTER).unicode_range('b'..='a'),
    ];
    assert_eq!(
        validate(&fonts),
        ["[IMG-01] font #1: unicode range U+0062..=U+0061 is inverted"]
    );
}

#[test]
fn img_01_family_override_bounds() {
    let check = |name: String| validate(&[Font::from_static(INTER).family(name)]);
    for ok in ["a".repeat(128), "源ノ角".to_owned(), "Inter".to_owned()] {
        assert_eq!(check(ok.clone()), Vec::<String>::new(), "{ok:?}");
    }
    for bad in [String::new(), "a".repeat(129), "Bad\u{7}Name".to_owned()] {
        let problems = check(bad.clone());
        assert_eq!(problems.len(), 1, "{bad:?}");
        assert!(
            problems[0].starts_with("[IMG-01] font #0: family name"),
            "{}",
            problems[0]
        );
    }
}

#[test]
fn img_01_font_formats_load() {
    let ttf: &'static [u8] = sfnt([0, 1, 0, 0], Some("Tiny Ttf"), 0).leak();
    let otf: &'static [u8] = sfnt(*b"OTTO", Some("Tiny Otf"), 0).leak();
    for (font, family) in [
        (Font::from_static(ttf), "Tiny Ttf"),
        (Font::from_static(otf), "Tiny Otf"),
        (Font::from_bytes(ttc("Tiny Ttc")), "Tiny Ttc"),
        (Font::from_static(INTER).family("Inter"), "Inter"),
        (Font::from_static(JP400).family("Kana"), "Kana"),
    ] {
        let registry = registry(vec![font]);
        assert_eq!(registry.build_error(), None, "{family}");
        assert_eq!(registry.families(), [family]);
        assert!(registry.chain().is_some());
    }
}

#[test]
fn img_01_bad_fonts_fail_to_load() {
    let mut woff1 = b"wOFF".to_vec();
    woff1.extend_from_slice(&[0; 40]);
    let cases: [(Vec<u8>, &str); 5] = [
        (woff1, "Unsupported font format"),
        (Vec::new(), "Unsupported font format"),
        (b"\0\x01\0".to_vec(), "Unsupported font format"),
        (b"\0\x01\0\0garbage".to_vec(), ""),
        (sfnt([0, 1, 0, 0], None, 0), ""),
    ];
    for (bytes, why) in cases {
        let len = bytes.len();
        let registry = registry(vec![Font::from_static(INTER), Font::from_bytes(bytes)]);
        let error = registry.build_error().expect("a load error");
        let prefix = format!("[IMG-01] font #1 ({len} bytes) could not be loaded: ");
        assert!(error.starts_with(&prefix), "{error}");
        assert!(error.ends_with(why), "{error}");
        assert!(!error.ends_with(": "), "{error}");
    }
}

#[test]
fn img_01_families_keep_their_first_appearance_order() {
    let registry = registry(vec![
        Font::from_static(JP700).family("B"),
        Font::from_static(JP400).family("A"),
        Font::from_static(INTER).family("B"),
    ]);
    assert_eq!(registry.build_error(), None);
    assert_eq!(registry.families(), ["B", "A"]);
}

#[test]
fn img_01_no_font_is_empty() {
    let registry = registry(Vec::new());
    assert!(registry.is_empty());
    assert_eq!(registry.build_error(), None);
    assert!(registry.chain().is_none());
}

#[cfg(feature = "default-font")]
#[test]
fn img_01_bundled_font_is_a_last_resort_outside_the_chain() {
    let registry = FontRegistry::init(&FontsConfig {
        fonts: Vec::new(),
        bundled: true,
        emoji: false,
    });
    assert_eq!(registry.build_error(), None);
    assert!(!registry.is_empty());
    assert!(registry.families().is_empty());
    assert!(registry.chain().is_none());
}

#[test]
fn img_01_equality_is_by_digest_family_ranges_and_order() {
    let a = Font::from_static(INTER);
    assert_eq!(a, Font::from_bytes(INTER.to_vec()));
    assert_ne!(a, Font::from_static(JP400));
    assert_ne!(a, a.clone().family("Inter"));
    assert_ne!(a, a.clone().unicode_range('a'..='z'));
    let b = Font::from_static(JP400);
    let config = |fonts: Vec<Font>| FontsConfig {
        fonts,
        bundled: false,
        emoji: false,
    };
    assert_eq!(
        config(vec![a.clone(), b.clone()]),
        config(vec![a.clone(), b.clone()])
    );
    assert_ne!(config(vec![a.clone(), b.clone()]), config(vec![b, a]));
}

#[test]
fn img_01_debug_never_prints_bytes() {
    let font = Font::from_static(b"\0\x01\0\0secret")
        .family("Kana")
        .unicode_range('\u{3041}'..='\u{3096}');
    let debug = format!("{font:?}");
    let digest = blake3::hash(b"\0\x01\0\0secret").to_hex();
    assert_eq!(
        debug,
        format!(
            "Font {{ family: Some(\"Kana\"), ranges: [U+3041..=U+3096], len: 10, blake3: \"{}\", .. }}",
            &digest[..8]
        )
    );
    let config = format!(
        "{:?}",
        FontsConfig {
            fonts: vec![font],
            bundled: true,
            emoji: false,
        }
    );
    assert!(
        !config.contains("secret") && !config.contains("115, 101"),
        "{config}"
    );
}

#[cfg(feature = "emoji")]
#[test]
fn img_04_emoji_font_is_a_last_resort_outside_the_chain() {
    let registry = FontRegistry::init(&FontsConfig {
        fonts: vec![Font::from_static(JP400)],
        bundled: false,
        emoji: true,
    });
    assert_eq!(registry.build_error(), None);
    assert!(registry.has_emoji());
    assert!(!registry.families().iter().any(|f| f == EMOJI_FAMILY));
    assert_eq!(registry.families().len(), 1);
}

#[cfg(feature = "emoji")]
#[test]
fn img_04_emoji_font_alone_is_no_text_font() {
    let registry = FontRegistry::init(&FontsConfig {
        fonts: Vec::new(),
        bundled: false,
        emoji: true,
    });
    assert!(registry.has_emoji());
    assert!(registry.is_empty());
    assert!(registry.chain().is_none());
}

#[test]
fn img_04_emoji_off_registers_no_emoji_font() {
    let registry = FontRegistry::init(&FontsConfig {
        fonts: vec![Font::from_static(JP400)],
        bundled: false,
        emoji: false,
    });
    assert!(!registry.has_emoji());
}

#[test]
fn fr_36_the_digest_follows_fonts_order_family_ranges_and_flags() {
    let config = |fonts: Vec<Font>, bundled: bool, emoji: bool| FontsConfig {
        fonts,
        bundled,
        emoji,
    };
    let digest = |c: &FontsConfig| config_digest(c);
    let a = Font::from_static(JP400);
    let b = Font::from_static(JP700);
    let base = config(vec![a.clone(), b.clone()], false, false);
    assert_eq!(digest(&base), digest(&base.clone()));
    assert_eq!(
        registry(vec![a.clone(), b.clone()]).digest(),
        digest(&config(vec![a.clone(), b.clone()], false, false))
    );
    let variants = [
        config(vec![b.clone(), a.clone()], false, false),
        config(vec![a.clone()], false, false),
        config(vec![a.clone().family("X"), b.clone()], false, false),
        config(
            vec![a.clone().unicode_range('a'..='z'), b.clone()],
            false,
            false,
        ),
        config(vec![a.clone(), b.clone()], true, false),
        config(vec![a.clone(), b.clone()], false, true),
    ];
    for (i, variant) in variants.iter().enumerate() {
        assert_ne!(digest(variant), digest(&base), "variant {i}");
    }
}
