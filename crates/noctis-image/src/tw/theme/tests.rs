//! TW-02: the Theme file grammar, its errors (with 1-based positions) and
//! the `ThemeTokens` impl.

use super::*;
use crate::tw::validate;

/// The facade's test theme (`crates/noctis/noctis-theme.css`).
const TEST_THEME: &str = "/* The facade's test theme. */
@theme {
  --color-brand: #5865f2;
  --color-brand-soft: #8b93f8;
  --color-surface: #11131a;
  --color-muted: #9aa3b5;
  --font-display: \"Inter\", sans-serif;
}

@keyframes rise {
  from { opacity: 0; }
  to { opacity: 1; }
}
";

fn err(source: &str) -> ThemeError {
    Theme::parse(source).expect_err(source)
}

#[track_caller]
fn assert_err(source: &str, kind: &ThemeErrorKind, line: u32, column: u32) {
    let e = err(source);
    assert_eq!(
        (e.kind(), e.line(), e.column()),
        (kind, line, column),
        "{source:?}"
    );
}

fn name(name: &str) -> String {
    name.to_owned()
}

#[test]
fn tw_02_test_theme_tokens_and_keyframes() {
    let theme = Theme::parse(TEST_THEME).unwrap();
    assert!(theme.contains(ThemeNamespace::Color, "brand"));
    assert!(theme.contains(ThemeNamespace::Color, "brand-soft"));
    assert!(theme.contains(ThemeNamespace::Font, "display"));
    assert!(!theme.contains(ThemeNamespace::Text, "brand"));
    assert!(!theme.contains(ThemeNamespace::Color, "display"));
    let keyframes: Vec<_> = theme.keyframes().collect();
    assert_eq!(
        keyframes,
        [("rise", "from { opacity: 0; }\n  to { opacity: 1; }")]
    );
    assert_eq!(theme.animations().count(), 0);
}

#[test]
fn tw_02_values_are_trimmed_and_verbatim() {
    let theme = Theme::parse("@theme { --color-a:   rgb(1 2 3)  /* c */ ; }").unwrap();
    assert_eq!(
        theme.tokens[slot(ThemeNamespace::Color)]["a"],
        "rgb(1 2 3)  /* c */"
    );
}

#[test]
fn tw_02_longest_prefix_wins() {
    let theme = Theme::parse("@theme { --font-weight-heavy: 900; }").unwrap();
    assert!(theme.contains(ThemeNamespace::FontWeight, "heavy"));
    assert!(!theme.contains(ThemeNamespace::Font, "weight-heavy"));
}

#[test]
fn tw_02_animate_entries_are_animations_not_tokens() {
    let theme = Theme::parse("@theme { --animate-rise: rise 1s; }").unwrap();
    assert_eq!(
        theme.animations().collect::<Vec<_>>(),
        [("rise", "rise 1s")]
    );
    // Theme animations draw only from Story 10.6 on.
    assert!(validate("animate-rise", &theme).is_err());
}

#[test]
fn tw_02_empty_and_comment_only_files() {
    for source in ["", "/* nothing */", " \n\t/* a */ /* b */\n"] {
        let theme = Theme::parse(source).unwrap();
        assert!(theme.tokens.iter().all(BTreeMap::is_empty), "{source:?}");
        assert_eq!(theme.animations().count(), 0);
        assert_eq!(theme.keyframes().count(), 0);
    }
    assert_eq!(
        Theme::parse("").unwrap().digest(),
        *blake3::hash(b"").as_bytes()
    );
    assert_eq!(Theme::empty().digest(), *blake3::hash(b"").as_bytes());
    assert_eq!(Theme::parse("").unwrap(), Theme::empty());
}

#[test]
fn tw_02_digest_is_the_blake3_of_the_source() {
    let theme = Theme::parse(TEST_THEME).unwrap();
    assert_eq!(
        theme.digest(),
        *blake3::hash(TEST_THEME.as_bytes()).as_bytes()
    );
    let spaced = format!("{TEST_THEME}\n");
    assert_ne!(Theme::parse(&spaced).unwrap().digest(), theme.digest());
}

#[test]
fn tw_02_blocks_merge_and_comments_are_allowed() {
    let theme = Theme::parse(
        "/* a */ @theme /* b */ { /* c */ --color-a: red; ; --text-title: 2rem; }\n\
         @theme { --radius-card: 1rem; }",
    )
    .unwrap();
    assert!(theme.contains(ThemeNamespace::Color, "a"));
    assert!(theme.contains(ThemeNamespace::Text, "title"));
    assert!(theme.contains(ThemeNamespace::Radius, "card"));
}

#[test]
fn tw_02_unexpected_rules() {
    assert_err(".card { color: red }", &UnexpectedRule, 1, 1);
    assert_err("\n  @import \"x\";", &UnexpectedRule, 2, 3);
    assert_err("@theme inline {}", &UnexpectedRule, 1, 8);
    assert_err("@theme { @media print {} }", &UnexpectedRule, 1, 10);
}

#[test]
fn tw_02_not_custom_property() {
    assert_err("@theme { color: red; }", &NotCustomProperty, 1, 10);
}

#[test]
fn tw_02_unknown_namespace_lists_every_prefix() {
    let e = err("@theme {\n  --foo-x: 1;\n}");
    assert_eq!(
        (e.kind(), e.line(), e.column()),
        (
            &UnknownNamespace {
                name: name("--foo-x")
            },
            2,
            3
        )
    );
    let text = e.to_string();
    for ns in NAMESPACES {
        assert!(text.contains(ns.css_prefix()), "{text}");
    }
    assert!(text.contains("`--animate-`"), "{text}");
}

