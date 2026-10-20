//! Command Pattern expansion: [`expand`] (CMD-15, CMD-16), and the spacing
//! check that runs before it: [`validate_pattern_spacing`] (CMD-01, CMD-02).

use crate::{Actual, Bound, Limits, RuleId, Violation};

/// One token of a pattern.
enum Token<'p> {
    Word(&'p str),
    Group { alts: Vec<&'p str>, optional: bool },
}

impl Token<'_> {
    /// How many choices this token contributes to the product.
    fn choices(&self) -> usize {
        match self {
            Token::Word(_) => 1,
            Token::Group { alts, optional } => alts.len() + usize::from(*optional),
        }
    }
}

/// Where the scanner is.
enum State {
    /// Between tokens.
    Between,
    /// Inside a plain word that started at this byte.
    Word(usize),
    /// Inside a group opened at this byte; the current alternative starts at
    /// the second byte.
    Group { open: usize, alt_start: usize },
    /// Right after a group's `)` (a `?` may follow).
    AfterGroup,
    /// Right after a group's `?`.
    AfterOptional,
}

fn syntax(pattern: &str, describe: &'static str, c: char, at: usize) -> Vec<Violation> {
    vec![
        Violation::new(
            RuleId::CMD_16,
            pattern,
            Bound::Describe(describe),
            Actual::Text(c.to_string()),
        )
        .with_note(format!("at byte {at}")),
    ]
}

const UNCLOSED: &str = "unclosed group";
const UNMATCHED: &str = "unmatched `)`";
const STRAY_BAR: &str = "`|` outside a group";
const STRAY_QUESTION: &str = "`?` must directly follow a group";
const NESTED: &str = "nested group";
const EMPTY_ALT: &str = "empty alternative";
const SPACE_IN_GROUP: &str = "whitespace inside a group";
const GLUED: &str = "a group must be a whole word";

/// Splits `pattern` into tokens, or returns the first CMD-16 syntax error.
fn tokenize(pattern: &str) -> Result<Vec<Token<'_>>, Vec<Violation>> {
    let mut tokens = Vec::new();
    let mut alts = Vec::new();
    let mut state = State::Between;
    for (at, c) in pattern.char_indices() {
        state = match state {
            State::Between => match c {
                '(' => State::Group {
                    open: at,
                    alt_start: at + 1,
                },
                ')' => return Err(syntax(pattern, UNMATCHED, c, at)),
                '|' => return Err(syntax(pattern, STRAY_BAR, c, at)),
                '?' => return Err(syntax(pattern, STRAY_QUESTION, c, at)),
                c if c.is_whitespace() => State::Between,
                _ => State::Word(at),
            },
            State::Word(start) => match c {
                '(' => return Err(syntax(pattern, GLUED, c, at)),
                ')' => return Err(syntax(pattern, UNMATCHED, c, at)),
                '|' => return Err(syntax(pattern, STRAY_BAR, c, at)),
                '?' => return Err(syntax(pattern, STRAY_QUESTION, c, at)),
                c if c.is_whitespace() => {
                    tokens.push(Token::Word(&pattern[start..at]));
                    State::Between
                }
                _ => State::Word(start),
            },
            State::Group { open, alt_start } => match c {
                '(' => return Err(syntax(pattern, NESTED, c, at)),
                '?' => return Err(syntax(pattern, STRAY_QUESTION, c, at)),
                '|' | ')' if alt_start == at => return Err(syntax(pattern, EMPTY_ALT, c, at)),
                '|' => {
                    alts.push(&pattern[alt_start..at]);
                    State::Group {
                        open,
                        alt_start: at + 1,
                    }
                }
                ')' => {
                    alts.push(&pattern[alt_start..at]);
                    State::AfterGroup
                }
                c if c.is_whitespace() => return Err(syntax(pattern, SPACE_IN_GROUP, c, at)),
                _ => State::Group { open, alt_start },
            },
            State::AfterGroup | State::AfterOptional => {
                let optional = matches!(state, State::AfterOptional);
                match c {
                    '?' if !optional => State::AfterOptional,
                    '?' => return Err(syntax(pattern, STRAY_QUESTION, c, at)),
                    '|' => return Err(syntax(pattern, STRAY_BAR, c, at)),
                    ')' => return Err(syntax(pattern, UNMATCHED, c, at)),
                    c if c.is_whitespace() => {
                        tokens.push(Token::Group {
                            alts: std::mem::take(&mut alts),
                            optional,
                        });
                        State::Between
                    }
                    _ => return Err(syntax(pattern, GLUED, c, at)),
                }
            }
        };
    }
    match state {
        State::Between => {}
        State::Word(start) => tokens.push(Token::Word(&pattern[start..])),
        State::Group { open, .. } => return Err(syntax(pattern, UNCLOSED, '(', open)),
        State::AfterGroup | State::AfterOptional => tokens.push(Token::Group {
            alts,
            optional: matches!(state, State::AfterOptional),
        }),
    }
    Ok(tokens)
}

