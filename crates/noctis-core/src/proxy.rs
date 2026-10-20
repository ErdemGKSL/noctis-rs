//! Validation of the `http_proxy` value (AD-24).
//!
//! twilight formats every request URL as `"http://{proxy}/api/v10/.."`, so the
//! proxy must be a bare authority (`host[:port]`); an optional `http://`
//! prefix and one trailing `/` are accepted and dropped.

/// The longest accepted proxy authority, in bytes.
const MAX_LEN: usize = 255;

/// Why an `http_proxy` value was rejected. The message names the fix and
/// never contains the value.
///
/// ```
/// use noctis_core::HttpProxyError;
///
/// assert!(HttpProxyError::UnsupportedScheme.to_string().contains("http://host:port"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HttpProxyError {
    /// Nothing is left after an optional `http://` prefix and trailing `/`.
    #[error("the proxy is empty: use `host:port` or `http://host:port`")]
    Empty,
    /// A scheme other than `http` (e.g. `https://`).
    #[error(
        "unsupported scheme: use `host:port` or `http://host:port`; HTTPS proxies are not supported"
    )]
    UnsupportedScheme,
    /// A character outside printable ASCII (`!` to `~`), e.g. whitespace.
    #[error(
        "invalid character at index {index}: use only printable ASCII without spaces, \
         as in `host:port`"
    )]
    InvalidChar {
        /// The character index (after the scheme and trailing `/` are dropped).
        index: usize,
    },
    /// A path, query, fragment, credentials or backslash.
    #[error(
        "the proxy is not an authority: remove any path, query, fragment or `user@` part and \
         use `host:port`"
    )]
    NotAnAuthority,
    /// The authority is longer than 255 bytes.
    #[error("the proxy is {len} bytes long: use an authority of at most 255 bytes")]
    TooLong {
        /// The rejected length in bytes.
        len: usize,
    },
    /// The host is missing or malformed.
    #[error(
        "invalid proxy host: use a host name, an IPv4 address or a bracketed IPv6 address \
         such as `[::1]`, optionally followed by `:port`"
    )]
    InvalidHost,
    /// The port is not a number in 1..=65535.
    #[error("invalid proxy port: use 1 to 5 digits with a value in 1..=65535, or omit `:port`")]
    InvalidPort,
}

/// Normalizes an `http_proxy` value into the authority twilight expects.
pub(crate) fn normalize_http_proxy(raw: &str) -> Result<String, HttpProxyError> {
    if raw.is_empty() {
        return Err(HttpProxyError::Empty);
    }
    let mut rest = raw;
    if let Some((scheme, after)) = raw.split_once("://") {
        if !scheme.eq_ignore_ascii_case("http") {
            return Err(HttpProxyError::UnsupportedScheme);
        }
        rest = after;
    }
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Err(HttpProxyError::Empty);
    }
    if let Some(index) = rest.chars().position(|c| !('!'..='~').contains(&c)) {
        return Err(HttpProxyError::InvalidChar { index });
    }
    if rest.contains(['/', '?', '#', '@', '\\']) {
        return Err(HttpProxyError::NotAnAuthority);
    }
    if rest.len() > MAX_LEN {
        return Err(HttpProxyError::TooLong { len: rest.len() });
    }
    let port = split_host_port(rest)?;
    if let Some(port) = port {
        check_port(port)?;
    }
    Ok(rest.to_owned())
}

/// Checks the host part and returns the port part, if any.
fn split_host_port(authority: &str) -> Result<Option<&str>, HttpProxyError> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (body, after) = bracketed
            .split_once(']')
            .ok_or(HttpProxyError::InvalidHost)?;
        let body_ok = !body.is_empty()
            && body
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
        if !body_ok {
            return Err(HttpProxyError::InvalidHost);
        }
        if after.is_empty() {
            return Ok(None);
        }
        return after
            .strip_prefix(':')
            .map(Some)
            .ok_or(HttpProxyError::InvalidHost);
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if host.is_empty() || host.contains(['[', ']']) || port.is_some_and(|p| p.contains(':')) {
        return Err(HttpProxyError::InvalidHost);
    }
    Ok(port)
}

