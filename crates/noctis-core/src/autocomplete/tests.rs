//! Story 6.7 Matrix 3-14 and 25-27: decoding, `respond` and the choice
//! filter.

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

/// A detached context keeping at most `max` choices.
fn ctx(max: usize) -> (AutocompleteCtx, Slot) {
    let slot = Slot::default();
    let parts = AutocompleteParts {
        path: "config set prefix",
        module: "m",
        shard: 0,
        guild_id: None,
        channel_id: None,
        user_id: Id::new(1),
        user_locale: None,
        guild_locale: None,
        focused: focused(3, &json!("")),
        app: None,
        max_choices: max,
    };
    (AutocompleteCtx::new(parts, Arc::clone(&slot)), slot)
}

fn collected(choices: Vec<AutocompleteChoice>) -> Collected {
    let (ctx, slot) = ctx(25);
    let _ = ctx.respond(choices);
    slot.lock().unwrap().take().unwrap()
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

/// `respond` keeps the first `max` and counts the rest; `Debug` names the
/// path but no text.
#[test]
fn respond_keeps_the_first_choices() {
    let (ctx, slot) = ctx(2);
    assert!(format!("{ctx:?}").contains("config set prefix"));
    let _ = ctx.respond(strings(5));
    let collected = slot.lock().unwrap().take().unwrap();
    assert_eq!(names(&collected.choices), ["c0", "c1"]);
    assert_eq!(collected.returned, 5);
}
