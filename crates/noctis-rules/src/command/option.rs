//! Chat-input command options: [`validate_options`] (CMD-01..04, CMD-06,
//! CMD-13, OPT-01, OPT-03..11, OPT-13) and [`measure_options`] (CMD-05).

use std::fmt;

use super::{LocalizedText, validate_name};
use crate::len::discord_len;
use crate::{Actual, Bound, Limits, RuleId, Violation};

/// The largest INTEGER magnitude Discord accepts (2^53 − 1, OPT-03).
const INTEGER_MAX: i64 = 9_007_199_254_740_991;
/// The largest NUMBER magnitude Discord accepts (2^53, OPT-04).
const NUMBER_MAX: i64 = 9_007_199_254_740_992;
/// [`NUMBER_MAX`] as a double (exactly representable).
const NUMBER_MAX_F: f64 = 9_007_199_254_740_992.0;

const INTEGER_RANGE: &str = "between -9007199254740991 and 9007199254740991";
const NUMBER_RANGE: &str = "a finite number between -9007199254740992 and 9007199254740992";
pub(crate) const FILE_TYPE: &str = "`image`, `video`, `audio` or a dot-prefixed extension";

/// The value type of a chat-input command option (OPT-02).
///
/// The set is closed: subcommands and groups (types 1 and 2) come from Command
/// Paths, so an option can never be one and can never carry `required` on one
/// (OPT-14):
///
/// ```compile_fail
/// use noctis_rules::command::OptionType;
///
/// let _ = OptionType::SubCommand; // [OPT-14]
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionType {
    /// `STRING` (3).
    String,
    /// `INTEGER` (4).
    Integer,
    /// `BOOLEAN` (5).
    Boolean,
    /// `USER` (6).
    User,
    /// `CHANNEL` (7).
    Channel,
    /// `ROLE` (8).
    Role,
    /// `MENTIONABLE` (9).
    Mentionable,
    /// `NUMBER` (10).
    Number,
    /// `ATTACHMENT` (11).
    Attachment,
}

impl OptionType {
    /// Every option type, in Discord value order.
    pub const ALL: [OptionType; 9] = [
        Self::String,
        Self::Integer,
        Self::Boolean,
        Self::User,
        Self::Channel,
        Self::Role,
        Self::Mentionable,
        Self::Number,
        Self::Attachment,
    ];

    /// The Discord option type value (3..=11).
    pub const fn value(self) -> u8 {
        match self {
            Self::String => 3,
            Self::Integer => 4,
            Self::Boolean => 5,
            Self::User => 6,
            Self::Channel => 7,
            Self::Role => 8,
            Self::Mentionable => 9,
            Self::Number => 10,
            Self::Attachment => 11,
        }
    }

    /// The type name as Discord spells it, for example `"STRING"`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::String => "STRING",
            Self::Integer => "INTEGER",
            Self::Boolean => "BOOLEAN",
            Self::User => "USER",
            Self::Channel => "CHANNEL",
            Self::Role => "ROLE",
            Self::Mentionable => "MENTIONABLE",
            Self::Number => "NUMBER",
            Self::Attachment => "ATTACHMENT",
        }
    }

    /// STRING, INTEGER or NUMBER: the types that take choices and autocomplete.
    const fn is_choosable(self) -> bool {
        matches!(self, Self::String | Self::Integer | Self::Number)
    }

    const fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Number)
    }
}

/// A `min_value` / `max_value` bound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumericValue {
    /// An integer bound (INTEGER or NUMBER options).
    Int(i64),
    /// A floating-point bound (NUMBER options only).
    Float(f64),
}

impl NumericValue {
    #[expect(
        clippy::cast_precision_loss,
        reason = "compared only after the ±2^53 range check, where the cast is exact"
    )]
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(v) => v as f64,
            Self::Float(v) => v,
        }
    }

    /// `self > other`, in `i64` when both are integers, else in `f64`.
    fn greater_than(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a > b,
            _ => self.as_f64() > other.as_f64(),
        }
    }

    /// The OPT-03 / OPT-04 range violation for a value of an option of `kind`.
    fn range_error(self, kind: OptionType) -> Option<(RuleId, &'static str)> {
        let ok = match (kind, self) {
            (OptionType::Integer, Self::Int(v)) => (-INTEGER_MAX..=INTEGER_MAX).contains(&v),
            (OptionType::Integer, Self::Float(_)) => false,
            (_, Self::Int(v)) => (-NUMBER_MAX..=NUMBER_MAX).contains(&v),
            (_, Self::Float(v)) => v.is_finite() && v.abs() <= NUMBER_MAX_F,
        };
        match (ok, kind) {
            (true, _) => None,
            (false, OptionType::Integer) => Some((RuleId::OPT_03, INTEGER_RANGE)),
            (false, _) => Some((RuleId::OPT_04, NUMBER_RANGE)),
        }
    }
}

impl fmt::Display for NumericValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
        }
    }
}

/// The value of a [`Choice`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChoiceValue<'a> {
    /// A STRING choice value.
    Str(&'a str),
    /// An INTEGER (or NUMBER) choice value.
    Int(i64),
    /// A NUMBER choice value.
    Float(f64),
}

impl ChoiceValue<'_> {
    /// The value's type name as an OPT-09 actual value.
    const fn type_name(self) -> &'static str {
        match self {
            Self::Str(_) => "string",
            Self::Int(_) => "integer",
            Self::Float(_) => "number",
        }
    }

    const fn numeric(self) -> Option<NumericValue> {
        match self {
            Self::Str(_) => None,
            Self::Int(v) => Some(NumericValue::Int(v)),
            Self::Float(v) => Some(NumericValue::Float(v)),
        }
    }

    /// The CMD-05 length: [`discord_len`] of a string, else the `Display` length.
    fn measured_len(self) -> usize {
        match self {
            Self::Str(s) => discord_len(s),
            Self::Int(v) => v.to_string().len(),
            Self::Float(v) => v.to_string().len(),
        }
    }

    /// Whether the value fits an option of `kind` (STRING, INTEGER or NUMBER).
    const fn fits(self, kind: OptionType) -> bool {
        matches!(
            (kind, self),
            (OptionType::String, Self::Str(_))
                | (OptionType::Integer, Self::Int(_))
                | (OptionType::Number, Self::Int(_) | Self::Float(_))
        )
    }
}

/// One predefined choice of an option (OPT-08, OPT-09).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice<'a> {
    /// The displayed name and its localized variants.
    pub name: LocalizedText<'a>,
    /// The value sent to the handler.
    pub value: ChoiceValue<'a>,
}

impl<'a> Choice<'a> {
    /// A choice without localized names.
    pub const fn new(name: &'a str, value: ChoiceValue<'a>) -> Self {
        Self {
            name: LocalizedText::new(name),
            value,
        }
    }

    /// Sets the localized names.
    #[must_use]
    pub const fn name_localized(self, names: &'a [&'a str]) -> Self {
        Self {
            name: self.name.with_localized(names),
            ..self
        }
    }
}

