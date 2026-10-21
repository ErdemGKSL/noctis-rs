//! The duration grammar of attribute arguments such as `ttl = "15m"`
//! (and Story 13.4's `per = "…"`).
//!
//! A duration is one or more `<digits><unit>` groups with the units `d`,
//! `h`, `m` and `s`, each at most once and in that order, with no spaces,
//! signs, decimals or upper case: `"15m"`, `"1h30m"`, `"1d2h3m4s"`.

/// Why a duration string was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurationError {
    /// The text is not in the grammar.
    Syntax,
    /// The total is outside `1s..=365d` (or overflows).
    OutOfRange,
}

/// The shortest accepted duration, in seconds.
const MIN_SECS: u64 = 1;
/// The longest accepted duration (365 days), in seconds.
const MAX_SECS: u64 = 365 * 86_400;

/// The units in their required order, with their length in seconds.
const UNITS: [(u8, u64); 4] = [(b'd', 86_400), (b'h', 3_600), (b'm', 60), (b's', 1)];

/// Parses `text` into seconds, in `1..=31_536_000`.
///
/// # Errors
///
/// [`DurationError::Syntax`] for text outside the grammar, and
/// [`DurationError::OutOfRange`] for a total below 1 s, above 365 d, or too
/// large to compute.
pub(crate) fn parse_secs(text: &str) -> Result<u64, DurationError> {
    let mut rest = text.as_bytes();
    if rest.is_empty() {
        return Err(DurationError::Syntax);
    }
    // The index in `UNITS` the next group's unit must be at or after.
    let mut next_unit = 0;
    let mut overflow = false;
    let mut total: u64 = 0;
    while !rest.is_empty() {
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return Err(DurationError::Syntax);
        }
        let (number, tail) = rest.split_at(digits);
        let Some((&unit, tail)) = tail.split_first() else {
            return Err(DurationError::Syntax);
        };
        let Some(offset) = UNITS
            .get(next_unit..)
            .and_then(|units| units.iter().position(|&(u, _)| u == unit))
        else {
            return Err(DurationError::Syntax);
        };
        let index = next_unit + offset;
        next_unit = index + 1;
        let factor = UNITS.get(index).map_or(1, |&(_, f)| f);
        let group = number
            .iter()
            .try_fold(0_u64, |acc, &d| {
                acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
            })
            .and_then(|n| n.checked_mul(factor));
        // Keep checking the syntax after an overflow: a syntax error wins.
        match group.and_then(|g| total.checked_add(g)) {
            Some(sum) => total = sum,
            None => overflow = true,
        }
        rest = tail;
    }
    if overflow || !(MIN_SECS..=MAX_SECS).contains(&total) {
        return Err(DurationError::OutOfRange);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::{DurationError, parse_secs};

    #[test]
    fn accepted_durations() {
        for (text, secs) in [
            ("1s", 1),
            ("90s", 90),
            ("15m", 900),
            ("1h30m", 5_400),
            ("2d", 172_800),
            ("1d2h3m4s", 93_784),
            ("365d", 31_536_000),
            ("0d1s", 1),
            ("8760h", 31_536_000),
        ] {
            assert_eq!(parse_secs(text), Ok(secs), "{text}");
        }
    }

    #[test]
    fn out_of_range_durations() {
        for text in [
            "0s",
            "0m",
            "366d",
            "365d1s",
            "99999999999999999999d",
            "18446744073709551615d",
        ] {
            assert_eq!(parse_secs(text), Err(DurationError::OutOfRange), "{text}");
        }
    }

    #[test]
    fn syntax_errors() {
        for text in [
            "", "15", "m", "15 m", " 15m", "-1m", "+1m", "1.5h", "15M", "15x", "1m1h", "1m1m",
            "15m ", "１５m", "1h 30m", "1hm", "d1",
        ] {
            assert_eq!(parse_secs(text), Err(DurationError::Syntax), "{text:?}");
        }
    }
}
