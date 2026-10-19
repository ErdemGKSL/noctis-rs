//! Components V2 message model: one type per component, placement in the
//! type system, Route-built `custom_id`s and content-addressed attachments.
//!
//! This crate is internal; use `noctis` (`noctis::ui`) instead. Its API is
//! not stable and may change in any release.
//!
//! # Model
//!
//! Every component is its own type with by-value, chainable setters; text
//! takes `impl Into<Cow<'static, str>>`. Each type serializes straight to
//! Discord's JSON with unset fields and `false` flags left out.
//! [`Components`] is a message body: it numbers every component in
//! pre-order from 1 (CV2-14); a lone component carries no `id`.
//!
//! # Placement
//!
//! Sealed marker traits say where a component may go, and the methods that
//! place one are bounded by them, so a misplaced component fails `cargo check`
//! with the rule id (CV2-05, CV2-07):
//!
//! | Component | Top level ([`TopLevel`]) | Container ([`ContainerChild`]) | Section accessory ([`SectionAccessory`]) | Action Row |
//! | --- | --- | --- | --- | --- |
//! | [`Container`] | yes | no | no | no |
//! | [`Section`], [`TextDisplay`], [`MediaGallery`], [`File`], [`Separator`], [`ActionRow`] | yes | yes | no | no |
//! | [`Thumbnail`] | no | no | yes | no |
//! | [`RouteButton`], [`LinkButton`], [`PremiumButton`], [`Button`] | no | no | yes | 1-5 ([`RowItem`]) |
//! | [`StringSelect`], [`UserSelect`], [`RoleSelect`], [`MentionableSelect`], [`ChannelSelect`] | no | no | no | alone ([`RowSelect`]) |
//!
//! [`ContainerComponent`] and [`TopLevelComponent`] hold any component of
//! their level, for lists built at runtime.
//!
//! # `custom_id`s
//!
//! Interactive components are built only from a Route value through
//! [`RouteComponents`], which requires the Route to have a handler
//! ([`Handled`], `[CID-04]`) and, for selects, to be declared `select`.
//! There is no string `custom_id` constructor (AD-19). A Route without a View
//! envelope, select nonce or integrity tag is encoded at once; the others are
//! resolved by core's Responder before sending (AD-18), and serializing one
//! still unresolved is an error.
//!
//! # Attachments
//!
//! An [`Attachment`] is named by its content (`<16 hex of blake3>.<ext>`).
//! Adding it to an [`AttachmentSet`] returns an [`AttachmentRef`]. [`File`]
//! takes only an [`AttachmentRef`] (CV2-12); [`Media`] takes an
//! [`AttachmentRef`] or a URL, and an `attachment://` name given as a URL (or
//! a ref from another set) is checked at runtime by ATT-05.
//!
//! ```
//! use noctis_ui::{
//!     ActionRow, Attachment, AttachmentSet, Components, Container, File, LinkButton, Media,
//!     MediaGallery, MediaItem, Rgb, Section, Separator, TextDisplay,
//! };
//!
//! let mut files = AttachmentSet::new();
//! let card = files.add(Attachment::new(&b"png bytes"[..], "png")?);
//! let body = Components::new().push(
//!     Container::new()
//!         .accent(Rgb::new(0x57, 0xF2, 0x87))
//!         .push(Section::new([TextDisplay::new("### Saved")], LinkButton::new("https://x.y").label("Docs")))
//!         .push(MediaGallery::new([MediaItem::new(Media::from(card.clone())).alt("Card")]))
//!         .push(Separator::small())
//!         .push(File::new(card))
//!         .push(ActionRow::buttons([LinkButton::new("https://a.b").label("Help")])),
//! );
//! let json = serde_json::to_value(&body).unwrap();
//! assert_eq!(json[0]["components"][0]["accessory"]["id"], 4);
//! # Ok::<(), noctis_ui::UiError>(())
//! ```

mod attachment;
mod component;
mod components;
mod error;
mod placement;
mod route;
mod wire;

pub use attachment::{Attachment, AttachmentOrigin, AttachmentRef, AttachmentSet};
pub use component::{
    ActionRow, Button, ButtonStyle, ChannelSelect, Container, ContainerComponent, Emoji, File,
    LinkButton, Media, MediaGallery, MediaItem, MentionableSelect, PremiumButton, Rgb, RoleSelect,
    RouteButton, Section, SelectOption, Separator, Spacing, StringSelect, TextDisplay, Thumbnail,
    TopLevelComponent, UserSelect,
};
pub use components::Components;
pub use error::UiError;
pub use placement::{ContainerChild, RowItem, RowSelect, SectionAccessory, TopLevel};
#[doc(hidden)]
pub use route::CustomId;
pub use route::{Handled, RouteComponents};
