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
//!   derives, the `Route` and `Payload` traits,
//!   [`RouteComponents`](prelude::RouteComponents), [`Rgb`](ui::Rgb) and the
//!   macros [`components!`], [`label!`], [`placeholder!`], [`content!`] and
//!   [`rgb!`] into scope.
//! - [`ui`]: the Components V2 message model: one type per component,
//!   placement checked by the compiler, components built from Routes, and
//!   attachments.
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

/// A literal Components V2 tree, checked at `cargo check`; yields a
/// [`ui::Components`]. See [`ui`](ui#literal-macros-and-components).
pub use noctis_macros::components;
/// Legacy message content literal, checked at `cargo check` against MSG-01;
/// yields a [`ui::Content`].
pub use noctis_macros::content;
/// A button label literal, checked at `cargo check` against CV2-08; yields a
/// [`ui::Label`].
pub use noctis_macros::label;
/// A select placeholder literal, checked at `cargo check` against CV2-09;
/// yields a [`ui::Placeholder`].
pub use noctis_macros::placeholder;
/// A `0xRRGGBB` colour literal, checked at `cargo check` against CV2-13;
/// yields a [`ui::Rgb`].
pub use noctis_macros::rgb;

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
    //! - [`SelectRoute`]: a Route declared `select` (`#[route("…", select)]`
    //!   implements it), the only kind a message select is built from.
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
        SelectRoute, check,
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
    //! - [`memory`]: the bounded in-memory default of every port.
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
        //! [`MemoryIdentifyQueue`] grants identifies by bucket
        //! (`shard_id % max_concurrency`) in one process: one Lease per bucket
        //! per [`IDENTIFY_INTERVAL`](MemoryIdentifyQueue::IDENTIFY_INTERVAL)
        //! (5 s), buckets in parallel, FIFO within a bucket. It does not cover
        //! several processes: a Cluster implementation must serialize per
        //! bucket across processes (see the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md)).
        //!
        //! ```
        //! use std::num::NonZeroU16;
        //! use std::sync::Arc;
        //! use std::time::Duration;
        //!
        //! use noctis::ports::{CacheStore, IdentifyQueue};
        //! use noctis::ports::memory::{MemoryCacheStore, MemoryIdentifyQueue, MemoryLimits};
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
        //!
        //! let identify = MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap());
        //! assert_eq!(identify.max_concurrency().get(), 16);
        //! let q: Arc<dyn IdentifyQueue> = Arc::new(identify);
        //! # let _ = q;
        //! ```

        pub use noctis_ports::memory::{
            AllowAllGuilds, DiscordCacheResources, MemoryAssetStore, MemoryCacheStore,
            MemoryDiscordCache, MemoryIdentifyQueue, MemoryLimits, MemoryModuleStateStore,
            MemoryRateLimitStore, MemoryRefStore, MemoryRenderCache, MemorySessionStore,
            MemoryUsage, NoLocaleOverlay,
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
    //! - [`RouteComponents`]: `.button()`, `.string_select(..)`,
    //!   `.user_select()`, `.role_select()`, `.mentionable_select()` and
    //!   `.channel_select()` on a Route value (see [`ui`](crate::ui)).
    //! - [`Rgb`] and the macros [`components!`], [`label!`],
    //!   [`placeholder!`], [`content!`] and [`rgb!`].

    pub use noctis_codec::{Payload, Route};
    pub use noctis_macros::{Payload, PayloadEnum, Route};
    pub use noctis_macros::{components, content, label, placeholder, rgb};
    pub use noctis_ui::{Rgb, RouteComponents};
}

/// Paths the derive macros expand to; not public API.
#[doc(hidden)]
pub mod __private {
    pub use noctis_codec as codec;
    pub use noctis_ui as ui;
}

