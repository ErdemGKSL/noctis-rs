//! Checked text newtypes: [`Label`] (CV2-08), [`Placeholder`] (CV2-09),
//! [`Content`] (MSG-01), [`AltText`] (ATT-06) and [`WebhookName`] (WH-04).
//!
//! `label!`, `placeholder!`, `content!`, `alt!` and `webhook_name!` build them from a literal and
//! check its length at `cargo check`; `try_new` and `TryFrom` are the runtime
//! path and call the same `noctis-rules` function. Neither ever truncates:
//! Lenient truncation belongs to [`validate`](crate::validate). Each converts
//! into `Cow<'static, str>` and [`Text`], so it goes wherever a builder takes
//! text.

use std::borrow::Cow;

use noctis_rules::attachment::validate_attachment_description;
use noctis_rules::component::{validate_button_label, validate_select_placeholder};
use noctis_rules::message::{validate_content, validate_webhook_name};
use noctis_rules::{Limits, Violation};

use crate::Text;
use crate::error::{UiError, checked};

macro_rules! literal_newtype {
    (
        $(#[$meta:meta])*
        $name:ident, $check:path
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(Cow<'static, str>);

        impl $name {
            /// Checks `text` against `limits`; never truncates.
            ///
            /// # Errors
            ///
            /// [`UiError::Limits`] when `text` is too long.
            pub fn try_new(
                text: impl Into<Cow<'static, str>>,
                limits: &Limits,
            ) -> Result<Self, UiError> {
                let text = text.into();
                let violations: Vec<Violation> = $check(&text, limits);
                checked(Self(text), violations)
            }

            /// The text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// A literal already checked by the matching macro; not public API.
            #[doc(hidden)]
            pub const fn __from_checked(text: &'static str) -> Self {
                Self(Cow::Borrowed(text))
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl From<$name> for Cow<'static, str> {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl From<$name> for Text {
            fn from(value: $name) -> Self {
                Text::Literal(value.0)
            }
        }

        impl TryFrom<&'static str> for $name {
            type Error = UiError;

            /// [`try_new`](Self::try_new) against `Limits::default()`.
            fn try_from(text: &'static str) -> Result<Self, UiError> {
                Self::try_new(text, &Limits::default())
            }
        }

        impl TryFrom<String> for $name {
            type Error = UiError;

            /// [`try_new`](Self::try_new) against `Limits::default()`.
            fn try_from(text: String) -> Result<Self, UiError> {
                Self::try_new(text, &Limits::default())
            }
        }
    };
}

literal_newtype! {
    /// A button label of at most 80 characters (CV2-08), measured with
    /// `discord_len`.
    ///
    /// `label!("Save")` checks a literal at `cargo check`; `try_new` and
    /// `TryFrom` check runtime text with `validate_button_label`, the
    /// function the macro calls. It goes wherever a label goes:
    ///
    /// ```
    /// use noctis_rules::{Limits, RuleId};
    /// use noctis_ui::{Label, LinkButton, UiError};
    ///
    /// let button = LinkButton::new("https://x.y").label(Label::try_from("Docs")?);
    /// # let _ = button;
    /// let Err(UiError::Limits { violations }) = Label::try_new("a".repeat(81), &Limits::default()) else {
    ///     panic!("81 characters");
    /// };
    /// assert_eq!(violations[0].rule, RuleId::CV2_08);
    /// # Ok::<(), UiError>(())
    /// ```
    Label, validate_button_label
}

literal_newtype! {
    /// A select placeholder of at most 150 characters (CV2-09), measured with
    /// `discord_len`.
    ///
    /// `placeholder!("Pick one")` checks a literal at `cargo check`;
    /// `try_new` and `TryFrom` check runtime text with
    /// `validate_select_placeholder`, the function the macro calls.
    ///
    /// ```
    /// use noctis_rules::Limits;
    /// use noctis_ui::{Placeholder, UiError};
    ///
    /// assert_eq!(Placeholder::try_from("Pick one")?.as_str(), "Pick one");
    /// assert!(Placeholder::try_new("a".repeat(151), &Limits::default()).is_err());
    /// # Ok::<(), UiError>(())
    /// ```
    Placeholder, validate_select_placeholder
}

literal_newtype! {
    /// Legacy message `content` of at most 2000 characters (MSG-01), measured
    /// with `discord_len`, for [`LegacyMessage::new`](crate::LegacyMessage::new).
    ///
    /// `content!("Done")` checks a literal at `cargo check`; `try_new` and
    /// `TryFrom` check runtime text with
    /// `noctis_rules::message::validate_content` (path `content`).
    ///
    /// ```
    /// use noctis_rules::{Limits, RuleId};
    /// use noctis_ui::{Content, LegacyMessage, UiError};
    ///
    /// let msg = LegacyMessage::new(Content::try_from("Done")?);
    /// assert_eq!(msg.content_text(), "Done");
    /// let Err(UiError::Limits { violations }) = Content::try_new("a".repeat(2001), &Limits::default()) else {
    ///     panic!("2001 characters");
    /// };
    /// assert_eq!((violations[0].rule, violations[0].path.as_str()), (RuleId::MSG_01, "content"));
    /// # Ok::<(), UiError>(())
    /// ```
    Content, validate_content
}

literal_newtype! {
    /// Alt text (a `description`) of at most 1024 characters (ATT-06),
    /// measured with `discord_len`; empty is allowed.
    ///
    /// `alt!("Profile card")` checks a literal at `cargo check`; `try_new`
    /// and `TryFrom` check runtime text with
    /// `noctis_rules::attachment::validate_attachment_description`, the
    /// function the macro calls. It goes wherever alt text goes
    /// ([`MediaItem::alt`](crate::MediaItem::alt),
    /// [`Thumbnail::alt`](crate::Thumbnail::alt),
    /// [`Attachment::alt`](crate::Attachment::alt)):
    ///
    /// ```
    /// use noctis_rules::{Limits, RuleId};
    /// use noctis_ui::{AltText, Media, MediaItem, UiError};
    ///
    /// let item = MediaItem::new(Media::url("https://i/a.png")).alt(AltText::try_from("A cat")?);
    /// # let _ = item;
    /// assert!(AltText::try_new("a".repeat(1024), &Limits::default()).is_ok());
    /// let Err(UiError::Limits { violations }) = AltText::try_new("a".repeat(1025), &Limits::default()) else {
    ///     panic!("1025 characters");
    /// };
    /// assert_eq!(violations[0].rule, RuleId::ATT_06);
    /// # Ok::<(), UiError>(())
    /// ```
    AltText, validate_attachment_description
}

literal_newtype! {
    /// A webhook name (or a per-message `username`) of 1 to 80 characters,
    /// measured with `discord_len`, without `clyde` or `discord` in any
    /// ASCII case (WH-04).
    ///
    /// `webhook_name!("Audit log")` checks a literal at `cargo check`;
    /// `try_new` and `TryFrom` check runtime text with
    /// `noctis_rules::message::validate_webhook_name` (path `name`), the
    /// function the macro calls.
    ///
    /// ```
    /// use noctis_rules::{Limits, RuleId};
    /// use noctis_ui::{UiError, WebhookName};
    ///
    /// assert_eq!(WebhookName::try_from("Audit log")?.as_str(), "Audit log");
    /// let Err(UiError::Limits { violations }) = WebhookName::try_new("Discord bot", &Limits::default()) else {
    ///     panic!("a reserved word");
    /// };
    /// assert_eq!((violations[0].rule, violations[0].path.as_str()), (RuleId::WH_04, "name"));
    /// # Ok::<(), UiError>(())
    /// ```
    WebhookName, validate_webhook_name
}

#[cfg(test)]
mod tests {
    use noctis_rules::{Bound, RuleId};

    use super::*;

    fn bounds(name: String) -> Vec<(RuleId, Bound)> {
        match WebhookName::try_new(name, &Limits::default()) {
            Ok(_) => Vec::new(),
            Err(UiError::Limits { violations }) => {
                violations.into_iter().map(|v| (v.rule, v.limit)).collect()
            }
            Err(other) => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn wh_04_webhook_name_bounds() {
        for ok in ["a".to_owned(), "a".repeat(80), "😀".repeat(40)] {
            assert!(bounds(ok).is_empty());
        }
        let between = (RuleId::WH_04, Bound::Between { min: 1, max: 80 });
        for bad in [String::new(), "a".repeat(81), "😀".repeat(41)] {
            assert_eq!(bounds(bad), std::slice::from_ref(&between));
        }
    }

    #[test]
    fn wh_04_webhook_name_reserved_words() {
        let reserved = (
            RuleId::WH_04,
            Bound::Describe("a name without `clyde` or `discord` (any case)"),
        );
        for bad in ["Clyde", "my discord log", "DISCORD"] {
            assert_eq!(
                bounds(bad.to_owned()),
                std::slice::from_ref(&reserved),
                "{bad}"
            );
        }
        for ok in ["clyd e", "Díscord"] {
            assert!(bounds(ok.to_owned()).is_empty(), "{ok}");
        }
    }
}
