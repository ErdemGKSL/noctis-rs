//! Locale runtime, merged catalogs and the default `noctis.*` strings.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! It ships:
//!
//! - [`Builtin`], the [`Translate`] implementation that holds the English
//!   (`en-US`) text of every reserved notice key in [`keys`]; every other
//!   locale falls back to it (LOC-04);
//! - [`Catalog`], the compiled locale files of one `i18n!` invocation;
//! - [`AppStrings`], an App's catalogs layered over [`Builtin`], which
//!   rejects a key that two catalogs define ([`DuplicateKey`]);
//! - [`format`], the placeholder substitution every lookup uses.
//!
//! ```
//! use noctis_i18n::{keys, Builtin};
//! use noctis_rules::{Locale, Translate};
//!
//! let text = Builtin.translate(Locale::Tr, keys::ERROR_GENERIC, &[]);
//! assert_eq!(text.as_deref(), Some("Something went wrong. Please try again later."));
//! ```

use std::fmt::{Display, Write as _};

use noctis_rules::{Locale, MessageKey, Translate};

mod catalog;

pub use catalog::{AppStrings, Catalog, DuplicateKey};

/// The reserved `noctis.*` notice keys the framework renders (AD-16).
///
/// ```
/// use noctis_i18n::keys;
///
/// assert_eq!(keys::ERROR_GENERIC.as_str(), "noctis.error.generic");
/// assert!(keys::ALL.iter().all(|key| key.is_framework()));
/// ```
pub mod keys {
    use noctis_rules::MessageKey;

    /// `noctis.error.generic`: the default user-facing error.
    pub const ERROR_GENERIC: MessageKey = MessageKey::new("noctis.error.generic");
    /// `noctis.validation.failed`: an option failed to decode or validate.
    pub const VALIDATION_FAILED: MessageKey = MessageKey::new("noctis.validation.failed");
    /// `noctis.expired`: the default Expired Handler notice.
    pub const EXPIRED: MessageKey = MessageKey::new("noctis.expired");
    /// `noctis.stale`: reserved; v1 sends `noctis.expired` for stale ids
    /// too.
    pub const STALE: MessageKey = MessageKey::new("noctis.stale");
    /// `noctis.disabled_here`: the Module is disabled in this guild.
    pub const DISABLED_HERE: MessageKey = MessageKey::new("noctis.disabled_here");
    /// `noctis.rate_limited`: the rate-limit notice (`{retry}` is a relative timestamp).
    pub const RATE_LIMITED: MessageKey = MessageKey::new("noctis.rate_limited");
    /// `noctis.guard.failed`: a Guard refused the interaction.
    pub const GUARD_FAILED: MessageKey = MessageKey::new("noctis.guard.failed");
    /// `noctis.guard.owner`: only the user who started the interaction may use it.
    pub const GUARD_OWNER: MessageKey = MessageKey::new("noctis.guard.owner");
    /// `noctis.guard.permissions`: the user lacks the required permissions.
    pub const GUARD_PERMISSIONS: MessageKey = MessageKey::new("noctis.guard.permissions");

    /// Every key above, in declaration order.
    pub const ALL: [MessageKey; 9] = [
        ERROR_GENERIC,
        VALIDATION_FAILED,
        EXPIRED,
        STALE,
        DISABLED_HERE,
        RATE_LIMITED,
        GUARD_FAILED,
        GUARD_OWNER,
        GUARD_PERMISSIONS,
    ];
}

/// The built-in strings for the reserved [`keys`].
///
/// Only `en-US` text ships today. A lookup tries the locale, then its
/// [`Locale::sibling`], then `en-US`, so every locale resolves to the English
/// text (LOC-04). A key outside [`keys::ALL`] is `None`.
///
/// ```
/// use noctis_i18n::{keys, Builtin};
/// use noctis_rules::{Locale, MessageKey, Translate};
///
/// let retry: &dyn std::fmt::Display = &"<t:5:R>";
/// assert_eq!(
///     Builtin.translate(Locale::De, keys::RATE_LIMITED, &[("retry", retry)]).as_deref(),
///     Some("You're doing that too often. Try again <t:5:R>."),
/// );
/// assert_eq!(Builtin.translate(Locale::EnUs, MessageKey::new("shop.missing"), &[]), None);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct Builtin;