pub mod ui {
    //! The Components V2 message model.
    //!
    //! Every component is its own type with by-value, chainable setters
    //! (text is `impl Into<Cow<'static, str>>`), and serializes straight to
    //! Discord's JSON with unset fields left out:
    //!
    //! - Layout: [`Container`] (type 17, [`Container::push`],
    //!   [`Container::accent`] with an [`Rgb`], [`Container::spoiler`]),
    //!   [`Section`] (type 9, `Section::new([TextDisplay; N], accessory)`) and
    //!   [`ActionRow`] (type 1, [`ActionRow::buttons`] or [`ActionRow::select`]).
    //! - Content: [`TextDisplay`] (type 10), [`Separator`] (type 14, with
    //!   [`Spacing`]), [`Thumbnail`] (type 11), [`MediaGallery`] (type 12, of
    //!   [`MediaItem`]s), [`File`] (type 13) and [`Media`] (a URL or an
    //!   attachment).
    //! - Buttons (type 2): [`RouteButton`] (styles 1-4, [`ButtonStyle`],
    //!   [`Emoji`]), [`LinkButton`], [`PremiumButton`], and [`Button`] holding
    //!   any of them.
    //! - Selects: [`StringSelect`] (type 3, of [`SelectOption`]s),
    //!   [`UserSelect`] (5), [`RoleSelect`] (6), [`MentionableSelect`] (7) and
    //!   [`ChannelSelect`] (8).
    //! - [`Components`]: a message body; it numbers every component in
    //!   pre-order from 1, and no API sets a numeric id (CV2-14).
    //! - [`ContainerComponent`] and [`TopLevelComponent`]: any component of a
    //!   Container or of the top level, for lists built at runtime.
    //! - [`Attachment`], [`AttachmentOrigin`], [`AttachmentSet`] and
    //!   [`AttachmentRef`]: content-addressed files; [`File`] takes only an
    //!   [`AttachmentRef`], which only [`AttachmentSet::add`] returns (CV2-12);
    //!   [`Media`] takes an [`AttachmentRef`] or a URL, and an `attachment://`
    //!   name given as a URL (or a ref from another set) is checked at runtime
    //!   by ATT-05.
    //! - [`UiError`]: a bad attachment extension, a mismatched Ref slot, an
    //!   edit of a V2 message back into a legacy one
    //!   ([`UiError::V2ToLegacy`]), or a message or value over its Limits
    //!   ([`UiError::Limits`]).
    //!
    //! # Arity
    //!
    //! Array constructors take only legal lengths, as sealed traits on
    //! [`Arity`]`<N>`: [`ActionRow::buttons`] 1 to 5 ([`RowButtons`],
    //! CV2-06), [`Section::new`] 1 to 3 texts ([`SectionTexts`], CV2-07) and
    //! [`MediaGallery::new`] 1 to 10 items ([`GalleryItems`], CV2-10). Any
    //! other length fails `cargo check` with the rule id. A length known only
    //! at runtime goes through [`ActionRow::try_buttons`],
    //! [`Section::try_new`] or [`MediaGallery::try_new`], which return
    //! [`UiError::Limits`] instead.
    //!
    //! ```compile_fail,E0277
    //! use noctis::ui::{ActionRow, LinkButton};
    //!
    //! let link = || LinkButton::new("https://x.y");
    //! let _ = ActionRow::buttons([link(), link(), link(), link(), link(), link()]); // [CV2-06]
    //! ```
    //!
    //! ```
    //! use noctis::rules::{Limits, RuleId};
    //! use noctis::ui::{ActionRow, LinkButton, UiError};
    //!
    //! let urls = vec!["https://a.b"; 6];
    //! let six = ActionRow::try_buttons(urls.into_iter().map(LinkButton::new), &Limits::default());
    //! assert!(matches!(six, Err(UiError::Limits { violations }) if violations[0].rule == RuleId::CV2_06));
    //! ```
    //!
    //! # Literal macros and `components!`
    //!
    //! The literal macros measure their literal at `cargo check` with the
    //! `noctis-rules` function the runtime path calls:
    //! [`label!`](crate::label) ([`Label`], CV2-08, at most 80 characters),
    //! [`placeholder!`](crate::placeholder) ([`Placeholder`], CV2-09, 150),
    //! [`content!`](crate::content) ([`Content`], MSG-01, 2000) and
    //! [`rgb!`](crate::rgb) ([`Rgb`], CV2-13, `0x000000..=0xFFFFFF`). Each
    //! works in a `const`. Text known only at runtime (localized text
    //! included) goes through `Label::try_new(text, &limits)` (or
    //! `Placeholder`, `Content`) and `Rgb::try_from(u32)`; none of them ever
    //! truncates.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{Label, LegacyMessage, LinkButton};
    //!
    //! const SAVE: Label = label!("Save");
    //! const ACCENT: Rgb = rgb!(0x5865_F2);
    //! let link = LinkButton::new("https://x.y").label(SAVE);
    //! let notice = LegacyMessage::new(content!("Saved."));
    //! assert_eq!(ACCENT, Rgb::new(0x58, 0x65, 0xF2));
    //! # let _ = (link, notice, placeholder!("Pick one"));
    //! ```
    //!
    //! ```compile_fail
    //! let name = "Save";
    //! let _ = noctis::label!(name); // label! takes one string literal
    //! ```
    //!
    //! ```compile_fail
    //! let _ = noctis::rgb!(-1); // rgb! takes one non-negative integer literal
    //! ```
    //!
    //! [`components!`](crate::components) writes a fixed-shape tree:
    //!
    //! ```text
    //! components! { node,* }
    //! node      := container | section | text | gallery | file | separator | row | {hole} | ..spread
    //! container := container [ "(" accent = expr ")" ] "[" node,* "]"
    //! section   := section "[" text,* ";" accessory = (thumbnail | button | {hole}) "]"
    //! text      := text "(" string-literal | expr ")"
    //! thumbnail := thumbnail "(" media [, alt = text] ")"      // a string literal media is a URL
    //! gallery   := gallery "[" item "(" media [, alt = text] ")",* "]"
    //! file      := file "(" attachment_ref ")"
    //! separator := separator [ "(" [spacing = small | large] [, divider = expr] ")" ]  // both optional, any order
    //! row       := row "[" (button | {hole}),* "]" | row "[" select "]"
    //! button    := route.button( [label] ) .setter(..)* | link(url = "..", label = "..") | premium(sku)
    //! select    := route.string_select(options) .setter(..)* | route.user_select() .setter(..)* | ..
    //! ```
    //!
    //! It expands into the builders above (so the placement traits still
    //! apply to `{hole}`s and `..spread`s) and fails `cargo check` with the
    //! rule id, spanned on the offending node or literal, when the literal
    //! tree breaks CV2-03 (40 components; a hole counts 1, a spread 0),
    //! CV2-04 (4000 literal text characters; non-literal text counts 0),
    //! CV2-05, CV2-06, CV2-07, CV2-08 (literal labels and link URLs), CV2-09
    //! (literal placeholders, `min_values`, `max_values` and array-literal
    //! options), CV2-10, CV2-11, CV2-13 (literal accents, Separator spacing),
    //! CV2-17 (literal media URLs) or CID-02 (two components of the same
    //! literal Route value). The runtime [`validate`] still checks the built
    //! message before sending.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::rules::Limits;
    //! use noctis::ui::{ButtonStyle, TextDisplay, V2Message, validate};
    //!
    //! #[derive(Route)]
    //! #[route("t.press")]
    //! struct Press {
    //!     n: u8,
    //! }
    //! # impl noctis::__private::ui::Handled for Press {}
    //!
    //! let name = String::from("noctis");
    //! let footer = TextDisplay::new("-# footer");
    //! let body = components! {
    //!     container(accent = rgb!(0x5865_F2)) [
    //!         text("### Settings"),
    //!         separator(spacing = large),
    //!         section [ text("Logs"), text(name); accessory = Press { n: 1 }.button("Change") ],
    //!         row [ Press { n: 2 }.button("Save").style(ButtonStyle::Primary), link(url = "https://x.y", label = "Docs") ],
    //!     ],
    //!     {footer},
    //! };
    //! let mut msg = V2Message::new(body);
    //! assert_eq!(validate(&mut msg, &Limits::default()).unwrap().components, 11);
    //! ```
    //!
    //! ```compile_fail
    //! use noctis::components;
    //!
    //! let _ = components! { box [] }; // unknown node
    //! ```
    //!
    //! # Messages
    //!
    //! - [`V2Message`]: [`Components`] plus an [`AttachmentSet`] and
    //!   [`Mentions`]; every body sets `IS_COMPONENTS_V2` (CV2-01), and it has
    //!   no `content`, `embeds`, `poll`, `sticker_ids` or
    //!   `shared_client_theme` (CV2-02).
    //! - [`LegacyMessage`]: plain `content` and at most 5 Action Rows
    //!   ([`Arity`]`<N>: `[`LegacyRows`], CV2-15); the only legacy path, never
    //!   for interactive UI.
    //! - Flags per endpoint (MSG-07): [`CreateFlags`] (Create Message, Execute
    //!   Webhook), [`EditFlags`] (Edit Message, Edit Original Interaction
    //!   Response, Edit Webhook Message, callback type 7) and
    //!   [`CallbackFlags`] (callback type 4 and follow-ups, INT-09). None
    //!   offers `IS_COMPONENTS_V2`.
    //! - [`Mentions`], [`Users`], [`Roles`]: pings are opt-in; the default is
    //!   `parse: []` (MSG-05), and `Users::All` cannot carry ids (MSG-03).
    //! - [`MessageFormat`]: the format of the message an edit replaces; a
    //!   legacy-to-V2 edit clears `content`, `embeds`, `poll` and
    //!   `sticker_ids`.
    //! - [`MessageBody`]: the borrowed JSON body core's Responder sends.
    //!
    //! # Limits
    //!
    //! - [`validate`]`(&mut msg, &limits)`: checks a built [`V2Message`] or
    //!   [`LegacyMessage`] (the sealed [`Validate`] trait) against
    //!   [`Limits`](crate::rules::Limits) before sending and returns a
    //!   [`CostReport`], or [`UiError::Limits`]` { violations }` with every
    //!   violation. It lowers the tree into
    //!   [`rules::component`](crate::rules::component) descriptors, so counts,
    //!   paths and order match `components!`, and adds CID-02 (duplicate
    //!   `custom_id`s), ATT-05 (`attachment://` references), ATT-06, MSG-01,
    //!   MSG-04 and MSG-09 (empty bodies).
    //! - [`cost`]`(&msg, &limits)`: the [`CostReport`] (components at every
    //!   depth, text characters, warnings) without changing or sending.
    //! - Paths name each node by kind and position, as in
    //!   `container[0].section[2].accessory.label`, `row[0].select[0].options[3].label`,
    //!   `attachments[0].description` or `allowed_mentions.users`.
    //! - Strict mode (the default) never changes the message. Lenient mode
    //!   truncates display text with `…` first (Text Displays to the CV2-04
    //!   total, button labels, select placeholders and option labels and
    //!   descriptions, gallery, Thumbnail and attachment alt text, legacy
    //!   content), each a [`LimitWarning::Truncated`]; counts, URLs, values
    //!   and ids still fail.
    //! - [`LimitWarning::MissingAlt`]: a Thumbnail or gallery item that is not
    //!   a rendered image has no alt text (both modes). Every warning is also
    //!   logged at WARN with target `noctis::ui`.
    //!
    //! ```
    //! use noctis::rules::{LimitMode, Limits, RuleId};
    //! use noctis::ui::{ActionRow, Components, LinkButton, UiError, V2Message, validate};
    //!
    //! let row = || ActionRow::buttons([LinkButton::new("https://x.y").label("a".repeat(81))]);
    //! let mut msg = V2Message::new(Components::new().push(row()));
    //! let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
    //!     panic!("label too long");
    //! };
    //! assert_eq!(violations[0].rule, RuleId::CV2_08);
    //! assert_eq!(violations[0].path, "row[0].button[0].label");
    //!
    //! let mut limits = Limits::default();
    //! limits.mode = LimitMode::Lenient;
    //! let report = validate(&mut msg, &limits).unwrap();
    //! assert_eq!(report.components, 2);
    //! assert_eq!(report.warnings.len(), 1);
    //! ```
    //!
    //! # Placement
    //!
    //! The placing methods are bounded by sealed traits, so a misplaced
    //! component fails `cargo check` with the rule id (CV2-05, CV2-07):
    //!
    //! | Component | Top level ([`TopLevel`]) | Container ([`ContainerChild`]) | Section accessory ([`SectionAccessory`]) | Action Row |
    //! | --- | --- | --- | --- | --- |
    //! | [`Container`] | yes | no | no | no |
    //! | [`Section`], [`TextDisplay`], [`MediaGallery`], [`File`], [`Separator`], [`ActionRow`] | yes | yes | no | no |
    //! | [`Thumbnail`] | no | no | yes | no |
    //! | [`RouteButton`], [`LinkButton`], [`PremiumButton`], [`Button`] | no | no | yes | 1-5 ([`RowItem`]) |
    //! | the five selects | no | no | no | alone ([`RowSelect`]) |
    //!
    //! # Components from Routes
    //!
    //! [`RouteComponents`] (also in the [`prelude`](crate::prelude)) builds
    //! buttons and selects from a Route value; there is no string `custom_id`.
    //! The Route needs a handler (`#[handler]`), or the call fails with
    //! `[CID-04]`; a select also needs a Route declared `select`
    //! ([`SelectRoute`](crate::codec::SelectRoute)). `custom_id`s that need a
    //! View envelope, select nonce, signature or Ref are finalized by the
    //! Responder before sending.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{
    //!     ActionRow, ButtonStyle, Components, Container, LinkButton, Rgb, Section, Separator,
    //!     TextDisplay,
    //! };
    //!
    //! #[derive(Route)]
    //! #[route("shop.buy")]
    //! struct Buy {
    //!     item: u64,
    //! }
    //! # impl noctis::__private::ui::Handled for Buy {}
    //!
    //! let body = Components::new().push(
    //!     Container::new()
    //!         .accent(Rgb::new(0x58, 0x65, 0xF2))
    //!         .push(Section::new([TextDisplay::new("**Sword** · 255 g")], Buy { item: 255 }.button().label("Buy")))
    //!         .push(Separator::small())
    //!         .push(ActionRow::buttons([LinkButton::new("https://example.com").label("Docs")])),
    //! );
    //! let json = serde_json::to_value(&body).unwrap();
    //! assert_eq!(json[0]["components"][0]["accessory"]["custom_id"], "0shop.buy:47");
    //! # let _ = ButtonStyle::Primary;
    //! ```
    //!
    //! A V2 create body:
    //!
    //! ```
    //! use noctis::ui::{Components, CreateFlags, Mentions, TextDisplay, Users, V2Message};
    //!
    //! let msg = V2Message::new(Components::new().push(TextDisplay::new("Saved")))
    //!     .mentions(Mentions::none().users(Users::All));
    //! let json = serde_json::to_value(msg.create_body(CreateFlags::SUPPRESS_NOTIFICATIONS)).unwrap();
    //! assert_eq!(json["flags"], 32768 | 4096);
    //! assert_eq!(json["allowed_mentions"], serde_json::json!({"parse": ["users"]}));
    //! ```

    pub use noctis_ui::{
        ActionRow, Arity, Attachment, AttachmentOrigin, AttachmentRef, AttachmentSet, Button,
        ButtonStyle, CallbackFlags, ChannelSelect, Components, Container, ContainerChild,
        ContainerComponent, Content, CostReport, CreateFlags, EditFlags, Emoji, File, GalleryItems,
        Label, LegacyMessage, LegacyRows, LimitWarning, LinkButton, Media, MediaGallery, MediaItem,
        MentionableSelect, Mentions, MessageBody, MessageFormat, Placeholder, PremiumButton, Rgb,
        RoleSelect, Roles, RouteButton, RouteComponents, RowButtons, RowItem, RowSelect, Section,
        SectionAccessory, SectionTexts, SelectOption, Separator, Spacing, StringSelect,
        TextDisplay, Thumbnail, TopLevel, TopLevelComponent, UiError, UserSelect, Users, V2Message,
        Validate, cost, validate,
    };
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
    //! - [`message`]: legacy `content`, allowed mentions, nonce, stickers,
    //!   non-empty bodies ([`message::validate_v2_not_empty`],
    //!   [`message::validate_legacy_not_empty`]) and webhook names (MSG-01,
    //!   MSG-03..06, MSG-09, WH-04).
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
