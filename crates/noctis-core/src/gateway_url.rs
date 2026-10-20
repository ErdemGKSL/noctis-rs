//! Validation of the `gateway_url` value (AD-24).
//!
//! twilight appends `/?v=10&encoding=json&compress=..` to its gateway
//! `proxy_url`, so the URL must be `ws://` or `wss://` followed by a bare
//! authority (`host[:port]`); one trailing `/` is accepted and dropped.

use crate::authority::{AuthorityError, check_authority};

/// Why a `gateway_url` value was rejected. The message names the fix and
/// never contains the value.
///
/// ```
/// use noctis_core::GatewayUrlError;
///
/// assert!(GatewayUrlError::UnsupportedScheme.to_string().contains("ws://host:port"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GatewayUrlError {
    /// The URL, or the part after `ws://`/`wss://` and a trailing `/`, is empty.
    #[error("the gateway URL is empty: use `ws://host:port` or `wss://host:port`")]
    Empty,
    /// No `ws://` or `wss://` scheme.
    #[error("unsupported scheme: use `ws://host:port` or `wss://host:port`")]
    UnsupportedScheme,
    /// A character outside printable ASCII (`!` to `~`), e.g. whitespace.
    #[error(
        "invalid character at index {index}: use only printable ASCII without spaces, \
         as in `ws://host:port`"
    )]
    InvalidChar {
        /// The character index after the scheme and a trailing `/` are dropped.
        index: usize,
    },
    /// A path, query, fragment, credentials or backslash.
    #[error(
        "the gateway URL is not an authority: remove any path, query, fragment or `user@` part \
         and use `ws://host:port`"
    )]
    NotAnAuthority,
    /// The authority is longer than 255 bytes.
    #[error(
        "the gateway authority (host and port) is {len} bytes long: use an authority of at most \
         255 bytes"
    )]
    TooLong {
        /// The rejected length in bytes.
        len: usize,
    },
    /// The host is missing or malformed.
    #[error(
        "invalid gateway host: use a host name, an IPv4 address or a bracketed IPv6 address \
         such as `[::1]`, optionally followed by `:port`"
    )]
    InvalidHost,
    /// The port is not a number in 1..=65535.
    #[error("invalid gateway port: use 1 to 5 digits with a value in 1..=65535, or omit `:port`")]
    InvalidPort,
}

impl From<AuthorityError> for GatewayUrlError {
    fn from(e: AuthorityError) -> Self {
        match e {
            AuthorityError::InvalidChar { index } => Self::InvalidChar { index },
            AuthorityError::NotAnAuthority => Self::NotAnAuthority,
            AuthorityError::TooLong { len } => Self::TooLong { len },
            AuthorityError::InvalidHost => Self::InvalidHost,
            AuthorityError::InvalidPort => Self::InvalidPort,
        }
    }
}

/// Normalizes a `gateway_url` value into `"{ws|wss}://{authority}"`.
pub(crate) fn normalize_gateway_url(raw: &str) -> Result<String, GatewayUrlError> {
    if raw.is_empty() {
        return Err(GatewayUrlError::Empty);
    }
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or(GatewayUrlError::UnsupportedScheme)?;
    if !scheme.eq_ignore_ascii_case("ws") && !scheme.eq_ignore_ascii_case("wss") {
        return Err(GatewayUrlError::UnsupportedScheme);
    }
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Err(GatewayUrlError::Empty);
    }
    check_authority(rest)?;
    Ok(format!("{}://{rest}", scheme.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        normalize_gateway_url(raw).unwrap()
    }

    fn err(raw: &str) -> GatewayUrlError {
        normalize_gateway_url(raw).unwrap_err()
    }

    #[test]
    fn accepted() {
        let a255 = format!("ws://{}", "a".repeat(255));
        let cases = [
            ("ws://127.0.0.1:3000", "ws://127.0.0.1:3000"),
            ("WS://Proxy:3000/", "ws://Proxy:3000"),
            ("wss://gateway.discord.gg", "wss://gateway.discord.gg"),
            ("Wss://[::1]:8080", "wss://[::1]:8080"),
            ("ws://p:1", "ws://p:1"),
            ("ws://p:65535", "ws://p:65535"),
            ("ws://p:08080", "ws://p:08080"),
            (a255.as_str(), a255.as_str()),
        ];
        for (raw, want) in cases {
            assert_eq!(ok(raw), want, "{raw}");
        }
    }

    #[test]
    fn empty() {
        for raw in ["", "ws://", "wss:///", "ws:///"] {
            assert_eq!(err(raw), GatewayUrlError::Empty, "{raw}");
        }
    }

    #[test]
    fn scheme() {
        for raw in [
            "127.0.0.1:3000",
            "p",
            "http://p",
            "https://p",
            "://p",
            "wsx://p",
            "ftp://p:21",
            " ws://p",
        ] {
            assert_eq!(err(raw), GatewayUrlError::UnsupportedScheme, "{raw:?}");
        }
    }

    #[test]
    fn bad_char() {
        let cases = [
            ("ws:// p", 0),
            ("ws://a b", 1),
            ("ws://p\n", 1),
            ("ws://a\tb", 1),
            ("ws://pröxy", 2),
            ("ws://🙂", 0),
            ("ws://a\u{7f}", 1),
        ];
        for (raw, index) in cases {
            assert_eq!(err(raw), GatewayUrlError::InvalidChar { index }, "{raw:?}");
        }
    }

    #[test]
    fn not_authority() {
        for raw in [
            "ws://p:3000/gateway",
            "ws://p//",
            "ws://p?v=10",
            "ws://p#x",
            "wss://u:pw@p",
            "ws://a\\b",
            "ws://ws://p",
        ] {
            assert_eq!(err(raw), GatewayUrlError::NotAnAuthority, "{raw}");
        }
    }

    #[test]
    fn length() {
        let raw = format!("ws://{}", "a".repeat(256));
        assert_eq!(err(&raw), GatewayUrlError::TooLong { len: 256 });
    }

    #[test]
    fn host() {
        for raw in [
            "ws://:3000",
            "ws://::1",
            "ws://[::1",
            "ws://[]",
            "ws://[::1]x",
            "ws://[zz]",
            "ws://a]b",
            "ws://pro<xy",
        ] {
            assert_eq!(err(raw), GatewayUrlError::InvalidHost, "{raw}");
        }
    }

    #[test]
    fn port() {
        for raw in [
            "ws://p:",
            "ws://p:0",
            "ws://p:65536",
            "ws://p:123456",
            "ws://p:+80",
            "ws://p:8a",
            "ws://[::1]:0",
        ] {
            assert_eq!(err(raw), GatewayUrlError::InvalidPort, "{raw}");
        }
    }

    #[test]
    fn messages_never_echo() {
        let all = [
            GatewayUrlError::Empty,
            GatewayUrlError::UnsupportedScheme,
            GatewayUrlError::InvalidChar { index: 3 },
            GatewayUrlError::NotAnAuthority,
            GatewayUrlError::TooLong { len: 300 },
            GatewayUrlError::InvalidHost,
            GatewayUrlError::InvalidPort,
        ];
        for e in all {
            let text = e.to_string();
            assert!(text.contains("use") || text.contains("remove"), "{text}");
        }
    }
}
