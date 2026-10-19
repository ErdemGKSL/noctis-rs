//! Content-addressed attachments: [`Attachment`], [`AttachmentSet`] and the
//! [`AttachmentRef`] that File and Media components take (FR-30, CV2-12).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use arrayvec::ArrayString;
use bytes::Bytes;

use crate::UiError;

/// Longest extension, in characters.
const MAX_EXT_LEN: usize = 10;
/// Hex digits of the content hash in a name.
const HASH_LEN: usize = 16;
/// `attachment://`.
const SCHEME: &str = "attachment://";
/// `attachment://` + 16 hex digits + `.` + a 10-character extension.
const MAX_URL_LEN: usize = SCHEME.len() + HASH_LEN + 1 + MAX_EXT_LEN;

/// Where an attachment's bytes came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AttachmentOrigin {
    /// Bytes the bot supplied.
    Plain,
    /// An image rendered by Noctis from a Template.
    Rendered,
}

/// A file to upload with a message, named by its content.
///
/// The name is `<first 16 hex digits of the blake3 hash>.<ext>`, so the same
/// bytes always get the same name; [`with_digest`](Self::with_digest) and
/// [`rendered`](Self::rendered) take a digest the caller already has. The
/// extension is 1 to 10 ASCII letters or digits, kept verbatim.
///
/// ```
/// use bytes::Bytes;
/// use noctis_ui::{Attachment, AttachmentOrigin, UiError};
///
/// let empty = Attachment::new(Bytes::new(), "png")?;
/// assert_eq!(empty.name(), "af1349b9f5f9a1a6.png");
/// assert_eq!(empty.origin(), AttachmentOrigin::Plain);
///
/// let card = Attachment::rendered(Bytes::new(), [0xAB; 32], "webp")?.alt("Profile card");
/// assert_eq!(card.name(), "abababababababab.webp");
/// assert_eq!(card.alt_text(), Some("Profile card"));
///
/// assert!(matches!(Attachment::new(Bytes::new(), ".png"), Err(UiError::InvalidExtension { .. })));
/// # Ok::<(), UiError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    url: ArrayString<MAX_URL_LEN>,
    bytes: Bytes,
    content_type: Option<Cow<'static, str>>,
    alt: Option<Cow<'static, str>>,
    origin: AttachmentOrigin,
}

impl Attachment {
    /// An attachment of `bytes`, named by their blake3 hash.
    ///
    /// # Errors
    ///
    /// [`UiError::InvalidExtension`] when `ext` is not 1 to 10 ASCII letters
    /// or digits.
    pub fn new(bytes: impl Into<Bytes>, ext: &str) -> Result<Self, UiError> {
        let bytes = bytes.into();
        let digest = *blake3::hash(&bytes).as_bytes();
        Self::build(bytes, &digest, ext, AttachmentOrigin::Plain)
    }

    /// An attachment of `bytes` named by `digest`, a content hash the caller
    /// already computed.
    ///
    /// # Errors
    ///
    /// [`UiError::InvalidExtension`] as for [`new`](Self::new).
    pub fn with_digest(
        bytes: impl Into<Bytes>,
        digest: [u8; 32],
        ext: &str,
    ) -> Result<Self, UiError> {
        Self::build(bytes.into(), &digest, ext, AttachmentOrigin::Plain)
    }

    /// A rendered image, named by its render `digest`; its origin is
    /// [`AttachmentOrigin::Rendered`].
    ///
    /// # Errors
    ///
    /// [`UiError::InvalidExtension`] as for [`new`](Self::new).
    pub fn rendered(bytes: Bytes, digest: [u8; 32], ext: &'static str) -> Result<Self, UiError> {
        Self::build(bytes, &digest, ext, AttachmentOrigin::Rendered)
    }

