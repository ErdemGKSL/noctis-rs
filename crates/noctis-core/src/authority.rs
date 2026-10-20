//! The shared `host[:port]` authority check of `http_proxy` and
//! `gateway_url` (AD-24).

/// The longest accepted authority, in bytes.
const MAX_LEN: usize = 255;

/// Why an authority was rejected; each caller maps it onto its public error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorityError {
    /// A character outside printable ASCII (`!` to `~`).
    InvalidChar {
        /// The character index in the checked string.
        index: usize,
    },
    /// A path, query, fragment, credentials or backslash.
    NotAnAuthority,
    /// Longer than 255 bytes.
    TooLong {
        /// The rejected length in bytes.
        len: usize,
    },
    /// The host is missing or malformed.
    InvalidHost,
    /// The port is not 1-5 digits with a value in 1..=65535.
    InvalidPort,
}

/// Checks that `authority` is a bare `host[:port]` (reg-name, IPv4 or
/// bracketed IPv6 host).
pub(crate) fn check_authority(authority: &str) -> Result<(), AuthorityError> {
    if let Some(index) = authority.chars().position(|c| !('!'..='~').contains(&c)) {
        return Err(AuthorityError::InvalidChar { index });
    }
    if authority.contains(['/', '?', '#', '@', '\\']) {
        return Err(AuthorityError::NotAnAuthority);
    }
    if authority.len() > MAX_LEN {
        return Err(AuthorityError::TooLong {
            len: authority.len(),
        });
    }
    if let Some(port) = split_host_port(authority)? {
        check_port(port)?;
    }
    Ok(())
}

/// Checks the host part and returns the port part, if any.
fn split_host_port(authority: &str) -> Result<Option<&str>, AuthorityError> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (body, after) = bracketed
            .split_once(']')
            .ok_or(AuthorityError::InvalidHost)?;
        if body.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(AuthorityError::InvalidHost);
        }
        if after.is_empty() {
            return Ok(None);
        }
        return after
            .strip_prefix(':')
            .map(Some)
            .ok_or(AuthorityError::InvalidHost);
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if host.is_empty()
        || !host.bytes().all(is_reg_name_byte)
        || port.is_some_and(|p| p.contains(':'))
    {
        return Err(AuthorityError::InvalidHost);
    }
    Ok(port)
}

/// RFC 3986 `reg-name` bytes: unreserved, `%` and sub-delims.
fn is_reg_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~%!$&'()*+,;=".contains(&b)
}

/// Checks a port: 1-5 ASCII digits with a value in 1..=65535.
fn check_port(port: &str) -> Result<(), AuthorityError> {
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AuthorityError::InvalidPort);
    }
    match port.parse::<u32>() {
        Ok(1..=65_535) => Ok(()),
        _ => Err(AuthorityError::InvalidPort),
    }
}