/// One chat-input command option, as `derive(Options)` or dynamic code declares
/// it.
///
/// Empty `channel_types`, `choices` and `file_types` mean "not set".
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionDecl<'a> {
    /// The option name (one name word, CMD-01..03).
    pub name: &'a str,
    /// Localized names (only measured by [`measure_options`]).
    pub name_localized: &'a [&'a str],
    /// The description and its localized variants (CMD-04).
    pub description: LocalizedText<'a>,
    /// The option type.
    pub kind: OptionType,
    /// Whether the option is required (OPT-01).
    pub required: bool,
    /// The smallest allowed value (INTEGER / NUMBER, OPT-03..05).
    pub min_value: Option<NumericValue>,
    /// The largest allowed value (INTEGER / NUMBER, OPT-03..05).
    pub max_value: Option<NumericValue>,
    /// The minimum text length (STRING, OPT-06).
    pub min_length: Option<usize>,
    /// The maximum text length (STRING, OPT-06).
    pub max_length: Option<usize>,
    /// Allowed Discord channel type values (CHANNEL, OPT-07).
    pub channel_types: &'a [u8],
    /// Predefined choices (STRING / INTEGER / NUMBER, OPT-08..10).
    pub choices: &'a [Choice<'a>],
    /// Whether the option uses autocomplete (OPT-10, OPT-11).
    pub autocomplete: bool,
    /// The upload filter (ATTACHMENT, OPT-13).
    pub file_types: &'a [&'a str],
}

impl<'a> OptionDecl<'a> {
    /// An optional option with everything else unset.
    pub const fn new(name: &'a str, kind: OptionType, description: &'a str) -> Self {
        Self {
            name,
            name_localized: &[],
            description: LocalizedText::new(description),
            kind,
            required: false,
            min_value: None,
            max_value: None,
            min_length: None,
            max_length: None,
            channel_types: &[],
            choices: &[],
            autocomplete: false,
            file_types: &[],
        }
    }

    /// Sets the localized names.
    #[must_use]
    pub const fn name_localized(self, names: &'a [&'a str]) -> Self {
        Self {
            name_localized: names,
            ..self
        }
    }

    /// Sets the localized descriptions.
    #[must_use]
    pub const fn description_localized(self, descriptions: &'a [&'a str]) -> Self {
        Self {
            description: self.description.with_localized(descriptions),
            ..self
        }
    }

    /// Sets whether the option is required.
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }

    /// Sets `min_value`.
    #[must_use]
    pub const fn min_value(self, value: NumericValue) -> Self {
        Self {
            min_value: Some(value),
            ..self
        }
    }

    /// Sets `max_value`.
    #[must_use]
    pub const fn max_value(self, value: NumericValue) -> Self {
        Self {
            max_value: Some(value),
            ..self
        }
    }

    /// Sets `min_length`.
    #[must_use]
    pub const fn min_length(self, length: usize) -> Self {
        Self {
            min_length: Some(length),
            ..self
        }
    }

    /// Sets `max_length`.
    #[must_use]
    pub const fn max_length(self, length: usize) -> Self {
        Self {
            max_length: Some(length),
            ..self
        }
    }

    /// Sets the allowed channel type values.
    #[must_use]
    pub const fn channel_types(self, types: &'a [u8]) -> Self {
        Self {
            channel_types: types,
            ..self
        }
    }

    /// Sets the choices.
    #[must_use]
    pub const fn choices(self, choices: &'a [Choice<'a>]) -> Self {
        Self { choices, ..self }
    }

    /// Sets whether the option uses autocomplete.
    #[must_use]
    pub const fn autocomplete(self, autocomplete: bool) -> Self {
        Self {
            autocomplete,
            ..self
        }
    }

    /// Sets the upload filter.
    #[must_use]
    pub const fn file_types(self, file_types: &'a [&'a str]) -> Self {
        Self { file_types, ..self }
    }
}

/// The option's name, or `options[i]` when the name is empty.
fn label(option: &OptionDecl<'_>, index: usize) -> String {
    if option.name.is_empty() {
        format!("options[{index}]")
    } else {
        option.name.to_owned()
    }
}

/// Checks one options array of a chat-input command (or subcommand).
///
/// Every violation's path starts with the option's name (or `options[i]` when
/// the name is empty): `name`, `name.min_value`, `name.choices[j]`,
/// `name.file_types[k]`, …; CMD-13 uses the path `options` and CMD-06 the
/// duplicated name itself (`""` for duplicated empty names). The order is
/// CMD-13; then per option in input order its name rules, CMD-04, `min_value`
/// and `max_value` (OPT-05 misuse, integer bound, OPT-03/04 range), OPT-05
/// ordering, `min_length` and `max_length`, OPT-06 ordering, OPT-07, OPT-08,
/// each choice (OPT-09 name, type, value length or range, OPT-05 within the
/// bounds), OPT-10, OPT-11 and OPT-13; then OPT-01; then CMD-06 in
/// first-occurrence order. Orderings are checked only when both bounds passed
/// their own checks.
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::{OptionDecl, OptionType, validate_options};
///
/// let options = [
///     OptionDecl::new("reason", OptionType::String, "Why").max_length(200),
///     OptionDecl::new("days", OptionType::Integer, "How long").required(true),
/// ];
/// let errs = validate_options(&options, &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::OPT_01);
/// assert_eq!(errs[0].path, "days");
/// ```
pub fn validate_options(options: &[OptionDecl<'_>], limits: &Limits) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    if options.len() > limits.command_options {
        out.push(Violation::new(
            RuleId::CMD_13,
            "options",
            Bound::AtMost(limits.command_options),
            Actual::Count(options.len()),
        ));
    }
    for (index, option) in options.iter().enumerate() {
        check_option(option, &label(option, index), limits, &mut out);
    }
    check_required_order(options, &mut out);
    check_unique_names(options, &mut out);
    if out.is_empty() { Ok(()) } else { Err(out) }
}

/// Every per-option rule, in order.
fn check_option(option: &OptionDecl<'_>, p: &str, limits: &Limits, out: &mut Vec<Violation>) {
    for mut v in validate_name(option.name, limits) {
        p.clone_into(&mut v.path);
        out.push(v);
    }
    check_description(option, p, limits, out);
    let bounds = check_value_bounds(option, p, out);
    check_lengths(option, p, limits, out);
    let kind = option.kind;
    if !option.channel_types.is_empty() && kind != OptionType::Channel {
        out.push(Violation::new(
            RuleId::OPT_07,
            format!("{p}.channel_types"),
            Bound::Describe("channel_types only on CHANNEL options"),
            Actual::Text(kind.name().to_owned()),
        ));
    }
    check_choices(option, p, bounds, limits, out);
    if !option.choices.is_empty() && option.autocomplete {
        out.push(Violation::new(
            RuleId::OPT_10,
            p,
            Bound::Describe("choices or autocomplete, not both"),
            Actual::Text(p.to_owned()),
        ));
    }
    if option.autocomplete && !kind.is_choosable() {
        out.push(Violation::new(
            RuleId::OPT_11,
            format!("{p}.autocomplete"),
            Bound::Describe("autocomplete only on STRING, INTEGER or NUMBER options"),
            Actual::Text(kind.name().to_owned()),
        ));
    }
    check_file_types(option, p, limits, out);
}

