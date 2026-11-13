//! Noctis-lowered utilities: Tailwind classes takumi has the CSS property for
//! but no utility, so Noctis turns them into inline declarations itself.
//!
//! This table is hand-written and takumi-free (the macros read it through
//! [`classify`](super::classify)); `tests/tw_crosscheck.rs` asserts takumi's
//! own parser still does not know any of these classes, so a takumi bump that
//! adds one fails and names the entry to delete.
//!
//! Lowering ([`split`]) removes these classes from the string takumi parses
//! and returns their declarations, which go before the node's `Style`
//! declarations, so a `Style` value still wins. A breakpoint variant or `!`
//! on one of them is not accepted by the grammar (TW-01).

#[cfg(any(feature = "render", test))]
use std::borrow::Cow;
#[cfg(any(feature = "render", test))]
use std::fmt::Write as _;

/// The longest integer `z-*` accepts, in digits: `i32::MAX` has ten.
const MAX_Z_DIGITS: usize = 10;

/// What one lowered class sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lowered {
    /// `z-index: <n>`, or `auto` for `None`.
    ZIndex(Option<i32>),
    /// One `font-variant-numeric` keyword.
    Numeric(Numeric),
}

/// A `font-variant-numeric` utility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Numeric {
    /// `normal-nums`: resets every numeric feature set before it.
    Normal,
    /// `lining-nums`
    Lining,
    /// `oldstyle-nums`
    Oldstyle,
    /// `proportional-nums`
    Proportional,
    /// `tabular-nums`
    Tabular,
    /// `slashed-zero`
    SlashedZero,
}

/// The fixed lowered utilities, sorted by class.
pub(crate) const FIXED: &[(&str, Lowered)] = &[
    ("lining-nums", Lowered::Numeric(Numeric::Lining)),
    ("normal-nums", Lowered::Numeric(Numeric::Normal)),
    ("oldstyle-nums", Lowered::Numeric(Numeric::Oldstyle)),
    ("proportional-nums", Lowered::Numeric(Numeric::Proportional)),
    ("slashed-zero", Lowered::Numeric(Numeric::SlashedZero)),
    ("tabular-nums", Lowered::Numeric(Numeric::Tabular)),
    ("z-auto", Lowered::ZIndex(None)),
];

/// The lowered utility `class` names (no variant, no `!`), if any:
/// a [`FIXED`] entry, `z-<n>`, `-z-<n>` or `z-[<n>]` / `z-[-<n>]`, where
/// `<n>` is a decimal `i32` without a sign or leading zero.
pub(crate) fn parse(class: &str) -> Option<Lowered> {
    if let Ok(index) = FIXED.binary_search_by(|(key, _)| (*key).cmp(class)) {
        return FIXED.get(index).map(|&(_, lowered)| lowered);
    }
    let (body, negative) = match class.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (class, false),
    };
    let suffix = body.strip_prefix("z-")?;
    let value = match suffix
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
    {
        Some(_) if negative => return None,
        Some(inner) => match inner.strip_prefix('-') {
            Some(digits) => integer(digits)?.checked_neg()?,
            None => integer(inner)?,
        },
        None if negative => integer(suffix)?.checked_neg()?,
        None => integer(suffix)?,
    };
    Some(Lowered::ZIndex(Some(value)))
}

/// A canonical non-negative decimal `i32`: digits only, no leading zero.
fn integer(digits: &str) -> Option<i32> {
    let canonical = !digits.is_empty()
        && digits.len() <= MAX_Z_DIGITS
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    canonical.then(|| digits.parse().ok()).flatten()
}

#[cfg(any(feature = "render", test))]
/// A class list with its lowered utilities taken out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Split<'a> {
    /// The classes takumi parses: the input when it holds no lowered utility.
    pub(crate) classes: Cow<'a, str>,
    /// The declarations of the lowered utilities (`z-index: 10;
    /// font-variant-numeric: tabular-nums`); empty when there are none.
    pub(crate) css: String,
}

