//! Story 6.7 Matrix 3-14 and 25-27: decoding and the choice filter;
//! Story 6.11 Matrix 24-27: `Suggestions` and their lowering.

use noctis_rules::LimitMode;
use noctis_ui::ResolvedChannel;
use serde_json::json;

use super::*;
use crate::options::__options::partial;

/// An autocomplete interaction of `config set prefix` with `leaves`.
fn raw(leaves: &Value) -> Vec<u8> {
    json!({
        "type": 4,
        "data": {
            "name": "config",
            "type": 1,
            "options": [{ "type": 2, "name": "set", "options": [
                { "type": 1, "name": "prefix", "options": leaves }
            ]}],
            "resolved": { "channels": { "5": {
                "id": "5", "name": "general", "type": 0, "permissions": "0"
            }}}
        }
    })
    .to_string()
    .into_bytes()
}

fn parse(leaves: &Value) -> Result<(CommandInput, Focused), OptionError> {
    AutocompleteInput::from_json(&raw(leaves))
}

/// A focused option of `kind` with `value`.
fn focused(kind: u64, value: &Value) -> Focused {
    let leaves = json!([{ "type": kind, "name": "value", "value": value, "focused": true }]);
    parse(&leaves).unwrap().1
}

/// A detached context.
fn ctx() -> AutocompleteCtx {
    AutocompleteCtx::new(AutocompleteParts {
        path: "config set prefix",
        module: "m",
        shard: 0,
        guild_id: None,
        channel_id: None,
        user_id: Id::new(1),
        locales: crate::locale::LocaleCell::new(crate::Locales::both(noctis_rules::Locale::Tr)),
        focused: focused(3, &json!("")),
        app: None,
    })
}

/// What a method's choices collect to without label lowering: the first
/// 25 and how many there were.
fn collected(mut choices: Vec<AutocompleteChoice>) -> Collected {
    let returned = choices.len();
    choices.truncate(25);
    Collected {
        choices,
        returned,
        labels_cut: 0,
    }
}

fn strings(n: usize) -> Vec<AutocompleteChoice> {
    (0..n)
        .map(|i| AutocompleteChoice::new(format!("c{i}"), format!("v{i}")))
        .collect()
}

fn names(choices: &[AutocompleteChoice]) -> Vec<&str> {
    choices.iter().map(AutocompleteChoice::name).collect()
}

/// Matrix 2 and 3 (OPT-12): every value that does not decode is `None`;
/// unknown options are ignored; the focused value is in the input.
#[test]
fn opt_12_partial_input_is_lenient() {
    let leaves = json!([
        { "type": 3, "name": "value", "value": "pr", "focused": true },
        { "type": 4, "name": "limit", "value": "abc" },
        { "type": 7, "name": "channel", "value": "5" },
        { "type": 7, "name": "gone", "value": "6" },
        { "type": 3, "name": "extra", "value": "x" },
    ]);
    let (input, focused) = parse(&leaves).unwrap();
    assert_eq!(input.path(), "config set prefix");
    assert_eq!(focused.name(), "value");
    assert_eq!(partial::<String>(&input, "value").as_deref(), Some("pr"));
    assert_eq!(partial::<i64>(&input, "limit"), None);
    assert_eq!(
        partial::<ResolvedChannel>(&input, "channel").map(|c| c.channel.id.get()),
        Some(5)
    );
    assert!(partial::<ResolvedChannel>(&input, "gone").is_none());
    assert_eq!(partial::<i64>(&input, "missing"), None);
    // A STRING sent where an INTEGER is declared is mistyped, not an error.
    assert_eq!(partial::<i64>(&input, "extra"), None);
}

/// Matrix 4: the focused STRING text, exactly.
#[test]
fn focused_string_is_exact() {
    for text in ["", "çğ😀"] {
        let f = focused(3, &json!(text));
        assert_eq!(f.value(), text);
        assert_eq!(f.kind(), OptionType::String);
        let leaves = json!([{ "type": 3, "name": "value", "value": text, "focused": true }]);
        let (input, _) = parse(&leaves).unwrap();
        assert_eq!(partial::<String>(&input, "value").as_deref(), Some(text));
    }
}

