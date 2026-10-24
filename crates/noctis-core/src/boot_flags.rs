//! `BootFlags`: the Boot Flag selection an App is built with (FR-73, AD-7).

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

pub use noctis_rules::boot_flag::BootFlagError;
use noctis_rules::boot_flag::check;

/// Which Boot Flags an App is built with: `all` (the default), `none`, or a
/// set of flags.
///
/// Modules carry flags with `Module::flags([..])` and single registrations
/// with `flags = [..]` on their kind attribute. An unflagged Module or
/// registration always exists; a flagged one exists when the selection is
/// `all` or holds **any** of its flags; a pruned Module drops its whole
/// subtree. `none` keeps only unflagged items.
///
/// The text form (`--features`, `NOCTIS_FEATURES`) is `all`, `none` or
/// `a,b,..`: single commas, no spaces, no empty items, no duplicates, each
/// item a valid Boot Flag (1 to 32 characters of `a-z`, `0-9`, `_` and
/// `-`; never `all` or `none`). `Display` is canonical: `all`, `none`, or
/// the flags sorted and comma-joined. Equality is set equality.
///
/// ```
/// use noctis_core::BootFlags;
///
/// let flags: BootFlags = "economy,beta".parse()?;
/// assert_eq!(flags.to_string(), "beta,economy");
/// assert_eq!(flags, BootFlags::only(["beta", "economy"])?);
/// assert!(flags.selects(&["economy"]));
/// assert!(!flags.selects(&["admin"]));
/// assert!(flags.selects(&[]));
/// assert_eq!(BootFlags::default(), BootFlags::all());
/// assert_eq!("none".parse::<BootFlags>()?, BootFlags::none());
/// # Ok::<(), noctis_core::BootFlagError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootFlags(Selection);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Selection {
    #[default]
    All,
    /// The selected flags; empty is `none`.
    Only(BTreeSet<Box<str>>),
}

impl BootFlags {
    /// Every flag: nothing is pruned. The default.
    #[must_use]
    pub fn all() -> Self {
        Self(Selection::All)
    }

    /// No flag: only unflagged Modules and registrations exist.
    #[must_use]
    pub fn none() -> Self {
        Self(Selection::Only(BTreeSet::new()))
    }

    /// The flags `flags`, each a valid Boot Flag given once; no flags is
    /// [`none`](Self::none).
    ///
    /// ```
    /// use noctis_core::{BootFlagError, BootFlags};
    ///
    /// assert_eq!(BootFlags::only::<[&str; 0]>([]), Ok(BootFlags::none()));
    /// assert_eq!(BootFlags::only(["all"]), Err(BootFlagError::Reserved { flag: "all" }));
    /// assert_eq!(BootFlags::only(["a", "a"]), Err(BootFlagError::Duplicate));
    /// ```
    ///
    /// # Errors
    ///
    /// The first flag, in order, that breaks the grammar or repeats an
    /// earlier one.
    pub fn only<I>(flags: I) -> Result<Self, BootFlagError>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut set = BTreeSet::new();
        for flag in flags {
            let flag = flag.as_ref();
            check(flag)?;
            if !set.insert(Box::from(flag)) {
                return Err(BootFlagError::Duplicate);
            }
        }
        Ok(Self(Selection::Only(set)))
    }

    /// Whether this is [`all`](Self::all).
    #[must_use]
    pub fn is_all(&self) -> bool {
        matches!(self.0, Selection::All)
    }

    /// Whether an item declaring `flags` exists under this selection:
    /// always for no flags, else when the selection is `all` or holds any
    /// of them.
    #[must_use]
    pub fn selects(&self, flags: &[&str]) -> bool {
        match &self.0 {
            Selection::All => true,
            Selection::Only(set) => flags.is_empty() || flags.iter().any(|f| set.contains(*f)),
        }
    }

    /// The selected flags, sorted; empty for `all` and `none`.
    pub(crate) fn flags(&self) -> impl Iterator<Item = &str> {
        let set = match &self.0 {
            Selection::All => None,
            Selection::Only(set) => Some(set),
        };
        set.into_iter().flatten().map(AsRef::as_ref)
    }
}

impl FromStr for BootFlags {
    type Err = BootFlagError;