impl Builtin {
    /// The raw template of `key` in `locale`, without fallback.
    fn template(locale: Locale, key: MessageKey) -> Option<&'static str> {
        if locale != Locale::EnUs {
            return None;
        }
        let text = match key.as_str() {
            "noctis.error.generic" => "Something went wrong. Please try again later.",
            "noctis.validation.failed" => "That input isn't valid. Check it and try again.",
            "noctis.expired" => "This interaction has expired. Run the command again.",
            "noctis.stale" => "This message is out of date. Run the command again.",
            "noctis.disabled_here" => "This feature is disabled here.",
            "noctis.rate_limited" => "You're doing that too often. Try again {retry}.",
            "noctis.guard.failed" => "You can't use this.",
            "noctis.guard.owner" => "Only the person who started this can use it.",
            "noctis.guard.permissions" => "You don't have permission to use this.",
            _ => return None,
        };
        Some(text)
    }
}

impl Translate for Builtin {
    fn translate(
        &self,
        locale: Locale,
        key: MessageKey,
        args: &[(&str, &dyn Display)],
    ) -> Option<String> {
        let template = Self::template(locale, key)
            .or_else(|| locale.sibling().and_then(|s| Self::template(s, key)))
            .or_else(|| Self::template(Locale::EnUs, key))?;
        Some(format(template, args))
    }
}