const SPACING_HELP: &str = "separate words with exactly one ASCII space";

/// Checks that the words of a Command Path or Pattern are separated by
/// exactly one ASCII space (U+0020).
///
/// [`expand`] collapses whitespace, so `"config  set"` and `"config\tset"`
/// would expand to `config set`. `#[command]` and the App build call this
/// first, on the pattern as written, so a doubled, leading or trailing space
/// or any other whitespace fails as it does on a plain path.
///
/// Returns, in this order (path = the pattern, help
/// `separate words with exactly one ASCII space`):
///
/// - one CMD-02 for the first empty word (a leading, trailing or doubled
///   space), `between 1 and command_name_len`, actual `0`, note
///   `empty word at byte N`;
/// - one CMD-01 for the first whitespace character other than U+0020
///   (`\t`, U+00A0, `\n`, …), note `at byte N`.
///
/// An empty pattern reports nothing ([`validate_tree`](super::validate_tree)
/// reports CMD-10 for it).
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::validate_pattern_spacing;
///
/// let limits = Limits::default();
/// assert!(validate_pattern_spacing("config (set|get)? x", &limits).is_empty());
/// let v = &validate_pattern_spacing("config  set", &limits)[0];
/// assert_eq!(v.rule, RuleId::CMD_02);
/// assert_eq!(v.notes, ["empty word at byte 7"]);
/// assert_eq!(validate_pattern_spacing("a\tb", &limits)[0].rule, RuleId::CMD_01);
/// ```
pub fn validate_pattern_spacing(pattern: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    if pattern.is_empty() {
        return out;
    }
    let mut at = 0;
    for word in pattern.split(' ') {
        if word.is_empty() {
            out.push(
                Violation::new(
                    RuleId::CMD_02,
                    pattern,
                    Bound::Between {
                        min: 1,
                        max: limits.command_name_len,
                    },
                    Actual::Count(0),
                )
                .with_note(format!("empty word at byte {at}"))
                .with_help(SPACING_HELP),
            );
            break;
        }
        at += word.len() + 1;
    }
    if let Some((at, c)) = pattern
        .char_indices()
        .find(|&(_, c)| c.is_whitespace() && c != ' ')
    {
        out.push(
            Violation::new(
                RuleId::CMD_01,
                pattern,
                Bound::Describe("words separated by single ASCII spaces"),
                Actual::Text(c.to_string()),
            )
            .with_note(format!("at byte {at}"))
            .with_help(SPACING_HELP),
        );
    }
    out
}

