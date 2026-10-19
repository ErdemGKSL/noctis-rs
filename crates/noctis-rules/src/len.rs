//! Length counting for user text: [`discord_len`] and [`truncate`].
//!
//! Discord does not say whether its "characters" are Unicode scalars or UTF-16
//! units (catalog, "Unconfirmed values"), so Noctis counts the stricter of the two.

use std::borrow::Cow;

/// The ellipsis [`truncate`] appends (U+2026, one unit).
const ELLIPSIS: char = '\u{2026}';

/// The Discord length of `text`: the larger of its Unicode scalar count and its
/// UTF-16 unit count.
///
/// Every scalar takes one or two UTF-16 units, so this always equals the UTF-16
/// unit count. This is the only length function Noctis uses for user text.
///
/// ```
/// use noctis_rules::len::discord_len;
///
/// assert_eq!(discord_len("abc"), 3);
/// assert_eq!(discord_len("😀"), 2);
/// ```
pub fn discord_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Shortens `text` to at most `max` [`discord_len`] units, ending with `…`.
///
/// Text that already fits is returned unchanged (borrowed). Otherwise the result
/// is the longest prefix that leaves room for the `…` (one unit), followed by `…`;
/// with `max == 0` it is empty. The cut falls on a Unicode scalar boundary, so a
/// surrogate pair is never split, but it is not grapheme-aware: a combining mark
/// or a ZWJ sequence can be cut apart.
///
/// ```
/// use noctis_rules::len::truncate;
///
/// assert_eq!(truncate("abc", 3), "abc");
/// assert_eq!(truncate("abcd", 3), "ab…");
/// assert_eq!(truncate("ab🎉", 3), "ab…");
/// ```
pub fn truncate(text: &str, max: usize) -> Cow<'_, str> {
    if discord_len(text) <= max {
        return Cow::Borrowed(text);
    }
    let Some(budget) = max.checked_sub(ELLIPSIS.len_utf16()) else {
        return Cow::Borrowed("");
    };
    let mut used = 0;
    let mut end = 0;
    for (index, c) in text.char_indices() {
        used += c.len_utf16();
        if used > budget {
            break;
        }
        end = index + c.len_utf8();
    }
    let mut out = String::with_capacity(end + ELLIPSIS.len_utf8());
    out.push_str(&text[..end]);
    out.push(ELLIPSIS);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn counts_the_matrix_examples() {
        for (text, want) in [
            ("", 0),
            ("abc", 3),
            ("ğüş", 3),
            ("日本語", 3),
            ("\u{FFFF}", 1),
            ("\u{10000}", 2),
            ("\u{10FFFF}", 2),
            ("😀", 2),
            ("👍🏽", 4),
            ("👨\u{200D}👩\u{200D}👧", 8),
            ("🇹🇷", 4),
            ("e\u{301}", 2),
            ("a\0b", 3),
        ] {
            assert_eq!(discord_len(text), want, "{text:?}");
        }
    }

    #[test]
    fn counts_at_the_cv2_04_limit() {
        assert_eq!(discord_len(&"a".repeat(4000)), 4000);
        assert_eq!(discord_len(&"😀".repeat(2000)), 4000);
        assert_eq!(discord_len(&"😀".repeat(2001)), 4002);
    }

    #[test]
    fn truncate_returns_fitting_text_borrowed() {
        for (text, max) in [("abc", 3), ("", 0), ("", 5)] {
            let out = truncate(text, max);
            assert!(matches!(out, Cow::Borrowed(_)), "{text:?} {max}");
            assert_eq!(out, text);
        }
    }

    #[test]
    fn truncate_matrix() {
        for (text, max, want) in [
            ("abcd", 3, "ab…"),
            ("ab", 1, "…"),
            ("a", 0, ""),
            ("ab🎉", 3, "ab…"),
            ("🎉🎉", 2, "…"),
            ("🎉🎉", 3, "🎉…"),
            ("e\u{301}x", 2, "e…"),
        ] {
            assert_eq!(truncate(text, max), want, "{text:?} {max}");
        }
    }

    proptest! {
        #[test]
        fn len_is_the_larger_count(s in any::<String>()) {
            let chars = s.chars().count();
            let utf16 = s.encode_utf16().count();
            let len = discord_len(&s);
            prop_assert_eq!(len, chars.max(utf16));
            prop_assert_eq!(len, utf16);
            prop_assert!(len >= chars);
            prop_assert!(len <= 2 * chars);
        }

        #[test]
        fn len_is_additive(a in any::<String>(), b in any::<String>()) {
            prop_assert_eq!(discord_len(&(a.clone() + &b)), discord_len(&a) + discord_len(&b));
        }

        #[test]
        fn truncate_fits_and_keeps_the_longest_prefix(s in any::<String>(), max in 0usize..=64) {
            let out = truncate(&s, max);
            prop_assert!(discord_len(&out) <= max);
            if discord_len(&s) <= max {
                prop_assert_eq!(&*out, s.as_str());
            } else if max == 0 {
                prop_assert_eq!(&*out, "");
            } else {
                let prefix = out.strip_suffix('…');
                prop_assert!(prefix.is_some());
                let prefix = prefix.unwrap_or_default();
                prop_assert!(s.starts_with(prefix));
                prop_assert!(discord_len(prefix) < max);
                let next = s[prefix.len()..].chars().next();
                prop_assert!(next.is_some());
                let next = next.unwrap_or_default();
                // The next scalar would not have fit beside the ellipsis.
                prop_assert!(discord_len(prefix) + next.len_utf16() + 1 > max);
            }
        }
    }
}