#[cfg(any(feature = "render", test))]
/// Takes the lowered utilities out of `classes` (split the way takumi splits,
/// `str::split_whitespace`) and returns their declarations.
///
/// The last `z-*` wins; `font-variant-numeric` keywords combine per feature
/// (figure, spacing, slashed zero) with the later class winning, and
/// `normal-nums` clears the ones before it. A class with a variant or `!` is
/// left to takumi (the grammar rejects it, TW-01).
pub(crate) fn split(classes: &str) -> Split<'_> {
    if !classes
        .split_whitespace()
        .any(|class| parse(class).is_some())
    {
        return Split {
            classes: Cow::Borrowed(classes),
            css: String::new(),
        };
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut z = None;
    let mut numeric = NumericState::default();
    for class in classes.split_whitespace() {
        match parse(class) {
            Some(Lowered::ZIndex(value)) => z = Some(value),
            Some(Lowered::Numeric(keyword)) => numeric.apply(keyword),
            None => kept.push(class),
        }
    }
    let mut css = String::new();
    // Writing to a `String` cannot fail.
    if let Some(value) = z {
        match value {
            Some(n) => {
                let _ = write!(css, "z-index: {n}");
            }
            None => css.push_str("z-index: auto"),
        }
    }
    if let Some(value) = numeric.css() {
        if !css.is_empty() {
            css.push_str("; ");
        }
        let _ = write!(css, "font-variant-numeric: {value}");
    }
    Split {
        classes: Cow::Owned(kept.join(" ")),
        css,
    }
}

#[cfg(any(feature = "render", test))]
/// The `font-variant-numeric` features the classes of one node set.
#[derive(Debug, Default)]
struct NumericState {
    set: bool,
    figure: Option<&'static str>,
    spacing: Option<&'static str>,
    slashed_zero: bool,
}

#[cfg(any(feature = "render", test))]
impl NumericState {
    fn apply(&mut self, keyword: Numeric) {
        self.set = true;
        match keyword {
            Numeric::Normal => {
                self.figure = None;
                self.spacing = None;
                self.slashed_zero = false;
            }
            Numeric::Lining => self.figure = Some("lining-nums"),
            Numeric::Oldstyle => self.figure = Some("oldstyle-nums"),
            Numeric::Proportional => self.spacing = Some("proportional-nums"),
            Numeric::Tabular => self.spacing = Some("tabular-nums"),
            Numeric::SlashedZero => self.slashed_zero = true,
        }
    }

    /// The declaration value, or `None` when no numeric class was seen.
    fn css(&self) -> Option<String> {
        if !self.set {
            return None;
        }
        let parts: Vec<&str> = [
            self.figure,
            self.spacing,
            self.slashed_zero.then_some("slashed-zero"),
        ]
        .into_iter()
        .flatten()
        .collect();
        Some(if parts.is_empty() {
            "normal".to_owned()
        } else {
            parts.join(" ")
        })
    }
}

/// The classes the cross-check asserts takumi does not know: every fixed
/// entry and one of each integer form.
pub(crate) fn samples() -> Vec<String> {
    FIXED
        .iter()
        .map(|(key, _)| (*key).to_owned())
        .chain(
            ["z-0", "z-10", "z-50", "-z-10", "z-[5]", "z-[-5]", "z-[999]"]
                .into_iter()
                .map(str::to_owned),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_table_is_sorted_and_unique() {
        assert!(FIXED.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn integer_forms_parse() {
        for (class, value) in [
            ("z-0", 0),
            ("z-10", 10),
            ("-z-10", -10),
            ("z-[5]", 5),
            ("z-[-5]", -5),
            ("z-2147483647", i32::MAX),
            ("-z-2147483647", -i32::MAX),
        ] {
            assert_eq!(parse(class), Some(Lowered::ZIndex(Some(value))), "{class}");
        }
        assert_eq!(parse("z-auto"), Some(Lowered::ZIndex(None)));
    }

    #[test]
    fn malformed_integers_are_rejected() {
        for class in [
            "z-",
            "z-01",
            "z-+1",
            "z-1.5",
            "z-2147483648",
            "z-[]",
            "z-[01]",
            "-z-[5]",
            "z-[ 5]",
            "-z-auto",
            "z-10px",
            "z--1",
            "zz-1",
        ] {
            assert_eq!(parse(class), None, "{class}");
        }
    }

    #[test]
    fn split_keeps_other_classes_and_borrows_when_untouched() {
        let untouched = split("flex  p-4");
        assert!(matches!(untouched.classes, Cow::Borrowed("flex  p-4")));
        assert!(untouched.css.is_empty());

        let split = split("relative z-10 p-4 tabular-nums -z-5 sm:z-20 !z-30");
        assert_eq!(split.classes, "relative p-4 sm:z-20 !z-30");
        assert_eq!(split.css, "z-index: -5; font-variant-numeric: tabular-nums");
    }

    #[test]
    fn numeric_keywords_combine_and_reset() {
        assert_eq!(
            split("oldstyle-nums tabular-nums slashed-zero lining-nums").css,
            "font-variant-numeric: lining-nums tabular-nums slashed-zero"
        );
        assert_eq!(
            split("tabular-nums normal-nums").css,
            "font-variant-numeric: normal"
        );
        assert_eq!(
            split("tabular-nums normal-nums proportional-nums").css,
            "font-variant-numeric: proportional-nums"
        );
        assert_eq!(split("z-auto").css, "z-index: auto");
    }
}