#[test]
fn tw_02_invalid_tokens() {
    for prop in [
        "--color-Brand",
        "--color-",
        "--color-a-",
        "--color-r\u{f8}d",
    ] {
        assert_err(
            &format!("@theme {{ {prop}: #fff; }}"),
            &InvalidToken { name: name(prop) },
            1,
            10,
        );
    }
    let ok = format!("--color-{}", "a".repeat(64));
    assert!(Theme::parse(&format!("@theme {{ {ok}: #fff; }}")).is_ok());
    let long = format!("--color-{}", "a".repeat(65));
    assert_err(
        &format!("@theme {{ {long}: #fff; }}"),
        &InvalidToken { name: long.clone() },
        1,
        10,
    );
    assert_err(
        "@theme { --animate-Rise: x; }",
        &InvalidToken {
            name: name("--animate-Rise"),
        },
        1,
        10,
    );
}

#[test]
fn tw_02_unicode_token_is_invalid() {
    assert_err(
        "@theme { --color-r\u{f8}d: #f00; }",
        &InvalidToken {
            name: name("--color-r\u{f8}d"),
        },
        1,
        10,
    );
}

#[test]
fn tw_02_empty_value() {
    assert_err(
        "@theme { --color-a: ; }",
        &EmptyValue {
            name: name("--color-a"),
        },
        1,
        10,
    );
    assert_err(
        "@theme { --color-a:\n}",
        &EmptyValue {
            name: name("--color-a"),
        },
        1,
        10,
    );
}

#[test]
fn tw_02_duplicates_across_blocks() {
    assert_err(
        "@theme { --color-a: red; }\n@theme {\n  --color-a: blue;\n}",
        &Duplicate {
            name: name("--color-a"),
        },
        3,
        3,
    );
    assert_err(
        "@theme { --animate-a: x; --animate-a: y; }",
        &Duplicate {
            name: name("--animate-a"),
        },
        1,
        26,
    );
    assert_err(
        "@keyframes rise {}\n@keyframes rise {}",
        &Duplicate { name: name("rise") },
        2,
        12,
    );
}

#[test]
fn tw_02_builtin_animations() {
    assert_err(
        "@keyframes spin {}",
        &BuiltinAnimation { name: name("spin") },
        1,
        12,
    );
    assert_err(
        "@theme { --animate-pulse: x; }",
        &BuiltinAnimation {
            name: name("--animate-pulse"),
        },
        1,
        10,
    );
    for builtin in BUILTIN_ANIMATIONS {
        assert!(Theme::parse(&format!("@keyframes {builtin} {{}}")).is_err());
    }
}

#[test]
fn tw_02_syntax_errors() {
    assert_err("@theme {", &Syntax, 1, 8);
    assert_err("@theme {\n  --color-a: red;\n", &Syntax, 1, 8);
    assert_err("@keyframes rise {", &Syntax, 1, 17);
    assert_err("@theme { --color-a red; }", &Syntax, 1, 20);
    assert_err("@theme { 5px }", &Syntax, 1, 10);
    assert_err("@keyframes \"rise\" {}", &Syntax, 1, 12);
    assert_err("@keyframes rise;", &Syntax, 1, 16);
    assert_err("@theme", &Syntax, 1, 1);
}

#[test]
fn tw_02_size_boundary() {
    let pad = |total: usize| {
        let body = "@theme { --color-a: red; }\n/*";
        let mut text = String::from(body);
        text.push_str(&"x".repeat(total - body.len() - 2));
        text.push_str("*/");
        assert_eq!(text.len(), total);
        text
    };
    assert!(Theme::parse(&pad(MAX_THEME_BYTES)).is_ok());
    assert_err(
        &pad(MAX_THEME_BYTES + 1),
        &ThemeErrorKind::TooLarge {
            bytes: MAX_THEME_BYTES + 1,
        },
        1,
        1,
    );
    assert_eq!(MAX_THEME_BYTES, 262_144);
}

#[test]
fn tw_02_display() {
    assert_eq!(
        err("@keyframes spin {}").to_string(),
        "[TW-02] 1:12: `spin` redefines a built-in animation (`spin`, `ping`, `pulse`, \
         `bounce`), which would render static; give it another name"
    );
    assert_eq!(
        err("@theme { --color-a: ; }").to_string(),
        "[TW-02] 1:10: `--color-a` has an empty value"
    );
}

#[test]
#[should_panic(expected = "validated at compile time")]
fn tw_02_embedded_panics_on_an_invalid_file() {
    let _ = Theme::__embedded("@keyframes spin {}");
}

#[test]
fn tw_02_embedded_parses_a_valid_file() {
    assert_eq!(
        Theme::__embedded(TEST_THEME),
        Theme::parse(TEST_THEME).unwrap()
    );
}

#[test]
fn theme_tokens_satisfy_validate() {
    let theme = Theme::parse(TEST_THEME).unwrap();
    assert!(
        validate(
            "flex bg-brand text-muted bg-surface/50 font-display",
            &theme
        )
        .is_ok()
    );
    let bad = validate("bg-nope from-brand", &theme).unwrap_err();
    let names: Vec<&str> = bad
        .classes()
        .iter()
        .map(crate::tw::UnknownClass::class)
        .collect();
    assert_eq!(names, ["bg-nope", "from-brand"]);
    let bp = Theme::parse("@theme { --breakpoint-hover: 1px; }").unwrap();
    assert!(validate("hover:flex", &bp).is_ok());
}

#[test]
fn every_namespace_has_its_slot() {
    for (index, ns) in NAMESPACES.iter().enumerate() {
        assert_eq!(slot(*ns), index);
    }
}
