//! Messages around a component tree: [`V2Message`], [`LegacyMessage`], the
//! per-endpoint flag sets, [`Mentions`] and the [`MessageBody`] core sends
//! (FR-23, CV2-01, CV2-02, CV2-15, MSG-05, MSG-07).

mod body;
mod flags;
mod mentions;

use std::borrow::Cow;

pub use body::MessageBody;
pub use flags::{CallbackFlags, CreateFlags, EditFlags, MessageFormat};
pub use mentions::{Mentions, Roles, Users};

use crate::arity::{Arity, LegacyRows};
use crate::route::CustomId;
use crate::wire::Wire;
use crate::{ActionRow, Attachment, AttachmentSet, Components, UiError};

/// A Components V2 message: a component tree, the attachments it uses and
/// who it may ping.
///
/// Every body it gives sets `IS_COMPONENTS_V2` (CV2-01), and it has no
/// `content`, `embeds`, `poll`, `sticker_ids` or `shared_client_theme`
/// (CV2-02). Flags are chosen per endpoint when the body is asked for, so the
/// same message can be sent, replied or edited.
///
/// ```
/// use noctis_ui::{Components, CreateFlags, Mentions, TextDisplay, Users, V2Message};
///
/// let msg = V2Message::new(Components::new().push(TextDisplay::new("hi")))
///     .mentions(Mentions::none().users(Users::All));
/// assert_eq!(
///     serde_json::to_value(msg.create_body(CreateFlags::SUPPRESS_NOTIFICATIONS)).unwrap(),
///     serde_json::json!({
///         "flags": 36864,
///         "components": [{"type": 10, "id": 1, "content": "hi"}],
///         "allowed_mentions": {"parse": ["users"]}
///     })
/// );
/// ```
///
/// There is no legacy field to set:
///
/// ```compile_fail,E0599
/// # use noctis_ui::{Components, V2Message};
/// let _ = V2Message::new(Components::new()).content("x"); // [CV2-02]
/// ```
///
/// ```compile_fail,E0599
/// # use noctis_ui::{Components, V2Message};
/// let _ = V2Message::new(Components::new()).embeds(Vec::<()>::new()); // [CV2-02]
/// ```
///
/// ```compile_fail,E0599
/// # use noctis_ui::{Components, V2Message};
/// let _ = V2Message::new(Components::new()).poll(()); // [CV2-02]
/// ```
///
/// ```compile_fail,E0599
/// # use noctis_ui::{Components, V2Message};
/// let _ = V2Message::new(Components::new()).sticker_ids([1_u64]); // [CV2-02]
/// ```
///
/// ```compile_fail,E0599
/// # use noctis_ui::{Components, V2Message};
/// let _ = V2Message::new(Components::new()).shared_client_theme(()); // [CV2-02]
/// ```
#[derive(Debug, Clone)]
pub struct V2Message {
    pub(crate) components: Components,
    pub(crate) attachments: AttachmentSet,
    pub(crate) mentions: Mentions,
}

impl V2Message {
    /// A message of `components`, with no attachment and no ping.
    pub fn new(components: Components) -> Self {
        Self {
            components,
            attachments: AttachmentSet::default(),
            mentions: Mentions::none(),
        }
    }

    /// Replaces the attachment set. Bodies never carry it: core's Responder
    /// uploads it (and diffs it on an edit).
    #[must_use]
    pub fn attachments(mut self, attachments: AttachmentSet) -> Self {
        self.attachments = attachments;
        self
    }

    /// Replaces who the message may ping; the default pings nobody (MSG-05).
    #[must_use]
    pub fn mentions(mut self, mentions: Mentions) -> Self {
        self.mentions = mentions;
        self
    }

    /// The component tree.
    pub fn components(&self) -> &Components {
        &self.components
    }

    /// The attachments the tree uses.
    pub fn attachment_set(&self) -> &AttachmentSet {
        &self.attachments
    }

