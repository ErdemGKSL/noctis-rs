//! The localized notice a failed handler run shows its user (FR-22,
//! AD-16, LOC-04): which key and which text (the locale comes from the
//! run's locale cell, `crate::locale`).

use noctis_i18n::{Builtin, keys};
use noctis_rules::{Locale, MessageKey, Translate};

use crate::HandlerError;
use crate::options::OptionError;

/// The text of `key` in `locale`. A key no catalog has renders the generic
/// error text, with one `warn` on target `noctis::i18n`.
pub(crate) fn text(strings: &dyn Translate, locale: Locale, key: MessageKey) -> String {
    text_with(strings, locale, key, &[])
}

/// The text of `key` in `locale` with its `{name}` arguments replaced;
/// like [`text`] for a key no catalog has.
pub(crate) fn text_with(
    strings: &dyn Translate,
    locale: Locale,
    key: MessageKey,
    args: &[(&str, &dyn std::fmt::Display)],
) -> String {
    if let Some(text) = strings.translate(locale, key, args) {
        return text;
    }
    tracing::warn!(target: "noctis::i18n", key = key.as_str(), "missing message key");
    strings
        .translate(locale, keys::ERROR_GENERIC, &[])
        .or_else(|| Builtin.translate(Locale::EnUs, keys::ERROR_GENERIC, &[]))
        .unwrap_or_default()
}

/// The notice key of a failed run: the error's own key, then
/// `noctis.validation.failed` for an option that failed to decode, else
/// `noctis.error.generic`.
pub(crate) fn key_for(error: &HandlerError) -> MessageKey {
    if let Some(key) = error.user_message() {
        key
    } else if error.as_error().is::<OptionError>() {
        keys::VALIDATION_FAILED
    } else {
        keys::ERROR_GENERIC
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Display;
    use std::io;

    use super::*;
    use crate::test_support::Capture;

    const GENERIC: &str = "Something went wrong. Please try again later.";

    #[test]
    fn known_key_renders_its_text() {
        assert_eq!(
            text(&Builtin, Locale::Tr, keys::GUARD_OWNER),
            "Only the person who started this can use it."
        );
    }

    #[test]
    fn unknown_key_warns_and_renders_generic() {
        let capture = Capture::default();
        let text = tracing::subscriber::with_default(capture.subscriber(), || {
            text(&Builtin, Locale::EnUs, MessageKey::new("shop.missing"))
        });
        assert_eq!(text, GENERIC);
        let lines = capture.lines_with(&["WARN", "noctis::i18n", "key=\"shop.missing\""]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
    }

    /// A translator without any text: the last resort is `Builtin`.
    struct Empty;

    impl Translate for Empty {
        fn translate(
            &self,
            _: Locale,
            _: MessageKey,
            _: &[(&str, &dyn Display)],
        ) -> Option<String> {
            None
        }
    }

    #[test]
    fn empty_translator_falls_back_to_builtin_generic() {
        assert_eq!(text(&Empty, Locale::De, keys::EXPIRED), GENERIC);
    }

    #[test]
    fn key_rule() {
        let owner = keys::GUARD_OWNER;
        assert_eq!(
            key_for(&HandlerError::new(io::Error::other("x"))),
            keys::ERROR_GENERIC
        );
        assert_eq!(
            key_for(&HandlerError::new(io::Error::other("x")).with_message(owner)),
            owner
        );
        assert_eq!(
            key_for(&HandlerError::user(keys::MODULE_DISABLED)),
            keys::MODULE_DISABLED
        );
        let decode = OptionError::Missing {
            option: "member".into(),
        };
        assert_eq!(
            key_for(&HandlerError::new(decode.clone())),
            keys::VALIDATION_FAILED
        );
        assert_eq!(
            key_for(&HandlerError::new(decode).with_message(owner)),
            owner
        );
    }
}
