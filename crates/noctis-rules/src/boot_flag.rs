//! The Boot Flag grammar (FR-73, AD-7), shared by the kind attributes'
//! `flags = [..]`, `Module::flags` and the `--features` / `NOCTIS_FEATURES`
//! selection: 1 to [`MAX_LEN`] characters of `a-z`, `0-9`, `_` and `-`;
//! `all` and `none` are reserved.
//!
//! Boot Flags are a framework convention, not a Discord rule, so they have
//! no rule id.

use std::error::Error;
use std::fmt;

/// The most characters a Boot Flag may have.
pub const MAX_LEN: usize = 32;

/// The selection words a Boot Flag may never be.
const RESERVED: [&str; 2] = ["all", "none"];

/// Why a Boot Flag, or an item of a Boot Flag selection, was rejected.
/// Indexes count characters.
///
/// ```
/// use noctis_rules::boot_flag::BootFlagError;
///
/// assert_eq!(
///     BootFlagError::Duplicate.to_string(),
///     "a Boot Flag is given twice: keep one"
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootFlagError {
    /// The flag (or a list item) is empty.
    ///
    /// ```
    /// use noctis_rules::boot_flag::BootFlagError;
    ///
    /// assert_eq!(
    ///     BootFlagError::Empty.to_string(),
    ///     "a Boot Flag is empty: write flags such as `beta`, separated by single commas"
    /// );
    /// ```
    Empty,
    /// The flag has more than [`MAX_LEN`] characters.
    ///
    /// ```
    /// use noctis_rules::boot_flag::BootFlagError;
    ///
    /// assert_eq!(
    ///     BootFlagError::TooLong { len: 33 }.to_string(),
    ///     "the Boot Flag has 33 characters: shorten it to at most 32"
    /// );
    /// ```
    TooLong {
        /// The number of characters.
        len: usize,
    },
    /// The flag has a character outside `a-z`, `0-9`, `_` and `-`.
    ///
    /// ```
    /// use noctis_rules::boot_flag::BootFlagError;
    ///
    /// assert_eq!(
    ///     BootFlagError::InvalidChar { index: 0, ch: 'B' }.to_string(),
    ///     "`B` at index 0 is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-`"
    /// );
    /// ```
    InvalidChar {
        /// The character index of the first invalid character.
        index: usize,
        /// The invalid character.
        ch: char,
    },
    /// The flag is `all` or `none`, which only a whole selection may be.
    ///
    /// ```
    /// use noctis_rules::boot_flag::BootFlagError;
    ///
    /// assert_eq!(
    ///     BootFlagError::Reserved { flag: "all" }.to_string(),
    ///     "`all` is reserved: write `all` or `none` alone, never in a list or as a declared flag"
    /// );
    /// ```
    Reserved {
        /// The reserved word: `all` or `none`.
        flag: &'static str,
    },
    /// The same flag is given twice.
    Duplicate,
}

impl fmt::Display for BootFlagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "a Boot Flag is empty: write flags such as `beta`, separated by single commas",
            ),
            Self::TooLong { len } => write!(
                f,
                "the Boot Flag has {len} characters: shorten it to at most {MAX_LEN}"
            ),
            Self::InvalidChar { index, ch } => write!(
                f,
                "`{ch}` at index {index} is not allowed in a Boot Flag: use only a-z, 0-9, `_` \
                 and `-`"
            ),
            Self::Reserved { flag } => write!(
                f,
                "`{flag}` is reserved: write `all` or `none` alone, never in a list or as a \
                 declared flag"
            ),
            Self::Duplicate => f.write_str("a Boot Flag is given twice: keep one"),
        }
    }
}

impl Error for BootFlagError {}

/// Checks one Boot Flag: not empty, at most [`MAX_LEN`] characters (counted
/// before the characters are checked), only `a-z`, `0-9`, `_` and `-` (the
/// first invalid character wins), and neither `all` nor `none`.
///
/// ```
/// use noctis_rules::boot_flag::{BootFlagError, check};
///
/// assert_eq!(check("eco_v2-x"), Ok(()));
/// assert_eq!(check("Beta"), Err(BootFlagError::InvalidChar { index: 0, ch: 'B' }));
/// assert_eq!(check("none"), Err(BootFlagError::Reserved { flag: "none" }));
/// ```
///
/// # Errors
///
/// The first broken rule, in the order above.
pub fn check(flag: &str) -> Result<(), BootFlagError> {
    if flag.is_empty() {
        return Err(BootFlagError::Empty);
    }
    let len = flag.chars().count();
    if len > MAX_LEN {
        return Err(BootFlagError::TooLong { len });
    }
    if let Some((index, ch)) = flag
        .chars()
        .enumerate()
        .find(|&(_, ch)| !matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-'))
    {
        return Err(BootFlagError::InvalidChar { index, ch });
    }
    match RESERVED.iter().find(|&&word| word == flag) {
        Some(&flag) => Err(BootFlagError::Reserved { flag }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::BootFlagError::{Empty, InvalidChar, Reserved, TooLong};
    use super::*;

    #[test]
    fn grammar_matrix() {
        let a32 = "a".repeat(32);
        let a33 = "a".repeat(33);
        let u33 = "ü".repeat(33);
        let cases: [(&str, Result<(), BootFlagError>); 22] = [
            // F1
            ("beta", Ok(())),
            ("a", Ok(())),
            ("0", Ok(())),
            ("_", Ok(())),
            ("-", Ok(())),
            ("eco_v2-x", Ok(())),
            // F2, F3, F4
            (&a32, Ok(())),
            (&a33, Err(TooLong { len: 33 })),
            (&u33, Err(TooLong { len: 33 })),
            // F5, F6
            ("", Err(Empty)),
            ("Beta", Err(InvalidChar { index: 0, ch: 'B' })),
            // F7
            ("a b", Err(InvalidChar { index: 1, ch: ' ' })),
            ("a.b", Err(InvalidChar { index: 1, ch: '.' })),
            ("a/b", Err(InvalidChar { index: 1, ch: '/' })),
            ("a:b", Err(InvalidChar { index: 1, ch: ':' })),
            // F8
            ("ü", Err(InvalidChar { index: 0, ch: 'ü' })),
            (
                "🙂x",
                Err(InvalidChar {
                    index: 0, ch: '🙂'
                }),
            ),
            // F9
            ("all", Err(Reserved { flag: "all" })),
            ("none", Err(Reserved { flag: "none" })),
            // F10
            ("ALL", Err(InvalidChar { index: 0, ch: 'A' })),
            ("alls", Ok(())),
            ("none_", Ok(())),
        ];
        for (flag, expected) in cases {
            assert_eq!(check(flag), expected, "{flag:?}");
        }
    }

    #[test]
    fn display_texts() {
        assert_eq!(
            Empty.to_string(),
            "a Boot Flag is empty: write flags such as `beta`, separated by single commas"
        );
        assert_eq!(
            TooLong { len: 40 }.to_string(),
            "the Boot Flag has 40 characters: shorten it to at most 32"
        );
        assert_eq!(
            InvalidChar { index: 2, ch: ' ' }.to_string(),
            "` ` at index 2 is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-`"
        );
        assert_eq!(
            Reserved { flag: "none" }.to_string(),
            "`none` is reserved: write `all` or `none` alone, never in a list or as a declared flag"
        );
        assert_eq!(
            BootFlagError::Duplicate.to_string(),
            "a Boot Flag is given twice: keep one"
        );
    }
}
