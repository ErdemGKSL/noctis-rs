//! One command name word: [`validate_name`] (CMD-01, CMD-02, CMD-03), and a
//! context-menu name: [`validate_context_menu_name`] (CTX-01).

use std::sync::LazyLock;

use regex::Regex;

use crate::len::discord_len;
use crate::{Actual, Bound, Limits, RuleId, Violation};

/// Matches one character the CMD-01 charset does not allow.
#[expect(
    clippy::expect_used,
    reason = "the pattern is a literal that the tests compile"
)]
static INVALID_CHAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[^-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]").expect("CMD-01 regex compiles")
});

const CHARSET: &str = "letters, numbers, `-`, `_`, `ʼ`, Devanagari or Thai only";

/// Checks one command, group or subcommand name word.
///
/// Returns the violations (with an empty path) in the order CMD-02 (length,
/// counted with [`discord_len`] against `limits.command_name_len`), CMD-01
/// (charset; the note names the first invalid character, and an ASCII `'`
/// gets a help suggesting `ʼ`), CMD-03 (lowercase; the help suggests the
/// lowercased word). An empty word reports CMD-02 only.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::validate_name;
///
/// let limits = Limits::default();
/// assert!(validate_name("ayarlar", &limits).is_empty());
/// let rules: Vec<_> = validate_name("It's", &limits).iter().map(|v| v.rule).collect();
/// assert_eq!(rules, [RuleId::CMD_01, RuleId::CMD_03]);
/// ```
pub fn validate_name(word: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    let len = discord_len(word);
    if len == 0 || len > limits.command_name_len {
        out.push(Violation::new(
            RuleId::CMD_02,
            "",
            Bound::Between {
                min: 1,
                max: limits.command_name_len,
            },
            Actual::Count(len),
        ));
    }
    if word.is_empty() {
        return out;
    }
    if let Some(found) = INVALID_CHAR.find(word) {
        let c = found.as_str().chars().next().unwrap_or_default();
        let mut v = Violation::new(
            RuleId::CMD_01,
            "",
            Bound::Describe(CHARSET),
            Actual::Text(word.to_owned()),
        )
        .with_note(format!(
            "first invalid character `{c}` (U+{:04X})",
            u32::from(c)
        ));
        if word.contains('\'') {
            v = v.with_help("use `ʼ` (U+02BC) instead of `'`");
        }
        out.push(v);
    }
    if word.chars().any(|c| !is_lowercase_char(c)) {
        out.push(
            Violation::new(
                RuleId::CMD_03,
                "",
                Bound::Describe("lowercase"),
                Actual::Text(word.to_owned()),
            )
            .with_help(format!("use `{}`", word.to_lowercase())),
        );
    }
    out
}

/// Checks a context-menu command name (CTX-01).
///
/// Only the length counts: 1 to `limits.context_menu_name_len` units of
/// [`discord_len`]. Mixed case, spaces and any character are allowed (CMD-01
/// and CMD-03 do not apply). The violation has an empty path.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::validate_context_menu_name;
///
/// let limits = Limits::default();
/// assert!(validate_context_menu_name("Show Profile", &limits).is_empty());
/// let errs = validate_context_menu_name(&"a".repeat(33), &limits);
/// assert_eq!(errs[0].rule, RuleId::CTX_01);
/// ```
pub fn validate_context_menu_name(name: &str, limits: &Limits) -> Vec<Violation> {
    let len = discord_len(name);
    let max = limits.context_menu_name_len;
    if len == 0 || len > max {
        vec![Violation::new(
            RuleId::CTX_01,
            "",
            Bound::Between { min: 1, max },
            Actual::Count(len),
        )]
    } else {
        Vec::new()
    }
}

