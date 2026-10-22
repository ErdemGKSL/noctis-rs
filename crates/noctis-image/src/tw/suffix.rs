//! The closed suffix vocabulary: every value a utility suffix may spell.
//!
//! `xtask tw-grammar` maps each takumi value parse type to a list of
//! [`Rule`]s written over these items (`xtask/src/tw_grammar/table.rs`). The
//! vocabulary only narrows takumi: a suffix one of these rules accepts must
//! make takumi accept the class, which `tests/tw_crosscheck.rs` proves for
//! every rule's [`Rule::samples`].

/// The Tailwind v4 palette names takumi-core knows (its `PALETTE` table).
pub(crate) const PALETTE: &[&str] = &[
    "slate", "gray", "zinc", "neutral", "stone", "taupe", "mauve", "mist", "olive", "red",
    "orange", "amber", "yellow", "lime", "green", "emerald", "teal", "cyan", "sky", "blue",
    "indigo", "violet", "purple", "fuchsia", "pink", "rose",
];

/// The palette shades, as spelled in a class.
pub(crate) const SHADES: &[&str] = &[
    "50", "100", "200", "300", "400", "500", "600", "700", "800", "900", "950",
];

/// Named colors outside the palette scale.
const NAMED_COLORS: &[&str] = &["transparent", "black", "white"];

/// The length keywords takumi's `Length::parse_tw` reads, except `auto`.
pub(crate) const LENGTH_KEYWORDS: &[&str] = &[
    "full", "px", "3xs", "2xs", "xs", "sm", "md", "lg", "xl", "2xl", "3xl", "4xl", "5xl", "6xl",
    "7xl", "dvw", "svw", "lvw", "dvh", "svh", "lvh",
];

/// The `text-*` font-size keywords.
pub(crate) const FONT_SIZES: &[&str] = &[
    "xs", "sm", "base", "lg", "xl", "2xl", "3xl", "4xl", "5xl", "6xl", "7xl", "8xl", "9xl",
];

/// One parser of a utility prefix: takumi's `PropertyParser::<variant>`
/// writing `TailwindProperty::<property>`, with its value grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Parser {
    /// The `PropertyParser` variant.
    pub(crate) variant: &'static str,
    /// The `TailwindProperty` constructor it writes.
    pub(crate) property: &'static str,
    /// The value grammar of the variant's parse type, tried in order.
    pub(crate) value: &'static [Rule],
    /// Whether the property is in one of takumi's `try_neg` lists.
    pub(crate) negatable: bool,
}

/// Whether a matched value survives `try_negative` (`-m-4` yes, `-m-auto` no).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Negation {
    /// The value has a negative form.
    Allowed,
    /// Negating the value fails, which rejects the whole class.
    Rejected,
}

/// What a bracketed arbitrary value (`w-[10px]`) may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArbitraryKind {
    /// `-?\d+(\.\d+)?(px|rem|em|%|vw|vh)`.
    Length,
    /// `-?\d+(\.\d+)?`.
    Number,
    /// `-?\d+`.
    Integer,
    /// `-?\d+(\.\d+)?deg`.
    Angle,
    /// `\d+(\.\d+)?%`.
    Percentage,
    /// `#` and 3, 4, 6 or 8 hex digits.
    Color,
    /// `\d+/\d+` with non-zero parts.
    Ratio,
    /// An identifier: `[A-Za-z][A-Za-z0-9-]*`.
    Keyword,
}

/// One shape of suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuffixRule {
    /// Exactly one of these words.
    Keywords(&'static [&'static str]),
    /// The spacing scale: `0` or `[1-9]\d{0,3}`, optionally `.25`, `.5` or `.75`.
    Spacing,
    /// `[1-9]\d?/[1-9]\d?`.
    Fraction,
    /// A decimal integer without sign or leading zeros, within `min..=max`.
    Integer {
        /// Smallest accepted value.
        min: u32,
        /// Largest accepted value.
        max: u32,
    },
    /// A decimal number without sign or leading zeros: an integer part up to
    /// `max`, optionally followed by `.` and 1 to `decimals` digits.
    Number {
        /// Largest accepted integer part.
        max: u32,
        /// Most fraction digits; `0` forbids a fraction.
        decimals: u8,
    },
    /// `\d{1,3}%` (gradient stop positions such as `via-50%`).
    Percent,
    /// A palette color (`red-500`), `transparent`, `black`, `white`, and
    /// `current` when set; optionally `/` and an integer opacity `0..=100`.
    Color {
        /// Whether `current` is accepted.
        current: bool,
    },
    /// A keyword, a `/`, then a value the modifier rule accepts (`sm/6`).
    Slash {
        /// The words before the `/`.
        base: &'static [&'static str],
        /// The rule for the text after the `/`.
        modifier: &'static SuffixRule,
    },
    /// `[` + an arbitrary value + `]`.
    Arbitrary(ArbitraryKind),
}

/// A suffix rule with its negation behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rule {
    /// The suffix shape.
    pub(crate) kind: SuffixRule,
    /// Whether a match can be negated.
    pub(crate) negation: Negation,
}

