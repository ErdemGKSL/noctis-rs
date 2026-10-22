//! [`Classes`]: a Tailwind class string, checked (a `tw!` literal) or not.

use std::borrow::Cow;

/// A Tailwind class string.
///
/// `tw!("…")` and `tw = "…"` in `view!` give *checked* classes: they were
/// validated at compile time against the grammar and the crate's
/// `noctis-theme.css`. [`Classes::dynamic`] gives *unchecked* classes, which
/// the runtime class check validates (it warns once per distinct string, and
/// fails tests on test Apps).
///
/// The default is the empty, checked class string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classes {
    text: Cow<'static, str>,
    checked: bool,
}

impl Classes {
    /// Checked classes; only the macros call it, after validating `text`.
    #[doc(hidden)]
    pub const fn __literal(text: &'static str) -> Self {
        Self {
            text: Cow::Borrowed(text),
            checked: true,
        }
    }

    /// Unchecked classes, built at run time (for example chosen from a fixed
    /// set by a match). The runtime class check validates them.
    ///
    /// **Never pass user text**: user data enters a Template only as text
    /// content (a text child), and dynamic values use the typed [`Style`]
    /// setters.
    ///
    /// [`Style`]: crate::Style
    pub fn dynamic(text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            text: text.into(),
            checked: false,
        }
    }

    /// `self` and `other` joined with one space; checked only if both are.
    #[must_use]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "by value like the other builders, so joins chain without borrows"
    )]
    pub fn and(self, other: Classes) -> Classes {
        Self {
            text: Cow::Owned(format!("{} {}", self.text, other.text)),
            checked: self.checked && other.checked,
        }
    }

    /// The class string.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the macros validated these classes at compile time.
    pub(crate) fn is_checked(&self) -> bool {
        self.checked
    }
}

impl Default for Classes {
    fn default() -> Self {
        Self::__literal("")
    }
}