/// Whether `c` lowercases to exactly itself.
fn is_lowercase_char(c: char) -> bool {
    let mut lower = c.to_lowercase();
    lower.next() == Some(c) && lower.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(word: &str, limits: &Limits) -> Vec<RuleId> {
        validate_name(word, limits).iter().map(|v| v.rule).collect()
    }

    const VALID: &[&str] = &[
        "ayarlar",
        "設定",
        "नमस्ते",
        "สวัสดี",
        "a-b_c",
        "itʼs",
        "١٢٣",
        "ß",
        "ǆ",
        "x1",
    ];

    const CMD_01_CASES: &[(&str, char)] = &[
        ("it's", '\''),
        ("a!b", '!'),
        ("a.b", '.'),
        ("🎉", '🎉'),
        ("x\u{A0}y", '\u{A0}'),
        ("e\u{301}", '\u{301}'),
        ("a|b", '|'),
    ];

    #[test]
    fn cmd_01_valid_words_pass() {
        for word in VALID {
            assert_eq!(validate_name(word, &Limits::default()), [], "{word:?}");
        }
    }

    #[test]
    fn cmd_02_length_bounds() {
        let l = Limits::default();
        let between = Bound::Between { min: 1, max: 32 };
        assert_eq!(validate_name(&"a".repeat(32), &l), []);
        let v = validate_name(&"a".repeat(33), &l);
        assert_eq!(
            v,
            [Violation::new(
                RuleId::CMD_02,
                "",
                between.clone(),
                Actual::Count(33)
            )]
        );
        let v = validate_name("", &l);
        assert_eq!(
            v,
            [Violation::new(
                RuleId::CMD_02,
                "",
                between.clone(),
                Actual::Count(0)
            )]
        );
        assert_eq!(validate_name(&"𠀀".repeat(16), &l), []);
        let v = validate_name(&"𠀀".repeat(17), &l);
        assert_eq!(
            v,
            [Violation::new(
                RuleId::CMD_02,
                "",
                between,
                Actual::Count(34)
            )]
        );
        let custom = Limits {
            command_name_len: 10,
            ..Limits::default()
        };
        let v = validate_name(&"a".repeat(11), &custom);
        assert_eq!(
            v,
            [Violation::new(
                RuleId::CMD_02,
                "",
                Bound::Between { min: 1, max: 10 },
                Actual::Count(11)
            )]
        );
    }

    #[test]
    fn cmd_01_ascii_apostrophe_gets_help() {
        let v = validate_name("it's", &Limits::default());
        assert_eq!(
            v,
            [Violation::new(
                RuleId::CMD_01,
                "",
                Bound::Describe(CHARSET),
                Actual::Text("it's".into())
            )
            .with_note("first invalid character `'` (U+0027)")
            .with_help("use `ʼ` (U+02BC) instead of `'`")]
        );
    }

    #[test]
    fn cmd_01_other_characters_name_the_first_offender() {
        for &(word, c) in &CMD_01_CASES[1..] {
            let v = validate_name(word, &Limits::default());
            assert_eq!(v.len(), 1, "{word:?}");
            assert_eq!(v[0].rule, RuleId::CMD_01);
            assert_eq!(v[0].actual, Actual::Text(word.into()));
            assert_eq!(
                v[0].notes,
                [format!(
                    "first invalid character `{c}` (U+{:04X})",
                    u32::from(c)
                )],
                "{word:?}"
            );
            assert_eq!(v[0].help, None, "{word:?}");
        }
        let v = validate_name("🎉", &Limits::default());
        assert_eq!(v[0].notes, ["first invalid character `🎉` (U+1F389)"]);
        let v = validate_name("x\u{A0}y", &Limits::default());
        assert_eq!(v[0].notes, ["first invalid character `\u{A0}` (U+00A0)"]);
    }

    #[test]
    fn cmd_03_uppercase_suggests_lowercase() {
        for (word, lower) in [
            ("Config", "config"),
            ("İstanbul", "i̇stanbul"),
            ("ΣΑΣ", "σας"),
            ("ǅ", "ǆ"),
            ("Ⅷ", "ⅷ"),
        ] {
            let v = validate_name(word, &Limits::default());
            assert_eq!(
                v,
                [Violation::new(
                    RuleId::CMD_03,
                    "",
                    Bound::Describe("lowercase"),
                    Actual::Text(word.into())
                )
                .with_help(format!("use `{lower}`"))],
                "{word:?}"
            );
        }
    }

    #[test]
    fn cmd_01_cmd_02_cmd_03_multiple_in_order() {
        let l = Limits::default();
        assert_eq!(rules("It's", &l), [RuleId::CMD_01, RuleId::CMD_03]);
        assert_eq!(rules(&"A".repeat(33), &l), [RuleId::CMD_02, RuleId::CMD_03]);
    }

    fn ctx_01(max: usize, count: usize) -> Vec<Violation> {
        vec![Violation::new(
            RuleId::CTX_01,
            "",
            Bound::Between { min: 1, max },
            Actual::Count(count),
        )]
    }

    #[test]
    fn ctx_01_valid_context_menu_names() {
        let a32 = "a".repeat(32);
        let valid = [
            "Show Profile",
            "show profile",
            "A",
            a32.as_str(),
            "プロフィールを表示",
            "Report 🚩",
            "It's",
        ];
        for name in valid {
            assert_eq!(
                validate_context_menu_name(name, &Limits::default()),
                [],
                "{name:?}"
            );
        }
    }

    #[test]
    fn ctx_01_context_menu_name_length() {
        let l = Limits::default();
        assert_eq!(
            validate_context_menu_name(&"a".repeat(33), &l),
            ctx_01(32, 33)
        );
        assert_eq!(validate_context_menu_name("", &l), ctx_01(32, 0));
        assert_eq!(validate_context_menu_name(&"𠀀".repeat(16), &l), []);
        assert_eq!(
            validate_context_menu_name(&"𠀀".repeat(17), &l),
            ctx_01(32, 34)
        );
        let custom = Limits {
            context_menu_name_len: 10,
            ..Limits::default()
        };
        assert_eq!(
            validate_context_menu_name(&"a".repeat(11), &custom),
            ctx_01(10, 11)
        );
    }

    #[test]
    fn cmd_01_catalog_regex_agrees() {
        let catalog = Regex::new(r"^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$")
            .expect("catalog regex compiles");
        for word in VALID {
            assert!(catalog.is_match(word), "{word:?}");
        }
        for (word, _) in CMD_01_CASES {
            assert!(!catalog.is_match(word), "{word:?}");
            assert!(rules(word, &Limits::default()).contains(&RuleId::CMD_01));
        }
    }
}