impl Rule {
    /// A rule whose values can be negated.
    pub(crate) const fn new(kind: SuffixRule) -> Self {
        Self {
            kind,
            negation: Negation::Allowed,
        }
    }

    /// A rule whose values cannot be negated (`auto`).
    pub(crate) const fn no_neg(kind: SuffixRule) -> Self {
        Self {
            kind,
            negation: Negation::Rejected,
        }
    }

    /// Whether `suffix` matches this rule.
    pub(crate) fn matches(&self, suffix: &str) -> bool {
        self.kind.matches(suffix)
    }

    /// Representative suffixes this rule accepts; every one is cross-checked.
    pub(crate) fn samples(&self) -> Vec<String> {
        self.kind.samples()
    }
}

impl SuffixRule {
    /// Whether `suffix` matches this rule.
    pub(crate) fn matches(&self, suffix: &str) -> bool {
        match *self {
            Self::Keywords(words) => words.contains(&suffix),
            Self::Spacing => is_spacing(suffix),
            Self::Fraction => is_fraction(suffix),
            Self::Integer { min, max } => {
                parse_plain_int(suffix).is_some_and(|n| (min..=max).contains(&n))
            }
            Self::Number { max, decimals } => is_number(suffix, max, decimals),
            Self::Percent => suffix
                .strip_suffix('%')
                .is_some_and(|digits| (1..=3).contains(&digits.len()) && all_digits(digits)),
            Self::Color { current } => is_color(suffix, current),
            Self::Slash { base, modifier } => suffix
                .split_once('/')
                .is_some_and(|(head, tail)| base.contains(&head) && modifier.matches(tail)),
            Self::Arbitrary(kind) => suffix
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .is_some_and(|value| kind.matches(value)),
        }
    }

    /// Representative suffixes this rule accepts.
    pub(crate) fn samples(&self) -> Vec<String> {
        let owned = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match *self {
            Self::Keywords(words) => owned(words),
            Self::Spacing => owned(&["0", "1", "0.5", "2.5", "4", "96", "9999.75"]),
            Self::Fraction => owned(&["1/2", "2/3", "11/12", "99/99"]),
            Self::Integer { min, max } => {
                let mut values = vec![min, max, min.saturating_add(1).min(max)];
                values.dedup();
                values.iter().map(u32::to_string).collect()
            }
            Self::Number { max, decimals } => {
                let mut values = vec!["0".to_owned(), max.to_string()];
                if decimals > 0 {
                    values.push("1.5".to_owned());
                }
                values
            }
            Self::Percent => owned(&["0%", "50%", "100%"]),
            Self::Color { current } => {
                let mut values = owned(&[
                    "red-500",
                    "slate-50",
                    "rose-950",
                    "olive-300",
                    "transparent",
                    "black",
                    "white",
                    "red-500/0",
                    "blue-600/50",
                    "white/100",
                ]);
                if current {
                    values.extend(owned(&["current", "current/50"]));
                }
                values
            }
            Self::Slash { base, modifier } => {
                let tails = modifier.samples();
                base.iter()
                    .flat_map(|head| {
                        tails
                            .iter()
                            .take(2)
                            .map(move |tail| format!("{head}/{tail}"))
                    })
                    .collect()
            }
            Self::Arbitrary(kind) => kind.samples().iter().map(|v| format!("[{v}]")).collect(),
        }
    }
}

impl ArbitraryKind {
    /// Whether the text between the brackets matches.
    fn matches(self, value: &str) -> bool {
        match self {
            Self::Length => ["px", "rem", "em", "%", "vw", "vh"]
                .iter()
                .any(|unit| value.strip_suffix(unit).is_some_and(is_signed_decimal)),
            Self::Number => is_signed_decimal(value),
            Self::Integer => {
                let digits = value.strip_prefix('-').unwrap_or(value);
                !digits.is_empty() && digits.len() <= 5 && all_digits(digits)
            }
            Self::Angle => value.strip_suffix("deg").is_some_and(is_signed_decimal),
            Self::Percentage => value
                .strip_suffix('%')
                .is_some_and(|number| !number.starts_with('-') && is_signed_decimal(number)),
            Self::Color => value.strip_prefix('#').is_some_and(|hex| {
                matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
            }),
            Self::Ratio => value.split_once('/').is_some_and(|(a, b)| {
                [a, b]
                    .iter()
                    .all(|part| parse_plain_int(part).is_some_and(|n| n > 0))
            }),
            Self::Keyword => {
                let mut bytes = value.bytes();
                bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
                    && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }
        }
    }

    /// Bracket contents this kind accepts.
    fn samples(self) -> &'static [&'static str] {
        match self {
            Self::Length => &["10px", "1.5rem", "2em", "50%", "100vw", "-4px"],
            Self::Number => &["0.5", "2", "-1"],
            Self::Integer => &["3", "-2"],
            Self::Angle => &["45deg", "-12.5deg"],
            Self::Percentage => &["50%", "12.5%"],
            Self::Color => &["#ff0000", "#f00", "#ff000080", "#abcd"],
            Self::Ratio => &["16/9", "4/3"],
            Self::Keyword => &["Inter", "my-font"],
        }
    }
}

