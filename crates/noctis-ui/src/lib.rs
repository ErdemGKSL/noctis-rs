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
//!
//! # Messages
//!
//! A [`V2Message`] wraps [`Components`], its [`AttachmentSet`] and its
//! [`Mentions`]. It has no `content`, `embeds`, `poll`, `sticker_ids` or
//! `shared_client_theme` (CV2-02), and every body it gives sets
//! `IS_COMPONENTS_V2` (CV2-01). [`LegacyMessage`] is the only legacy path:
//! plain `content` and at most 5 Action Rows (`Arity<N>: LegacyRows`,
//! CV2-15), never for interactive UI.
//!
//! Flags are typed per endpoint and passed when a body is asked for:
//! [`CreateFlags`] (Create Message, Execute Webhook), [`EditFlags`] (every
//! edit and callback type 7) and [`CallbackFlags`] (callback type 4 and
//! follow-ups); each has consts only for the flags its endpoints accept
//! (MSG-07). Mentions ping nobody unless opted in through [`Users`] and
//! [`Roles`] (MSG-03, MSG-05). An edit takes the existing message's
//! [`MessageFormat`]: legacy-to-V2 clears the legacy fields, V2-to-legacy is
//! [`UiError::V2ToLegacy`].
//!
//! ```
//! use noctis_ui::{
//!     Components, EditFlags, LegacyMessage, MessageFormat, TextDisplay, UiError, V2Message,
//! };
//!
//! let msg = V2Message::new(Components::new().push(TextDisplay::new("hi")));
//! let json = serde_json::to_value(msg.edit_body(MessageFormat::Legacy, EditFlags::empty())).unwrap();
//! assert_eq!(json["flags"], 32768);
//! assert_eq!(json["sticker_ids"], serde_json::json!([]));
//!
//! let notice = LegacyMessage::new("plain text");
//! assert!(matches!(notice.edit_body(MessageFormat::V2, EditFlags::empty()), Err(UiError::V2ToLegacy)));
//! ```
//!
//! # Arity and literals
//!
//! Array constructors take only legal lengths: [`ActionRow::buttons`] 1 to 5
//! ([`RowButtons`], CV2-06), [`Section::new`] 1 to 3 texts
//! ([`SectionTexts`], CV2-07) and [`MediaGallery::new`] 1 to 10 items
//! ([`GalleryItems`], CV2-10); any other length fails `cargo check`.
//! [`ActionRow::try_buttons`], [`Section::try_new`] and
//! [`MediaGallery::try_new`] take lengths known only at runtime. [`Label`]
//! (CV2-08), [`Placeholder`] (CV2-09), [`Content`] (MSG-01) and
//! `Rgb::try_from` (CV2-13) check runtime values with the same `noctis-rules`
//! function the literal macros `label!`, `placeholder!`, `content!` and
//! `rgb!` call at compile time.
//!
//! ```
//! use noctis_rules::Limits;
//! use noctis_ui::{ActionRow, Label, LinkButton, UiError};
//!
//! let open = Label::try_from("Open")?;
//! let urls = vec!["https://a.b", "https://c.d"];
//! let links = urls.into_iter().map(|u| LinkButton::new(u).label(open.clone()));
//! let row = ActionRow::try_buttons(links, &Limits::default())?;
//! # let _ = row;
//! # Ok::<(), UiError>(())
//! ```
//!
//! # Limits
//!
//! [`validate`] checks a built [`V2Message`] or [`LegacyMessage`] against
//! `noctis_rules::Limits` before it is sent, and [`cost`] measures one
//! without changing it. Both lower the tree into `noctis_rules::component`
//! descriptors, so counts, paths and violation order match `components!`;
//! they add CID-02, ATT-05, ATT-06, MSG-01, MSG-04 and MSG-09. A failure is
//! [`UiError::Limits`] with path-precise violations; Lenient mode truncates
//! display text with `…` instead, as a [`LimitWarning::Truncated`]. Images
//! without alt text are a [`LimitWarning::MissingAlt`] in both modes.
//!
//! ```
//! use noctis_rules::{Limits, RuleId};
//! use noctis_ui::{Components, TextDisplay, UiError, V2Message, cost, validate};
//!
//! let texts = (0..41).map(|_| TextDisplay::new("a"));
//! let mut msg = V2Message::new(texts.fold(Components::new(), Components::push));
//! assert_eq!(cost(&msg, &Limits::default()).components, 41);
//! let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
//!     panic!("41 components");
//! };
//! assert_eq!(violations[0].rule, RuleId::CV2_03);
//! assert_eq!(violations[0].to_string().lines().next(), Some("[CV2-03] components: limit at most 40, actual 41"));
//! ```

mod arity;
mod attachment;
mod component;
mod components;
mod error;
mod limits;
mod literal;
mod message;
mod placement;
mod route;
mod wire;

pub use arity::{Arity, GalleryItems, LegacyRows, RowButtons, SectionTexts};
pub use attachment::{Attachment, AttachmentOrigin, AttachmentRef, AttachmentSet};
pub use component::{
    ActionRow, Button, ButtonStyle, ChannelSelect, Container, ContainerComponent, Emoji, File,
    LinkButton, Media, MediaGallery, MediaItem, MentionableSelect, PremiumButton, Rgb, RoleSelect,
    RouteButton, Section, SelectOption, Separator, Spacing, StringSelect, TextDisplay, Thumbnail,
    TopLevelComponent, UserSelect,
};
pub use components::Components;
pub use error::UiError;
pub use limits::{CostReport, LimitWarning, Validate, cost, validate};
pub use literal::{Content, Label, Placeholder};
pub use message::{
    CallbackFlags, CreateFlags, EditFlags, LegacyMessage, Mentions, MessageBody, MessageFormat,
    Roles, Users, V2Message,
};
pub use placement::{ContainerChild, RowItem, RowSelect, SectionAccessory, TopLevel};
#[doc(hidden)]
pub use route::CustomId;
pub use route::{Handled, RouteComponents};

/// Helpers the `components!` expansion calls; not public API.
#[doc(hidden)]
pub mod __macro {
    use crate::{Button, RowItem};

    /// Converts any Action Row button into a [`Button`], so a `row [..]` of
    /// mixed kinds is one array.
    pub fn row_button<B: RowItem>(button: B) -> Button {
        button.into_row_button()
    }
}
