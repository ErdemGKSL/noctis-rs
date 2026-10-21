//! `custom_id` route grammar and codec caps (CID-01, CID-03), shared by the
//! codec and the kind derive macros so neither copies a constant (AD-3).
//!
//! ```
//! use noctis_rules::RuleId;
//! use noctis_rules::cid::{ROUTE_ID_MAX_LEN, is_route_id_byte, validate_route_id};
//!
//! assert_eq!(ROUTE_ID_MAX_LEN, 24);
//! assert!(is_route_id_byte(b'.') && !is_route_id_byte(b'A'));
//! assert!(validate_route_id("shop.buy").is_empty());
//! assert_eq!(validate_route_id("Shop")[0].rule, RuleId::CID_03);
//! ```

use crate::{Actual, Bound, RuleId, Violation};

/// Longest route id, in characters (CID-03).
pub const ROUTE_ID_MAX_LEN: usize = 24;

/// Largest schema version a Route may declare; `0` writes no `S` section.
pub const SCHEMA_MAX: u8 = 61;

/// Most unit variants a `PayloadEnum` may have: one base62 digit each.
pub const PAYLOAD_ENUM_MAX_VARIANTS: usize = 62;

/// The characters a route id may use, as the violation describes them.
const ROUTE_ID_CHARSET: &str = "[a-z0-9._-]";

/// Whether `b` may appear in a route id (`[a-z0-9._-]`).
pub const fn is_route_id_byte(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')
}

/// Checks a route id against the route grammar (CID-03).
///
/// Every applicable violation is returned, each with path `route id` and a
/// help line, in this order:
///
/// 1. the length, in characters, is outside `1..=24`;
/// 2. a character is outside `[a-z0-9._-]` (a note names the first one and
///    its byte index);
/// 3. the id starts with `_`, which is reserved for Noctis.
///
/// ```
/// use noctis_rules::cid::validate_route_id;
/// use noctis_rules::{Actual, Bound};
///
/// let errs = validate_route_id("");
/// assert_eq!(errs[0].limit, Bound::Between { min: 1, max: 24 });
/// assert_eq!(errs[0].actual, Actual::Count(0));
/// assert_eq!(validate_route_id("_X").len(), 2);
/// ```
pub fn validate_route_id(id: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let len = id.chars().count();
    if len == 0 || len > ROUTE_ID_MAX_LEN {
        out.push(
            Violation::new(
                RuleId::CID_03,
                "route id",
                Bound::Between {
                    min: 1,
                    max: ROUTE_ID_MAX_LEN,
                },
                Actual::Count(len),
            )
            .with_help("use 1 to 24 characters of `[a-z0-9._-]`, such as `shop.buy`"),
        );
    }
    if let Some((index, c)) = id
        .char_indices()
        .find(|&(_, c)| !u8::try_from(c).is_ok_and(is_route_id_byte))
    {
        out.push(
            Violation::new(
                RuleId::CID_03,
                "route id",
                Bound::Describe(ROUTE_ID_CHARSET),
                Actual::Text(id.to_owned()),
            )
            .with_note(format!("`{c}` at byte {index} is not allowed"))
            .with_help("use lowercase ASCII letters, digits, `.`, `_` and `-` only"),
        );
    }
    if id.starts_with('_') {
        out.push(
            Violation::new(
                RuleId::CID_03,
                "route id",
                Bound::Describe("an id that does not start with `_`"),
                Actual::Text(id.to_owned()),
            )
            .with_help("ids starting with `_` are reserved for Noctis; rename the route"),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{is_route_id_byte, validate_route_id};
    use crate::{Actual, Bound, RuleId};

    #[test]
    fn cid_03_route_ids() {
        let a24 = "a".repeat(24);
        for id in ["shop.buy", "a", a24.as_str(), "a.b-c_9", "0"] {
            assert!(validate_route_id(id).is_empty(), "{id}");
        }

        let errs = validate_route_id("");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].rule, RuleId::CID_03);
        assert_eq!(errs[0].path, "route id");
        assert_eq!(errs[0].limit, Bound::Between { min: 1, max: 24 });
        assert_eq!(errs[0].actual, Actual::Count(0));
        assert!(errs[0].help.is_some());

        let errs = validate_route_id(&"a".repeat(25));
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].actual, Actual::Count(25));

        for (id, note) in [
            ("Shop", "`S` at byte 0 is not allowed"),
            ("a b", "` ` at byte 1 is not allowed"),
            ("a:b", "`:` at byte 1 is not allowed"),
            ("a,b", "`,` at byte 1 is not allowed"),
            ("a~b", "`~` at byte 1 is not allowed"),
            ("a!b", "`!` at byte 1 is not allowed"),
            ("é", "`é` at byte 0 is not allowed"),
        ] {
            let errs = validate_route_id(id);
            assert_eq!(errs.len(), 1, "{id}");
            assert_eq!(errs[0].rule, RuleId::CID_03);
            assert_eq!(errs[0].limit, Bound::Describe("[a-z0-9._-]"));
            assert_eq!(errs[0].actual, Actual::Text(id.to_owned()));
            assert_eq!(errs[0].notes, [note]);
            assert!(errs[0].help.is_some());
        }

        for id in ["_x", "_"] {
            let errs = validate_route_id(id);
            assert_eq!(errs.len(), 1, "{id}");
            assert_eq!(
                errs[0].limit,
                Bound::Describe("an id that does not start with `_`")
            );
            assert!(errs[0].help.is_some());
        }

        let errs = validate_route_id(&"A".repeat(25));
        assert_eq!(errs.len(), 2);
        assert!(matches!(errs[0].limit, Bound::Between { .. }));
        assert_eq!(errs[1].limit, Bound::Describe("[a-z0-9._-]"));

        let errs = validate_route_id("_X");
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0].limit, Bound::Describe("[a-z0-9._-]"));
        assert_eq!(
            errs[1].limit,
            Bound::Describe("an id that does not start with `_`")
        );
        assert!(errs[0].to_string().starts_with("[CID-03] route id: "));
    }

    #[test]
    fn route_id_bytes() {
        for b in *b"az09._-" {
            assert!(is_route_id_byte(b), "{}", char::from(b));
        }
        for b in [b'A', b':', b',', b'~', b'!', b' ', 0x80] {
            assert!(!is_route_id_byte(b), "{b}");
        }
    }
}
