use std::num::NonZeroU64;

use super::{CustomEmoji, Piece, scan, split};

const ID: &str = "123456789012345678";

fn emoji(id: u64, animated: bool) -> CustomEmoji {
    CustomEmoji {
        id: NonZeroU64::new(id).unwrap(),
        animated,
    }
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    split(text).collect()
}

/// The single emoji `text` holds when it is exactly one token.
fn sole(text: &str) -> Option<CustomEmoji> {
    match pieces(text).as_slice() {
        [Piece::Emoji(e)] => Some(*e),
        _ => None,
    }
}

/// Reassembles the input from the pieces, copying each token back from the
/// input, and checks that no text piece is empty.
fn rejoin(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    for piece in split(text) {
        match piece {
            Piece::Text(t) => {
                assert!(!t.is_empty(), "empty text piece in {text:?}");
                assert!(rest.starts_with(t));
                out.push_str(t);
                rest = &rest[t.len()..];
            }
            Piece::Emoji(_) => {
                let end = rest.find('>').unwrap() + 1;
                out.push_str(&rest[..end]);
                rest = &rest[end..];
            }
        }
    }
    assert!(rest.is_empty());
    out
}

#[test]
fn img_04_token_static_and_animated() {
    let id = ID.parse().unwrap();
    assert_eq!(sole(&format!("<:pepe:{ID}>")), Some(emoji(id, false)));
    assert_eq!(sole(&format!("<a:pepe:{ID}>")), Some(emoji(id, true)));
}

#[test]
fn img_04_name_bounds() {
    let n32 = "a".repeat(32);
    let n33 = "a".repeat(33);
    assert_eq!(sole(&format!("<:p:{ID}>")), None);
    assert!(sole(&format!("<:pp:{ID}>")).is_some());
    assert!(sole(&format!("<:{n32}:{ID}>")).is_some());
    assert_eq!(sole(&format!("<:{n33}:{ID}>")), None);
}

#[test]
fn img_04_name_characters() {
    assert!(sole(&format!("<:pe_pe9:{ID}>")).is_some());
    for name in ["pe-pe", "pépé", "pe pe", ""] {
        let text = format!("<:{name}:{ID}>");
        assert_eq!(pieces(&text), [Piece::Text(&text)], "{name:?}");
    }
}

#[test]
fn img_04_id_bounds() {
    assert_eq!(sole("<:pp:1234567890123456>"), None, "16 digits");
    assert_eq!(
        sole("<:pp:12345678901234567>"),
        Some(emoji(12_345_678_901_234_567, false))
    );
    assert_eq!(
        sole("<:pp:18446744073709551615>"),
        Some(emoji(u64::MAX, false))
    );
    assert_eq!(sole("<:pp:18446744073709551616>"), None, "overflow");
    assert_eq!(sole("<:pp:123456789012345678901>"), None, "21 digits");
    assert_eq!(sole("<:pp:00000000000000000>"), None, "id 0");
    assert_eq!(sole("<:pp:+1234567890123456>"), None, "sign");
}

#[test]
fn img_04_shape() {
    let id: u64 = ID.parse().unwrap();
    let e = emoji(id, false);
    for text in [
        format!("<A:x1:{ID}>"),
        format!("<:pepe:{ID}"),
        format!("< :pepe:{ID}>"),
    ] {
        assert_eq!(pieces(&text), [Piece::Text(&text)], "{text:?}");
    }
    assert_eq!(
        pieces(&format!("<:pepe:{ID}>>")),
        [Piece::Emoji(e), Piece::Text(">")]
    );
    assert_eq!(
        pieces(&format!("<<:pepe:{ID}>")),
        [Piece::Text("<"), Piece::Emoji(e)]
    );
}

#[test]
fn img_04_split_pieces() {
    let id: u64 = ID.parse().unwrap();
    let e = emoji(id, false);
    assert_eq!(pieces(""), []);
    assert_eq!(pieces("plain"), [Piece::Text("plain")]);
    assert_eq!(
        pieces(&format!("a<:pp:{ID}>b")),
        [Piece::Text("a"), Piece::Emoji(e), Piece::Text("b")]
    );
    assert_eq!(
        pieces(&format!("<:pp:{ID}><a:pp:{ID}>")),
        [Piece::Emoji(e), Piece::Emoji(emoji(id, true))]
    );
    assert_eq!(
        pieces(&format!("<:pp:{ID}> mid <:pp:{ID}>")),
        [Piece::Emoji(e), Piece::Text(" mid "), Piece::Emoji(e)]
    );
    assert_eq!(
        pieces(&format!("é<:pp:{ID}>😀")),
        [Piece::Text("é"), Piece::Emoji(e), Piece::Text("😀")]
    );
}

#[test]
fn img_04_split_restores_input() {
    for text in [
        String::new(),
        "<<<>>>".to_owned(),
        format!("x <:pp:{ID}> <a:p:{ID}> é<a:pp:{ID}><:pp:{ID}>😀 <"),
    ] {
        assert_eq!(rejoin(&text), text);
    }
}

#[test]
fn img_04_scan_distinct_in_order() {
    let id: u64 = ID.parse().unwrap();
    let text = format!("<:a1:{ID}> <:b2:{ID}> <a:a1:{ID}> <:a1:12345678901234567>");
    assert_eq!(
        scan(text.as_bytes()),
        [
            emoji(id, false),
            emoji(id, true),
            emoji(12_345_678_901_234_567, false)
        ]
    );
}

#[test]
fn img_04_scan_non_utf8_and_empty() {
    let mut bytes = vec![0xff, 0xfe, 0x00];
    bytes.extend_from_slice(format!("<:pp:{ID}>").as_bytes());
    bytes.extend_from_slice(&[0xc3, 0x28]);
    assert_eq!(scan(&bytes), [emoji(ID.parse().unwrap(), false)]);

    let none = scan(b"no tokens here");
    assert!(none.is_empty());
    assert_eq!(none.capacity(), 0, "no allocation without a token");
}
