//! Option boundary passes (CMD-02, CMD-04, CMD-13, OPT-03, OPT-04, OPT-06,
//! OPT-08, OPT-09, OPT-13): `#[derive(Options)]` and `#[derive(Choices)]`
//! literals exactly at a limit compile.

use noctis::CommandOption;
use noctis::prelude::*;
use noctis::rules::command::{NumericValue, OptionType};
use noctis::ui::ResolvedAttachment;

#[derive(Options)]
#[expect(dead_code, reason = "only the declarations are read")]
struct TwentyFive {
    #[option(description = "d")]
    o0: i64,
    #[option(description = "d")]
    o1: i64,
    #[option(description = "d")]
    o2: i64,
    #[option(description = "d")]
    o3: i64,
    #[option(description = "d")]
    o4: i64,
    #[option(description = "d")]
    o5: i64,
    #[option(description = "d")]
    o6: i64,
    #[option(description = "d")]
    o7: i64,
    #[option(description = "d")]
    o8: i64,
    #[option(description = "d")]
    o9: i64,
    #[option(description = "d")]
    o10: i64,
    #[option(description = "d")]
    o11: i64,
    #[option(description = "d")]
    o12: i64,
    #[option(description = "d")]
    o13: i64,
    #[option(description = "d")]
    o14: i64,
    #[option(description = "d")]
    o15: i64,
    #[option(description = "d")]
    o16: i64,
    #[option(description = "d")]
    o17: i64,
    #[option(description = "d")]
    o18: i64,
    #[option(description = "d")]
    o19: i64,
    #[option(description = "d")]
    o20: i64,
    #[option(description = "d")]
    o21: i64,
    #[option(description = "d")]
    o22: i64,
    #[option(description = "d")]
    o23: i64,
    #[option(description = "d")]
    o24: i64,
}

#[test]
fn cmd_13_25_options() {
    assert_eq!(TwentyFive::OPTIONS.len(), 25);
}

#[derive(Options)]
#[expect(dead_code, reason = "only the declarations are read")]
struct Texts {
    #[option(description = "d")]
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: i64,
    #[option(
        description = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )]
    desc: i64,
    #[option(
        description = "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"
    )]
    emoji: i64,
}

#[test]
fn cmd_02_cmd_04_name_32_and_description_100() {
    let o = Texts::OPTIONS;
    assert_eq!(o[0].name.len(), 32);
    assert_eq!(o[1].description.default.len(), 100);
    assert_eq!(o[2].description.default.chars().count(), 50);
}

#[derive(Options)]
#[expect(dead_code, reason = "only the declarations are read")]
struct Bounds {
    #[option(description = "d", min_value = -9007199254740991, max_value = 9007199254740991)]
    int: i64,
    #[option(description = "d", min_value = -9007199254740992.0, max_value = 9007199254740992)]
    number: f64,
    #[option(description = "d", min_length = 0, max_length = 6000)]
    text: String,
}

#[test]
fn opt_03_opt_04_opt_06_bounds() {
    let o = Bounds::OPTIONS;
    assert_eq!(
        o[0].min_value,
        Some(NumericValue::Int(-9_007_199_254_740_991))
    );
    assert_eq!(
        o[0].max_value,
        Some(NumericValue::Int(9_007_199_254_740_991))
    );
    assert_eq!(
        o[1].min_value,
        Some(NumericValue::Float(-9_007_199_254_740_992.0))
    );
    assert_eq!(
        o[1].max_value,
        Some(NumericValue::Int(9_007_199_254_740_992))
    );
    assert_eq!((o[2].min_length, o[2].max_length), (Some(0), Some(6000)));
}

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Many {
    #[choice("V0")]
    V0,
    #[choice("V1")]
    V1,
    #[choice("V2")]
    V2,
    #[choice("V3")]
    V3,
    #[choice("V4")]
    V4,
    #[choice("V5")]
    V5,
    #[choice("V6")]
    V6,
    #[choice("V7")]
    V7,
    #[choice("V8")]
    V8,
    #[choice("V9")]
    V9,
    #[choice("V10")]
    V10,
    #[choice("V11")]
    V11,
    #[choice("V12")]
    V12,
    #[choice("V13")]
    V13,
    #[choice("V14")]
    V14,
    #[choice("V15")]
    V15,
    #[choice("V16")]
    V16,
    #[choice("V17")]
    V17,
    #[choice("V18")]
    V18,
    #[choice("V19")]
    V19,
    #[choice("V20")]
    V20,
    #[choice("V21")]
    V21,
    #[choice("V22")]
    V22,
    #[choice("V23")]
    V23,
    #[choice("V24")]
    V24,
}

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Long {
    #[choice(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        value = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )]
    A,
    #[choice(
        "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
        value = "😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀"
    )]
    B,
}

#[derive(Options)]
#[expect(dead_code, reason = "only the declarations are read")]
struct WithChoices {
    #[option(description = "d")]
    many: Many,
    #[option(description = "d")]
    long: Option<Long>,
}

#[test]
fn opt_08_25_choices() {
    assert_eq!(WithChoices::OPTIONS[0].choices.len(), 25);
    assert_eq!(<Many as CommandOption>::KIND, OptionType::String);
}

#[test]
fn opt_09_choice_name_and_value_100() {
    assert_eq!(WithChoices::OPTIONS[1].choices.len(), 2);
}

#[derive(Options)]
#[expect(dead_code, reason = "only the declarations are read")]
struct Files {
    #[option(
        description = "d",
        file_types = ["image", "video", "audio", ".a", ".b", ".c", ".d", ".e", ".f", ".PNG"]
    )]
    file: ResolvedAttachment,
}

#[test]
fn opt_13_10_entries() {
    assert_eq!(Files::OPTIONS[0].file_types.len(), 10);
}
