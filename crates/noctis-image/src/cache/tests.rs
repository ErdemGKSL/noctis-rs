//! The FR-36 key, store key and entry codec rows.

use std::collections::HashSet;
use std::time::Duration;

use super::*;

fn pairs() -> AssetPairs {
    AssetPairs::new(vec![
        ("fonts".to_owned(), [1; 32]),
        ("image:logo".to_owned(), [2; 32]),
        ("remote.cdn".to_owned(), [3; 32]),
        ("remote.fallback".to_owned(), [4; 32]),
    ])
}

fn base(assets: &AssetPairs) -> KeyInputs<'_> {
    KeyInputs {
        schema: KEY_SCHEMA,
        noctis_version: NOCTIS_VERSION,
        takumi_version: TAKUMI_VERSION,
        template_id: "card",
        template_version: 1,
        theme: [7; 32],
        data: b"data",
        assets,
        size: Size::new(64, 32),
        format: Format::Webp { quality: 80 },
        animation: Some(AnimationSpec::new(
            Duration::from_secs(1),
            10,
            AnimatedFormat::Webp { quality: 80 },
        )),
        budget: 2_000_000,
        still_fallback: true,
    }
}

#[test]
fn fr_36_the_key_is_deterministic() {
    let assets = pairs();
    assert_eq!(base(&assets).key(), base(&assets).key());
}

/// Changes `pairs()` with `edit`.
fn edited(edit: impl FnOnce(&mut Vec<(String, [u8; 32])>)) -> AssetPairs {
    let mut list = pairs().0;
    edit(&mut list);
    AssetPairs::new(list)
}

#[test]
#[expect(clippy::too_many_lines, reason = "one variant per key field")]
fn fr_36_every_field_changes_the_key() {
    let assets = pairs();
    let pair_variants: Vec<AssetPairs> = vec![
        edited(|p| p[0].1 = [9; 32]),
        edited(|p| p[1].1 = [9; 32]),
        edited(|p| p[2].1 = [9; 32]),
        edited(|p| p[3].1 = [9; 32]),
        edited(|p| p.push(("image:extra".to_owned(), [5; 32]))),
        edited(|p| {
            p.remove(1);
        }),
    ];
    let b = base(&assets);
    let spec = b.animation.unwrap();
    let mut variants: Vec<(&str, KeyInputs<'_>)> = vec![
        (
            "schema",
            KeyInputs {
                schema: b"noctis.render.v2",
                ..b.clone()
            },
        ),
        (
            "noctis",
            KeyInputs {
                noctis_version: "0.0.0-other",
                ..b.clone()
            },
        ),
        (
            "takumi",
            KeyInputs {
                takumi_version: "takumi=2.18.0;takumi-core=0.32.0",
                ..b.clone()
            },
        ),
        (
            "id",
            KeyInputs {
                template_id: "cards",
                ..b.clone()
            },
        ),
        (
            "version",
            KeyInputs {
                template_version: 2,
                ..b.clone()
            },
        ),
        (
            "theme",
            KeyInputs {
                theme: [8; 32],
                ..b.clone()
            },
        ),
        (
            "data",
            KeyInputs {
                data: b"datb",
                ..b.clone()
            },
        ),
        (
            "width",
            KeyInputs {
                size: Size::new(65, 32),
                ..b.clone()
            },
        ),
        (
            "height",
            KeyInputs {
                size: Size::new(64, 33),
                ..b.clone()
            },
        ),
        (
            "format tag",
            KeyInputs {
                format: Format::Jpeg { quality: 80 },
                ..b.clone()
            },
        ),
        (
            "quality",
            KeyInputs {
                format: Format::Webp { quality: 81 },
                ..b.clone()
            },
        ),
        (
            "animation none",
            KeyInputs {
                animation: None,
                ..b.clone()
            },
        ),
        (
            "duration",
            KeyInputs {
                animation: Some(AnimationSpec {
                    duration: spec.duration + Duration::from_nanos(1),
                    ..spec
                }),
                ..b.clone()
            },
        ),
        (
            "fps",
            KeyInputs {
                animation: Some(AnimationSpec { fps: 11, ..spec }),
                ..b.clone()
            },
        ),
        (
            "animated format",
            KeyInputs {
                animation: Some(AnimationSpec {
                    format: AnimatedFormat::Gif,
                    ..spec
                }),
                ..b.clone()
            },
        ),
        (
            "animated quality",
            KeyInputs {
                animation: Some(AnimationSpec {
                    format: AnimatedFormat::Webp { quality: 81 },
                    ..spec
                }),
                ..b.clone()
            },
        ),
        (
            "budget",
            KeyInputs {
                budget: 1_999_999,
                ..b.clone()
            },
        ),
        (
            "still_fallback",
            KeyInputs {
                still_fallback: false,
                ..b.clone()
            },
        ),
    ];
    for (i, assets) in pair_variants.iter().enumerate() {
        let name = ["fonts", "image", "cdn", "fallback", "added", "removed"][i];
        variants.push((
            name,
            KeyInputs {
                assets,
                ..b.clone()
            },
        ));
    }
    assert!(variants.len() >= 20);
    let base_key = b.key();
    let mut seen = HashSet::new();
    for (name, inputs) in &variants {
        let key = inputs.key();
        assert_ne!(key, base_key, "{name}");
        assert!(seen.insert(key), "{name} collides with another variant");
    }
}