fn all_digits(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

/// A decimal integer without sign or leading zeros (`0`, `12`, never `012`).
fn parse_plain_int(text: &str) -> Option<u32> {
    let canonical = text == "0" || (!text.starts_with('0') && !text.is_empty());
    (canonical && text.len() <= 9 && all_digits(text))
        .then(|| text.parse().ok())
        .flatten()
}

/// `-?\d+(\.\d+)?`, at most 6 digits on either side.
fn is_signed_decimal(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (int, frac) = unsigned.split_once('.').unwrap_or((unsigned, "1"));
    !int.is_empty()
        && int.len() <= 6
        && all_digits(int)
        && !frac.is_empty()
        && frac.len() <= 6
        && all_digits(frac)
}

fn is_spacing(text: &str) -> bool {
    let (int, frac) = match text.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (text, None),
    };
    let int_ok = int == "0"
        || (int.len() <= 4 && !int.starts_with('0') && !int.is_empty() && all_digits(int));
    int_ok && frac.is_none_or(|frac| matches!(frac, "25" | "5" | "75"))
}

fn is_fraction(text: &str) -> bool {
    text.split_once('/').is_some_and(|(a, b)| {
        [a, b]
            .iter()
            .all(|part| (1..=2).contains(&part.len()) && !part.starts_with('0') && all_digits(part))
    })
}

fn is_number(text: &str, max: u32, decimals: u8) -> bool {
    let (int, frac) = match text.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (text, None),
    };
    parse_plain_int(int).is_some_and(|n| n <= max)
        && frac.is_none_or(|frac| {
            !frac.is_empty() && frac.len() <= usize::from(decimals) && all_digits(frac)
        })
}

fn is_color(text: &str, current: bool) -> bool {
    let (base, opacity) = match text.split_once('/') {
        Some((base, opacity)) => (base, Some(opacity)),
        None => (text, None),
    };
    let base_ok = NAMED_COLORS.contains(&base)
        || (current && base == "current")
        || base
            .rsplit_once('-')
            .is_some_and(|(name, shade)| PALETTE.contains(&name) && SHADES.contains(&shade));
    base_ok && opacity.is_none_or(is_opacity)
}

/// An integer opacity modifier within `0..=100`.
pub(crate) fn is_opacity(text: &str) -> bool {
    parse_plain_int(text).is_some_and(|n| n <= 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_accepts_its_own_samples() {
        let rules = [
            SuffixRule::Keywords(&["a", "b-c"]),
            SuffixRule::Spacing,
            SuffixRule::Fraction,
            SuffixRule::Integer { min: 1, max: 12 },
            SuffixRule::Number {
                max: 200,
                decimals: 2,
            },
            SuffixRule::Percent,
            SuffixRule::Color { current: true },
            SuffixRule::Slash {
                base: &["sm"],
                modifier: &SuffixRule::Spacing,
            },
            SuffixRule::Arbitrary(ArbitraryKind::Length),
            SuffixRule::Arbitrary(ArbitraryKind::Number),
            SuffixRule::Arbitrary(ArbitraryKind::Integer),
            SuffixRule::Arbitrary(ArbitraryKind::Angle),
            SuffixRule::Arbitrary(ArbitraryKind::Percentage),
            SuffixRule::Arbitrary(ArbitraryKind::Color),
            SuffixRule::Arbitrary(ArbitraryKind::Ratio),
            SuffixRule::Arbitrary(ArbitraryKind::Keyword),
        ];
        for rule in rules {
            for sample in rule.samples() {
                assert!(
                    rule.matches(&sample),
                    "{rule:?} rejects its sample {sample}"
                );
            }
        }
    }

    #[test]
    fn spacing_bounds() {
        for ok in ["0", "0.5", "1", "2.5", "9999", "12.75"] {
            assert!(is_spacing(ok), "{ok}");
        }
        for bad in [
            "", "00", "01", "10000", "0.1", "1.", ".5", "1e3", "inf", "NaN", "-1", "+1",
        ] {
            assert!(!is_spacing(bad), "{bad}");
        }
    }

    #[test]
    fn color_bounds() {
        let color = SuffixRule::Color { current: false };
        assert!(color.matches("red-500/0") && color.matches("red-500/100"));
        for bad in [
            "red-500/101",
            "red-500/",
            "red-500/5.5",
            "red-501",
            "Red-500",
            "current",
            "brand",
        ] {
            assert!(!color.matches(bad), "{bad}");
        }
        assert!(SuffixRule::Color { current: true }.matches("current"));
    }

    #[test]
    fn arbitrary_rejects_functions_and_empty() {
        let length = SuffixRule::Arbitrary(ArbitraryKind::Length);
        for bad in [
            "[]",
            "[calc(1px+1px)]",
            "[url(a.png)]",
            "[10]",
            "[10px",
            "10px]",
        ] {
            assert!(!length.matches(bad), "{bad}");
        }
        let color = SuffixRule::Arbitrary(ArbitraryKind::Color);
        assert!(color.matches("[#ff00]"));
        assert!(!color.matches("[#ff000]"));
        assert!(!color.matches("[red]"));
    }
}
