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
//! | [`RawComponent`] | yes | yes | no | no |
//! | [`Thumbnail`] | no | no | yes | no |
//! | [`RouteButton`], [`LinkButton`], [`PremiumButton`], [`Button`] | no | no | yes | 1-5 ([`RowItem`]) |
//! | [`StringSelect`], [`UserSelect`], [`RoleSelect`], [`MentionableSelect`], [`ChannelSelect`] | no | no | no | alone ([`RowSelect`]) |
//!
//! A [`StringSelectBuilder`] (a String Select whose struct declares no
//! options) is placed nowhere until `.options(..)` makes it a
//! [`StringSelect`] (CV2-09).
//!
//! [`ContainerComponent`] and [`TopLevelComponent`] hold any component of
//! their level, for lists built at runtime.
//!
//! # Raw components
//!
//! [`RawComponent`] is the escape hatch for a component type Noctis does not
//! model (FR-31, CV2-16): checked JSON (component objects, no `id`, at most
//! 32 levels) that is sent verbatim at the top level or in a Container,
//! counts [`RawComponent::count`] toward CV2-03 and never takes a numeric
//! id. [`unknown_components`] picks the components Noctis does not model
//! out of a received message's raw `components`.
//!
//! # `custom_id`s
//!
//! Interactive components are built only from a Route value: the kind
//! derives of `noctis` (`#[derive(Button)]`, the select derives,
//! `#[derive(Modal)]`) emit an inherent `X { .. }.build()` that builds the
//! component from the struct's declared `defaults(..)` and the struct value
//! as its Payload; a struct without a `#[handler]` method does not compile
//! (`[CID-04]`). There is no string `custom_id` constructor (AD-19). A Route without a
//! select nonce or integrity tag is encoded at once; the others are
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
//! (CV2-08), [`Placeholder`] (CV2-09), [`Content`] (MSG-01), [`AltText`]
//! (ATT-06) and `Rgb::try_from` (CV2-13) check runtime values with the same
//! `noctis-rules` function the literal macros `label!`, `placeholder!`,
//! `content!`, `alt!` and `rgb!` call at compile time.
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
//! # Modals
//!
//! A [`Modal`] is built only from a `#[derive(Modal)]` struct
//! (`X { .. }.build()` or `X::blank(..)`), so its `custom_id` is the
//! struct's Route encoding (MOD-08) and there is no other constructor. Its
//! top level takes a [`ModalLabel`] or a [`TextDisplay`] ([`ModalTopLevel`],
//! MOD-02); a [`ModalLabel`] wraps one input ([`ModalInput`]): [`TextInput`]
//! (with [`TextInputStyle`]), [`ModalStringSelect`], [`ModalUserSelect`],
//! [`ModalRoleSelect`], [`ModalMentionableSelect`], [`ModalChannelSelect`],
//! [`FileUpload`], [`RadioGroup`] and [`CheckboxGroup`] (of [`ModalOption`]s)
//! or [`Checkbox`]. Any other message component fails `cargo check` with
//! `[MOD-03]`; modal selects have no `disabled` and a Checkbox has no
//! `required`. Each input takes a string key, its `custom_id`, naming the
//! submitted field. A modal serializes as callback `data`
//! (`{"custom_id", "title", "components"}`) with no numeric ids and no Action
//! Rows. [`validate`] and [`cost`] take a modal too.
//!
//! [`ModalForm`] (implemented by `#[derive(Modal)]` in `noctis`) decodes a
//! submission: `T::from_json(bytes)` reads the raw modal-submit interaction
//! JSON through [`ModalSubmission`], which reads Label- and Action-Row-wrapped
//! inputs itself (twilight-model drops Radio Groups, Checkbox Groups and
//! Checkboxes), joins selects and uploads to the resolved
//! [`ResolvedUser`]s, [`ResolvedRole`]s, [`ResolvedChannel`]s,
//! [`Mentionable`]s and [`ResolvedAttachment`]s, keeps unknown components as
//! `serde_json::Value`s and reports every bad value as a [`SubmitError`]
//! (MOD-09). [`ResolvedUser`] and [`ResolvedMessage`] are also the targets of
//! USER and MESSAGE context-menu commands. [`Choices`] is a closed option set whose type-level count is
//! bounded by [`RadioOptions`], [`CheckboxOptions`] and
//! [`StringSelectOptions`]. [`ResolvedAttachment::download`] fetches an
//! upload through a [`FileFetcher`] (this crate does no I/O), checking the
//! CDN host, the size limit and the length ([`DownloadError`]).
//!
//! ```
//! use noctis_ui::{ModalSubmission, SubmittedValue};
//!
//! let json = br#"{"type": 5, "data": {"custom_id": "0mod.report:", "components": [
//!     {"type": 18, "component": {"type": 22, "custom_id": "topics", "values": ["a", "b"]}}
//! ]}}"#;
//! let submission = ModalSubmission::from_slice(json)?;
//! assert_eq!(
//!     submission.get("topics"),
//!     Some(&SubmittedValue::CheckboxGroup(vec!["a".into(), "b".into()]))
//! );
//! # Ok::<(), noctis_ui::SubmitError>(())
//! ```
//!
//! # User text
//!
//! [`escape_markdown`] and [`escape_mentions`] neutralize user-controlled
//! text before it goes into a Text Display; nothing is escaped
//! automatically. They only insert characters and commute, and a message
//! pings nobody unless its `allowed_mentions` opt in.
//!
//! ```
//! use noctis_ui::{escape_markdown, escape_mentions};
//!
//! let bio = escape_mentions(&escape_markdown("**hi** <@1>"));
//! assert_eq!(bio, "\\*\\*hi\\*\\* <\u{200B}@1>");
//! ```
//!
//! # Limits
//!
//! [`validate`] checks a built [`V2Message`], [`LegacyMessage`] or
//! [`Modal`] against `noctis_rules::Limits` before it is sent, and [`cost`]
//! measures one without changing it; [`Budget`] turns that cost into what
//! is left of CV2-03 and CV2-04 beside a message's fixed parts and how many
//! items of one shape still fit (FR-62). [`validate`] and [`cost`] lower a
//! message tree into
//! `noctis_rules::component` descriptors (a modal into `noctis_rules::modal`
//! ones), so counts, paths and violation order match `components!`;
//! for messages they add CID-02, ATT-05, ATT-06, MSG-01, MSG-04 and MSG-09,
//! and for a modal CID-01 and CID-02 on its input keys and CV2-04 on its
//! Text Displays. A failure is
//! [`UiError::Limits`] with path-precise violations; Lenient mode truncates
//! display text with `…` instead, as a [`LimitWarning::Truncated`]. A
//! rendered image (FR-30) in a Thumbnail or gallery item without alt text of
//! its own is an ATT-06 violation in Strict mode and a
//! [`LimitWarning::MissingAlt`] in Lenient mode; other images without alt
//! text are a `MissingAlt` warning in both modes.
//! [`V2Message::used_attachments`] lists the set members the tree
//! references: the only files core's Responder uploads or keeps.
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
mod escape;
mod limits;
mod literal;
mod message;
mod modal;
mod placement;
mod route;
mod wire;