    /// The members of the attachment set the tree references, each once, in
    /// name order: those whose `attachment://<name>` URL appears at a File,
    /// Thumbnail or Media Gallery item (built from an
    /// [`AttachmentRef`](crate::AttachmentRef) or written as
    /// `Media::url("attachment://<name>")`). Core's Responder uploads (or,
    /// on an edit, keeps) only these; an unreferenced member is never sent.
    ///
    /// ```
    /// use noctis_ui::{Attachment, AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
    ///
    /// let mut set = AttachmentSet::new();
    /// let a = set.add(Attachment::new(&b"a"[..], "png")?);
    /// let _unused = set.add(Attachment::new(&b"b"[..], "png")?);
    /// let gallery = MediaGallery::new([MediaItem::new(a.clone()).alt("A"), MediaItem::new(a.clone()).alt("A")]);
    /// let msg = V2Message::new(Components::new().push(gallery)).attachments(set);
    /// let used: Vec<&str> = msg.used_attachments().map(|f| f.name()).collect();
    /// assert_eq!(used, [a.name()]);
    /// # Ok::<(), noctis_ui::UiError>(())
    /// ```
    pub fn used_attachments(&self) -> impl Iterator<Item = &Attachment> {
        let used = crate::limits::referenced_names(&self.components);
        self.attachments
            .iter()
            .filter(move |a| used.contains(a.name()))
    }

    /// Who the message may ping.
    pub fn allowed_mentions(&self) -> &Mentions {
        &self.mentions
    }

    /// Every Route `custom_id` of the tree, in pre-order; core resolves the
    /// unresolved ones before sending (AD-18).
    #[doc(hidden)]
    pub fn custom_ids_mut(&mut self) -> impl Iterator<Item = &mut CustomId> {
        self.components.custom_ids_mut()
    }

    /// The body of Create Message or Execute Webhook.
    pub fn create_body(&self, flags: CreateFlags) -> MessageBody<'_> {
        MessageBody::v2(self, flags.bits(), false)
    }

    /// The `data` of callback type 4 or a follow-up message.
    pub fn callback_body(&self, flags: CallbackFlags) -> MessageBody<'_> {
        MessageBody::v2(self, flags.bits(), false)
    }

    /// The body of an edit (Edit Message, Edit Original Interaction Response,
    /// Edit Webhook Message, or the `data` of callback type 7) of a message
    /// in `format`.
    ///
    /// Editing a legacy message also sends `content: null, embeds: [],
    /// poll: null, sticker_ids: []`, which Discord needs before it accepts the
    /// V2 flag (CV2-02).
    ///
    /// ```
    /// use noctis_ui::{Components, EditFlags, MessageFormat, TextDisplay, V2Message};
    ///
    /// let msg = V2Message::new(Components::new().push(TextDisplay::new("hi")));
    /// let json = serde_json::to_value(msg.edit_body(MessageFormat::Legacy, EditFlags::empty())).unwrap();
    /// assert_eq!(json["content"], serde_json::Value::Null);
    /// assert_eq!(json["embeds"], serde_json::json!([]));
    /// ```
    pub fn edit_body(&self, format: MessageFormat, flags: EditFlags) -> MessageBody<'_> {
        MessageBody::v2(self, flags.bits(), format == MessageFormat::Legacy)
    }
}

impl From<Components> for V2Message {
    fn from(components: Components) -> Self {
        Self::new(components)
    }
}

