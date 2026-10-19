//! Noctis: a Discord bot framework on twilight, with built-in sharding, a
//! Components V2 UI kit and image rendering.
//!
//! Bot code depends on this crate only; the `noctis-*` crates are internal.
//!
//! # Modules
//!
//! - [`rules`]: Discord limits, rule ids, text length and locales.
//! - [`prelude`]: `use noctis::prelude::*;` brings the [`Route`](prelude::Route),
//!   [`Payload`](prelude::Payload) and [`PayloadEnum`](prelude::PayloadEnum)
//!   derives and the `Route` and `Payload` traits into scope.
//! - [`codec`]: Payload field encodings ([`codec::Field`],
//!   [`codec::BoundedStr`]), the checks custom fields are tested with, and the
//!   traits the derives implement.
//! - [`ports`]: the Adapter port traits, the shared op vocabulary, typed
//!   values and the render contract.
//! - [`bytes`]: [`Bytes`](bytes::Bytes), the value type every byte-store port
//!   takes and returns.
//!
//! # Routes
//!
//! `#[derive(Route)]` turns a struct into a Route: a typed `custom_id` whose
//! fields are its Payload. The derive proves at compile time that every id
//! fits Discord's 100 characters (CID-01), and a struct that could exceed it
//! fails `cargo check` with the route id and the worst case.
//!
//! ```
//! use noctis::codec::RouteShape;
//! use noctis::prelude::*;
//! use twilight_model::id::Id;
//!
//! pub struct ItemMarker;
//!
//! /// `shop.buy: F 1 + route 8 + sep 1 + S 0 + E 0 + N 0 + body 14 + T 0 = 24 (limit 100)`
//! #[derive(Route, Debug, PartialEq)]
//! #[route("shop.buy")]
//! struct BuyButton {
//!     item: Id<ItemMarker>,
//!     qty: u8,
//! }
//!
//! assert_eq!(BuyButton::ID.as_str(), "shop.buy");
//! assert_eq!(BuyButton::SHAPE, RouteShape::INLINE);
//! assert_eq!(<BuyButton as Payload>::MAX_ENCODED_LEN, Some(14));
//!
//! /// A unit enum is a one-character field.
//! #[derive(PayloadEnum, Debug, Clone, Copy, PartialEq)]
//! enum Size {
//!     Small,
//!     Large,
//! }
//!
//! #[derive(Route)]
//! #[route("shop.size", schema = 1, view)]
//! struct SizePicker {
//!     size: Size,
//! }
//! assert_eq!(SizePicker::SCHEMA, 1);
//! ```
//!
//! `#[route("id", args…)]` takes the route id (1 to 24 characters of
//! `[a-z0-9._-]`, not starting with `_`) and, in any order, the codec
//! arguments `by_ref`, `overflow = spill`, `schema = n` (1..=61), `view`,
//! `select` and `signed`. Each derived Route also gets a
//! `__noctis_route_width_<Name>` test that prints its width breakdown
//! (`cargo test -- --nocapture noctis_route_width`).
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
    //!   [`FieldDecode::absent`] is a field's value when it is missing from
    //!   the end of an older `custom_id`: `Some(None)` for `Option<T>`, `None`
    //!   otherwise, so appending trailing `Option` fields needs no schema bump.
    //! - [`FieldError`]: why a field did not decode.
    //! - [`check`]: [`check::assert_round_trip`] and
    //!   [`check::assert_worst_case`] for the tests of custom fields.
    //! - [`Route`], [`RouteId`], [`RouteShape`] and [`BodyKind`]: a Route's
    //!   id, codec shape (inline, by Ref or spill, with optional View envelope,
    //!   select nonce and integrity tag), schema and Payload; usually derived
    //!   with `#[derive(Route)]` from the [`prelude`](crate::prelude).
    //! - [`Payload`] and [`PayloadDecode`]: an ordered field list, its
    //!   worst-case width [`Payload::MAX_ENCODED_LEN`], written through
    //!   [`FieldWriter`] and read through [`FieldReader`].
    //! - [`CodecError`] and [`Section`]: why an inline body or a `custom_id`
    //!   did not decode, and which layout section was wrong.
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

    pub use noctis_codec::{
        BodyKind, BoundedStr, BoundedStrError, CodecError, Field, FieldDecode, FieldError,
        FieldReader, FieldWriter, Payload, PayloadDecode, Route, RouteId, RouteShape, Section,
        check,
    };
}

