//! Noctis: a Discord bot framework on twilight, with built-in sharding, a
//! Components V2 UI kit and image rendering.
//!
//! Bot code depends on this crate only; the `noctis-*` crates are internal.
//!
//! # Cargo features
//!
//! All features are additive, and the default features build without the image
//! stack.
//!
//! - `image`: image rendering through takumi (off by default).
//! - `image-gif`: GIF/APNG output for rendered images.
//! - `image-emoji`: Twemoji colour emoji in image Templates.
//! - `default-font`: a bundled default font for image Templates.
//! - `i18n`: the localization runtime.
//! - `prefix`: opt-in prefix commands.
//! - `testkit`: Discord fakes and test helpers; enable it in
//!   `[dev-dependencies]` only.
#![deny(missing_docs)]

pub mod rules {
    //! Discord limits, rule ids, text length and locales.
    //!
    //! - [`Limits`] and [`LimitMode`]: every numeric Discord limit, each field
    //!   naming its rule id; the App builder hook that takes them is planned for a
    //!   later story.
    //! - [`RuleId`], [`Violation`], [`Bound`] and [`Actual`]: the rule ids of the
    //!   Discord Rules Catalog and the error that cites them.
    //! - [`discord_len`] and [`truncate`]: the only length function for user text,
    //!   and `…` truncation that respects it.
    //! - [`Locale`] and [`ParseLocaleError`]: Discord's 32 locale codes.
    //! - [`MessageKey`] and [`Translate`]: localization keys and the translation port.
    //!
    //! ```
    //! use noctis::rules::{discord_len, LimitMode, Limits, Locale, RuleId};
    //!
    //! assert_eq!(Limits::default().mode, LimitMode::Strict);
    //! let mut limits = Limits::default();
    //! limits.mode = LimitMode::Lenient;
    //! assert!(!limits.truncates(RuleId::CV2_03));
    //! assert!(limits.truncates(RuleId::CV2_04));
    //!
    //! assert_eq!("en-US".parse::<Locale>(), Ok(Locale::EnUs));
    //! assert_eq!(Locale::EnUs.sibling(), Some(Locale::EnGb));
    //! assert_eq!(discord_len("😀"), 2);
    //! ```

    pub use noctis_rules::len::{discord_len, truncate};
    pub use noctis_rules::{
        Actual, Bound, LimitMode, Limits, Locale, MessageKey, ParseLocaleError, RuleId, Translate,
        Violation,
    };
}