/// Matrix 5 and 6 (OPT-12): numeric text parses only when valid.
#[test]
fn opt_12_focused_numbers_parse_leniently() {
    let cases = [
        (json!("12"), Some(12)),
        (json!("-"), None),
        (json!("99999999999999999999"), None),
        (json!(12), Some(12)),
    ];
    for (value, expected) in cases {
        let f = focused(4, &value);
        assert_eq!(f.as_integer(), expected, "{value}");
        let leaves = json!([{ "type": 4, "name": "value", "value": value, "focused": true }]);
        let (input, _) = parse(&leaves).unwrap();
        assert_eq!(partial::<i64>(&input, "value"), expected, "{value}");
    }
    assert_eq!(focused(4, &json!(12)).value(), "12");
    assert_eq!(focused(10, &json!("1.5")).as_number(), Some(1.5));
    for text in ["NaN", "inf"] {
        let f = focused(10, &json!(text));
        assert_eq!(f.as_number(), None, "{text}");
        let leaves = json!([{ "type": 10, "name": "value", "value": text, "focused": true }]);
        let (input, _) = parse(&leaves).unwrap();
        assert_eq!(partial::<f64>(&input, "value"), None);
    }
}

/// Matrix 25 and 26: not exactly one focused option, or a bad shape.
#[test]
fn focus_and_shape_errors_are_malformed() {
    let none = json!([{ "type": 3, "name": "value", "value": "a" }]);
    let two = json!([
        { "type": 3, "name": "a", "value": "a", "focused": true },
        { "type": 3, "name": "b", "value": "b", "focused": true },
    ]);
    for leaves in [none, two, json!({ "not": "an array" })] {
        assert!(
            matches!(parse(&leaves), Err(OptionError::Malformed { .. })),
            "{leaves}"
        );
    }
    let command = json!({ "type": 2, "data": { "name": "x", "type": 1 } }).to_string();
    assert!(matches!(
        AutocompleteInput::from_json(command.as_bytes()),
        Err(OptionError::NotSlashCommand {
            interaction_type: 2,
            ..
        })
    ));
}

/// Matrix 7 and 8: no choices, and exactly 25.
#[test]
fn empty_and_full_lists_pass() {
    let limits = Limits::default();
    let (sent, warnings) = filter_choices(collected(Vec::new()), OptionType::String, &limits);
    assert!(sent.is_empty());
    assert_eq!(warnings, Warnings::default());
    let (sent, warnings) = filter_choices(collected(strings(25)), OptionType::String, &limits);
    assert_eq!(sent.len(), 25);
    assert_eq!(warnings, Warnings::default());
    assert_eq!(
        callback_body(&[]),
        json!({ "type": 8, "data": { "choices": [] } })
    );
}

/// Matrix 9 (INT-07): more than 25 is truncated, in both limit modes.
#[test]
fn int_07_truncates_in_both_modes() {
    for mode in [LimitMode::Strict, LimitMode::Lenient] {
        let mut limits = Limits::default();
        limits.mode = mode;
        for n in [26, 30] {
            let (sent, warnings) =
                filter_choices(collected(strings(n)), OptionType::String, &limits);
            assert_eq!(sent.len(), 25);
            assert_eq!(names(&sent)[24], "c24");
            assert_eq!(warnings.truncated, Some((n, 25)));
            assert_eq!(warnings.dropped, 0);
        }
    }
}

/// Matrix 10: name lengths in UTF-16 units.
#[test]
fn name_lengths_are_checked() {
    let choices = vec![
        AutocompleteChoice::new("a", "1"),
        AutocompleteChoice::new("b".repeat(100), "2"),
        AutocompleteChoice::new("c".repeat(101), "3"),
        AutocompleteChoice::new("", "4"),
        AutocompleteChoice::new("𝔸".repeat(50), "5"),
        AutocompleteChoice::new("𝔸".repeat(51), "6"),
    ];
    let (sent, warnings) =
        filter_choices(collected(choices), OptionType::String, &Limits::default());
    let values: Vec<_> = sent.iter().map(|c| c.value().clone()).collect();
    assert_eq!(
        values,
        ["1", "2", "5"].map(|v| AutocompleteValue::String(v.into()))
    );
    assert_eq!(warnings.dropped, 3);
    assert_eq!(warnings.rule_list(), "OPT-09");
}

