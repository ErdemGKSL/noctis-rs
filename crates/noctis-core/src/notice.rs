//! The localized notice a failed handler run shows its user (FR-22,
//! AD-16, LOC-04): which key, which locale, which text.

use noctis_i18n::{Builtin, keys};
use noctis_rules::{Locale, MessageKey, Translate};

use crate::HandlerError;
use crate::options::OptionError;

/// The locale of an ephemeral notice: the User Locale, then the Guild
/// Locale; `en-US` when neither is known or the code does not parse.
pub(crate) fn locale(user: Option<&str>, guild: Option<&str>) -> Locale {
    user.or(guild)
        .and_then(|code| code.parse().ok())
        .unwrap_or(Locale::EnUs)
}

/// The text of `key` in `locale`. A key no catalog has renders the generic
/// error text, with one `warn` on target `noctis::i18n`.
pub(crate) fn text(strings: &dyn Translate, locale: Locale, key: MessageKey) -> String {
    if let Some(text) = strings.translate(locale, key, &[]) {
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
    fn locale_is_user_then_guild_then_en_us() {
        assert_eq!(locale(Some("tr"), Some("de")), Locale::Tr);
        assert_eq!(locale(None, Some("de")), Locale::De);
        assert_eq!(locale(None, None), Locale::EnUs);
        assert_eq!(locale(Some("xx-YY"), Some("de")), Locale::EnUs);
        assert_eq!(locale(Some("en-us"), None), Locale::EnUs);
    }

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
            key_for(&HandlerError::user(keys::DISABLED_HERE)),
            keys::DISABLED_HERE
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