/// CMD-04 for the default and every localized description.
fn check_description(option: &OptionDecl<'_>, p: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let max = limits.command_description_len;
    let text = option.description;
    let variants = std::iter::once((text.default, "in the default description".to_owned())).chain(
        text.localized
            .iter()
            .enumerate()
            .map(|(i, t)| (*t, format!("in localized description #{i}"))),
    );
    for (variant, note) in variants {
        let len = discord_len(variant);
        if len == 0 || len > max {
            out.push(
                Violation::new(
                    RuleId::CMD_04,
                    p,
                    Bound::Between { min: 1, max },
                    Actual::Count(len),
                )
                .with_note(note),
            );
        }
    }
}

/// The bounds that passed their own checks.
#[derive(Clone, Copy, Default)]
struct ValidBounds {
    min: Option<NumericValue>,
    max: Option<NumericValue>,
}

/// `min_value` and `max_value`: OPT-05 misuse, integer bound, OPT-03/04 range,
/// then OPT-05 ordering.
fn check_value_bounds(option: &OptionDecl<'_>, p: &str, out: &mut Vec<Violation>) -> ValidBounds {
    let check = |field: &str, value: Option<NumericValue>, out: &mut Vec<Violation>| {
        let value = value?;
        let path = format!("{p}.{field}");
        let kind = option.kind;
        if !kind.is_numeric() {
            out.push(Violation::new(
                RuleId::OPT_05,
                path,
                Bound::Describe("min_value / max_value only on INTEGER or NUMBER options"),
                Actual::Text(kind.name().to_owned()),
            ));
            return None;
        }
        if let (OptionType::Integer, NumericValue::Float(_)) = (kind, value) {
            out.push(Violation::new(
                RuleId::OPT_05,
                path,
                Bound::Describe("an integer bound on an INTEGER option"),
                Actual::Text(value.to_string()),
            ));
            return None;
        }
        if let Some((rule, describe)) = value.range_error(kind) {
            out.push(Violation::new(
                rule,
                path,
                Bound::Describe(describe),
                Actual::Text(value.to_string()),
            ));
            return None;
        }
        Some(value)
    };
    let bounds = ValidBounds {
        min: check("min_value", option.min_value, out),
        max: check("max_value", option.max_value, out),
    };
    if let (Some(min), Some(max)) = (bounds.min, bounds.max)
        && min.greater_than(max)
    {
        out.push(Violation::new(
            RuleId::OPT_05,
            p,
            Bound::Describe("min_value ≤ max_value"),
            Actual::Text(format!("{min} > {max}")),
        ));
    }
    bounds
}

/// `min_length` and `max_length`: OPT-06 misuse or range, then ordering.
fn check_lengths(option: &OptionDecl<'_>, p: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let max_len = limits.option_length;
    let check = |field: &str, value: Option<usize>, min: usize, out: &mut Vec<Violation>| {
        let value = value?;
        let path = format!("{p}.{field}");
        if option.kind != OptionType::String {
            out.push(Violation::new(
                RuleId::OPT_06,
                path,
                Bound::Describe("min_length / max_length only on STRING options"),
                Actual::Text(option.kind.name().to_owned()),
            ));
            return None;
        }
        if value < min || value > max_len {
            out.push(Violation::new(
                RuleId::OPT_06,
                path,
                Bound::Between { min, max: max_len },
                Actual::Count(value),
            ));
            return None;
        }
        Some(value)
    };
    let min = check("min_length", option.min_length, 0, out);
    let max = check("max_length", option.max_length, 1, out);
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        out.push(Violation::new(
            RuleId::OPT_06,
            p,
            Bound::Describe("min_length ≤ max_length"),
            Actual::Text(format!("{min} > {max}")),
        ));
    }
}

/// OPT-08 (type, count), then OPT-09 / OPT-03 / OPT-04 / OPT-05 per choice.
fn check_choices(
    option: &OptionDecl<'_>,
    p: &str,
    bounds: ValidBounds,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let choices = option.choices;
    if choices.is_empty() {
        return;
    }
    let kind = option.kind;
    let path = format!("{p}.choices");
    if !kind.is_choosable() {
        out.push(Violation::new(
            RuleId::OPT_08,
            path.as_str(),
            Bound::Describe("choices only on STRING, INTEGER or NUMBER options"),
            Actual::Text(kind.name().to_owned()),
        ));
    }
    if choices.len() > limits.option_choices {
        out.push(Violation::new(
            RuleId::OPT_08,
            path.as_str(),
            Bound::AtMost(limits.option_choices),
            Actual::Count(choices.len()),
        ));
    }
    for (j, choice) in choices.iter().enumerate() {
        let path = format!("{path}[{j}]");
        check_choice_names(choice, &path, limits, out);
        if kind.is_choosable() {
            check_choice_value(choice.value, kind, &path, bounds, limits, out);
        }
    }
}

/// OPT-09 for the default and every localized choice name.
fn check_choice_names(choice: &Choice<'_>, path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let max = limits.choice_name_len;
    let name = choice.name;
    let variants = std::iter::once((name.default, "in the choice name".to_owned())).chain(
        name.localized
            .iter()
            .enumerate()
            .map(|(i, t)| (*t, format!("in localized choice name #{i}"))),
    );
    for (variant, note) in variants {
        let len = discord_len(variant);
        if len == 0 || len > max {
            out.push(
                Violation::new(
                    RuleId::OPT_09,
                    path,
                    Bound::Between { min: 1, max },
                    Actual::Count(len),
                )
                .with_note(note),
            );
        }
    }
}

/// OPT-09 type, else value length / OPT-03 / OPT-04, then OPT-05 against
/// each valid bound.
fn check_choice_value(
    value: ChoiceValue<'_>,
    kind: OptionType,
    path: &str,
    bounds: ValidBounds,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    if !value.fits(kind) {
        out.push(
            Violation::new(
                RuleId::OPT_09,
                path,
                Bound::Describe("a choice value of the option's type"),
                Actual::Text(value.type_name().to_owned()),
            )
            .with_note(format!("option type {}", kind.name())),
        );
        return;
    }
    let Some(number) = value.numeric() else {
        let len = value.measured_len();
        if len > limits.choice_value_len {
            out.push(
                Violation::new(
                    RuleId::OPT_09,
                    path,
                    Bound::AtMost(limits.choice_value_len),
                    Actual::Count(len),
                )
                .with_note("in the choice value"),
            );
        }
        return;
    };
    if let Some((rule, describe)) = number.range_error(kind) {
        out.push(Violation::new(
            rule,
            path,
            Bound::Describe(describe),
            Actual::Text(number.to_string()),
        ));
        return;
    }
    let below = bounds.min.is_some_and(|min| min.greater_than(number));
    let above = bounds.max.is_some_and(|max| number.greater_than(max));
    for outside in [below, above] {
        if outside {
            out.push(Violation::new(
                RuleId::OPT_05,
                path,
                Bound::Describe("a choice value within min_value and max_value"),
                Actual::Text(number.to_string()),
            ));
        }
    }
}

