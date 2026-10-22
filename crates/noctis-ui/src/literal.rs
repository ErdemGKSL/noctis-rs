//! Checked text newtypes: [`Label`] (CV2-08), [`Placeholder`] (CV2-09),
//! [`Content`] (MSG-01) and [`AltText`] (ATT-06).
//!
//! `label!`, `placeholder!`, `content!` and `alt!` build them from a literal and
//! check its length at `cargo check`; `try_new` and `TryFrom` are the runtime
//! path and call the same `noctis-rules` function. Neither ever truncates:
//! Lenient truncation belongs to [`validate`](crate::validate). Each converts
//! into `Cow<'static, str>`, so it goes wherever a builder takes text.

use std::borrow::Cow;

use noctis_rules::attachment::validate_attachment_description;
use noctis_rules::component::{validate_button_label, validate_select_placeholder};
use noctis_rules::message::validate_content;
use noctis_rules::{Limits, Violation};

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