/// A legacy message: plain `content` and up to 5 Action Rows (CV2-15).
///
/// It is the only legacy path and carries plain text only: no embeds, polls
/// or stickers. Never use it for interactive UI or dashboards; build a
/// [`V2Message`] (UD-1). A V2 message cannot be edited back into a legacy
/// one ([`UiError::V2ToLegacy`], CV2-01).
///
/// ```
/// use noctis_ui::{ActionRow, CreateFlags, LegacyMessage, LinkButton};
///
/// let msg = LegacyMessage::new("Read the docs.")
///     .rows([ActionRow::buttons([LinkButton::new("https://a.b").label("A")])]);
/// assert_eq!(
///     serde_json::to_value(msg.create_body(CreateFlags::empty())).unwrap(),
///     serde_json::json!({
///         "content": "Read the docs.",
///         "components": [{"type": 1, "components": [{"type": 2, "style": 5, "label": "A", "url": "https://a.b"}]}],
///         "allowed_mentions": {"parse": []}
///     })
/// );
/// ```
///
/// More than 5 rows fails with `[CV2-15]`:
///
/// ```compile_fail,E0277
/// use noctis_ui::{ActionRow, LegacyMessage, LinkButton};
///
/// let row = ActionRow::buttons([LinkButton::new("https://a.b")]);
/// let _ = LegacyMessage::new("x").rows([row.clone(), row.clone(), row.clone(), row.clone(), row.clone(), row]);
/// ```
#[derive(Debug, Clone)]
pub struct LegacyMessage {
    pub(crate) content: Cow<'static, str>,
    pub(crate) rows: Vec<ActionRow>,
    pub(crate) mentions: Mentions,
}

impl LegacyMessage {
    /// A message with `content` and no row; the content length is checked by
    /// the runtime validator.
    pub fn new(content: impl Into<Cow<'static, str>>) -> Self {
        Self {
            content: content.into(),
            rows: Vec::new(),
            mentions: Mentions::none(),
        }
    }

    /// Replaces the Action Rows: at most 5 (`Arity<N>: LegacyRows`, CV2-15).
    #[must_use]
    pub fn rows<const N: usize>(mut self, rows: [ActionRow; N]) -> Self
    where
        Arity<N>: LegacyRows,
    {
        self.rows = rows.into();
        self
    }

    /// Replaces who the message may ping; the default pings nobody (MSG-05).
    #[must_use]
    pub fn mentions(mut self, mentions: Mentions) -> Self {
        self.mentions = mentions;
        self
    }

    /// The `content`.
    pub fn content_text(&self) -> &str {
        &self.content
    }

    /// Who the message may ping.
    pub fn allowed_mentions(&self) -> &Mentions {
        &self.mentions
    }

    /// The number of Action Rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Every Route `custom_id` of the rows, in order; core resolves the
    /// unresolved ones before sending (AD-18).
    #[doc(hidden)]
    pub fn custom_ids_mut(&mut self) -> impl Iterator<Item = &mut CustomId> {
        let mut out = Vec::new();
        for row in &mut self.rows {
            row.custom_ids_mut(&mut out);
        }
        out.into_iter()
    }

    /// The body of Create Message or Execute Webhook; `flags` is left out
    /// when empty.
    pub fn create_body(&self, flags: CreateFlags) -> MessageBody<'_> {
        MessageBody::legacy(self, (!flags.is_empty()).then_some(flags.bits()))
    }

    /// The `data` of callback type 4 or a follow-up message; `flags` is left
    /// out when empty.
    pub fn callback_body(&self, flags: CallbackFlags) -> MessageBody<'_> {
        MessageBody::legacy(self, (!flags.is_empty()).then_some(flags.bits()))
    }

    /// The body of an edit of a message in `format`; it always sends `flags`.
    ///
    /// # Errors
    ///
    /// [`UiError::V2ToLegacy`] when `format` is [`MessageFormat::V2`]: a V2
    /// message cannot lose `IS_COMPONENTS_V2` (CV2-01).
    ///
    /// ```
    /// use noctis_ui::{EditFlags, LegacyMessage, MessageFormat, UiError};
    ///
    /// let msg = LegacyMessage::new("hi");
    /// assert!(msg.edit_body(MessageFormat::Legacy, EditFlags::empty()).is_ok());
    /// assert_eq!(
    ///     msg.edit_body(MessageFormat::V2, EditFlags::empty()).unwrap_err(),
    ///     UiError::V2ToLegacy
    /// );
    /// ```
    pub fn edit_body(
        &self,
        format: MessageFormat,
        flags: EditFlags,
    ) -> Result<MessageBody<'_>, UiError> {
        match format {
            MessageFormat::Legacy => Ok(MessageBody::legacy(self, Some(flags.bits()))),
            MessageFormat::V2 => Err(UiError::V2ToLegacy),
        }
    }
}