/// Checks a port: 1-5 ASCII digits with a value in 1..=65535.
fn check_port(port: &str) -> Result<(), HttpProxyError> {
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
        return Err(HttpProxyError::InvalidPort);
    }
    match port.parse::<u32>() {
        Ok(1..=65_535) => Ok(()),
        _ => Err(HttpProxyError::InvalidPort),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        normalize_http_proxy(raw).unwrap()
    }

    fn err(raw: &str) -> HttpProxyError {
        normalize_http_proxy(raw).unwrap_err()
    }

    #[test]
    fn accepted() {
        let a255 = "a".repeat(255);
        let cases = [
            ("127.0.0.1:3000", "127.0.0.1:3000"),
            ("proxy", "proxy"),
            ("http://proxy:3000", "proxy:3000"),
            ("HTTP://proxy:3000/", "proxy:3000"),
            ("Http://proxy", "proxy"),
            ("[::1]", "[::1]"),
            ("http://[::1]:8080", "[::1]:8080"),
            ("proxy:65535", "proxy:65535"),
            ("proxy:1", "proxy:1"),
            ("proxy:08080", "proxy:08080"),
            (a255.as_str(), a255.as_str()),
        ];
        for (raw, want) in cases {
            assert_eq!(ok(raw), want, "{raw}");
        }
    }

    #[test]
    fn empty() {
        for raw in ["", "http://", "/", "http:///"] {
            assert_eq!(err(raw), HttpProxyError::Empty, "{raw}");
        }
    }

    #[test]
    fn scheme() {
        for raw in [
            "https://proxy",
            "HTTPS://proxy",
            "ws://x",
            "://x",
            "ftp://x:21",
        ] {
            assert_eq!(err(raw), HttpProxyError::UnsupportedScheme, "{raw}");
        }
    }

    #[test]
    fn bad_char() {
        let cases = [
            (" proxy", 0),
            ("a b", 1),
            ("proxy\n", 5),
            ("a\tb", 1),
            ("pröxy:3000", 2),
            ("🙂", 0),
            ("a\u{7f}", 1),
        ];
        for (raw, index) in cases {
            assert_eq!(err(raw), HttpProxyError::InvalidChar { index }, "{raw:?}");
        }
    }

    #[test]
    fn not_authority() {
        for raw in [
            "proxy:3000/api",
            "proxy//",
            "proxy?x",
            "proxy#x",
            "user:pw@proxy",
            "a\\b",
            "http://http://x",
        ] {
            assert_eq!(err(raw), HttpProxyError::NotAnAuthority, "{raw}");
        }
    }

    #[test]
    fn length() {
        assert_eq!(err(&"a".repeat(256)), HttpProxyError::TooLong { len: 256 });
        let raw = format!("http://{}", "a".repeat(255));
        assert_eq!(ok(&raw), "a".repeat(255));
    }

    #[test]
    fn host() {
        for raw in [":3000", "::1", "[::1", "[]", "[::1]x", "[zz]", "a]b", "a[b"] {
            assert_eq!(err(raw), HttpProxyError::InvalidHost, "{raw}");
        }
    }

    #[test]
    fn port() {
        for raw in [
            "proxy:",
            "proxy:0",
            "proxy:65536",
            "proxy:123456",
            "proxy:+80",
            "proxy:8a",
            "[::1]:0",
        ] {
            assert_eq!(err(raw), HttpProxyError::InvalidPort, "{raw}");
        }
    }

    #[test]
    fn messages_never_echo() {
        let all = [
            HttpProxyError::Empty,
            HttpProxyError::UnsupportedScheme,
            HttpProxyError::InvalidChar { index: 3 },
            HttpProxyError::NotAnAuthority,
            HttpProxyError::TooLong { len: 300 },
            HttpProxyError::InvalidHost,
            HttpProxyError::InvalidPort,
        ];
        for e in all {
            let text = e.to_string();
            assert!(text.contains("use") || text.contains("remove"), "{text}");
        }
    }
}
