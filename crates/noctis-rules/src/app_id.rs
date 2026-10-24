//! The App id grammar (FR-71, FR-47), shared by `AppId::new`,
//! `AppBuilder::app_id`, `NOCTIS_APP_ID`, the kind attributes'
//! `apps = [..]` and `Module::apps`: 1 to [`MAX_LEN`] characters of `a-z`,
//! `0-9` and `-` (`[a-z0-9-]{1,32}`).
//!
//! App ids are a framework convention, not a Discord rule, so they have no
//! rule id.

use std::error::Error;
use std::fmt;

/// The most characters an App id may have.
pub const MAX_LEN: usize = 32;

/// Why an App id was rejected. Indexes count characters.
///
/// ```
/// use noctis_rules::app_id::AppIdError;
///
/// let err = AppIdError::InvalidChar { index: 1, ch: '_' };
/// assert!(err.to_string().contains("[a-z0-9-]{1,32}"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppIdError {
    /// The id is empty.
    ///
    /// ```
    /// use noctis_rules::app_id::AppIdError;
    ///
    /// assert_eq!(
    ///     AppIdError::Empty.to_string(),
    ///     "App id is empty: use 1-32 characters matching [a-z0-9-]{1,32}, such as `my-bot`"
    /// );
    /// ```
    Empty,
    /// The id has more than [`MAX_LEN`] characters.
    ///
    /// ```
    /// use noctis_rules::app_id::AppIdError;
    ///
    /// assert_eq!(
    ///     AppIdError::TooLong { len: 33 }.to_string(),
    ///     "App id has 33 characters: shorten it to at most 32 matching [a-z0-9-]{1,32}"
    /// );
    /// ```
    TooLong {
        /// The number of characters.
        len: usize,
    },
    /// The id has a character outside `a-z`, `0-9` and `-`.
    ///
    /// ```
    /// use noctis_rules::app_id::AppIdError;
    ///
    /// assert_eq!(
    ///     AppIdError::InvalidChar { index: 0, ch: 'M' }.to_string(),
    ///     "App id has `M` at index 0: use only a-z, 0-9 and - ([a-z0-9-]{1,32})"
    /// );
    /// ```
    InvalidChar {
        /// The character index of the first invalid character.
        index: usize,
        /// The invalid character.
        ch: char,
    },
}

impl fmt::Display for AppIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "App id is empty: use 1-32 characters matching [a-z0-9-]{1,32}, such as `my-bot`",
            ),
            Self::TooLong { len } => write!(
                f,
                "App id has {len} characters: shorten it to at most {MAX_LEN} matching \
                 [a-z0-9-]{{1,32}}"
            ),
            Self::InvalidChar { index, ch } => write!(
                f,
                "App id has `{ch}` at index {index}: use only a-z, 0-9 and - ([a-z0-9-]{{1,32}})"
            ),
        }
    }
}

impl Error for AppIdError {}

/// Checks one App id against `[a-z0-9-]{1,32}`: not empty, at most
/// [`MAX_LEN`] characters (counted before the characters are checked), then
/// only `a-z`, `0-9` and `-` (the first invalid character wins). The input
/// is never trimmed.
///
/// ```
/// use noctis_rules::app_id::{AppIdError, check};
///
/// assert_eq!(check("my-bot"), Ok(()));
/// assert_eq!(check("Main"), Err(AppIdError::InvalidChar { index: 0, ch: 'M' }));
/// assert_eq!(check(""), Err(AppIdError::Empty));
/// ```
///
/// # Errors
///
/// The first broken rule, in the order above.
pub fn check(id: &str) -> Result<(), AppIdError> {
    if id.is_empty() {
        return Err(AppIdError::Empty);
    }
    let len = id.chars().count();
    if len > MAX_LEN {
        return Err(AppIdError::TooLong { len });
    }
    match id
        .chars()
        .enumerate()
        .find(|&(_, ch)| !matches!(ch, 'a'..='z' | '0'..='9' | '-'))
    {
        Some((index, ch)) => Err(AppIdError::InvalidChar { index, ch }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::AppIdError::{Empty, InvalidChar, TooLong};
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn grammar_matrix() {
        let a32 = "a".repeat(32);
        let a33 = "a".repeat(33);
        let u33 = "ü".repeat(33);
        let cases: [(&str, Result<(), AppIdError>); 12] = [
            ("", Err(Empty)),
            ("a", Ok(())),
            ("-", Ok(())),
            ("my-bot-2", Ok(())),
            (&a32, Ok(())),
            (&a33, Err(TooLong { len: 33 })),
            (&u33, Err(TooLong { len: 33 })),
            ("Main", Err(InvalidChar { index: 0, ch: 'M' })),
            ("bot-ü", Err(InvalidChar { index: 4, ch: 'ü' })),
            ("a_b", Err(InvalidChar { index: 1, ch: '_' })),
            ("a b", Err(InvalidChar { index: 1, ch: ' ' })),
            (" a", Err(InvalidChar { index: 0, ch: ' ' })),
        ];
        for (id, expected) in cases {
            assert_eq!(check(id), expected, "{id:?}");
        }
    }

    proptest! {
        #[test]
        fn valid_ids_pass(id in "[a-z0-9-]{1,32}") {
            prop_assert_eq!(check(&id), Ok(()));
        }

        #[test]
        fn long_ids_fail(id in "[a-z0-9-]{33,64}") {
            prop_assert_eq!(check(&id), Err(TooLong { len: id.chars().count() }));
        }

        #[test]
        fn uppercase_fails_at_its_index(prefix in "[a-z0-9-]{0,10}", upper in "[A-Z]") {
            let id = format!("{prefix}{upper}");
            let ch = upper.chars().next().unwrap_or('A');
            prop_assert_eq!(check(&id), Err(InvalidChar { index: prefix.len(), ch }));
        }

        #[test]
        fn unicode_fails(prefix in "[a-z]{0,10}", ch in "[\u{80}-\u{10FFFF}]") {
            let id = format!("{prefix}{ch}");
            let c = ch.chars().next().unwrap_or('ü');
            prop_assert_eq!(check(&id), Err(InvalidChar { index: prefix.len(), ch: c }));
        }
    }
}