pub mod bytes {
    //! [`Bytes`]: the opaque value type of the byte-store ports (re-exported
    //! from the `bytes` crate).

    pub use bytes::Bytes;
}

pub mod ports {
    //! Adapter ports: swappable storage, coordination and rendering seams.
    //!
    //! - Byte stores, each a subset of the shared op vocabulary (`get`,
    //!   `put`, `put_if_absent`, `take`, `delete`, `touch`, `incr_with_ttl`):
    //!   [`RefStore`], [`SessionStore`], [`ModuleStateStore`] (plus
    //!   `version`), [`RateLimitStore`], [`CacheStore`], [`RenderCache`] and
    //!   [`AssetStore`].
    //! - [`IdentifyQueue`] and [`Lease`]: identify coordination per bucket.
    //! - [`DiscordCache`]: the typed Discord resource cache.
    //! - [`LocaleSource`]: runtime overlays of content strings.
    //! - [`GuildAccessList`]: per-App guild allow and deny lists (sync).
    //! - [`PortError`], [`ValueError`] and [`BoxFuture`]: what every port
    //!   method returns.
    //! - [`encode_value`] and [`decode_value`]: typed values as postcard with
    //!   a leading schema byte; a schema mismatch reads as a miss.
    //! - The render contract: [`Template`], [`ErasedView`],
    //!   [`TemplateDescriptor`], [`RenderJob`], [`decode_template`], [`Size`],
    //!   [`Format`], [`AnimationSpec`], [`AnimatedFormat`], [`Frame`],
    //!   [`OutputFormat`], [`RenderOutput`], [`EncodeSettings`],
    //!   [`RenderError`] and the [`Renderer`] port.
    //! - [`memory`]: the bounded in-memory default of every port except
    //!   [`IdentifyQueue`].
    //!
    //! Every port is `Send + Sync + 'static` and dyn-compatible, so the App
    //! holds it as `Arc<dyn Port>`. Keys are opaque and stored verbatim, and no
    //! method ever receives a secret.
    //!
    //! ```
    //! use std::sync::Arc;
    //!
    //! use noctis::ports::{CacheStore, decode_value, encode_value};
    //!
    //! let _store: Option<Arc<dyn CacheStore>> = None;
    //!
    //! let raw = encode_value(1, &("profile", 42u32)).unwrap();
    //! assert_eq!(raw[0], 1);
    //! assert_eq!(decode_value::<(String, u32)>(1, &raw), Ok(Some(("profile".to_owned(), 42))));
    //! assert_eq!(decode_value::<(String, u32)>(2, &raw), Ok(None));
    //! ```

    pub use noctis_ports::{
        AnimatedFormat, AnimationSpec, AssetStore, BoxFuture, CacheStore, DiscordCache,
        EncodeSettings, ErasedView, Format, Frame, GuildAccessList, IdentifyQueue, Lease,
        LocaleSource, ModuleStateStore, OutputFormat, PortError, RateLimitStore, RefStore,
        RenderCache, RenderError, RenderJob, RenderOutput, Renderer, SessionStore, Size, Template,
        TemplateDescriptor, ValueError, decode_template, decode_value, encode_value,
    };