/// Matrix 11: string value lengths.
#[test]
fn value_lengths_are_checked() {
    let choices = vec![
        AutocompleteChoice::new("a", ""),
        AutocompleteChoice::new("b", "v".repeat(100)),
        AutocompleteChoice::new("c", "v".repeat(101)),
    ];
    let (sent, warnings) =
        filter_choices(collected(choices), OptionType::String, &Limits::default());
    assert_eq!(names(&sent), ["a", "b"]);
    assert_eq!(warnings.dropped, 1);
}

/// Matrix 12 and 13: value types and numeric ranges.
#[test]
fn value_types_and_ranges_are_checked() {
    let limits = Limits::default();
    let max = (1_i64 << 53) - 1;
    let choices = vec![
        AutocompleteChoice::new("s", "1"),
        AutocompleteChoice::new("big", max + 1),
        AutocompleteChoice::new("ok", max),
    ];
    let (sent, warnings) = filter_choices(collected(choices), OptionType::Integer, &limits);
    assert_eq!(names(&sent), ["ok"]);
    assert_eq!(warnings.dropped, 2);
    assert_eq!(warnings.rule_list(), "OPT-09, OPT-03");
    let choices = vec![
        AutocompleteChoice::new("int", 3_i64),
        AutocompleteChoice::new("nan", f64::NAN),
        AutocompleteChoice::new("inf", f64::INFINITY),
        AutocompleteChoice::new("half", 0.5),
    ];
    let (sent, warnings) = filter_choices(collected(choices), OptionType::Number, &limits);
    assert_eq!(names(&sent), ["int", "half"]);
    assert_eq!(warnings.rule_list(), "OPT-04");
    assert_eq!(
        callback_body(&sent),
        json!({ "type": 8, "data": { "choices": [
            { "name": "int", "value": 3 },
            { "name": "half", "value": 0.5 },
        ]}})
    );
}

/// Matrix 14: truncation happens before the check.
#[test]
fn truncation_comes_before_the_check() {
    let mut choices = strings(30);
    choices[2] = AutocompleteChoice::new("", "bad");
    choices[26] = AutocompleteChoice::new("", "bad");
    let (sent, warnings) =
        filter_choices(collected(choices), OptionType::String, &Limits::default());
    assert_eq!(sent.len(), 24);
    assert!(!names(&sent).contains(&""));
    assert_eq!(warnings.truncated, Some((30, 25)));
    assert_eq!(warnings.dropped, 1);
}

/// Matrix 27: a type without autocomplete answers nothing.
#[test]
fn boolean_focus_sends_nothing() {
    let f = focused(5, &json!(true));
    assert_eq!(f.kind(), OptionType::Boolean);
    let (sent, warnings) = filter_choices(collected(strings(2)), f.kind(), &Limits::default());
    assert!(sent.is_empty());
    assert!(warnings.not_choosable);
}

/// Story 6.11 Matrix 24: 0, 25 and 26 suggestions collect to 0, 25 and
/// the first 25.
#[test]
fn suggestions_collect_the_first_25() {
    let labels = ctx().labels();
    let limits = Limits::default();
    for (n, kept) in [(0, 0), (25, 25), (26, 25)] {
        let s: Suggestions<i64> = (0_i64..).take(n).map(|i| (i, format!("c{i}"))).collect();
        assert_eq!((s.len(), s.is_empty()), (n, n == 0));
        let c = collect(s.lower(), &labels, &limits);
        assert_eq!((c.choices.len(), c.returned), (kept, n));
        let (sent, warnings) = filter_choices(c, OptionType::Integer, &limits);
        assert_eq!(sent.len(), kept);
        assert_eq!(warnings.truncated, (n == 26).then_some((26, 25)));
    }
}

