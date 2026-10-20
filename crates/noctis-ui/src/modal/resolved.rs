//! The entities a modal select or File Upload resolves to:
//! [`ResolvedUser`], [`ResolvedRole`], [`ResolvedChannel`], [`Mentionable`]
//! and [`ResolvedAttachment`], plus the attachment download seam
//! [`FileFetcher`] (MOD-09). This crate does no I/O (AD-2, AD-15): core
//! supplies the HTTP fetcher.

use std::error::Error;
use std::future::Future;

use bytes::Bytes;
use noctis_rules::Limits;
use twilight_model::application::interaction::{InteractionChannel, InteractionMember};
use twilight_model::channel::Attachment;
use twilight_model::guild::Role;
use twilight_model::id::Id;
use twilight_model::id::marker::AttachmentMarker;
use twilight_model::user::User;

/// A user picked in a User or Mentionable Select, with their guild member
/// data when the modal was submitted in a guild.
///
/// ```
/// use noctis_ui::ResolvedUser;
///
/// fn name(user: &ResolvedUser) -> &str {
///     user.member.as_ref().and_then(|m| m.nick.as_deref()).unwrap_or(&user.user.name)
/// }
/// # let _ = name;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedUser {
    /// The user.
    pub user: User,
    /// The guild member, absent outside a guild.
    pub member: Option<InteractionMember>,
}

/// A role picked in a Role or Mentionable Select.
///
/// ```
/// use noctis_ui::ResolvedRole;
///
/// fn name(role: &ResolvedRole) -> &str {
///     &role.role.name
/// }
/// # let _ = name;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedRole {
    /// The role.
    pub role: Role,
}

/// A channel picked in a Channel Select.
///
/// ```
/// use noctis_ui::ResolvedChannel;
///
/// fn name(channel: &ResolvedChannel) -> &str {
///     &channel.channel.name
/// }
/// # let _ = name;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedChannel {
    /// The channel, with the permissions the user has in it.
    pub channel: InteractionChannel,
}

/// A user or a role picked in a Mentionable Select.
///
/// ```
/// use noctis_ui::Mentionable;
///
/// fn label(m: &Mentionable) -> String {
///     match m {
///         Mentionable::User(u) => format!("<@{}>", u.user.id),
///         Mentionable::Role(r) => format!("<@&{}>", r.role.id),
///         _ => String::new(),
///     }
/// }
/// # let _ = label;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[expect(
    clippy::large_enum_variant,
    reason = "the shape mirrors Discord's two entity kinds; a select returns few of them"
)]
pub enum Mentionable {
    /// A user (with member data in a guild).
    User(ResolvedUser),
    /// A role.
    Role(ResolvedRole),
}

/// A file uploaded through a File Upload (type 19).
///
/// Its URL is an **ephemeral**, signed Discord CDN URL that expires: download
/// the file promptly with [`download`](Self::download), and re-check its
/// [`content_type`](Self::content_type) and contents yourself, since the
/// submitted data is untrusted (MOD-09).
///
/// ```
/// use noctis_ui::ResolvedAttachment;
///
/// fn is_png(file: &ResolvedAttachment) -> bool {
///     file.content_type() == Some("image/png") && file.filename().ends_with(".png")
/// }
/// # let _ = is_png;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResolvedAttachment {
    /// The attachment.
    pub attachment: Attachment,
}

/// The Discord CDN hosts [`ResolvedAttachment::download`] accepts.
const CDN_PREFIXES: [&str; 2] = [
    "https://cdn.discordapp.com/",
    "https://media.discordapp.net/",
];

impl ResolvedAttachment {
    /// The attachment id.
    pub fn id(&self) -> Id<AttachmentMarker> {
        self.attachment.id
    }

    /// The file name.
    pub fn filename(&self) -> &str {
        &self.attachment.filename
    }

    /// The size in bytes, as Discord reports it.
    pub fn size(&self) -> u64 {
        self.attachment.size
    }

    /// The media type Discord reports; untrusted (MOD-09).
    pub fn content_type(&self) -> Option<&str> {
        self.attachment.content_type.as_deref()
    }

    /// The ephemeral CDN URL; it expires, so download promptly.
    pub fn url(&self) -> &str {
        &self.attachment.url
    }

