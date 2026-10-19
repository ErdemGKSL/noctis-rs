//! [`PortError`]: why a port call failed.

use std::borrow::Cow;
use std::error::Error;

use crate::value::ValueError;

/// Why a port call failed.
///
/// Adapters build it with [`PortError::backend`], [`PortError::invalid_value`]
/// and [`PortError::unavailable`]; core turns it into a handler error or an
/// Error Sink event.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PortError {
    /// The backing store failed (a Redis, SQL or network error).
    #[error("port backend failed")]
    Backend(#[source] Box<dyn Error + Send + Sync>),
    /// The stored value under `key` is not what the op needs, such as an
    /// `incr_with_ttl` on a value that is not an 8-byte counter.
    #[error("invalid value under key `{key}`: {why}")]
    InvalidValue {
        /// The key, as the adapter received it.
        key: String,
        /// What is wrong with the value.
        why: Cow<'static, str>,
    },
    /// The port cannot serve calls right now (shutting down, not connected).
    #[error("port unavailable: {why}")]
    Unavailable {
        /// Why the port is unavailable.
        why: Cow<'static, str>,
    },
    /// A typed value did not encode or decode.
    #[error(transparent)]
    Value(#[from] ValueError),
}

impl PortError {
    /// A backend failure; use it as `map_err(PortError::backend)`.
    pub fn backend(err: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Backend(err.into())
    }

    /// The value stored under `key` is invalid for the op.
    pub fn invalid_value(key: &str, why: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidValue {
            key: key.to_owned(),
            why: why.into(),
        }
    }

    /// The port is unavailable.
    pub fn unavailable(why: impl Into<Cow<'static, str>>) -> Self {
        Self::Unavailable { why: why.into() }
    }
}