/// Story 6.11 Matrix 25: labels are cut to 100 units with `…`, a
/// `MessageKey` renders in the user's locale, falling back to `en-US` (Builtin without an App).
#[test]
fn labels_are_cut_and_resolved() {
    let labels = ctx().labels();
    let limits = Limits::default();
    let mut s = Suggestions::new();
    s.push("a".to_owned(), "x".repeat(100));
    s.push("b".to_owned(), "x".repeat(101));
    s.push("c".to_owned(), "😀".repeat(51));
    s.push("d".to_owned(), "");
    s.push("e".to_owned(), noctis_i18n::keys::GUARD_OWNER);
    let c = collect(s.lower(), &labels, &limits);
    assert_eq!(c.labels_cut, 2);
    let names = names(&c.choices);
    assert_eq!(names[0].len(), 100);
    assert_eq!(noctis_rules::len::discord_len(names[1]), 100);
    assert!(names[1].ends_with('…'));
    assert!(noctis_rules::len::discord_len(names[2]) <= 100);
    assert!(names[2].ends_with('…'));
    assert_eq!(names[4], "Only the person who started this can use it.");
    let (sent, warnings) = filter_choices(c, OptionType::String, &limits);
    assert_eq!(sent.len(), 4, "the empty label is dropped");
    assert_eq!((warnings.dropped, warnings.labels_cut), (1, 2));
}

/// Story 6.11 Matrix 25: with the App's translator, a `MessageKey` label is
/// that translator's text in the user's (non-English) locale.
#[test]
fn key_label_uses_the_app_translator_in_the_user_locale() {
    struct Strings;
    impl Translate for Strings {
        fn translate(
            &self,
            locale: noctis_rules::Locale,
            key: MessageKey,
            _: &[(&str, &dyn fmt::Display)],
        ) -> Option<String> {
            Some(format!("{locale:?}:{}", key.as_str()))
        }
    }
    // The context's locale (user `tr`), with the App's strings it would carry.
    let labels = Labels {
        strings: Some(Arc::new(Strings)),
        ..ctx().labels()
    };
    let mut s = Suggestions::new();
    s.push("e".to_owned(), noctis_i18n::keys::GUARD_OWNER);
    let c = collect(s.lower(), &labels, &Limits::default());
    let expected = format!(
        "{:?}:{}",
        noctis_rules::Locale::Tr,
        noctis_i18n::keys::GUARD_OWNER.as_str()
    );
    assert_eq!(names(&c.choices), [expected.as_str()]);
}

/// Story 6.11 Matrix 26: values Discord would reject are dropped.
#[test]
fn bad_values_are_dropped() {
    let labels = ctx().labels();
    let limits = Limits::default();
    let max = (1_i64 << 53) - 1;
    let ints: Suggestions<i64> = [max, -max, max + 1, -max - 1]
        .into_iter()
        .map(|v| (v, "n"))
        .collect();
    let (sent, warnings) = filter_choices(
        collect(ints.lower(), &labels, &limits),
        OptionType::Integer,
        &limits,
    );
    assert_eq!(sent.len(), 2);
    assert_eq!(warnings.dropped, 2);
    let floats: Suggestions<f64> = [1.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
        .into_iter()
        .map(|v| (v, "n"))
        .collect();
    let (sent, warnings) = filter_choices(
        collect(floats.lower(), &labels, &limits),
        OptionType::Number,
        &limits,
    );
    assert_eq!(sent.len(), 1);
    assert_eq!(warnings.dropped, 3);
    let strings: Suggestions<String> = ["v".repeat(100), "v".repeat(101)]
        .into_iter()
        .map(|v| (v, "n"))
        .collect();
    let (sent, warnings) = filter_choices(
        collect(strings.lower(), &labels, &limits),
        OptionType::String,
        &limits,
    );
    assert_eq!(sent.len(), 1);
    assert_eq!(warnings.dropped, 1);
}

/// Story 6.11 Matrix 28: `guild_id()` outside a guild is an error, and an
/// unregistered state panics naming its type; `Debug` names the path.
#[test]
fn ctx_guild_and_state() {
    let ctx = ctx();
    assert!(format!("{ctx:?}").contains("config set prefix"));
    let err = ctx.guild_id().unwrap_err();
    assert!(err.to_string().contains("not in a guild"), "{err}");
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.state::<u8>())).unwrap_err();
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(message.contains("`u8`"), "{message}");
}
