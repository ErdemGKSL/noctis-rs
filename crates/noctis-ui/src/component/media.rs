//! Media: Thumbnail (type 11), Media Gallery (type 12) and File (type 13).

use std::borrow::Cow;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::attachment::AttachmentRef;
use crate::wire::{Ids, Wire, flag, head, opt, serialize_via_wire};

/// An image or video source: an `https` URL or an attachment of the same
/// message.
///
/// Serializes as `{"url": …}`. A `Media` built from an [`AttachmentRef`]
/// remembers it, so its origin stays known.
///
/// ```
/// use noctis_ui::Media;
///
/// let media = Media::url("https://i/x.png");
/// assert_eq!(media.as_str(), "https://i/x.png");
/// assert!(media.attachment().is_none());
/// assert_eq!(serde_json::to_value(&media).unwrap(), serde_json::json!({"url": "https://i/x.png"}));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media(Source);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Url(Cow<'static, str>),
    Attachment(AttachmentRef),
}

impl Media {
    /// Media at `url` (URL rules are checked by the runtime validator).
    pub fn url(url: impl Into<Cow<'static, str>>) -> Self {
        Self(Source::Url(url.into()))
    }

    /// The URL: the given one, or `attachment://<name>`.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Source::Url(url) => url,
            Source::Attachment(r) => r.url(),
        }
    }

    /// The attachment this media refers to, if any.
    pub fn attachment(&self) -> Option<&AttachmentRef> {
        match &self.0 {
            Source::Url(_) => None,
            Source::Attachment(r) => Some(r),
        }
    }
}

impl From<AttachmentRef> for Media {
    fn from(r: AttachmentRef) -> Self {
        Self(Source::Attachment(r))
    }
}

impl Serialize for Media {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        UrlObject(self.as_str()).serialize(s)
    }
}

/// A media object: exactly `{"url": …}`.
struct UrlObject<'a>(&'a str);

impl Serialize for UrlObject<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        map.serialize_entry("url", self.0)?;
        map.end()
    }
}

/// A Thumbnail (type 11): a small image.
///
/// Placement: only as a Section accessory (CV2-05).
///
/// ```
/// use noctis_ui::{Media, Thumbnail};
///
/// let thumb = Thumbnail::new(Media::url("https://i/x.png")).alt("A").spoiler(true);
/// assert_eq!(
///     serde_json::to_value(&thumb).unwrap(),
///     serde_json::json!({"type": 11, "media": {"url": "https://i/x.png"}, "description": "A", "spoiler": true})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub(crate) media: Media,
    pub(crate) alt: Option<Cow<'static, str>>,
    pub(crate) spoiler: bool,
}

impl Thumbnail {
    /// A Thumbnail showing `media`.
    pub fn new(media: impl Into<Media>) -> Self {
        Self {
            media: media.into(),
            alt: None,
            spoiler: false,
        }
    }

    /// The alt text (`description`).
    #[must_use]
    pub fn alt(self, alt: impl Into<Cow<'static, str>>) -> Self {
        Self {
            alt: Some(alt.into()),
            ..self
        }
    }

    /// Whether the image is blurred until clicked.
    #[must_use]
    pub fn spoiler(self, spoiler: bool) -> Self {
        Self { spoiler, ..self }
    }
}

impl Wire for Thumbnail {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 11, ids)?;
        map.serialize_entry("media", &self.media)?;
        opt(&mut map, "description", self.alt.as_deref())?;
        flag(&mut map, "spoiler", self.spoiler)?;
        map.end()
    }
}

/// One item of a [`MediaGallery`]; it has no `type` and no numeric id.
///
/// ```
/// use noctis_ui::{Media, MediaItem};
///
/// let item = MediaItem::new(Media::url("https://i/x.png")).alt("x");
/// assert_eq!(
///     serde_json::to_value(&item).unwrap(),
///     serde_json::json!({"media": {"url": "https://i/x.png"}, "description": "x"})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    pub(crate) media: Media,
    pub(crate) alt: Option<Cow<'static, str>>,
    pub(crate) spoiler: bool,
}

impl MediaItem {
    /// An item showing `media`.
    pub fn new(media: impl Into<Media>) -> Self {
        Self {
            media: media.into(),
            alt: None,
            spoiler: false,
        }
    }

    /// The alt text (`description`).
    #[must_use]
    pub fn alt(self, alt: impl Into<Cow<'static, str>>) -> Self {
        Self {
            alt: Some(alt.into()),
            ..self
        }
    }

    /// Whether the item is blurred until clicked.
    #[must_use]
    pub fn spoiler(self, spoiler: bool) -> Self {
        Self { spoiler, ..self }
    }
}

impl Serialize for MediaItem {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("media", &self.media)?;
        opt(&mut map, "description", self.alt.as_deref())?;
        flag(&mut map, "spoiler", self.spoiler)?;
        map.end()
    }
}

/// A Media Gallery (type 12): a grid of images and videos.
///
/// Placement: top level or a Container.
///
/// ```
/// use noctis_ui::{Media, MediaGallery, MediaItem};
///
/// let gallery = MediaGallery::new([MediaItem::new(Media::url("https://i/x.png"))]);
/// assert_eq!(
///     serde_json::to_value(&gallery).unwrap(),
///     serde_json::json!({"type": 12, "items": [{"media": {"url": "https://i/x.png"}}]})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaGallery {
    pub(crate) items: Vec<MediaItem>,
}

impl MediaGallery {
    /// A gallery of `items`, in order (1 to 10; the count is checked by the
    /// runtime validator).
    pub fn new<const N: usize>(items: [MediaItem; N]) -> Self {
        Self {
            items: items.into(),
        }
    }
}

impl Wire for MediaGallery {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 12, ids)?;
        map.serialize_entry("items", &self.items)?;
        map.end()
    }
}

/// A File (type 13): an attached file of the same message.
///
/// Placement: top level or a Container. It takes only an [`AttachmentRef`],
/// which exists only once the file is added to an
/// [`AttachmentSet`](crate::AttachmentSet), so its `attachment://` URL always
/// names a real attachment (CV2-12):
///
/// ```compile_fail,E0308
/// use noctis_ui::File;
///
/// let _ = File::new("attachment://x.png"); // [CV2-12]
/// ```
///
/// ```
/// use noctis_ui::{Attachment, AttachmentSet, File};
///
/// let mut set = AttachmentSet::new();
/// let file = File::new(set.add(Attachment::new(&b"report"[..], "txt")?)).spoiler(true);
/// let json = serde_json::to_value(&file).unwrap();
/// assert!(json["file"]["url"].as_str().unwrap().starts_with("attachment://"));
/// # Ok::<(), noctis_ui::UiError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    file: AttachmentRef,
    spoiler: bool,
}

impl File {
    /// A File component for the attachment `file`.
    pub fn new(file: AttachmentRef) -> Self {
        Self {
            file,
            spoiler: false,
        }
    }

    /// Whether the file is blurred until clicked.
    #[must_use]
    pub fn spoiler(self, spoiler: bool) -> Self {
        Self { spoiler, ..self }
    }

    /// The attachment.
    pub fn attachment(&self) -> &AttachmentRef {
        &self.file
    }
}

impl Wire for File {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 13, ids)?;
        map.serialize_entry("file", &UrlObject(self.file.url()))?;
        flag(&mut map, "spoiler", self.spoiler)?;
        map.end()
    }
}

serialize_via_wire!(Thumbnail, MediaGallery, File);