    pub mod memory {
        //! The bounded in-memory defaults: what an App runs on when you swap
        //! nothing.
        //!
        //! | Store | Port | `max_bytes` | `max_ttl` |
        //! | --- | --- | --- | --- |
        //! | [`MemoryRefStore`] | [`RefStore`](super::RefStore) | 32 MiB | 7 days |
        //! | [`MemorySessionStore`] | [`SessionStore`](super::SessionStore) | 1 MiB | none |
        //! | [`MemoryModuleStateStore`] | [`ModuleStateStore`](super::ModuleStateStore) | 16 MiB | none |
        //! | [`MemoryRateLimitStore`] | [`RateLimitStore`](super::RateLimitStore) | 16 MiB | 1 day |
        //! | [`MemoryCacheStore`] | [`CacheStore`](super::CacheStore) | 32 MiB | none |
        //! | [`MemoryRenderCache`] | [`RenderCache`](super::RenderCache) | 64 MiB | 1 day |
        //! | [`MemoryAssetStore`] | [`AssetStore`](super::AssetStore) | 32 MiB | 1 day |
        //!
        //! Each store has `DEFAULT_LIMITS`, `new()`, `with_limits(`[`MemoryLimits`]`)`,
        //! `limits()` and `usage() -> `[`MemoryUsage`]. An entry weighs
        //! `key.len() + value.len() + MemoryLimits::ENTRY_OVERHEAD` bytes; a
        //! full budget evicts the least recently used entry, a single write
        //! larger than the budget is a `PortError::InvalidValue`, and every TTL
        //! is clamped to `max_ttl`. The defaults are caches: capacity may evict
        //! any entry, swap a persistent adapter for durable data.
        //!
        //! [`MemoryDiscordCache`] caches only the chosen
        //! [`DiscordCacheResources`] and at most a per-channel message cap.
        //! [`NoLocaleOverlay`] and [`AllowAllGuilds`] are the defaults of
        //! `LocaleSource` and `GuildAccessList`.
        //!
        //! ```
        //! use std::sync::Arc;
        //! use std::time::Duration;
        //!
        //! use noctis::ports::CacheStore;
        //! use noctis::ports::memory::{MemoryCacheStore, MemoryLimits};
        //!
        //! let store: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new());
        //! # let _ = store;
        //! assert_eq!(MemoryCacheStore::DEFAULT_LIMITS.max_bytes(), 32 * 1024 * 1024);
        //! assert_eq!(MemoryCacheStore::DEFAULT_LIMITS.max_ttl(), None);
        //!
        //! const SMALL: MemoryLimits = MemoryLimits::new(1024 * 1024, Some(Duration::from_secs(60)));
        //! let small = MemoryCacheStore::with_limits(SMALL);
        //! assert_eq!(small.limits(), SMALL);
        //! assert_eq!(small.usage().entries, 0);
        //! ```

        pub use noctis_ports::memory::{
            AllowAllGuilds, DiscordCacheResources, MemoryAssetStore, MemoryCacheStore,
            MemoryDiscordCache, MemoryLimits, MemoryModuleStateStore, MemoryRateLimitStore,
            MemoryRefStore, MemoryRenderCache, MemorySessionStore, MemoryUsage, NoLocaleOverlay,
        };
    }
}

pub mod prelude {
    //! The derives and the traits they implement: `use noctis::prelude::*;`.
    //!
    //! - [`Route`]: the trait and `#[derive(Route)]`, which also derives the
    //!   Payload and proves the 100-character bound at compile time.
    //! - [`Payload`]: the trait and `#[derive(Payload)]`, for the Payload of a
    //!   hand-written `Route`.
    //! - [`PayloadEnum`]: `#[derive(PayloadEnum)]` on a unit enum of at most 62
    //!   variants, a one-character [`Field`](crate::codec::Field).

    pub use noctis_codec::{Payload, Route};
    pub use noctis_macros::{Payload, PayloadEnum, Route};
}

/// Paths the derive macros expand to; not public API.
#[doc(hidden)]
pub mod __private {
    pub use noctis_codec as codec;
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
