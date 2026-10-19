//! Noctis: a Discord bot framework on twilight, with built-in sharding, a
//! Components V2 UI kit and image rendering.
//!
//! Bot code depends on this crate only; the `noctis-*` crates are internal.
//!
//! # Modules
//!
//! - [`rules`]: Discord limits, rule ids, text length and locales.
//! - [`codec`]: Payload field encodings ([`codec::Field`],
//!   [`codec::BoundedStr`]) and the checks custom fields are tested with.
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

pub mod codec {
    //! Payload field encodings for `custom_id`s.
    //!
    //! - [`Field`] and [`FieldDecode`]: a Payload field's encoding, worst-case
    //!   width (`MAX_LEN`) and canonical decoding. Built in for `bool`,
    //!   `u8`..`u64`, `i8`..`i64`, Discord ids, `Option<T>`, [`BoundedStr`],
    //!   `String`, `Cow<str>` and `Vec<T>`.
    //! - [`BoundedStr`] and [`BoundedStrError`]: printable-ASCII text of at most
    //!   `N` characters, `2 * N` wide once escaped.
    //! - [`FieldError`]: why a field did not decode.
    //! - [`check`]: [`check::assert_round_trip`] and
    //!   [`check::assert_worst_case`] for the tests of custom fields.
    //!
    //! ```
    //! use noctis::codec::{BoundedStr, Field, FieldDecode, FieldError, check};
    //!
    //! assert_eq!(<u8 as Field>::MAX_LEN, Some(2));
    //! assert_eq!(<BoundedStr<6> as Field>::MAX_LEN, Some(12));
    //! const SKU: BoundedStr<6> = BoundedStr::from_static("ab:12");
    //! check::assert_round_trip(&SKU);
    //!
    //! #[derive(Debug, Clone, Copy, PartialEq)]
    //! struct Percent(u8);
    //!
    //! impl Field for Percent {
    //!     const MAX_LEN: Option<usize> = <u8 as Field>::MAX_LEN;
    //!     fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result {
    //!         self.0.encode(out)
    //!     }
    //! }
    //!
    //! impl<'de> FieldDecode<'de> for Percent {
    //!     fn decode(raw: &'de str) -> Result<Self, FieldError> {
    //!         let v = u8::decode(raw)?;
    //!         if v <= 100 { Ok(Self(v)) } else { Err(FieldError::Overflow) }
    //!     }
    //! }
    //!
    //! check::assert_round_trip(&Percent(42));
    //! check::assert_worst_case(&Percent(100));
    //! ```

    pub use noctis_codec::{BoundedStr, BoundedStrError, Field, FieldDecode, FieldError, check};
}

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
    //! - [`command`]: Command Pattern expansion ([`command::expand`]), the
    //!   command name and tree rules ([`command::validate_name`],
    //!   [`command::validate_tree`], [`command::validate_merge`]), the option
    //!   rules ([`command::validate_options`], [`command::measure_options`]), the
    //!   context-menu name rule ([`command::validate_context_menu_name`]) and the
    //!   command metadata rules ([`command::validate_meta`]) and the autocomplete
    //!   response rules ([`command::validate_autocomplete`]) that `#[command]`,
    //!   `derive(Options)` and `App::build` call.
    //! - [`component`]: Components V2 trees and legacy Action Rows
    //!   ([`component::validate_components`],
    //!   [`component::validate_legacy_components`], [`component::cost`]) and
    //!   single values such as [`component::validate_button_label`] and
    //!   [`component::validate_accent_color`] (CV2-03..13, CV2-15..17).
    //! - [`modal`]: modals ([`modal::validate_modal`]) and single modal values
    //!   (MOD-01..07).
    //! - [`message`]: legacy `content`, allowed mentions, nonce, stickers and
    //!   webhook names (MSG-01, MSG-03..06, WH-04).
    //! - [`attachment`]: file sizes, counts, alt text and `attachment://`
    //!   references (ATT-01..03, ATT-05, ATT-06).
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
    //!
    //! ```
    //! use noctis::rules::command::{CommandDecl, LocalizedText, expand, validate_tree};
    //! use noctis::rules::{Limits, RuleId};
    //!
    //! let limits = Limits::default();
    //! let paths = expand("config (set|get) (prefix|welcome)", &limits).unwrap();
    //! assert_eq!(
    //!     paths,
    //!     ["config set prefix", "config set welcome", "config get prefix", "config get welcome"]
    //! );
    //!
    //! let paths = expand("config (set|get) (prefix|welcome)?", &limits).unwrap();
    //! let decls: Vec<_> = paths
    //!     .iter()
    //!     .map(|p| CommandDecl::handler("config", p).description(LocalizedText::new("d")))
    //!     .collect();
    //! let errs = validate_tree(&decls, &limits).unwrap_err();
    //! assert_eq!(errs[0].rule, RuleId::CMD_11);
    //! ```
    //!
    //! ```
    //! use noctis::rules::command::{OptionDecl, OptionType, validate_options};
    //! use noctis::rules::{Limits, RuleId};
    //!
    //! let options = [
    //!     OptionDecl::new("reason", OptionType::String, "Why"),
    //!     OptionDecl::new("days", OptionType::Integer, "How long").required(true),
    //! ];
    //! let errs = validate_options(&options, &Limits::default()).unwrap_err();
    //! assert_eq!(errs[0].rule, RuleId::OPT_01);
    //! assert_eq!(errs[0].path, "days");
    //! ```
    //!
    //! ```
    //! use noctis::rules::component::{Component, validate_components};
    //! use noctis::rules::modal::validate_modal_title;
    //! use noctis::rules::{Limits, RuleId};
    //!
    //! let limits = Limits::default();
    //! let tree = vec![Component::Text("a"); 41];
    //! let errs = validate_components(&tree, &limits).unwrap_err();
    //! assert_eq!(errs[0].rule, RuleId::CV2_03);
    //! assert_eq!(errs[0].path, "components");
    //!
    //! assert_eq!(validate_modal_title(&"a".repeat(46), &limits)[0].rule, RuleId::MOD_01);
    //! ```

    pub use noctis_rules::len::{discord_len, truncate};
    pub use noctis_rules::{
        Actual, Bound, LimitMode, Limits, Locale, MessageKey, ParseLocaleError, RuleId, Translate,
        Violation,
    };
    pub use noctis_rules::{attachment, command, component, message, modal};
}
