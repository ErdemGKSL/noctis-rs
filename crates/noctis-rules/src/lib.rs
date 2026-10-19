//! Discord limits and validators (rule ids), `Limits`, `Locale`, `MessageKey` and `Translate`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! # Contents
//!
//! - [`RuleId`]: the stable ids of the Discord Rules Catalog, and [`Violation`]
//!   (with [`Bound`] and [`Actual`]) for a broken rule.
//! - [`Limits`] and [`LimitMode`]: every numeric limit, configurable, each naming
//!   its rule id.
//! - [`len`]: [`len::discord_len`], the only length function for user text, and
//!   [`len::truncate`].
//! - [`Locale`] and [`ParseLocaleError`]: Discord's 32 locale codes (LOC-01).
//! - [`MessageKey`] and [`Translate`]: localization keys and the translation port.
//!
//! The crate is sync and does no I/O.

pub mod len;
mod limits;
mod locale;
mod message;
mod rule;

pub use limits::{LimitMode, Limits};
pub use locale::{Locale, ParseLocaleError};
pub use message::{MessageKey, Translate};
pub use rule::{Actual, Bound, RuleId, Violation};