#[test]
fn fr_36_variable_length_fields_are_framed() {
    let assets = pairs();
    let b = base(&assets);
    let ab_c = KeyInputs {
        template_id: "ab",
        data: b"c",
        ..b.clone()
    };
    let a_bc = KeyInputs {
        template_id: "a",
        data: b"bc",
        ..b.clone()
    };
    assert_ne!(ab_c.key(), a_bc.key());
    let empty = KeyInputs {
        data: b"",
        ..b.clone()
    };
    let zero = KeyInputs {
        data: b"\0",
        ..b.clone()
    };
    assert_ne!(empty.key(), zero.key());
}

#[test]
fn fr_36_pairs_are_sorted_by_id() {
    let mut reversed = pairs().0;
    reversed.reverse();
    let reversed = AssetPairs::new(reversed);
    let assets = pairs();
    assert_eq!(
        base(&assets).key(),
        KeyInputs {
            assets: &reversed,
            ..base(&assets)
        }
        .key()
    );
}

#[test]
fn fr_36_only_the_effective_budget_is_keyed() {
    let assets = pairs();
    let job = |upload_budget| RenderJob {
        template_id: "card",
        template_version: 1,
        data: Bytes::from_static(b"data"),
        size: Size::new(4, 4),
        format: Format::Png,
        animation: None,
        upload_budget,
    };
    let key = |upload_budget, target| {
        KeyInputs::of(&job(upload_budget), [0; 32], &assets, target, true).key()
    };
    assert_eq!(key(3_000_000, 2_000_000), key(25_000_000, 2_000_000));
    assert_ne!(key(1_000_000, 2_000_000), key(2_000_000, 2_000_000));
}

#[test]
fn fr_36_unicode_inputs_have_a_stable_key() {
    let assets = AssetPairs::new(vec![("fonts".to_owned(), [1; 32])]);
    let inputs = KeyInputs {
        schema: KEY_SCHEMA,
        noctis_version: "0.1.0",
        takumi_version: "takumi=2.17.0;takumi-core=0.31.0",
        template_id: "kart.ş",
        template_version: 1,
        theme: [0; 32],
        data: "😀é".as_bytes(),
        assets: &assets,
        size: Size::new(8, 8),
        format: Format::Png,
        animation: None,
        budget: 2_097_152,
        still_fallback: true,
    };
    assert_eq!(inputs.key().hex(), GOLDEN);
}

/// The key of the Unicode row; a change means the layout changed, so bump
/// [`KEY_SCHEMA`] too.
const GOLDEN: &str = "9acaf51fc80f21f157ad41addeb658feabfe41b98515105555d2193e867eb56f";

