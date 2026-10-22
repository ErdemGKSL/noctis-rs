//! [`Text`]: display text that is either a literal or a locale key.
//!
//! Literal versus localized text is decided by syntax, never by a cargo
//! feature (maintainer decision §9): a string is always hard-coded text and
//! a [`MessageKey`] is always a locale key, resolved by core's Responder in
//! the response locale before the message is validated and sent.

use std::borrow::Cow;
use std::fmt;

use noctis_rules::MessageKey;
use serde::{Serialize, Serializer};

/// Display text of a Route component or modal: a literal or a locale key.
///
/// Every per-render override that sets a label, placeholder, title or
/// description takes `impl Into<Text>`: a `&'static str`, a `String`, a
/// `Cow<'static, str>`, a checked literal ([`Label`](crate::Label),
/// [`Placeholder`](crate::Placeholder), …) or a [`MessageKey`]. There is
/// deliberately no blanket conversion, so each source is explicit.
///
/// Core's Responder replaces every [`Text::Key`] with its translation in the
/// response locale before validating and sending; a key with no
/// translation is sent as the key itself, with one `warn` on target
/// `noctis::i18n`. A `Text` serialized directly (an unresolved key) writes
/// the key string, and Lenient truncation turns it into a literal.
///
/// ```
/// use std::borrow::Cow;
/// use noctis_rules::MessageKey;
/// use noctis_ui::Text;
///
/// assert_eq!(Text::from("a"), Text::Literal(Cow::Borrowed("a")));
/// assert_eq!(Text::from(String::from("a")).as_str(), "a");
/// let key = Text::from(MessageKey::new("shop.buy"));
/// assert_eq!(key, Text::Key(MessageKey::new("shop.buy")));
/// assert_eq!(key.as_str(), "shop.buy");
/// assert_eq!(serde_json::to_value(&key).unwrap(), "shop.buy");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    /// Hard-coded text, sent as written.
    Literal(Cow<'static, str>),
    /// A locale key, translated in the response locale before sending.
    Key(MessageKey),
}

impl Text {
    /// The literal, or the key as written for [`Text::Key`].
    pub fn as_str(&self) -> &str {
        match self {
            Self::Literal(text) => text,
            Self::Key(key) => key.as_str(),
        }
    }

    /// The locale key, if this is one.
    pub fn key(&self) -> Option<MessageKey> {
        match self {
            Self::Literal(_) => None,
            Self::Key(key) => Some(*key),
        }
    }

    /// Replaces a [`Text::Key`] with `Literal(f(key))`; a literal is kept.
    pub(crate) fn resolve(&mut self, f: &mut dyn FnMut(MessageKey) -> String) {
        if let Self::Key(key) = self {
            *self = Self::Literal(Cow::Owned(f(*key)));
        }
    }

    /// The literal of a declared `defaults(..)` text.
    pub(crate) const fn borrowed(text: &'static str) -> Self {
        Self::Literal(Cow::Borrowed(text))
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for Text {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// The literal, or the key string of an unresolved key.
impl Serialize for Text {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl From<&'static str> for Text {
    fn from(text: &'static str) -> Self {
        Self::Literal(Cow::Borrowed(text))
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::Literal(Cow::Owned(text))
    }
}

impl From<Cow<'static, str>> for Text {
    fn from(text: Cow<'static, str>) -> Self {
        Self::Literal(text)
    }
}

impl From<MessageKey> for Text {
    fn from(key: MessageKey) -> Self {
        Self::Key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AltText, Content, Label, Placeholder};

    #[test]
    fn t1_conversions() {
        let lit = Text::Literal(Cow::Borrowed("a"));
        assert_eq!(Text::from("a"), lit);
        assert_eq!(Text::from(String::from("a")), lit);
        assert_eq!(Text::from(Cow::Borrowed("a")), lit);
        assert_eq!(Text::from(Label::__from_checked("a")), lit);
        assert_eq!(Text::from(Placeholder::__from_checked("a")), lit);
        assert_eq!(Text::from(Content::__from_checked("a")), lit);
        assert_eq!(Text::from(AltText::__from_checked("a")), lit);
        assert_eq!(
            Text::from(MessageKey::new("x.y")),
            Text::Key(MessageKey::new("x.y"))
        );
    }

    #[test]
    fn t2_unresolved_key_serializes_as_key() {
        let key = Text::from(MessageKey::new("shop.buy"));
        assert_eq!(key.as_str(), "shop.buy");
        assert_eq!(key.key(), Some(MessageKey::new("shop.buy")));
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"shop.buy\"");
        assert_eq!(key.to_string(), "shop.buy");
    }

    #[test]
    fn resolve_replaces_only_keys() {
        let mut key = Text::from(MessageKey::new("a.b"));
        key.resolve(&mut |k| format!("<{k}>"));
        assert_eq!(key, Text::from("<a.b>".to_owned()));
        let mut lit = Text::from("x");
        lit.resolve(&mut |_| unreachable!("a literal is not resolved"));
        assert_eq!(lit, Text::from("x"));
    }
}
