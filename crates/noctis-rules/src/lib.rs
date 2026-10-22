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
//! - [`command`]: Command Pattern expansion, command name and tree rules,
//!   option, context-menu and metadata rules, and autocomplete responses
//!   (CMD-01..07, CMD-10..16, OPT-01, OPT-03..11, OPT-13, CTX-01, META-01..03,
//!   REG-01..04, INT-07).
//! - [`component`]: Components V2 trees and legacy Action Rows (CV2-03..13,
//!   CV2-15..17).
//! - [`modal`]: modals, Labels and their inputs (MOD-01..07).
//! - [`message`]: legacy `content`, allowed mentions, nonce, stickers,
//!   non-empty bodies, webhook names and threads, and the non-interactive
//!   components of foreign webhooks (MSG-01, MSG-03..06, MSG-09, WH-02, WH-04,
//!   WH-05).
//! - [`attachment`]: file sizes, counts, alt text and `attachment://`
//!   references (ATT-01..03, ATT-05, ATT-06).
//! - [`cid`]: the route id grammar ([`cid::validate_route_id`]) and the
//!   `custom_id` codec caps shared by the codec and the kind derives
//!   (CID-01, CID-03).
//!
//! The crate is sync and does no I/O.

pub mod attachment;
mod check;
pub mod cid;
pub mod command;
pub mod component;
mod key;
pub mod len;
mod limits;
mod locale;
pub mod message;
pub mod modal;
mod rule;

pub use key::{FRAMEWORK_KEYS, MessageKey, Translate, placeholder_names};
pub use limits::{LimitMode, Limits};
pub use locale::{Locale, ParseLocaleError};
pub use rule::{Actual, Bound, RuleId, Violation};
