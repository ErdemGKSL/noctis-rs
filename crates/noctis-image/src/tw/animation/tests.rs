//! The parse rows of the Story 10.6 I/O matrix.

use super::*;

fn under(value: &str) -> Option<&str> {
    parse(value, Sep::Underscore)
}

fn rows(cases: &[(&str, bool)]) {
    for &(value, ok) in cases {
        assert_eq!(under(value).is_some(), ok, "{value}");
    }
}

#[test]
fn happy_values_name_their_keyframes() {
    for value in [
        "rise_1s",
        "rise_1.5s_ease-in-out_infinite_alternate_both_running_200ms",
        "1s_rise",
        "rise_0.001s",
    ] {
        assert_eq!(under(value), Some("rise"), "{value}");
    }
}

#[test]
fn times() {
    rows(&[
        ("rise_0s", false),
        ("rise_0ms", false),
        ("rise_0.000s", false),
        ("rise_1s_0s", true),
        ("rise_999999s", true),
        ("rise_1000000s", false),
        ("rise_1.2345s", false),
        ("rise_-1s", false),
        ("rise_1e3s", false),
        ("rise_1s_2s_3s", false),
        ("rise_1S", false),
        ("rise_01s", false),
        ("rise_.5s", false),
    ]);
}

#[test]
fn counts() {
    rows(&[
        ("rise_1s_1", true),
        ("rise_1s_9999", true),
        ("rise_1s_10000", false),
        ("rise_1s_0", false),
        ("rise_1s_1.5", false),
        ("rise_1s_infinite_2", false),
    ]);
}

#[test]
fn names() {
    let long = "a".repeat(64);
    let longer = "a".repeat(65);
    assert_eq!(under(&format!("{long}_1s")), Some(long.as_str()));
    assert_eq!(under(&format!("{longer}_1s")), None);
    assert_eq!(under("ri-se_1s"), Some("ri-se"));
    rows(&[
        ("1s_linear", false),
        ("a_b_1s", false),
        ("none_1s", false),
        ("Rise_1s", false),
        ("rise-_1s", false),
        ("ease_1s", false),
        ("inherit_1s", false),
    ]);
}

#[test]
fn each_category_appears_once() {
    rows(&[
        ("rise_1s_linear_ease", false),
        ("rise_1s_normal_reverse", false),
        ("rise_1s_both_none", false),
        ("rise_1s_running_paused", false),
    ]);
}

#[test]
fn separators() {
    for value in ["_rise_1s", "rise__1s", "rise_1s_", "", "rise 1s"] {
        assert_eq!(under(value), None, "{value:?}");
    }
    assert_eq!(parse("rise  1s\t", Sep::Whitespace), Some("rise"));
    assert_eq!(parse("  ", Sep::Whitespace), None);
}

#[test]
fn functions_commas_and_non_ascii_are_rejected() {
    rows(&[
        ("rise_1s_cubic-bezier(0,0,1,1)", false),
        ("rise_1s,b_1s", false),
        ("var(--x)_1s", false),
        ("rise_1s_steps(4)", false),
        ("rïse_1s", false),
        ("ri\\se_1s", false),
    ]);
}

#[test]
fn keyframes_names_and_tokens() {
    assert!(is_keyframes_name("rise"));
    assert!(is_keyframes_name("fade-2"));
    assert!(!is_keyframes_name("2fade"));
    assert!(!is_keyframes_name("a_b"));
    assert!(!is_keyframes_name("none"));
    assert!(!is_keyframes_name("revert-layer"));
    assert!(!is_keyframes_name(""));
    assert!(is_animation_token("fade-in"));
    assert!(is_animation_token("a_b"));
    assert!(!is_animation_token("5"));
    assert!(!is_animation_token("a.b"));
    assert!(!is_animation_token("Fade"));
    assert!(!is_animation_token(""));
}
