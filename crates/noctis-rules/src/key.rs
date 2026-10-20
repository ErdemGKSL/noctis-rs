//! Localization keys and the [`Translate`] port.

use std::fmt;

use crate::Locale;

/// A validated localization key, such as `setup.step.channel`.
///
/// A key is one or more non-empty segments of ASCII letters, digits, `_` and `-`,
/// joined by `.`. Keys whose first segment is `noctis` are reserved for the
/// framework ([`MessageKey::is_framework`]).
///
/// ```
/// use noctis_rules::MessageKey;
///
/// const STEP: MessageKey = MessageKey::new("setup.step.channel");
/// assert_eq!(STEP.as_str(), "setup.step.channel");
/// assert!(!STEP.is_framework());
/// ```
///
/// An invalid key in a `const` item is a compile error:
///
/// ```compile_fail
/// const BAD: noctis_rules::MessageKey = noctis_rules::MessageKey::new("a..b");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageKey(&'static str);

impl MessageKey {
    /// Creates a key, validating it.
    ///
    /// # Panics
    ///
    /// Panics when `key` is empty, has an empty segment (`".a"`, `"a."`,
    /// `"a..b"`) or contains anything other than ASCII `[A-Za-z0-9_-]` and `.`.
    /// In a `const` item the panic is a compile error.
    pub const fn new(key: &'static str) -> Self {
        let bytes = key.as_bytes();
        assert!(!bytes.is_empty(), "MessageKey must not be empty");
        let mut i = 0;
        let mut segment_len = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'.' {
                assert!(segment_len > 0, "MessageKey has an empty segment");
                segment_len = 0;
            } else {
                assert!(
                    b.is_ascii_alphanumeric() || b == b'_' || b == b'-',
                    "MessageKey segments may only contain ASCII letters, digits, `_` and `-`"
                );
                segment_len += 1;
            }
            i += 1;
        }
        assert!(segment_len > 0, "MessageKey has an empty segment");
        Self(key)
    }

    /// The key as written.
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// Whether the key is in the reserved `noctis` namespace (its first segment is
    /// exactly `noctis`).
    pub const fn is_framework(self) -> bool {
        const NS: &[u8] = b"noctis";
        let bytes = self.0.as_bytes();
        if bytes.len() < NS.len() {
            return false;
        }
        let mut i = 0;
        while i < NS.len() {
            if bytes[i] != NS[i] {
                return false;
            }
            i += 1;
        }
        bytes.len() == NS.len() || bytes[NS.len()] == b'.'
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Looks up and formats a localized message.
///
/// The trait is object-safe and requires `Send + Sync`, so a `&dyn Translate`
/// can be held across `.await` on a multi-threaded runtime; render functions take
/// `&dyn Translate`. `noctis-i18n` is the
/// implementation (AD-16). Implementors:
///
/// - look the key up in `locale`, then in its [`Locale::sibling`], then in the
///   default locale (AD-16, FR-43);
/// - replace every `{name}` placeholder with the matching entry of `args`;
/// - return `None` only when no fallback has the key.
pub trait Translate: Send + Sync {
    /// The message for `key` in `locale`, with `args` substituted, or `None` when
    /// no locale in the fallback chain has the key.
    fn translate(
        &self,
        locale: Locale,
        key: MessageKey,
        args: &[(&str, &dyn fmt::Display)],
    ) -> Option<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn valid_keys_construct() {
        const KEY: MessageKey = MessageKey::new("setup.step.channel");
        assert_eq!(KEY.as_str(), "setup.step.channel");
        assert_eq!(KEY.to_string(), "setup.step.channel");
        for key in ["a", "A-b_9", "noctis.error.generic"] {
            assert_eq!(MessageKey::new(key).as_str(), key);
        }
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn empty_key_panics() {
        let _ = MessageKey::new("");
    }

    #[test]
    #[should_panic(expected = "empty segment")]
    fn leading_dot_panics() {
        let _ = MessageKey::new(".a");
    }

    #[test]
    #[should_panic(expected = "empty segment")]
    fn trailing_dot_panics() {
        let _ = MessageKey::new("a.");
    }

    #[test]
    #[should_panic(expected = "empty segment")]
    fn double_dot_panics() {
        let _ = MessageKey::new("a..b");
    }

    #[test]
    #[should_panic(expected = "may only contain")]
    fn space_panics() {
        let _ = MessageKey::new("a b");
    }

    #[test]
    #[should_panic(expected = "may only contain")]
    fn non_ascii_panics() {
        let _ = MessageKey::new("ğ");
    }

    #[test]
    #[should_panic(expected = "may only contain")]
    fn slash_panics() {
        let _ = MessageKey::new("a/b");
    }

    #[test]
    fn framework_namespace() {
        assert!(MessageKey::new("noctis.expired").is_framework());
        assert!(MessageKey::new("noctis").is_framework());
        assert!(!MessageKey::new("noctisx.a").is_framework());
        assert!(!MessageKey::new("a.noctis").is_framework());
    }

    struct Table(HashMap<(Locale, &'static str), &'static str>);

    impl Translate for Table {
        fn translate(
            &self,
            locale: Locale,
            key: MessageKey,
            args: &[(&str, &dyn fmt::Display)],
        ) -> Option<String> {
            let template = self.0.get(&(locale, key.as_str()))?;
            let mut out = (*template).to_owned();
            for (name, value) in args {
                out = out.replace(&format!("{{{name}}}"), &value.to_string());
            }
            Some(out)
        }
    }

    #[test]
    fn translate_is_object_safe() {
        let table = Table(HashMap::from([(
            (Locale::Tr, "greet"),
            "Merhaba {name}, {name}!",
        )]));
        let t: &dyn Translate = &table;
        let name: &dyn fmt::Display = &"Ada";
        assert_eq!(
            t.translate(Locale::Tr, MessageKey::new("greet"), &[("name", name)]),
            Some("Merhaba Ada, Ada!".to_owned())
        );
        assert_eq!(
            t.translate(Locale::Tr, MessageKey::new("missing"), &[]),
            None
        );
    }
}