    /// Downloads the file through `fetcher`.
    ///
    /// The URL is ephemeral (signed and expiring), so call this promptly
    /// after the submission. Only `https://cdn.discordapp.com/` and
    /// `https://media.discordapp.net/` URLs are fetched, a file larger than
    /// `limits.attachment_bytes` is refused before fetching, and the
    /// fetcher is asked for at most [`size`](Self::size) bytes. The
    /// [`FileFetcher`] contract makes the fetcher fail (never truncate) on a
    /// body over that cap, never read beyond it and never follow a redirect
    /// off the two CDN hosts. Re-check the content type of what you get
    /// (MOD-09).
    ///
    /// # Errors
    ///
    /// [`DownloadError::Host`] for any other URL, [`DownloadError::TooLarge`]
    /// over the limit (the fetcher is not called for either),
    /// [`DownloadError::Fetch`] when the fetcher fails and
    /// [`DownloadError::SizeMismatch`] when it returns another length than
    /// Discord reported.
    ///
    /// ```
    /// use std::error::Error;
    /// use std::future::Future;
    ///
    /// use bytes::Bytes;
    /// use noctis_rules::Limits;
    /// use noctis_ui::{DownloadError, FileFetcher, ResolvedAttachment};
    ///
    /// struct Fake;
    /// impl FileFetcher for Fake {
    ///     fn fetch(&self, _url: &str, _max: u64) -> impl Future<Output = Result<Bytes, Box<dyn Error + Send + Sync>>> + Send {
    ///         async { Ok(Bytes::from_static(b"png")) }
    ///     }
    /// }
    ///
    /// async fn save(file: &ResolvedAttachment) -> Result<Bytes, DownloadError> {
    ///     file.download(&Fake, &Limits::default()).await
    /// }
    /// # let _ = save;
    /// ```
    pub async fn download<F>(&self, fetcher: &F, limits: &Limits) -> Result<Bytes, DownloadError>
    where
        F: FileFetcher + ?Sized + Sync,
    {
        let url = self.url();
        if !CDN_PREFIXES.iter().any(|p| url.starts_with(p)) {
            return Err(DownloadError::Host {
                url: url.to_owned(),
            });
        }
        let size = self.size();
        let limit = u64::try_from(limits.attachment_bytes).unwrap_or(u64::MAX);
        if size > limit {
            return Err(DownloadError::TooLarge { size, limit });
        }
        let bytes = fetcher
            .fetch(url, size)
            .await
            .map_err(DownloadError::Fetch)?;
        let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if actual != size {
            return Err(DownloadError::SizeMismatch {
                expected: size,
                actual,
            });
        }
        Ok(bytes)
    }
}