    /// Parses `all`, `none` or `a,b,..`; error indexes count characters in
    /// the whole value, and the first failing item, left to right, wins.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => return Ok(Self::all()),
            "none" => return Ok(Self::none()),
            _ => {}
        }
        let mut set = BTreeSet::new();
        let mut offset = 0;
        for item in s.split(',') {
            check(item).map_err(|err| match err {
                BootFlagError::InvalidChar { index, ch } => BootFlagError::InvalidChar {
                    index: offset + index,
                    ch,
                },
                other => other,
            })?;
            if !set.insert(Box::from(item)) {
                return Err(BootFlagError::Duplicate);
            }
            offset += item.chars().count() + 1;
        }
        Ok(Self(Selection::Only(set)))
    }
}

impl fmt::Display for BootFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Selection::All => f.write_str("all"),
            Selection::Only(set) if set.is_empty() => f.write_str("none"),
            Selection::Only(set) => {
                for (i, flag) in set.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    f.write_str(flag)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BootFlagError::{Duplicate, Empty, InvalidChar, Reserved, TooLong};
    use super::*;

    fn only(flags: &[&str]) -> BootFlags {
        BootFlags::only(flags).expect("valid flags")
    }

    #[test]
    fn selection_matrix() {
        let a32 = "a".repeat(32);
        let a33 = "a".repeat(33);
        let cases: [(&str, Result<BootFlags, BootFlagError>); 19] = [
            ("all", Ok(BootFlags::all())),
            ("none", Ok(BootFlags::none())),
            ("a", Ok(only(&["a"]))),
            ("b,a", Ok(only(&["a", "b"]))),
            ("", Err(Empty)),
            ("a,", Err(Empty)),
            (",a", Err(Empty)),
            ("a,,b", Err(Empty)),
            ("a,a", Err(Duplicate)),
            ("all,a", Err(Reserved { flag: "all" })),
            ("a,none", Err(Reserved { flag: "none" })),
            ("all,all", Err(Reserved { flag: "all" })),
            ("a,B", Err(InvalidChar { index: 2, ch: 'B' })),
            ("a, b", Err(InvalidChar { index: 2, ch: ' ' })),
            (&a32, Ok(only(&[&a32]))),
            (&a33, Err(TooLong { len: 33 })),
            ("a,ü", Err(InvalidChar { index: 2, ch: 'ü' })),
            ("B,,", Err(InvalidChar { index: 0, ch: 'B' })),
            ("ü,a,B", Err(InvalidChar { index: 0, ch: 'ü' })),
        ];
        for (value, expected) in cases {
            assert_eq!(value.parse::<BootFlags>(), expected, "{value:?}");
        }
        assert_eq!(
            "üa,b,C".parse::<BootFlags>(),
            Err(InvalidChar { index: 0, ch: 'ü' })
        );
        assert_eq!(
            "ab,cd,eF".parse::<BootFlags>(),
            Err(InvalidChar { index: 7, ch: 'F' })
        );
    }

    #[test]
    fn display_is_canonical() {
        assert_eq!(BootFlags::all().to_string(), "all");
        assert_eq!(BootFlags::none().to_string(), "none");
        assert_eq!(only(&["a"]).to_string(), "a");
        assert_eq!(only(&["b", "a"]).to_string(), "a,b");
        for text in ["all", "none", "a", "a,b", "eco_v2-x,z"] {
            let parsed: BootFlags = text.parse().expect("valid");
            assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn only_rules() {
        assert_eq!(BootFlags::only(Vec::<String>::new()), Ok(BootFlags::none()));
        assert_eq!(BootFlags::only(["all"]), Err(Reserved { flag: "all" }));
        assert_eq!(BootFlags::only(["a", "a"]), Err(Duplicate));
        assert_eq!(BootFlags::only([""]), Err(Empty));
        assert_eq!(
            BootFlags::only(["Beta"]),
            Err(InvalidChar { index: 0, ch: 'B' })
        );
        assert_eq!(only(&["a", "b"]), only(&["b", "a"]));
    }

    #[test]
    fn selects_any_of() {
        let ab = only(&["a", "b"]);
        assert!(ab.selects(&[]));
        assert!(ab.selects(&["b"]));
        assert!(ab.selects(&["x", "a"]));
        assert!(!ab.selects(&["x"]));
        assert!(BootFlags::all().selects(&["x"]));
        assert!(BootFlags::none().selects(&[]));
        assert!(!BootFlags::none().selects(&["a"]));
        assert!(BootFlags::all().is_all());
        assert!(!BootFlags::none().is_all());
        assert_eq!(ab.flags().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(BootFlags::all().flags().count(), 0);
    }
}