pub use arity::{
    Arity, CheckboxOptions, GalleryItems, LegacyRows, RadioOptions, RowButtons, SectionTexts,
    StringSelectOptions,
};
pub use attachment::{Attachment, AttachmentOrigin, AttachmentRef, AttachmentSet};
pub use component::{
    ActionRow, Button, ButtonStyle, ChannelSelect, Container, ContainerComponent, Emoji, File,
    LinkButton, Media, MediaGallery, MediaItem, MentionableSelect, PremiumButton, RawComponent,
    Rgb, RoleSelect, RouteButton, Section, SelectOption, Separator, Spacing, StringSelect,
    StringSelectBuilder, TextDisplay, Thumbnail, TopLevelComponent, UserSelect, unknown_components,
};
pub use components::Components;
pub use error::UiError;
pub use escape::{escape_markdown, escape_mentions};
pub use limits::{Budget, CostReport, LimitWarning, Validate, cost, validate};
pub use literal::{AltText, Content, Label, Placeholder};
pub use message::{
    CallbackFlags, CreateFlags, EditFlags, LegacyMessage, Mentions, MessageBody, MessageFormat,
    Roles, Users, V2Message,
};
pub use modal::{
    Checkbox, CheckboxGroup, Choices, DownloadError, FileFetcher, FileUpload, Mentionable, Modal,
    ModalChannelSelect, ModalComponent, ModalForm, ModalLabel, ModalMentionableSelect, ModalOption,
    ModalRoleSelect, ModalStringSelect, ModalSubmission, ModalUserSelect, RadioGroup,
    ResolvedAttachment, ResolvedChannel, ResolvedMessage, ResolvedRole, ResolvedUser, SubmitError,
    SubmittedValue, TextInput, TextInputStyle,
};
pub use placement::{
    ContainerChild, ModalInput, ModalTopLevel, RowItem, RowSelect, SectionAccessory, TopLevel,
};
pub use route::Handled;
#[doc(hidden)]
pub use route::{ComponentRoute, CustomId};

/// Helpers the `components!`, `derive(Modal)` and `derive(Choices)`
/// expansions call; not public API.
#[doc(hidden)]
pub mod __macro {
    pub use twilight_model::channel::ChannelType;

    pub use crate::modal::form::{
        ChannelSelectField, CheckboxField, CheckboxGroupField, FileUploadField,
        MentionableSelectField, RadioGroupField, RoleSelectField, Spec, StringSelectField,
        TextDisplayField, TextInputField, UserSelectField, channel_select, checkbox,
        checkbox_group, file_upload, mentionable_select, radio_group, role_select, string_select,
        text_display, text_input, user_select,
    };
    pub use crate::modal::form::{ModalFromPayload, decode_modal_payload};
    pub use crate::placement::{OptionsGiven, RowButtonItem, RowOne};
    pub use crate::route::{
        ButtonDefaults, ChannelSelectDefaults, ComponentRoute, ComponentValue, DefaultEmoji,
        ModalDefaults, SelectDefaults, StringSelectDefaults, build_button, build_channel_select,
        build_mentionable_select, build_modal, build_role_select, build_string_select,
        build_string_select_builder, build_user_select, choices_options,
    };
    use crate::{ActionRow, Button};

    /// Converts any Action Row button into a [`Button`], so a `row [..]` of
    /// 2 to 5 mixed kinds is one array; a select is `[CV2-06]`.
    pub fn row_button<B: RowButtonItem>(button: B) -> Button {
        button.into_row_button()
    }

    /// The Action Row of a one-item `row [..]`: any button or select.
    pub fn row_one<C: RowOne>(component: C) -> ActionRow {
        component.into_row()
    }
}