/// Fetches a URL's bytes; the seam [`ResolvedAttachment::download`] goes
/// through. Core supplies the HTTP implementation; tests use a fake.
///
/// # Contract
///
/// `max_bytes` is the most the caller accepts. An implementation **must**:
///
/// - return an error, never a truncated body, when the body would exceed
///   `max_bytes`;
/// - never read (or buffer) more than `max_bytes` of the body;
/// - never follow a redirect to a host outside `https://cdn.discordapp.com/`
///   and `https://media.discordapp.net/`.
///
/// ```
/// use std::error::Error;
/// use std::future::Future;
///
/// use bytes::Bytes;
/// use noctis_ui::FileFetcher;
///
/// struct Empty;
/// impl FileFetcher for Empty {
///     fn fetch(&self, _url: &str, _max_bytes: u64) -> impl Future<Output = Result<Bytes, Box<dyn Error + Send + Sync>>> + Send {
///         async { Ok(Bytes::new()) }
///     }
/// }
/// ```
pub trait FileFetcher {
    /// Fetches `url`, reading at most `max_bytes`.
    ///
    /// # Errors
    ///
    /// Must fail (never truncate) when the body would exceed `max_bytes`,
    /// must not read beyond it, and must not follow a redirect to a host
    /// outside `https://cdn.discordapp.com/` and `https://media.discordapp.net/`.
    fn fetch(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> impl Future<Output = Result<Bytes, Box<dyn Error + Send + Sync>>> + Send;
}

/// Why [`ResolvedAttachment::download`] failed (MOD-09).
///
/// ```
/// use noctis_ui::DownloadError;
///
/// let err = DownloadError::TooLarge { size: 30, limit: 20 };
/// assert_eq!(err.to_string(), "[MOD-09] the file has 30 bytes, over the limit of 20");
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DownloadError {
    /// The URL is not on the Discord CDN; nothing was fetched.
    #[error("[MOD-09] `{url}` is not a Discord CDN URL")]
    Host {
        /// The refused URL.
        url: String,
    },
    /// The file is larger than `Limits::attachment_bytes`; nothing was
    /// fetched.
    #[error("[MOD-09] the file has {size} bytes, over the limit of {limit}")]
    TooLarge {
        /// The reported size.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// The fetched length differs from the reported size.
    #[error("[MOD-09] fetched {actual} bytes, but Discord reported {expected}")]
    SizeMismatch {
        /// The reported size.
        expected: u64,
        /// The fetched length.
        actual: u64,
    },
    /// The fetcher failed.
    #[error("[MOD-09] the download failed")]
    Fetch(#[source] Box<dyn Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Fake {
        calls: AtomicUsize,
        seen: Mutex<Vec<(String, u64)>>,
        reply: Result<&'static [u8], &'static str>,
    }

    impl Fake {
        fn new(reply: Result<&'static [u8], &'static str>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                seen: Mutex::new(Vec::new()),
                reply,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn seen(&self) -> Vec<(String, u64)> {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl FileFetcher for Fake {
        fn fetch(
            &self,
            url: &str,
            max_bytes: u64,
        ) -> impl Future<Output = Result<Bytes, Box<dyn Error + Send + Sync>>> + Send {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((url.to_owned(), max_bytes));
            let reply = self.reply;
            async move {
                match reply {
                    Ok(b) => Ok(Bytes::from_static(b)),
                    Err(e) => Err(e.into()),
                }
            }
        }
    }

    fn file(url: &str, size: u64) -> ResolvedAttachment {
        let json = serde_json::json!({
            "id": "900", "filename": "a.png", "size": size, "url": url,
            "proxy_url": url, "content_type": "image/png"
        });
        ResolvedAttachment {
            attachment: serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}")),
        }
    }

    const CDN: &str = "https://cdn.discordapp.com/attachments/1/2/a.png?ex=1";

    #[tokio::test]
    async fn mod_09_download_ok() {
        let fake = Fake::new(Ok(b"abc"));
        let f = file(CDN, 3);
        assert_eq!((f.id().get(), f.filename(), f.size()), (900, "a.png", 3));
        assert_eq!(f.content_type(), Some("image/png"));
        let bytes = f.download(&fake, &Limits::default()).await;
        assert_eq!(bytes.ok(), Some(Bytes::from_static(b"abc")));
        assert_eq!(fake.calls(), 1);
        assert_eq!(fake.seen(), vec![(CDN.to_owned(), 3)]);
        let media = file("https://media.discordapp.net/attachments/1/2/a.png", 3);
        assert!(media.download(&fake, &Limits::default()).await.is_ok());
    }

    #[tokio::test]
    async fn mod_09_download_bad_host_is_not_fetched() {
        let fake = Fake::new(Ok(b"abc"));
        for url in [
            "https://evil.example/a.png",
            "http://cdn.discordapp.com/a.png",
            "https://cdn.discordapp.com.evil.example/a.png",
        ] {
            let err = file(url, 3).download(&fake, &Limits::default()).await;
            assert!(matches!(err, Err(DownloadError::Host { .. })), "{url}");
        }
        assert_eq!(fake.calls(), 0);
    }

    #[tokio::test]
    async fn mod_09_download_too_large_is_not_fetched() {
        let fake = Fake::new(Ok(b"abc"));
        let limits = Limits::default();
        let size = u64::try_from(limits.attachment_bytes).unwrap_or(u64::MAX) + 1;
        let err = file(CDN, size).download(&fake, &limits).await;
        assert!(matches!(err, Err(DownloadError::TooLarge { size: s, .. }) if s == size));
        assert_eq!(fake.calls(), 0);
    }

    #[tokio::test]
    async fn mod_09_download_size_mismatch_and_fetch_error() {
        let fake = Fake::new(Ok(b"abcd"));
        let err = file(CDN, 3).download(&fake, &Limits::default()).await;
        assert!(matches!(
            err,
            Err(DownloadError::SizeMismatch {
                expected: 3,
                actual: 4
            })
        ));
        let failing = Fake::new(Err("timeout"));
        let err = file(CDN, 3).download(&failing, &Limits::default()).await;
        let Err(DownloadError::Fetch(source)) = err else {
            panic!("expected a fetch error, got {err:?}");
        };
        assert_eq!(source.to_string(), "timeout");
        assert_eq!(failing.calls(), 1);
    }
}
