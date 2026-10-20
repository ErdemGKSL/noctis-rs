//! The bot token newtype (NFR-6, AD-29).

use std::fmt;
use std::str::FromStr;

/// A Discord bot token.
///
/// [`Token::new`] strips one leading `"Bot "` and then requires at least one
/// character, every one printable ASCII (`!` to `~`). There is no maximum
/// length.
///
/// The secret never leaves Noctis's core: [`Debug`] prints `Token(<redacted>)`,
/// [`Display`](fmt::Display) prints `<redacted>`, and there is no getter,
/// comparison or serialization.
///
/// ```
/// use noctis_core::Token;
///
/// let token: Token = "Bot abc.DEF_ghi-1".parse()?;
/// assert_eq!(format!("{token:?}"), "Token(<redacted>)");
/// assert_eq!(token.to_string(), "<redacted>");
/// # Ok::<(), noctis_core::TokenError>(())
/// ```
#[derive(Clone)]
pub struct Token(Box<str>);

impl Token {
    /// Validates `token` (see [`Token`]).
    ///
    /// # Errors
    ///
    /// [`TokenError::Empty`] when nothing is left after the `"Bot "` prefix,
    /// [`TokenError::InvalidChar`] for a character outside printable ASCII.
    ///
    /// ```
    /// use noctis_core::{Token, TokenError};
    ///
    /// assert!(Token::new("abc.DEF_ghi-1").is_ok());
    /// assert_eq!(Token::new("Bot ").err(), Some(TokenError::Empty));
    /// assert_eq!(Token::new("ab c").err(), Some(TokenError::InvalidChar { index: 2 }));
    /// ```
    pub fn new(token: impl Into<String>) -> Result<Self, TokenError> {
        let mut token = token.into();
        if token.starts_with("Bot ") {
            token.drain(.."Bot ".len());
        }
        if token.is_empty() {
            return Err(TokenError::Empty);
        }
        if let Some(index) = token.chars().position(|c| !('!'..='~').contains(&c)) {
            return Err(TokenError::InvalidChar { index });
        }
        Ok(Self(token.into_boxed_str()))
    }

    /// The secret, without the `"Bot "` prefix. Only the HTTP client and the
    /// gateway see it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl FromStr for Token {
    type Err = TokenError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Why a [`Token`] was rejected. The message never contains the token.
///
/// ```
/// use noctis_core::{Token, TokenError};
///
/// let err = Token::new("tökén").unwrap_err();
/// assert_eq!(err, TokenError::InvalidChar { index: 1 });
/// assert!(!err.to_string().contains("tökén"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TokenError {
    /// Nothing is left after the optional `"Bot "` prefix.
    #[error("the bot token is empty: copy it from the Discord Developer Portal (Bot > Token)")]
    Empty,
    /// A character outside printable ASCII (`!` to `~`), such as whitespace.
    #[error(
        "the bot token has a character outside printable ASCII at index {index}: \
         remove whitespace and copy it again from the Discord Developer Portal"
    )]
    InvalidChar {
        /// Character index, counted after the `"Bot "` prefix is stripped.
        index: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let t = Token::new("abc.DEF_ghi-1").unwrap();
        assert_eq!(t.expose(), "abc.DEF_ghi-1");
        let parsed: Token = "abc.DEF_ghi-1".parse().unwrap();
        assert_eq!(parsed.expose(), "abc.DEF_ghi-1");
    }

    #[test]
    fn bot_prefix() {
        assert_eq!(Token::new("Bot abc").unwrap().expose(), "abc");
        assert_eq!(
            Token::new("Bot Bot abc").err(),
            Some(TokenError::InvalidChar { index: 3 })
        );
        assert_eq!(
            Token::new("bot abc").err(),
            Some(TokenError::InvalidChar { index: 3 })
        );
    }

    #[test]
    fn empty() {
        assert_eq!(Token::new("").err(), Some(TokenError::Empty));
        assert_eq!(Token::new("Bot ").err(), Some(TokenError::Empty));
    }

    #[test]
    fn whitespace_control_and_unicode() {
        for (input, index) in [
            (" abc", 0),
            ("ab c", 2),
            ("abc\n", 3),
            ("a\tb", 1),
            ("a\u{7f}", 1),
            ("tökén", 1),
            ("🙂", 0),
        ] {
            assert_eq!(
                Token::new(input).err(),
                Some(TokenError::InvalidChar { index }),
                "{input:?}"
            );
        }
    }

    #[test]
    fn bounds() {
        for input in ["a", "!", "~"] {
            assert_eq!(Token::new(input).unwrap().expose(), input);
        }
        let long = "a".repeat(10_000);
        assert_eq!(Token::new(long.clone()).unwrap().expose(), long);
    }

    #[test]
    fn redaction() {
        let t = Token::new("SECRET123").unwrap();
        assert_eq!(format!("{t:?}"), "Token(<redacted>)");
        assert_eq!(format!("{t}"), "<redacted>");
        assert!(!format!("{:#?}", Some(t.clone())).contains("SECRET123"));
    }
}