    fn build(
        bytes: Bytes,
        digest: &[u8; 32],
        ext: &str,
        origin: AttachmentOrigin,
    ) -> Result<Self, UiError> {
        if ext.is_empty()
            || ext.len() > MAX_EXT_LEN
            || !ext.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(UiError::InvalidExtension {
                ext: ext.to_owned(),
            });
        }
        let mut url = ArrayString::new();
        // At most `MAX_URL_LEN` characters by construction.
        let written = (|| -> core::fmt::Result {
            url.write_str(SCHEME)?;
            for b in digest.iter().take(HASH_LEN / 2) {
                write!(url, "{b:02x}")?;
            }
            url.write_char('.')?;
            url.write_str(ext)
        })();
        debug_assert!(written.is_ok(), "an attachment url fits MAX_URL_LEN");
        Ok(Self {
            url,
            bytes,
            content_type: None,
            alt: None,
            origin,
        })
    }

    /// The alt text (`description`).
    #[must_use]
    pub fn alt(self, alt: impl Into<Cow<'static, str>>) -> Self {
        Self {
            alt: Some(alt.into()),
            ..self
        }
    }

    /// The MIME type, such as `image/png`.
    #[must_use]
    pub fn content_type(self, content_type: impl Into<Cow<'static, str>>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..self
        }
    }

    /// The file name, `<hash>.<ext>`.
    pub fn name(&self) -> &str {
        name_of(&self.url)
    }

    /// The file contents.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    /// The MIME type, if set.
    pub fn mime(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// The alt text, if set.
    pub fn alt_text(&self) -> Option<&str> {
        self.alt.as_deref()
    }

    /// Where the bytes came from.
    pub fn origin(&self) -> AttachmentOrigin {
        self.origin
    }
}

fn name_of(url: &str) -> &str {
    url.strip_prefix(SCHEME).unwrap_or(url)
}

/// A reference to an attachment of an [`AttachmentSet`]: the only way to
/// build a [`File`](crate::File) or an attachment [`Media`](crate::Media).
///
/// It has no public constructor; [`AttachmentSet::add`] returns it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttachmentRef {
    url: ArrayString<MAX_URL_LEN>,
    origin: AttachmentOrigin,
}

impl AttachmentRef {
    /// The file name, `<hash>.<ext>`.
    pub fn name(&self) -> &str {
        name_of(&self.url)
    }

    /// `attachment://<name>`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Where the attachment's bytes came from.
    pub fn origin(&self) -> AttachmentOrigin {
        self.origin
    }
}

/// The attachments of one message, keyed by name.
///
/// Adding the same content twice keeps the first attachment and returns
/// equal references.
///
/// ```
/// use noctis_ui::{Attachment, AttachmentSet};
///
/// let mut set = AttachmentSet::new();
/// let a = set.add(Attachment::new(&b"x"[..], "png")?);
/// let b = set.add(Attachment::new(&b"x"[..], "png")?);
/// assert_eq!(a, b);
/// assert_eq!(set.len(), 1);
/// assert_eq!(a.url(), format!("attachment://{}", a.name()));
/// assert!(set.get(a.name()).is_some());
/// # Ok::<(), noctis_ui::UiError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentSet {
    files: BTreeMap<ArrayString<MAX_URL_LEN>, Attachment>,
}

impl AttachmentSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `attachment` (or keeps the one already under its name) and
    /// returns its reference.
    pub fn add(&mut self, attachment: Attachment) -> AttachmentRef {
        let kept = self.files.entry(attachment.url).or_insert(attachment);
        AttachmentRef {
            url: kept.url,
            origin: kept.origin,
        }
    }

    /// The attachment named `name`.
    pub fn get(&self, name: &str) -> Option<&Attachment> {
        let mut url = ArrayString::<MAX_URL_LEN>::new();
        url.try_push_str(SCHEME).ok()?;
        url.try_push_str(name).ok()?;
        self.files.get(&url)
    }

    /// The attachments, in name order.
    pub fn iter(&self) -> impl Iterator<Item = &Attachment> {
        self.files.values()
    }

    /// The number of attachments.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}