#[test]
fn fr_36_the_store_key_is_versioned_lowercase_hex() {
    let key = RenderKey([0xAB; 32]).store_key();
    assert_eq!(key.len(), 74);
    assert_eq!(key, format!("render:v1:{}", "ab".repeat(32)));
    let assets = pairs();
    let other = base(&assets).key().store_key();
    assert!(other.starts_with("render:v1:"));
    assert!(
        other[10..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

fn output(format: OutputFormat, settings: EncodeSettings, bytes: &'static [u8]) -> RenderOutput {
    RenderOutput {
        bytes: Bytes::from_static(bytes),
        digest: [5; 32],
        format,
        settings_used: settings,
    }
}

#[test]
fn fr_36_entries_round_trip_without_copying() {
    let cases = [
        output(
            OutputFormat::Png,
            EncodeSettings {
                size: Size::new(64, 32),
                quality: None,
                fps: None,
                frames: 1,
            },
            b"\x89PNG-bytes",
        ),
        output(
            OutputFormat::Webp,
            EncodeSettings {
                size: Size::new(300, 200),
                quality: Some(80),
                fps: Some(30),
                frames: 45,
            },
            b"RIFF-webp",
        ),
        output(
            OutputFormat::Gif,
            EncodeSettings {
                size: Size::new(1, 1),
                quality: None,
                fps: Some(50),
                frames: 90,
            },
            b"GIF89a",
        ),
    ];
    for out in cases {
        let entry = encode_entry(&out);
        assert_eq!(entry.len(), HEADER_LEN + out.bytes.len());
        let decoded = decode_entry(&entry, RenderKey([5; 32])).unwrap();
        assert_eq!(decoded, out);
        assert!(std::ptr::eq(
            decoded.bytes.as_ptr(),
            entry[HEADER_LEN..].as_ptr()
        ));
    }
}

#[test]
fn fr_36_invalid_entries_do_not_decode() {
    let valid = encode_entry(&output(
        OutputFormat::Webp,
        EncodeSettings {
            size: Size::new(4, 4),
            quality: Some(80),
            fps: Some(10),
            frames: 2,
        },
        b"x",
    ));
    let key = RenderKey([0; 32]);
    assert!(decode_entry(&valid, key).is_some());
    let patched = |at: usize, value: u8| {
        let mut entry = valid.to_vec();
        entry[at] = value;
        Bytes::from(entry)
    };
    let width_zero = {
        let mut entry = valid.to_vec();
        entry[5..9].fill(0);
        Bytes::from(entry)
    };
    let frames_zero = {
        let mut entry = valid.to_vec();
        entry[18..22].fill(0);
        Bytes::from(entry)
    };
    let cases = [
        ("empty", Bytes::new()),
        ("21 bytes", valid.slice(..21)),
        ("no image byte", valid.slice(..22)),
        ("magic", patched(0, b'X')),
        ("tag 5", patched(4, 5)),
        ("width 0", width_zero),
        ("frames 0", frames_zero),
        ("quality presence 2", patched(13, 2)),
        ("fps presence 2", patched(15, 2)),
    ];
    for (name, entry) in cases {
        assert!(decode_entry(&entry, key).is_none(), "{name}");
    }
}

/// The version of package `name` in `Cargo.lock`.
fn locked_version(lock: &str, name: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == format!("name = \"{name}\"")
            && let Some(version) = lines.next()
            && let Some(v) = version
                .strip_prefix("version = \"")
                .and_then(|v| v.strip_suffix('"'))
        {
            versions.push(v.to_owned());
        }
    }
    versions
}

#[test]
fn fr_36_the_takumi_version_matches_cargo_lock() {
    let lock =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
    let takumi = locked_version(&lock, "takumi");
    let core = locked_version(&lock, "takumi-core");
    assert_eq!(takumi.len(), 1, "{takumi:?}");
    assert_eq!(core.len(), 1, "{core:?}");
    assert_eq!(
        TAKUMI_VERSION,
        format!("takumi={};takumi-core={}", takumi[0], core[0]),
        "bump TAKUMI_VERSION with the takumi pin"
    );
}