/// Expands a Command Pattern into its Command Paths, in odometer order.
///
/// See the [module docs](super) for the grammar and the order. Errors, in this
/// precedence: the first CMD-16 syntax error (with a `at byte N` note); else a
/// CMD-15 violation when the expansion count exceeds `limits.pattern_expansions`
/// (computed before any expansion is built); else one CMD-15 violation per
/// distinct expansion produced more than once (first-occurrence order).
///
/// An empty pattern yields `[""]`; [`validate_tree`](super::validate_tree) then
/// reports CMD-10. Name rules are not checked here.
///
/// # Errors
///
/// Returns every CMD-15 violation, or the first CMD-16 one; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::expand;
///
/// let limits = Limits::default();
/// assert_eq!(expand("ping", &limits).unwrap(), ["ping"]);
/// assert_eq!(expand("a (b", &limits).unwrap_err()[0].rule, RuleId::CMD_16);
/// ```
pub fn expand(pattern: &str, limits: &Limits) -> Result<Vec<String>, Vec<Violation>> {
    let tokens = tokenize(pattern)?;
    let count = tokens
        .iter()
        .fold(1usize, |acc, token| acc.saturating_mul(token.choices()));
    if count > limits.pattern_expansions {
        return Err(vec![Violation::new(
            RuleId::CMD_15,
            pattern,
            Bound::AtMost(limits.pattern_expansions),
            Actual::Count(count),
        )]);
    }

    let mut partial: Vec<Vec<&str>> = vec![Vec::new()];
    for token in &tokens {
        partial = partial
            .into_iter()
            .flat_map(|prefix| {
                let choices: Vec<Option<&str>> = match token {
                    Token::Word(word) => vec![Some(*word)],
                    Token::Group { alts, optional } => alts
                        .iter()
                        .map(|alt| Some(*alt))
                        .chain(optional.then_some(None))
                        .collect(),
                };
                choices.into_iter().map(move |choice| {
                    let mut next = prefix.clone();
                    next.extend(choice);
                    next
                })
            })
            .collect();
    }
    let paths: Vec<String> = partial.into_iter().map(|words| words.join(" ")).collect();

    let mut seen: Vec<(&str, usize)> = Vec::new();
    for path in &paths {
        match seen.iter_mut().find(|(p, _)| p == path) {
            Some((_, n)) => *n += 1,
            None => seen.push((path, 1)),
        }
    }
    let duplicates: Vec<Violation> = seen
        .into_iter()
        .filter(|&(_, n)| n > 1)
        .map(|(path, n)| {
            Violation::new(
                RuleId::CMD_15,
                pattern,
                Bound::Describe("no duplicate expansions"),
                Actual::Text(path.to_owned()),
            )
            .with_note(format!("produced {n} times"))
        })
        .collect();
    if duplicates.is_empty() {
        Ok(paths)
    } else {
        Err(duplicates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ok(pattern: &str) -> Vec<String> {
        expand(pattern, &Limits::default()).unwrap_or_else(|e| panic!("{pattern:?}: {e:?}"))
    }

    fn err(pattern: &str) -> Vec<Violation> {
        expand(pattern, &Limits::default()).expect_err(pattern)
    }

    fn assert_syntax(pattern: &str, describe: &'static str, actual: &str, at: usize) {
        let errs = err(pattern);
        assert_eq!(errs.len(), 1, "{pattern:?}");
        let v = &errs[0];
        assert_eq!(v.rule, RuleId::CMD_16, "{pattern:?}");
        assert_eq!(v.path, pattern);
        assert_eq!(v.limit, Bound::Describe(describe), "{pattern:?}");
        assert_eq!(v.actual, Actual::Text(actual.into()), "{pattern:?}");
        assert_eq!(v.notes, [format!("at byte {at}")], "{pattern:?}");
    }

    #[test]
    fn cmd_02_spacing_empty_word() {
        let limits = Limits::default();
        for (pattern, at) in [("config  set", 7), (" ping", 0), ("ping ", 5)] {
            let errs = validate_pattern_spacing(pattern, &limits);
            assert_eq!(
                errs,
                [Violation::new(
                    RuleId::CMD_02,
                    pattern,
                    Bound::Between { min: 1, max: 32 },
                    Actual::Count(0)
                )
                .with_note(format!("empty word at byte {at}"))
                .with_help(SPACING_HELP)],
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn cmd_01_spacing_non_space_whitespace() {
        let limits = Limits::default();
        for (pattern, c, at) in [
            ("config\tset", '\t', 6),
            ("config\u{a0}set", '\u{a0}', 6),
            ("a\nb", '\n', 1),
        ] {
            let errs = validate_pattern_spacing(pattern, &limits);
            assert_eq!(
                errs,
                [Violation::new(
                    RuleId::CMD_01,
                    pattern,
                    Bound::Describe("words separated by single ASCII spaces"),
                    Actual::Text(c.to_string())
                )
                .with_note(format!("at byte {at}"))
                .with_help(SPACING_HELP)],
                "{pattern:?}"
            );
        }
        assert_eq!(
            validate_pattern_spacing("config\tset", &limits)[0].to_string(),
            "[CMD-01] config\tset: limit words separated by single ASCII spaces, actual `\\t`\n  \
             = note: at byte 6\n  = help: separate words with exactly one ASCII space"
        );
    }

    #[test]
    fn cmd_02_spacing_reports_both_rules_in_order() {
        let rules: Vec<_> = validate_pattern_spacing(" a\tb", &Limits::default())
            .iter()
            .map(|v| v.rule)
            .collect();
        assert_eq!(rules, [RuleId::CMD_02, RuleId::CMD_01]);
    }

    #[test]
    fn cmd_02_spacing_canonical_patterns_pass() {
        for pattern in ["", "a", "config (set|get)? x", "ğ ü"] {
            assert!(
                validate_pattern_spacing(pattern, &Limits::default()).is_empty(),
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn cmd_15_ac_example_expands_in_order() {
        assert_eq!(
            ok("config (set|get) (prefix|welcome)"),
            [
                "config set prefix",
                "config set welcome",
                "config get prefix",
                "config get welcome"
            ]
        );
    }

    #[test]
    fn cmd_15_optional_group_absent_choice_comes_last() {
        assert_eq!(
            ok("config (set|get) (prefix|welcome)?"),
            [
                "config set prefix",
                "config set welcome",
                "config set",
                "config get prefix",
                "config get welcome",
                "config get"
            ]
        );
    }

    #[test]
    fn cmd_15_plain_and_single_alternative() {
        assert_eq!(ok("ping"), ["ping"]);
        assert_eq!(ok("a (b)"), ["a b"]);
        assert_eq!(ok("(a|b)"), ["a", "b"]);
    }

    #[test]
    fn cmd_15_whitespace_collapses() {
        assert_eq!(ok("  ping   pong "), ["ping pong"]);
        for pattern in ["a\tb", "a\u{3000}b", "a\nb"] {
            assert_eq!(ok(pattern), ["a b"], "{pattern:?}");
        }
    }

    #[test]
    fn cmd_15_empty_pattern_yields_one_empty_path() {
        assert_eq!(ok(""), [""]);
        assert_eq!(ok("   "), [""]);
    }

    #[test]
    fn cmd_15_optional_only() {
        assert_eq!(ok("(a)?"), ["a", ""]);
        assert_eq!(ok("(a)? b"), ["a b", "b"]);
    }

    #[test]
    fn cmd_15_unicode_words_unchanged() {
        assert_eq!(
            ok("ayarlar (göster|değiştir)"),
            ["ayarlar göster", "ayarlar değiştir"]
        );
        assert_eq!(ok("設定 (表示|変更)"), ["設定 表示", "設定 変更"]);
    }

    #[test]
    fn cmd_15_duplicate_alternative() {
        let errs = err("a (b|b)");
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CMD_15,
                "a (b|b)",
                Bound::Describe("no duplicate expansions"),
                Actual::Text("a b".into())
            )
            .with_note("produced 2 times")]
        );
    }

    #[test]
    fn cmd_15_duplicates_reported_once_each_in_first_occurrence_order() {
        let errs = err("(a|a|b|b|c)");
        let texts: Vec<_> = errs.iter().map(|v| (&v.actual, &v.notes[0])).collect();
        assert_eq!(
            texts,
            [
                (&Actual::Text("a".into()), &"produced 2 times".to_owned()),
                (&Actual::Text("b".into()), &"produced 2 times".to_owned()),
            ]
        );
    }

    #[test]
    fn cmd_15_optional_duplicate() {
        let errs = err("(a)? (a)?");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].rule, RuleId::CMD_15);
        assert_eq!(errs[0].actual, Actual::Text("a".into()));
        assert_eq!(errs[0].notes, ["produced 2 times"]);
    }

    #[test]
    fn cmd_15_case_difference_is_not_a_duplicate() {
        assert_eq!(ok("a (b|B)"), ["a b", "a B"]);
    }

    #[test]
    fn cmd_15_cap_boundary() {
        assert_eq!(ok("x (a|b|c|d|e) (f|g|h|i|j)").len(), 25);
    }

    #[test]
    fn cmd_15_cap_plus_one() {
        let over = Violation::new(RuleId::CMD_15, "", Bound::AtMost(25), Actual::Count(26));
        for pattern in [
            "x (a|b|c|d|e|f|g|h|i|j|k|l|m) (n|o)",
            "x (a|b|c|d|e|f|g|h|i|j|k|l)? (m)?",
        ] {
            let errs = err(pattern);
            assert_eq!(
                errs,
                [Violation {
                    path: pattern.into(),
                    ..over.clone()
                }]
            );
        }
        // Duplicates are not checked once the cap is exceeded.
        let errs = err("x (a|a|b|c|d|e|f|g|h|i|j|k|l) (n|o)");
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].limit, Bound::AtMost(25));
    }

    #[test]
    fn cmd_15_custom_cap() {
        let pattern = "x (a|b|c|d|e|f|g|h|i|j|k|l|m) (n|o)";
        let mut limits = Limits {
            pattern_expansions: 30,
            ..Limits::default()
        };
        assert_eq!(expand(pattern, &limits).map(|p| p.len()), Ok(26));
        limits.pattern_expansions = 1;
        let errs = expand("(a|b)", &limits).expect_err("over the cap");
        assert_eq!(
            (&errs[0].limit, &errs[0].actual),
            (&Bound::AtMost(1), &Actual::Count(2))
        );
        limits.pattern_expansions = 0;
        let errs = expand("ping", &limits).expect_err("over the cap");
        assert_eq!(
            (&errs[0].limit, &errs[0].actual),
            (&Bound::AtMost(0), &Actual::Count(1))
        );
    }

    #[test]
    fn cmd_15_count_saturates() {
        let group = "(a|b|c|d|e|f|g|h|i|j)";
        let pattern = vec![group; 30].join(" ");
        let errs = err(&pattern);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].rule, RuleId::CMD_15);
        assert_eq!(errs[0].actual, Actual::Count(usize::MAX));
    }

    #[test]
    fn cmd_16_unclosed_group() {
        assert_syntax("a (b", UNCLOSED, "(", 2);
        assert_syntax("ğ (x", UNCLOSED, "(", 3);
    }

    #[test]
    fn cmd_16_unmatched_close() {
        assert_syntax("a b)", UNMATCHED, ")", 3);
        assert_syntax("(a))", UNMATCHED, ")", 3);
    }

    #[test]
    fn cmd_16_stray_bar() {
        assert_syntax("a | b", STRAY_BAR, "|", 2);
        assert_syntax("a|b", STRAY_BAR, "|", 1);
    }

    #[test]
    fn cmd_16_stray_question_mark() {
        assert_syntax("a ?", STRAY_QUESTION, "?", 2);
        assert_syntax("a?", STRAY_QUESTION, "?", 1);
        assert_syntax("a (b)??", STRAY_QUESTION, "?", 6);
        assert_syntax("a (b?)", STRAY_QUESTION, "?", 4);
    }

    #[test]
    fn cmd_16_nested_group() {
        assert_syntax("a ((b))", NESTED, "(", 3);
    }

    #[test]
    fn cmd_16_empty_alternative() {
        assert_syntax("a ()", EMPTY_ALT, ")", 3);
        assert_syntax("a (b|)", EMPTY_ALT, ")", 5);
        assert_syntax("a (|b)", EMPTY_ALT, "|", 3);
    }

    #[test]
    fn cmd_16_whitespace_inside_group() {
        assert_syntax("a (b c)", SPACE_IN_GROUP, " ", 4);
        assert_syntax("a ( b)", SPACE_IN_GROUP, " ", 3);
    }

    #[test]
    fn cmd_16_glued_group() {
        assert_syntax("a(b)", GLUED, "(", 1);
        assert_syntax("a (b)c", GLUED, "c", 5);
        assert_syntax("a (b)?c", GLUED, "c", 6);
        assert_syntax("(a)(b)", GLUED, "(", 3);
    }

    #[test]
    fn cmd_16_reports_only_the_first_error() {
        assert_syntax("a | b)", STRAY_BAR, "|", 2);
    }

    #[derive(Clone, Debug)]
    enum Tok {
        Word(String),
        Group(Vec<String>, bool),
    }

    fn tok() -> impl Strategy<Value = Tok> {
        prop_oneof![
            "[a-z]{1,3}".prop_map(Tok::Word),
            (
                proptest::collection::btree_set("[a-z]{1,3}", 1..=3),
                any::<bool>()
            )
                .prop_map(|(alts, optional)| Tok::Group(alts.into_iter().collect(), optional)),
        ]
    }

    fn render(tokens: &[Tok]) -> String {
        tokens
            .iter()
            .map(|t| match t {
                Tok::Word(w) => w.clone(),
                Tok::Group(alts, optional) => {
                    format!("({}){}", alts.join("|"), if *optional { "?" } else { "" })
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Reference model: recursive odometer.
    fn model(tokens: &[Tok]) -> Vec<String> {
        let Some((first, rest)) = tokens.split_first() else {
            return vec![String::new()];
        };
        let tails = model(rest);
        let choices: Vec<Option<&String>> = match first {
            Tok::Word(w) => vec![Some(w)],
            Tok::Group(alts, optional) => alts
                .iter()
                .map(Some)
                .chain(optional.then_some(None))
                .collect(),
        };
        let mut out = Vec::new();
        for choice in choices {
            for tail in &tails {
                let joined = match (choice, tail.is_empty()) {
                    (Some(w), true) => w.clone(),
                    (Some(w), false) => format!("{w} {tail}"),
                    (None, _) => tail.clone(),
                };
                out.push(joined);
            }
        }
        out
    }

    proptest! {
        #[test]
        fn cmd_15_property_count_and_order(tokens in proptest::collection::vec(tok(), 1..=4)) {
            let expected = model(&tokens);
            let mut unique = expected.clone();
            unique.sort();
            unique.dedup();
            prop_assume!(unique.len() == expected.len());
            let limits = Limits {
                pattern_expansions: usize::MAX,
                ..Limits::default()
            };
            let pattern = render(&tokens);
            let paths = expand(&pattern, &limits).expect("valid pattern");
            let product: usize = tokens
                .iter()
                .map(|t| match t {
                    Tok::Word(_) => 1,
                    Tok::Group(alts, optional) => alts.len() + usize::from(*optional),
                })
                .product();
            prop_assert_eq!(paths.len(), product);
            for path in &paths {
                prop_assert_eq!(path, &path.split_whitespace().collect::<Vec<_>>().join(" "));
            }
            prop_assert_eq!(paths, expected);
        }
    }
}