/// Replaces `{name}` placeholders in `template` with the matching `args`, in a
/// single pass.
///
/// A placeholder is `{`, one or more ASCII letters, digits or `_`, then `}`, and
/// its name must be in `args`. Anything else (`{}`, `{a b}`, an unclosed `{x`,
/// an unknown name) is copied verbatim; unused `args` are ignored; substituted
/// values are never scanned again.
///
/// ```
/// use noctis_i18n::format;
///
/// let n: &dyn std::fmt::Display = &3;
/// assert_eq!(format("{n} left, {other}", &[("n", n)]), "3 left, {other}");
/// assert_eq!(format("{{n}}", &[("n", n)]), "{3}");
/// ```
pub fn format(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        let value = (name_len > 0 && after.as_bytes().get(name_len) == Some(&b'}'))
            .then(|| &after[..name_len])
            .and_then(|name| args.iter().find(|(n, _)| *n == name));
        if let Some((_, value)) = value {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "{value}");
            rest = &after[name_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn d(value: &dyn Display) -> &dyn Display {
        value
    }

    #[test]
    fn loc_04_every_locale_falls_back_to_en_us() {
        for key in keys::ALL {
            let english = Builtin.translate(Locale::EnUs, key, &[]);
            assert!(english.is_some(), "{key} has no en-US text");
            for locale in Locale::ALL {
                assert_eq!(
                    Builtin.translate(locale, key, &[]),
                    english,
                    "{locale} {key}"
                );
            }
        }
    }

    #[test]
    fn unknown_key_is_none_in_every_locale() {
        for locale in Locale::ALL {
            assert_eq!(
                Builtin.translate(locale, MessageKey::new("shop.missing"), &[]),
                None
            );
            assert_eq!(
                Builtin.translate(locale, MessageKey::new("noctis.wizard.next"), &[]),
                None
            );
        }
    }

    #[test]
    fn key_set_is_complete_and_reserved() {
        assert_eq!(keys::ALL.len(), 9);
        let unique: HashSet<_> = keys::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(unique.len(), keys::ALL.len());
        for key in keys::ALL {
            assert!(key.is_framework(), "{key}");
            assert!(Builtin::template(Locale::EnUs, key).is_some(), "{key}");
        }
    }

    #[test]
    fn exact_texts() {
        let cases = [
            (
                keys::ERROR_GENERIC,
                "Something went wrong. Please try again later.",
            ),
            (
                keys::VALIDATION_FAILED,
                "That input isn't valid. Check it and try again.",
            ),
            (
                keys::EXPIRED,
                "This interaction has expired. Run the command again.",
            ),
            (
                keys::STALE,
                "This message is out of date. Run the command again.",
            ),
            (keys::DISABLED_HERE, "This feature is disabled here."),
            (
                keys::RATE_LIMITED,
                "You're doing that too often. Try again {retry}.",
            ),
            (keys::GUARD_FAILED, "You can't use this."),
            (
                keys::GUARD_OWNER,
                "Only the person who started this can use it.",
            ),
            (
                keys::GUARD_PERMISSIONS,
                "You don't have permission to use this.",
            ),
        ];
        for (key, text) in cases {
            assert_eq!(
                Builtin.translate(Locale::EnUs, key, &[]).as_deref(),
                Some(text)
            );
        }
    }

    #[test]
    fn placeholders() {
        let retry = "<t:5:R>";
        let args = [("retry", d(&retry))];
        assert_eq!(format("Try again {retry}.", &args), "Try again <t:5:R>.");
        // Missing argument: verbatim.
        assert_eq!(format("Try again {retry}.", &[]), "Try again {retry}.");
        // Extra argument: ignored.
        let extra = [("retry", d(&retry)), ("other", d(&1))];
        assert_eq!(format("Try again {retry}.", &extra), "Try again <t:5:R>.");
        // Malformed placeholders: verbatim.
        assert_eq!(format("{}", &args), "{}");
        assert_eq!(format("{a b}", &[("a", d(&1))]), "{a b}");
        assert_eq!(format("x {retry", &args), "x {retry");
        assert_eq!(format("{", &args), "{");
        // Doubled braces: outer brace, value, outer brace.
        assert_eq!(format("{{retry}}", &args), "{<t:5:R>}");
        // Single pass: a value that looks like a placeholder is not substituted.
        let sneaky = "{retry}";
        assert_eq!(format("{retry}", &[("retry", d(&sneaky))]), "{retry}");
        // Every occurrence is replaced.
        assert_eq!(format("{n}{n}", &[("n", d(&7))]), "77");
        // Unicode stays intact.
        assert_eq!(format("çğ😀 {n}", &[("n", d(&"ü"))]), "çğ😀 ü");
        assert_eq!(format("😀{😀}", &[]), "😀{😀}");
    }

    /// `noctis_rules::placeholder_names` finds exactly the placeholders
    /// `format` substitutes, in order, on every template above.
    #[test]
    fn placeholder_names_agree_with_format() {
        for template in [
            "Try again {retry}.",
            "{}",
            "{a b}",
            "x {retry",
            "{",
            "{{retry}}",
            "{retry}",
            "{n}{n}",
            "çğ😀 {n}",
            "😀{😀}",
            "{1} of {0}, {Name} {_x} {1a}",
        ] {
            let names: Vec<&str> = noctis_rules::placeholder_names(template).collect();
            let marks: Vec<String> = names.iter().map(|n| format!("\u{1}{n}\u{2}")).collect();
            let args: Vec<(&str, &dyn Display)> = names
                .iter()
                .zip(&marks)
                .map(|(n, m)| (*n, m as &dyn Display))
                .collect();
            let out = format(template, &args);
            let substituted: Vec<&str> = out
                .split('\u{1}')
                .skip(1)
                .filter_map(|part| part.split_once('\u{2}').map(|(name, _)| name))
                .collect();
            assert_eq!(substituted, names, "{template}");
            assert_eq!(format(template, &[]), template, "{template}");
        }
    }

    #[test]
    fn builtin_substitutes_args() {
        let retry = "<t:5:R>";
        assert_eq!(
            Builtin
                .translate(Locale::Ja, keys::RATE_LIMITED, &[("retry", d(&retry))])
                .as_deref(),
            Some("You're doing that too often. Try again <t:5:R>.")
        );
    }
}
