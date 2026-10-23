//! One command name word: [`validate_name`] (CMD-01, CMD-02, CMD-03), a
//! context-menu name: [`validate_context_menu_name`] (CTX-01), and one
//! localized value: [`validate_localized`] (LOC-02).

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

/// Which field a localized value fills (LOC-02), and so which base rules
/// [`validate_localized`] applies to it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocalizedField {
    /// A command, subcommand group, subcommand or option name word
    /// (CMD-01, CMD-02, CMD-03).
    Name,
    /// A command, group, subcommand or option description (CMD-04).
    Description,
    /// A choice name (OPT-09).
    ChoiceName,
    /// A context-menu name (CTX-01).
    ContextMenuName,
}

/// Checks one localized value of `field` with the same rules as the
/// default text (LOC-02): [`validate_name`] for [`LocalizedField::Name`],
/// the CMD-04 length for [`LocalizedField::Description`], the OPT-09 name
/// length for [`LocalizedField::ChoiceName`] and
/// [`validate_context_menu_name`] for [`LocalizedField::ContextMenuName`].
///
/// Every violation has an empty path; the caller names the locale and key.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::{LocalizedField, validate_localized};
///
/// let limits = Limits::default();
/// assert!(validate_localized(LocalizedField::Name, "yasakla", &limits).is_empty());
/// let errs = validate_localized(LocalizedField::Name, "Yasakla", &limits);
/// assert_eq!(errs[0].rule, RuleId::CMD_03);
/// let errs = validate_localized(LocalizedField::Description, "", &limits);
/// assert_eq!(errs[0].rule, RuleId::CMD_04);
/// let errs = validate_localized(LocalizedField::ChoiceName, &"a".repeat(101), &limits);
/// assert_eq!(errs[0].rule, RuleId::OPT_09);
/// assert!(validate_localized(LocalizedField::ContextMenuName, "Profili göster", &limits).is_empty());
/// ```
pub fn validate_localized(field: LocalizedField, text: &str, limits: &Limits) -> Vec<Violation> {
    match field {
        LocalizedField::Name => validate_name(text, limits),
        LocalizedField::Description => super::option::description_violation(text, "", limits)
            .into_iter()
            .collect(),
        LocalizedField::ChoiceName => super::option::choice_name_violation(text, "", limits)
            .into_iter()
            .collect(),
        LocalizedField::ContextMenuName => validate_context_menu_name(text, limits),
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

    fn localized(field: LocalizedField, text: &str) -> Vec<RuleId> {
        validate_localized(field, text, &Limits::default())
            .iter()
            .map(|v| v.rule)
            .collect()
    }

    /// Story 11.4 Matrix 25-28: localized names follow CMD-01..03.
    #[test]
    fn loc_02_localized_names() {
        use LocalizedField::Name;
        assert_eq!(localized(Name, "Yasakla"), [RuleId::CMD_03]);
        assert_eq!(localized(Name, "ya sakla"), [RuleId::CMD_01]);
        assert_eq!(localized(Name, "it's"), [RuleId::CMD_01]);
        assert_eq!(localized(Name, ""), [RuleId::CMD_02]);
        assert_eq!(localized(Name, &"a".repeat(33)), [RuleId::CMD_02]);
        assert_eq!(localized(Name, &"a".repeat(32)), []);
        assert_eq!(localized(Name, "kullanıcı"), []);
        assert_eq!(localized(Name, "नमस्ते"), []);
        let v = validate_localized(Name, "Yasakla", &Limits::default());
        assert_eq!(v[0].path, "");
    }

    /// Story 11.4 Matrix 29, 30: descriptions follow CMD-04, choice names
    /// OPT-09 (UTF-16 counted).
    #[test]
    fn loc_02_localized_descriptions_and_choice_names() {
        use LocalizedField::{ChoiceName, Description};
        for (field, rule) in [(Description, RuleId::CMD_04), (ChoiceName, RuleId::OPT_09)] {
            assert_eq!(localized(field, ""), [rule]);
            assert_eq!(localized(field, &"a".repeat(101)), [rule]);
            assert_eq!(localized(field, &"a".repeat(100)), []);
            assert_eq!(localized(field, &"😀".repeat(50)), []);
            assert_eq!(localized(field, &"😀".repeat(51)), [rule]);
        }
        let v = validate_localized(Description, "", &Limits::default());
        assert_eq!(
            v,
            [Violation::new(
                RuleId::CMD_04,
                "",
                Bound::Between { min: 1, max: 100 },
                Actual::Count(0)
            )]
        );
    }

    /// Story 11.4 Matrix 31: localized context-menu names follow CTX-01.
    #[test]
    fn ctx_01_localized_context_menu_names() {
        use LocalizedField::ContextMenuName;
        assert_eq!(
            localized(ContextMenuName, &"a".repeat(33)),
            [RuleId::CTX_01]
        );
        assert_eq!(localized(ContextMenuName, &"a".repeat(32)), []);
        assert_eq!(localized(ContextMenuName, "Profili Göster"), []);
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