/// OPT-13: type, count, then each entry.
fn check_file_types(option: &OptionDecl<'_>, p: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let file_types = option.file_types;
    if file_types.is_empty() {
        return;
    }
    let path = format!("{p}.file_types");
    if option.kind != OptionType::Attachment {
        out.push(Violation::new(
            RuleId::OPT_13,
            path.as_str(),
            Bound::Describe("file_types only on ATTACHMENT options"),
            Actual::Text(option.kind.name().to_owned()),
        ));
    }
    if file_types.len() > limits.option_file_types {
        out.push(Violation::new(
            RuleId::OPT_13,
            path.as_str(),
            Bound::AtMost(limits.option_file_types),
            Actual::Count(file_types.len()),
        ));
    }
    for (k, entry) in file_types.iter().enumerate() {
        if !is_file_type(entry) {
            out.push(Violation::new(
                RuleId::OPT_13,
                format!("{path}[{k}]"),
                Bound::Describe(FILE_TYPE),
                Actual::Text((*entry).to_owned()),
            ));
        }
    }
}

/// `image`, `video`, `audio`, or `.` followed by one or more ASCII letters or
/// digits.
pub(crate) fn is_file_type(entry: &str) -> bool {
    matches!(entry, "image" | "video" | "audio")
        || entry
            .strip_prefix('.')
            .is_some_and(|ext| !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// OPT-01: every required option after the first optional one.
fn check_required_order(options: &[OptionDecl<'_>], out: &mut Vec<Violation>) {
    let mut first_optional: Option<String> = None;
    for (index, option) in options.iter().enumerate() {
        match (&first_optional, option.required) {
            (None, false) => first_optional = Some(label(option, index)),
            (Some(optional), true) => out.push(
                Violation::new(
                    RuleId::OPT_01,
                    label(option, index),
                    Bound::Describe("required options before optional ones"),
                    Actual::Text(label(option, index)),
                )
                .with_note(format!("after optional `{optional}`")),
            ),
            _ => {}
        }
    }
}

/// CMD-06: duplicate names (byte equality), in first-occurrence order.
fn check_unique_names(options: &[OptionDecl<'_>], out: &mut Vec<Violation>) {
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (index, option) in options.iter().enumerate() {
        match groups.iter_mut().find(|(name, _)| *name == option.name) {
            Some((_, positions)) => positions.push(index),
            None => groups.push((option.name, vec![index])),
        }
    }
    for (name, positions) in groups.into_iter().filter(|(_, p)| p.len() > 1) {
        let list: Vec<String> = positions.iter().map(usize::to_string).collect();
        out.push(
            Violation::new(
                RuleId::CMD_06,
                name,
                Bound::Describe("unique option names"),
                Actual::Text(name.to_owned()),
            )
            .with_note(format!("declared at positions {}", list.join(", "))),
        );
    }
}

/// The CMD-05 share of one options array, for
/// [`CommandDecl::with_options`](super::CommandDecl::with_options).
///
/// Per option: the longest of its name and localized names, plus the longest
/// description variant, plus per choice the longest name variant and the value
/// length ([`discord_len`] of a string, the `Display` length of a number). The
/// sum saturates.
///
/// ```
/// use noctis_rules::Limits;
/// use noctis_rules::command::{OptionDecl, OptionType, measure_options};
///
/// let options = [OptionDecl::new("days", OptionType::Integer, "d")];
/// assert_eq!(measure_options(&options, &Limits::default()), 5);
/// ```
pub fn measure_options(options: &[OptionDecl<'_>], limits: &Limits) -> usize {
    let _ = limits;
    options.iter().fold(0, |acc: usize, option| {
        let name = option
            .name_localized
            .iter()
            .map(|n| discord_len(n))
            .fold(discord_len(option.name), usize::max);
        let choices = option.choices.iter().fold(0, |acc: usize, choice| {
            acc.saturating_add(
                choice
                    .name
                    .longest()
                    .saturating_add(choice.value.measured_len()),
            )
        });
        acc.saturating_add(name)
            .saturating_add(option.description.longest())
            .saturating_add(choices)
    })
}

/// Checks an autocomplete response for an option of `kind` (INT-07).
///
/// On a type other than STRING, INTEGER or NUMBER the only violation is OPT-11
/// (path `choices`). Otherwise more than `autocomplete_choices` choices is an
/// INT-07 violation (path `choices`) whose help says Noctis sends the first
/// ones: the caller truncates to `autocomplete_choices` and logs it as a
/// warning, in every [`LimitMode`](crate::LimitMode). Then each kept choice
/// (`choices[j]`) is checked as OPT-09 (name, localized names, value type and
/// length) and OPT-03 / OPT-04 (numeric range); dropped choices are not
/// checked.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::{Choice, ChoiceValue, OptionType, validate_autocomplete};
///
/// let choices = vec![Choice::new("a", ChoiceValue::Str("a")); 26];
/// let errs = validate_autocomplete(&choices, OptionType::String, &Limits::default());
/// assert_eq!(errs.len(), 1);
/// assert_eq!(errs[0].rule, RuleId::INT_07);
/// ```
pub fn validate_autocomplete(
    choices: &[Choice<'_>],
    kind: OptionType,
    limits: &Limits,
) -> Vec<Violation> {
    if !kind.is_choosable() {
        return vec![Violation::new(
            RuleId::OPT_11,
            "choices",
            Bound::Describe("autocomplete only on STRING, INTEGER or NUMBER options"),
            Actual::Text(kind.name().to_owned()),
        )];
    }
    let mut out = Vec::new();
    let max = limits.autocomplete_choices;
    if choices.len() > max {
        out.push(
            Violation::new(
                RuleId::INT_07,
                "choices",
                Bound::AtMost(max),
                Actual::Count(choices.len()),
            )
            .with_help(format!(
                "Noctis sends the first {max} choices and logs this as a warning"
            )),
        );
    }
    for (j, choice) in choices.iter().take(max).enumerate() {
        let path = format!("choices[{j}]");
        check_choice_names(choice, &path, limits, &mut out);
        check_choice_value(
            choice.value,
            kind,
            &path,
            ValidBounds::default(),
            limits,
            &mut out,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    use ChoiceValue::{Float as CFloat, Int as CInt, Str};
    use NumericValue::{Float, Int};

    const I53: i64 = 9_007_199_254_740_991;
    const N53: i64 = 9_007_199_254_740_992;

    fn o(name: &str, kind: OptionType) -> OptionDecl<'_> {
        OptionDecl::new(name, kind, "d")
    }

    fn check(options: &[OptionDecl<'_>]) -> Vec<Violation> {
        check_with(options, &Limits::default())
    }

    fn check_with(options: &[OptionDecl<'_>], limits: &Limits) -> Vec<Violation> {
        match validate_options(options, limits) {
            Ok(()) => Vec::new(),
            Err(errs) => {
                assert!(!errs.is_empty());
                errs
            }
        }
    }

    fn one(option: OptionDecl<'_>) -> Vec<Violation> {
        check(&[option])
    }

    fn rules(errs: &[Violation]) -> Vec<RuleId> {
        errs.iter().map(|v| v.rule).collect()
    }

    fn text(s: &str) -> Actual {
        Actual::Text(s.to_owned())
    }

    fn describe(rule: RuleId, path: &str, d: &'static str, actual: &str) -> Violation {
        Violation::new(rule, path, Bound::Describe(d), text(actual))
    }

    #[test]
    fn opt_02_option_type_values() {
        let values: Vec<u8> = OptionType::ALL.iter().map(|t| t.value()).collect();
        assert_eq!(values, (3..=11).collect::<Vec<u8>>());
        let names: Vec<&str> = OptionType::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            [
                "STRING",
                "INTEGER",
                "BOOLEAN",
                "USER",
                "CHANNEL",
                "ROLE",
                "MENTIONABLE",
                "NUMBER",
                "ATTACHMENT"
            ]
        );
    }

    #[test]
    fn opt_01_empty_is_ok() {
        assert_eq!(validate_options(&[], &Limits::default()), Ok(()));
    }

    #[test]
    fn opt_05_ac_valid_options() {
        let severity = [Choice::new("Low", Str("low"))];
        let options = [
            o("member", OptionType::User).required(true),
            o("days", OptionType::Integer)
                .min_value(Int(1))
                .max_value(Int(30)),
            o("reason", OptionType::String)
                .min_length(3)
                .max_length(200),
            o("log_to", OptionType::Channel).channel_types(&[0, 5]),
            o("severity", OptionType::String).choices(&severity),
            o("rule", OptionType::String).autocomplete(true),
            o("evidence", OptionType::Attachment).file_types(&["image", ".pdf"]),
        ];
        assert_eq!(validate_options(&options, &Limits::default()), Ok(()));
    }

    fn named(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("o{i}")).collect()
    }

    #[test]
    fn cmd_13_option_count() {
        let names = named(26);
        let opts: Vec<_> = names.iter().map(|n| o(n, OptionType::String)).collect();
        assert_eq!(check(&opts[..25]), []);
        let errs = check(&opts);
        assert_eq!(
            errs[0],
            Violation::new(
                RuleId::CMD_13,
                "options",
                Bound::AtMost(25),
                Actual::Count(26)
            )
        );
        let limits = Limits {
            command_options: 1,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&opts[..2], &limits)[0],
            Violation::new(
                RuleId::CMD_13,
                "options",
                Bound::AtMost(1),
                Actual::Count(2)
            )
        );
    }

    #[test]
    fn cmd_01_cmd_02_cmd_03_option_names() {
        for name in ["süre", "a-b_c", "設定"] {
            assert_eq!(one(o(name, OptionType::String)), [], "{name}");
        }
        let errs = one(o("Days", OptionType::String));
        assert_eq!(rules(&errs), [RuleId::CMD_03]);
        assert_eq!(errs[0].path, "Days");
        assert_eq!(errs[0].help.as_deref(), Some("use `days`"));
        let errs = one(o("it's", OptionType::String));
        let mut expected = validate_name("it's", &Limits::default());
        expected[0].path = "it's".into();
        assert_eq!(errs, expected);
        let a33 = "a".repeat(33);
        let errs = one(o(&a33, OptionType::String));
        assert_eq!(rules(&errs), [RuleId::CMD_02]);
        assert_eq!(errs[0].actual, Actual::Count(33));
        let errs = one(o("", OptionType::String));
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CMD_02,
                "options[0]",
                Bound::Between { min: 1, max: 32 },
                Actual::Count(0)
            )]
        );
    }

    fn cmd_04(path: &str, count: usize, note: &str) -> Violation {
        Violation::new(
            RuleId::CMD_04,
            path,
            Bound::Between { min: 1, max: 100 },
            Actual::Count(count),
        )
        .with_note(note)
    }

    #[test]
    fn cmd_04_option_descriptions() {
        let d = |desc: &str| one(OptionDecl::new("a", OptionType::String, desc));
        assert_eq!((d("")), [cmd_04("a", 0, "in the default description")]);
        assert_eq!((d(&"x".repeat(100))), []);
        assert_eq!(
            (d(&"x".repeat(101))),
            [cmd_04("a", 101, "in the default description")]
        );
        assert_eq!((d(&"😀".repeat(50))), []);
        assert_eq!(
            (d(&"😀".repeat(51))),
            [cmd_04("a", 102, "in the default description")]
        );
        let x101 = "x".repeat(101);
        let localized = ["", x101.as_str()];
        assert_eq!(
            one(o("a", OptionType::String).description_localized(&localized)),
            [
                cmd_04("a", 0, "in localized description #0"),
                cmd_04("a", 101, "in localized description #1"),
            ]
        );
    }

    #[test]
    fn opt_03_integer_bounds() {
        assert_eq!(
            one(o("a", OptionType::Integer)
                .min_value(Int(-I53))
                .max_value(Int(I53))),
            []
        );
        for (value, actual) in [
            (I53 + 1, "9007199254740992"),
            (-I53 - 1, "-9007199254740992"),
            (i64::MIN, "-9223372036854775808"),
        ] {
            assert_eq!(
                one(o("a", OptionType::Integer).max_value(Int(value))),
                [describe(
                    RuleId::OPT_03,
                    "a.max_value",
                    INTEGER_RANGE,
                    actual
                )]
            );
            assert_eq!(
                one(o("a", OptionType::Integer).min_value(Int(value))),
                [describe(
                    RuleId::OPT_03,
                    "a.min_value",
                    INTEGER_RANGE,
                    actual
                )]
            );
        }
    }

    #[test]
    #[expect(clippy::cast_precision_loss, reason = "2^53 is exact in f64")]
    fn opt_04_number_bounds() {
        for value in [
            Float(N53 as f64),
            Float(-N53 as f64),
            Int(N53),
            Float(-0.0),
            Float(0.5),
        ] {
            assert_eq!(
                one(o("a", OptionType::Number).min_value(value)),
                [],
                "{value}"
            );
        }
        for (value, actual) in [
            (Float(9_007_199_254_740_994.0), "9007199254740994"),
            (Int(N53 + 1), "9007199254740993"),
            (Float(f64::NAN), "NaN"),
            (Float(f64::INFINITY), "inf"),
            (Float(f64::NEG_INFINITY), "-inf"),
        ] {
            assert_eq!(
                one(o("a", OptionType::Number).min_value(value)),
                [describe(
                    RuleId::OPT_04,
                    "a.min_value",
                    NUMBER_RANGE,
                    actual
                )]
            );
        }
    }

    const MISUSE: &str = "min_value / max_value only on INTEGER or NUMBER options";

    #[test]
    fn opt_05_misuse_and_integer_bound() {
        assert_eq!(
            one(o("a", OptionType::String).min_value(Int(1))),
            [describe(RuleId::OPT_05, "a.min_value", MISUSE, "STRING")]
        );
        assert_eq!(
            one(o("a", OptionType::Boolean)
                .min_value(Int(9))
                .max_value(Int(1))),
            [
                describe(RuleId::OPT_05, "a.min_value", MISUSE, "BOOLEAN"),
                describe(RuleId::OPT_05, "a.max_value", MISUSE, "BOOLEAN"),
            ]
        );
        assert_eq!(
            one(o("a", OptionType::Integer).min_value(Float(1.0))),
            [describe(
                RuleId::OPT_05,
                "a.min_value",
                "an integer bound on an INTEGER option",
                "1"
            )]
        );
    }

    #[test]
    fn opt_05_ordering() {
        let int = |min, max| o("a", OptionType::Integer).min_value(min).max_value(max);
        let num = |min, max| o("a", OptionType::Number).min_value(min).max_value(max);
        let order = |actual| describe(RuleId::OPT_05, "a", "min_value ≤ max_value", actual);
        assert_eq!(one(int(Int(5), Int(5))), []);
        assert_eq!(one(int(Int(6), Int(5))), [order("6 > 5")]);
        assert_eq!(one(num(Float(1.5), Int(1))), [order("1.5 > 1")]);
        assert_eq!(one(num(Int(1), Float(1.0))), []);
        assert_eq!(rules(&one(int(Int(I53 + 1), Int(0)))), [RuleId::OPT_03]);
    }

    fn between(path: &str, min: usize, max: usize, count: usize) -> Violation {
        Violation::new(
            RuleId::OPT_06,
            path,
            Bound::Between { min, max },
            Actual::Count(count),
        )
    }

    #[test]
    fn opt_06_lengths() {
        let s = || o("a", OptionType::String);
        assert_eq!(one(s().min_length(0)), []);
        assert_eq!(one(s().min_length(6000)), []);
        assert_eq!(
            one(s().min_length(6001)),
            [between("a.min_length", 0, 6000, 6001)]
        );
        assert_eq!(one(s().max_length(1)), []);
        assert_eq!(one(s().max_length(6000)), []);
        assert_eq!(
            one(s().max_length(0)),
            [between("a.max_length", 1, 6000, 0)]
        );
        assert_eq!(
            one(s().max_length(6001)),
            [between("a.max_length", 1, 6000, 6001)]
        );
        assert_eq!(one(s().min_length(5).max_length(5)), []);
        assert_eq!(
            one(s().min_length(10).max_length(5)),
            [describe(
                RuleId::OPT_06,
                "a",
                "min_length ≤ max_length",
                "10 > 5"
            )]
        );
        assert_eq!(
            one(s().min_length(7000).max_length(6500)),
            [
                between("a.min_length", 0, 6000, 7000),
                between("a.max_length", 1, 6000, 6500),
            ]
        );
        let limits = Limits {
            option_length: 100,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&[s().max_length(101)], &limits),
            [between("a.max_length", 1, 100, 101)]
        );
        assert_eq!(
            one(o("a", OptionType::Integer).min_length(1)),
            [describe(
                RuleId::OPT_06,
                "a.min_length",
                "min_length / max_length only on STRING options",
                "INTEGER"
            )]
        );
    }

    #[test]
    fn opt_07_channel_types() {
        assert_eq!(one(o("a", OptionType::Channel).channel_types(&[0, 5])), []);
        assert_eq!(
            one(o("a", OptionType::String).channel_types(&[0])),
            [describe(
                RuleId::OPT_07,
                "a.channel_types",
                "channel_types only on CHANNEL options",
                "STRING"
            )]
        );
        assert_eq!(one(o("a", OptionType::String).channel_types(&[])), []);
    }

    #[test]
    fn opt_08_choice_count_and_type() {
        let names = named(26);
        let choices: Vec<Choice<'_>> = names.iter().map(|n| Choice::new(n, Str(n))).collect();
        assert_eq!(one(o("a", OptionType::String).choices(&choices[..25])), []);
        assert_eq!(
            one(o("a", OptionType::String).choices(&choices)),
            [Violation::new(
                RuleId::OPT_08,
                "a.choices",
                Bound::AtMost(25),
                Actual::Count(26)
            )]
        );
        let limits = Limits {
            option_choices: 2,
            ..Limits::default()
        };
        assert_eq!(
            check_with(
                &[o("a", OptionType::String).choices(&choices[..3])],
                &limits
            ),
            [Violation::new(
                RuleId::OPT_08,
                "a.choices",
                Bound::AtMost(2),
                Actual::Count(3)
            )]
        );
        assert_eq!(
            one(o("a", OptionType::Boolean).choices(&[Choice::new("Yes", CInt(1))])),
            [describe(
                RuleId::OPT_08,
                "a.choices",
                "choices only on STRING, INTEGER or NUMBER options",
                "BOOLEAN"
            )]
        );
        // Names are still checked on a non-choosable option, values are not.
        let errs = one(o("a", OptionType::Boolean).choices(&[Choice::new("", Str(""))]));
        assert_eq!(rules(&errs), [RuleId::OPT_08, RuleId::OPT_09]);
    }

    fn opt_09(count: usize, note: &str) -> Violation {
        Violation::new(
            RuleId::OPT_09,
            "a.choices[0]",
            Bound::Between { min: 1, max: 100 },
            Actual::Count(count),
        )
        .with_note(note)
    }

    #[test]
    fn opt_09_choice_names() {
        let with =
            |name: &str| one(o("a", OptionType::String).choices(&[Choice::new(name, Str("v"))]));
        assert_eq!(with(""), [opt_09(0, "in the choice name")]);
        assert_eq!(with(&"a".repeat(100)), []);
        assert_eq!(with(&"a".repeat(101)), [opt_09(101, "in the choice name")]);
        assert_eq!(with(&"😀".repeat(50)), []);
        assert_eq!(with(&"😀".repeat(51)), [opt_09(102, "in the choice name")]);
        let a101 = "a".repeat(101);
        let localized = ["", a101.as_str()];
        let choices = [Choice::new("n", Str("v")).name_localized(&localized)];
        assert_eq!(
            one(o("a", OptionType::String).choices(&choices)),
            [
                opt_09(0, "in localized choice name #0"),
                opt_09(101, "in localized choice name #1"),
            ]
        );
    }

    #[test]
    fn opt_09_choice_value_length() {
        let with =
            |value: &str| one(o("a", OptionType::String).choices(&[Choice::new("n", Str(value))]));
        assert_eq!(with(""), []);
        assert_eq!(with(&"a".repeat(100)), []);
        let over = |count| {
            Violation::new(
                RuleId::OPT_09,
                "a.choices[0]",
                Bound::AtMost(100),
                Actual::Count(count),
            )
            .with_note("in the choice value")
        };
        assert_eq!(with(&"a".repeat(101)), [over(101)]);
        assert_eq!(with(&"😀".repeat(51)), [over(102)]);
    }

    #[test]
    fn opt_09_choice_value_type() {
        let with = |kind, value| one(o("a", kind).choices(&[Choice::new("n", value)]));
        let wrong = |actual, kind: &str| {
            describe(
                RuleId::OPT_09,
                "a.choices[0]",
                "a choice value of the option's type",
                actual,
            )
            .with_note(format!("option type {kind}"))
        };
        assert_eq!(
            with(OptionType::String, CInt(1)),
            [wrong("integer", "STRING")]
        );
        assert_eq!(
            with(OptionType::Integer, Str("a")),
            [wrong("string", "INTEGER")]
        );
        assert_eq!(
            with(OptionType::Integer, CFloat(1.0)),
            [wrong("number", "INTEGER")]
        );
        assert_eq!(with(OptionType::Number, CInt(1)), []);
        assert_eq!(
            with(OptionType::Number, Str("a")),
            [wrong("string", "NUMBER")]
        );
    }

    #[test]
    fn opt_05_choice_ranges() {
        let errs = one(o("a", OptionType::Integer).choices(&[Choice::new("n", CInt(I53 + 1))]));
        assert_eq!(
            errs,
            [describe(
                RuleId::OPT_03,
                "a.choices[0]",
                INTEGER_RANGE,
                "9007199254740992"
            )]
        );
        let errs = one(o("a", OptionType::Number).choices(&[Choice::new("n", CFloat(f64::NAN))]));
        assert_eq!(
            errs,
            [describe(
                RuleId::OPT_04,
                "a.choices[0]",
                NUMBER_RANGE,
                "NaN"
            )]
        );
        let choices = [
            Choice::new("a", CInt(1)),
            Choice::new("b", CInt(10)),
            Choice::new("c", CInt(0)),
            Choice::new("d", CInt(11)),
        ];
        let within = "a choice value within min_value and max_value";
        assert_eq!(
            one(o("a", OptionType::Integer)
                .min_value(Int(1))
                .max_value(Int(10))
                .choices(&choices)),
            [
                describe(RuleId::OPT_05, "a.choices[2]", within, "0"),
                describe(RuleId::OPT_05, "a.choices[3]", within, "11"),
            ]
        );
        assert_eq!(
            one(o("a", OptionType::Integer)
                .min_value(Int(1))
                .choices(&[Choice::new("n", CInt(0))])),
            [describe(RuleId::OPT_05, "a.choices[0]", within, "0")]
        );
        assert_eq!(
            one(o("a", OptionType::Integer)
                .min_value(Int(I53 + 1))
                .choices(&[Choice::new("n", CInt(0))])),
            [describe(
                RuleId::OPT_03,
                "a.min_value",
                INTEGER_RANGE,
                "9007199254740992"
            )]
        );
        assert_eq!(
            one(o("a", OptionType::Integer)
                .min_value(Float(5.0))
                .choices(&[Choice::new("n", CInt(0))])),
            [describe(
                RuleId::OPT_05,
                "a.min_value",
                "an integer bound on an INTEGER option",
                "5"
            )]
        );
    }

    #[test]
    fn opt_10_choices_and_autocomplete() {
        let choices = [Choice::new("n", Str("v"))];
        assert_eq!(
            one(o("a", OptionType::String)
                .choices(&choices)
                .autocomplete(true)),
            [describe(
                RuleId::OPT_10,
                "a",
                "choices or autocomplete, not both",
                "a"
            )]
        );
        assert_eq!(one(o("a", OptionType::String).autocomplete(true)), []);
    }

    #[test]
    fn opt_11_autocomplete_types() {
        for kind in OptionType::ALL {
            let errs = one(o("a", kind).autocomplete(true));
            if kind.is_choosable() {
                assert_eq!(errs, [], "{kind:?}");
            } else {
                assert_eq!(
                    errs,
                    [describe(
                        RuleId::OPT_11,
                        "a.autocomplete",
                        "autocomplete only on STRING, INTEGER or NUMBER options",
                        kind.name()
                    )]
                );
            }
        }
    }

    #[test]
    fn opt_08_opt_10_opt_11_combined() {
        let choices = [Choice::new("Yes", CInt(1))];
        let errs = one(o("a", OptionType::Boolean)
            .choices(&choices)
            .autocomplete(true));
        assert_eq!(
            rules(&errs),
            [RuleId::OPT_08, RuleId::OPT_10, RuleId::OPT_11]
        );
    }

    #[test]
    fn opt_13_file_types() {
        let a = |types: &[&str]| one(o("a", OptionType::Attachment).file_types(types));
        assert_eq!(a(&["image", "video", "audio", ".pdf", ".PNG", ".7z"]), []);
        let ten = [
            ".a", ".b", ".c", ".d", ".e", ".f", ".g", ".h", ".i", ".j", ".k",
        ];
        assert_eq!(a(&ten[..10]), []);
        assert_eq!(
            a(&ten),
            [Violation::new(
                RuleId::OPT_13,
                "a.file_types",
                Bound::AtMost(10),
                Actual::Count(11)
            )]
        );
        let limits = Limits {
            option_file_types: 1,
            ..Limits::default()
        };
        assert_eq!(
            check_with(
                &[o("a", OptionType::Attachment).file_types(&ten[..2])],
                &limits
            ),
            [Violation::new(
                RuleId::OPT_13,
                "a.file_types",
                Bound::AtMost(1),
                Actual::Count(2)
            )]
        );
        let bad = [
            "Image",
            "pdf",
            ".",
            ".tar.gz",
            ". pdf",
            "",
            ".pdf ",
            ".pdf\u{301}",
            "*",
        ];
        for entry in bad {
            assert_eq!(
                a(&[entry]),
                [describe(
                    RuleId::OPT_13,
                    "a.file_types[0]",
                    FILE_TYPE,
                    entry
                )],
                "{entry:?}"
            );
        }
        assert_eq!(
            one(o("a", OptionType::String).file_types(&["image", "pdf"])),
            [
                describe(
                    RuleId::OPT_13,
                    "a.file_types",
                    "file_types only on ATTACHMENT options",
                    "STRING"
                ),
                describe(RuleId::OPT_13, "a.file_types[1]", FILE_TYPE, "pdf"),
            ]
        );
    }

    #[test]
    fn opt_01_required_before_optional() {
        let req = |n| o(n, OptionType::String).required(true);
        let opt = |n| o(n, OptionType::String);
        let order = |n| {
            describe(
                RuleId::OPT_01,
                n,
                "required options before optional ones",
                n,
            )
            .with_note("after optional `b`")
        };
        assert_eq!(
            check(&[req("a"), opt("b"), req("c"), req("d")]),
            [order("c"), order("d")]
        );
        assert_eq!(check(&[opt("a"), opt("b")]), []);
        assert_eq!(check(&[req("a"), req("b")]), []);
    }

    #[test]
    fn cmd_06_unique_option_names() {
        let s = |n| o(n, OptionType::String);
        let dup = |n, positions: &str| {
            describe(RuleId::CMD_06, n, "unique option names", n)
                .with_note(format!("declared at positions {positions}"))
        };
        assert_eq!(
            check(&[s("a"), s("b"), s("a"), s("a"), s("b")]),
            [dup("a", "0, 2, 3"), dup("b", "1, 4")]
        );
        assert_eq!(rules(&check(&[s("a"), s("A")])), [RuleId::CMD_03]);
        let errs = check(&[s(""), s("")]);
        assert_eq!(
            rules(&errs),
            [RuleId::CMD_02, RuleId::CMD_02, RuleId::CMD_06]
        );
        assert_eq!(errs[2], dup("", "0, 1"));
    }

    #[test]
    fn cmd_13_opt_01_cmd_06_all_at_once() {
        let names = named(22);
        let mut options = vec![
            o("Bad", OptionType::String).min_value(Int(1)),
            o("b", OptionType::String),
            o("c", OptionType::String).required(true),
            o("c", OptionType::String).required(true),
        ];
        options.extend(names.iter().map(|n| o(n, OptionType::String)));
        assert_eq!(options.len(), 26);
        let errs = check(&options);
        assert_eq!(
            rules(&errs),
            [
                RuleId::CMD_13,
                RuleId::CMD_03,
                RuleId::OPT_05,
                RuleId::OPT_01,
                RuleId::OPT_01,
                RuleId::CMD_06
            ]
        );
        let paths: Vec<&str> = errs.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(paths, ["options", "Bad", "Bad.min_value", "c", "c", "c"]);
        assert_eq!(errs[5].notes, ["declared at positions 2, 3"]);
    }

    #[test]
    fn cmd_05_measure_options() {
        let l = Limits::default();
        assert_eq!(measure_options(&[], &l), 0);
        assert_eq!(measure_options(&[o("days", OptionType::Integer)], &l), 5);
        let localized = ["tage", "günlerin"];
        assert_eq!(
            measure_options(
                &[o("days", OptionType::Integer).name_localized(&localized)],
                &l
            ),
            9
        );
        let emoji = ["😀😀"];
        let choices = [
            Choice::new("Low", Str("low")),
            Choice::new("x", CInt(-12)),
            Choice::new("y", CFloat(1.5)),
            Choice::new("z", CFloat(2.0)),
            Choice::new("a", Str("b")).name_localized(&emoji),
        ];
        assert_eq!(
            measure_options(&[o("s", OptionType::String).choices(&choices)], &l),
            2 + 6 + 4 + 4 + 2 + 5
        );
        let x100 = "x".repeat(100);
        let long = [x100.as_str()];
        assert_eq!(
            measure_options(
                &[o("s", OptionType::String).description_localized(&long)],
                &l
            ),
            1 + 100
        );
    }

    #[test]
    fn int_07_autocomplete_count() {
        let l = Limits::default();
        let choices = vec![Choice::new("a", Str("a")); 26];
        assert_eq!(validate_autocomplete(&[], OptionType::String, &l), []);
        assert_eq!(
            validate_autocomplete(&choices[..25], OptionType::String, &l),
            []
        );
        let errs = validate_autocomplete(&choices, OptionType::String, &l);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::INT_07,
                "choices",
                Bound::AtMost(25),
                Actual::Count(26)
            )
            .with_help("Noctis sends the first 25 choices and logs this as a warning")]
        );
        let lowered = Limits {
            autocomplete_choices: 2,
            ..Limits::default()
        };
        let errs = validate_autocomplete(&choices[..3], OptionType::String, &lowered);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::INT_07,
                "choices",
                Bound::AtMost(2),
                Actual::Count(3)
            )
            .with_help("Noctis sends the first 2 choices and logs this as a warning")]
        );
    }

    #[test]
    fn int_07_autocomplete_kept_choices() {
        let l = Limits::default();
        let mut choices = vec![Choice::new("a", Str("a")); 26];
        choices[25] = Choice::new("", Str("a"));
        assert_eq!(
            rules(&validate_autocomplete(&choices, OptionType::String, &l)),
            [RuleId::INT_07]
        );
        choices[0] = Choice::new("", Str("a"));
        let errs = validate_autocomplete(&choices, OptionType::String, &l);
        assert_eq!(rules(&errs), [RuleId::INT_07, RuleId::OPT_09]);
        assert_eq!(
            errs[1],
            Violation::new(
                RuleId::OPT_09,
                "choices[0]",
                Bound::Between { min: 1, max: 100 },
                Actual::Count(0)
            )
            .with_note("in the choice name")
        );
    }

    #[test]
    fn int_07_autocomplete_choice_rules() {
        let l = Limits::default();
        let one = |c: Choice<'_>, kind| validate_autocomplete(&[c], kind, &l);
        let a101 = "a".repeat(101);
        let errs = one(Choice::new(&a101, Str("a")), OptionType::String);
        assert_eq!(rules(&errs), [RuleId::OPT_09]);
        assert_eq!(errs[0].actual, Actual::Count(101));
        let localized = [a101.as_str()];
        let errs = one(
            Choice::new("a", Str("a")).name_localized(&localized),
            OptionType::String,
        );
        assert_eq!(errs[0].notes, ["in localized choice name #0"]);
        assert_eq!(
            one(Choice::new("a", CInt(1)), OptionType::String),
            [describe(
                RuleId::OPT_09,
                "choices[0]",
                "a choice value of the option's type",
                "integer"
            )
            .with_note("option type STRING")]
        );
        assert_eq!(
            one(Choice::new("a", Str(&a101)), OptionType::String),
            [Violation::new(
                RuleId::OPT_09,
                "choices[0]",
                Bound::AtMost(100),
                Actual::Count(101)
            )
            .with_note("in the choice value")]
        );
        let errs = one(Choice::new("a", CInt(I53 + 1)), OptionType::Integer);
        assert_eq!(rules(&errs), [RuleId::OPT_03]);
        assert_eq!(errs[0].path, "choices[0]");
        let errs = one(Choice::new("a", CFloat(f64::NAN)), OptionType::Number);
        assert_eq!(rules(&errs), [RuleId::OPT_04]);
        assert_eq!(errs[0].actual, text("NaN"));
        assert_eq!(one(Choice::new("a", CInt(1)), OptionType::Number), []);
    }

    #[test]
    fn int_07_opt_11_autocomplete_on_boolean() {
        let choices = vec![Choice::new("", CInt(1)); 26];
        assert_eq!(
            validate_autocomplete(&choices, OptionType::Boolean, &Limits::default()),
            [describe(
                RuleId::OPT_11,
                "choices",
                "autocomplete only on STRING, INTEGER or NUMBER options",
                "BOOLEAN"
            )]
        );
    }

    proptest! {
        #[test]
        fn opt_06_property_length_rules(min in 0usize..7000, max in 0usize..7000) {
            let errs = one(o("a", OptionType::String).min_length(min).max_length(max));
            let mut expected = Vec::new();
            if min > 6000 {
                expected.push(between("a.min_length", 0, 6000, min));
            }
            if max == 0 || max > 6000 {
                expected.push(between("a.max_length", 1, 6000, max));
            }
            if expected.is_empty() && min > max {
                expected.push(describe(
                    RuleId::OPT_06,
                    "a",
                    "min_length ≤ max_length",
                    &format!("{min} > {max}"),
                ));
            }
            prop_assert_eq!(errs, expected);
        }

        #[test]
        fn opt_03_property_integer_range(value in any::<i64>()) {
            let errs = one(o("a", OptionType::Integer).min_value(Int(value)));
            let outside = !(-I53..=I53).contains(&value);
            prop_assert_eq!(!errs.is_empty(), outside);
            if outside {
                prop_assert_eq!(rules(&errs), [RuleId::OPT_03]);
            }
        }
    }
}
