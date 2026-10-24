//! Remote images (IMG-03): [`RemoteImage`] values a Template declares in
//! `#[asset]` fields, resolved by the App's `AssetFetcher` before the job
//! enters the render pool.
//!
//! A `RemoteImage` is Template data, so it is serialized into the job and
//! decoded back by the Renderer: every value read from a job is untrusted,
//! and the URL grammar, the size normalization and the allow-list are
//! applied again when it is resolved (never only in the constructors).

mod config;
mod declare;
mod fetch;
#[cfg(test)]
pub(crate) mod test_server;
#[cfg(test)]
mod tests;

use std::fmt;
use std::net::Ipv6Addr;

use serde::{Deserialize, Serialize};
use twilight_model::guild::Guild;
use twilight_model::user::User;
use twilight_model::util::ImageHash;

pub(crate) use config::Origin;
pub use config::RemoteConfig;
pub(crate) use declare::Declared;
pub use declare::{AssetField, DeclaredImages, TemplateAssets};
pub(crate) use fetch::{AssetFetcher, Resolved};

/// The longest accepted URL, in bytes.
const MAX_URL_BYTES: usize = 2048;
/// The smallest and largest Discord CDN size.
const MIN_SIZE: u16 = 16;
const MAX_SIZE: u16 = 4096;
/// The number of Discord default avatars.
const DEFAULT_AVATARS: u8 = 6;

/// A remote image for an `#[asset]` field of a Template (IMG-03): a user's
/// avatar, a guild's icon, an allow-listed URL or the placeholder.
///
/// The App's `AssetFetcher` fetches every declared image before the render
/// (or reads it from the `AssetStore`); an image that cannot be used draws
/// the configured fallback ([`RemoteConfig::fallback`]) with a warning, or
/// fails the render with `RenderError::RemoteImage` when its field is
/// `#[asset(required)]`. Draw it with `<img src={&self.avatar} />`.
///
/// `Debug` and `Display` print the display form only (`avatar/<user>/<hash>@<size>`,
/// `icon/<guild>/<hash>@<size>`, `default-avatar/<i>`, `<origin><path>`
/// without the query, or `placeholder`).
///
/// ```
/// use noctis_image::RemoteImage;
///
/// let image = RemoteImage::url("https://cdn.discordapp.com/emojis/1.png?size=64")?;
/// assert_eq!(image.to_string(), "https://cdn.discordapp.com/emojis/1.png");
/// assert_eq!(RemoteImage::placeholder().to_string(), "placeholder");
/// # Ok::<(), noctis_image::RemoteUrlError>(())
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RemoteImage {
    source: Source,
}

/// What a [`RemoteImage`] points at. Ids are raw `u64`s and hashes
/// `([u8; 16], animated)`: twilight's `ImageHash` does not round-trip
/// through postcard.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Source {
    Avatar {
        user: u64,
        hash: [u8; 16],
        animated: bool,
        size: u16,
    },
    DefaultAvatar {
        index: u8,
    },
    Icon {
        guild: u64,
        hash: [u8; 16],
        animated: bool,
        size: u16,
    },
    Url(String),
    Placeholder,
}

/// Why [`RemoteImage::url`] rejected a URL (IMG-03).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RemoteUrlError {
    /// The URL is empty.
    #[error("the URL is empty")]
    Empty,
    /// The URL is over 2048 bytes.
    #[error("the URL is {len} bytes: at most 2048")]
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// A byte outside printable ASCII (0x21..=0x7E): a space, a control
    /// character or non-ASCII text.
    #[error("the URL has a byte outside printable ASCII at index {index}")]
    InvalidCharacter {
        /// The index of the first such byte.
        index: usize,
    },
    /// The URL does not start with `https://` or `http://`.
    #[error("the URL must start with `https://` or `http://`")]
    Scheme,
    /// The URL has user info (`user:pass@`).
    #[error("the URL must not have user info")]
    UserInfo,
    /// The URL has a fragment (`#…`).
    #[error("the URL must not have a fragment")]
    Fragment,
    /// The URL has no host.
    #[error("the URL has no host")]
    NoHost,
    /// The URL's port is not 1..=65535.
    #[error("the URL's port must be 1..=65535")]
    Malformed,
    /// The host is not lowercase ASCII letters, digits, `.` and `-`, or an
    /// IP literal.
    #[error("the URL's host must be ASCII letters, digits, `.` and `-`, or an IP literal")]
    InvalidHost,
}

/// `size` as a Discord CDN size: the next power of two, clamped to
/// 16..=4096.
pub(crate) fn normalize_size(size: u16) -> u16 {
    size.clamp(MIN_SIZE, MAX_SIZE).next_power_of_two()
}

impl RemoteImage {
    /// `user`'s avatar at `size` pixels (normalized to the next power of two
    /// in 16..=4096), or their default avatar when they have none. The cache
    /// key holds the user id, the avatar hash, the size and whether an
    /// animated frame set was requested, so a new avatar is a new key. An
    /// animated (`a_`) avatar animates in an animated Template.
    pub fn avatar(user: &User, size: u16) -> Self {
        let source = match user.avatar {
            Some(hash) => Source::Avatar {
                user: user.id.get(),
                hash: hash.bytes(),
                animated: hash.is_animated(),
                size: normalize_size(size),
            },
            None => Source::DefaultAvatar {
                index: default_avatar_index(user.id.get(), user.discriminator),
            },
        };
        Self { source }
    }

    /// `guild`'s icon at `size` pixels (normalized like
    /// [`avatar`](Self::avatar)); the placeholder when it has none.
    pub fn guild_icon(guild: &Guild, size: u16) -> Self {
        Self::icon(guild.id.get(), guild.icon, size)
    }

    /// The icon `hash` of guild `guild`.
    fn icon(guild: u64, hash: Option<ImageHash>, size: u16) -> Self {
        let source = match hash {
            Some(hash) => Source::Icon {
                guild,
                hash: hash.bytes(),
                animated: hash.is_animated(),
                size: normalize_size(size),
            },
            None => Source::Placeholder,
        };
        Self { source }
    }

    /// The image at `url`: `https://` or `http://`, at most 2048 printable
    /// ASCII bytes (also after canonicalization), no user info or fragment.
    /// Its origin must be allow-listed ([`RemoteConfig::allow_origin`]) when
    /// it is resolved, else it draws the fallback; plain `http://` can be
    /// allow-listed only for a loopback host, so any other `http://` URL
    /// draws the fallback.
    ///
    /// # Errors
    ///
    /// A [`RemoteUrlError`] naming the first rule `url` breaks.
    pub fn url(url: &str) -> Result<Self, RemoteUrlError> {
        let canonical = ParsedUrl::parse(url)?.canonical();
        if canonical.len() > MAX_URL_BYTES {
            return Err(RemoteUrlError::TooLong {
                len: canonical.len(),
            });
        }
        Ok(Self {
            source: Source::Url(canonical),
        })
    }

    /// The placeholder: always draws the fallback, with no fetch and no log,
    /// even in a required field.
    pub fn placeholder() -> Self {
        Self {
            source: Source::Placeholder,
        }
    }

    /// Whether this is the placeholder.
    pub(crate) fn is_placeholder(&self) -> bool {
        matches!(self.source, Source::Placeholder)
    }

    /// The URL and cache key of this image for a job that is `animated`,
    /// with the CDN origin `cdn` (no trailing `/`), re-validating decoded
    /// data. `None` for the placeholder.
    ///
    /// # Errors
    ///
    /// Why a value decoded from a job is not a valid image.
    pub(crate) fn target(&self, cdn: &str, animated: bool) -> Result<Option<Target>, String> {
        let target = match &self.source {
            Source::Avatar {
                user,
                hash,
                animated: hash_animated,
                size,
            } => {
                let (hash, size, animate) = (
                    ImageHash::new(*hash, *hash_animated),
                    normalize_size(*size),
                    *hash_animated && animated,
                );
                Target {
                    url: cdn_url(cdn, &format!("avatars/{user}/{hash}"), size, animate),
                    key: format!("img:v1:avatar:{user}:{hash}:{size}:{}", flag(animate)),
                }
            }
            Source::Icon {
                guild,
                hash,
                animated: hash_animated,
                size,
            } => {
                let (hash, size, animate) = (
                    ImageHash::new(*hash, *hash_animated),
                    normalize_size(*size),
                    *hash_animated && animated,
                );
                Target {
                    url: cdn_url(cdn, &format!("icons/{guild}/{hash}"), size, animate),
                    key: format!("img:v1:icon:{guild}:{hash}:{size}:{}", flag(animate)),
                }
            }
            Source::DefaultAvatar { index } => {
                if *index >= DEFAULT_AVATARS {
                    return Err(format!(
                        "default avatar {index} does not exist: use 0..={}",
                        DEFAULT_AVATARS - 1
                    ));
                }
                Target {
                    url: format!("{cdn}/embed/avatars/{index}.png"),
                    key: format!("img:v1:default-avatar:{index}"),
                }
            }
            Source::Url(url) => {
                let canonical = ParsedUrl::parse(url)
                    .map_err(|err| err.to_string())?
                    .canonical();
                Target {
                    key: format!("img:v1:url:{}", blake3::hash(canonical.as_bytes()).to_hex()),
                    url: canonical,
                }
            }
            Source::Placeholder => return Ok(None),
        };
        Ok(Some(target))
    }
}

/// Where a resolved image is fetched from and cached under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The canonical URL.
    pub(crate) url: String,
    /// The `AssetStore` key (`img:v1:…`).
    pub(crate) key: String,
}

/// The cache-key suffix of an animated (`a`) or still (`s`) request.
fn flag(animated: bool) -> char {
    if animated { 'a' } else { 's' }
}

/// `<cdn>/<path>.webp?size=<size>[&animated=true]`.
fn cdn_url(cdn: &str, path: &str, size: u16, animated: bool) -> String {
    let animated = if animated { "&animated=true" } else { "" };
    format!("{cdn}/{path}.webp?size={size}{animated}")
}

/// Discord's default avatar index: `(id >> 22) % 6` for migrated users
/// (discriminator 0), else `discriminator % 5`.
fn default_avatar_index(id: u64, discriminator: u16) -> u8 {
    let index = if discriminator == 0 {
        (id >> 22) % u64::from(DEFAULT_AVATARS)
    } else {
        u64::from(discriminator % 5)
    };
    u8::try_from(index).unwrap_or(0)
}

impl fmt::Display for RemoteImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            Source::Avatar {
                user,
                hash,
                animated,
                size,
            } => write!(
                f,
                "avatar/{user}/{}@{}",
                ImageHash::new(*hash, *animated),
                normalize_size(*size)
            ),
            Source::Icon {
                guild,
                hash,
                animated,
                size,
            } => write!(
                f,
                "icon/{guild}/{}@{}",
                ImageHash::new(*hash, *animated),
                normalize_size(*size)
            ),
            Source::DefaultAvatar { index } => write!(f, "default-avatar/{index}"),
            Source::Url(url) => match ParsedUrl::parse(url) {
                Ok(parsed) => f.write_str(&parsed.display()),
                Err(_) => f.write_str("invalid-url"),
            },
            Source::Placeholder => f.write_str("placeholder"),
        }
    }
}

impl fmt::Debug for RemoteImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RemoteImage({self})")
    }
}

/// `http` or `https`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Scheme {
    Http,
    Https,
}

impl Scheme {
    /// The scheme's name.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }

    /// The port a URL without one uses.
    pub(crate) fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }

    /// The scheme `text` (lowercased) names, with the rest after `://`.
    fn split(text: &str) -> Option<(Self, &str)> {
        let (scheme, rest) = text.split_once("://")?;
        let scheme = if scheme.eq_ignore_ascii_case("https") {
            Self::Https
        } else if scheme.eq_ignore_ascii_case("http") {
            Self::Http
        } else {
            return None;
        };
        Some((scheme, rest))
    }
}

/// A host and an optional port parsed from an authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostPort {
    /// Lowercase; an IPv6 literal keeps its brackets.
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
}

/// Why an authority is not `host[:port]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorityError {
    NoHost,
    Port,
    Host,
}

impl HostPort {
    /// Parses `authority` (no user info): a lowercase-able ASCII host
    /// `[a-z0-9.-]+` or a bracketed IPv6 literal, then an optional
    /// `:<1..=65535>`.
    pub(crate) fn parse(authority: &str) -> Result<Self, AuthorityError> {
        let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
            let (literal, after) = rest.split_once(']').ok_or(AuthorityError::Host)?;
            if literal.parse::<Ipv6Addr>().is_err() {
                return Err(AuthorityError::Host);
            }
            let port = match after {
                "" => None,
                _ => Some(after.strip_prefix(':').ok_or(AuthorityError::Host)?),
            };
            (format!("[{}]", literal.to_ascii_lowercase()), port)
        } else {
            let (host, port) = match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            };
            if host.is_empty() {
                return Err(AuthorityError::NoHost);
            }
            let host = host.to_ascii_lowercase();
            if !host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
            {
                return Err(AuthorityError::Host);
            }
            (host, port)
        };
        let port = match port {
            None => None,
            Some(port) => {
                let valid =
                    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit());
                match port.parse::<u16>() {
                    Ok(port) if valid && port != 0 => Some(port),
                    _ => return Err(AuthorityError::Port),
                }
            }
        };
        Ok(Self { host, port })
    }
}

/// A URL that passed the [`RemoteImage::url`] grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedUrl {
    pub(crate) scheme: Scheme,
    pub(crate) authority: HostPort,
    /// The path and query (from the first `/` or `?`), possibly empty.
    rest: String,
}

impl ParsedUrl {
    /// Checks `url` against the grammar.
    pub(crate) fn parse(url: &str) -> Result<Self, RemoteUrlError> {
        if url.is_empty() {
            return Err(RemoteUrlError::Empty);
        }
        if url.len() > MAX_URL_BYTES {
            return Err(RemoteUrlError::TooLong { len: url.len() });
        }
        if let Some(index) = url.bytes().position(|b| !(0x21..=0x7E).contains(&b)) {
            return Err(RemoteUrlError::InvalidCharacter { index });
        }
        let (scheme, rest) = Scheme::split(url).ok_or(RemoteUrlError::Scheme)?;
        if rest.contains('#') {
            return Err(RemoteUrlError::Fragment);
        }
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, rest) = rest.split_at(end);
        if authority.contains('@') {
            return Err(RemoteUrlError::UserInfo);
        }
        let authority = HostPort::parse(authority).map_err(|err| match err {
            AuthorityError::NoHost => RemoteUrlError::NoHost,
            AuthorityError::Port => RemoteUrlError::Malformed,
            AuthorityError::Host => RemoteUrlError::InvalidHost,
        })?;
        Ok(Self {
            scheme,
            authority,
            rest: rest.to_owned(),
        })
    }

    /// `scheme://host[:port]`, lowercased, with the port as written.
    pub(crate) fn origin_text(&self) -> String {
        match self.authority.port {
            Some(port) => format!("{}://{}:{port}", self.scheme.as_str(), self.authority.host),
            None => format!("{}://{}", self.scheme.as_str(), self.authority.host),
        }
    }

    /// The effective port.
    pub(crate) fn port(&self) -> u16 {
        self.authority
            .port
            .unwrap_or_else(|| self.scheme.default_port())
    }

    /// The canonical URL: the scheme and host lowercased, the path and query
    /// kept (an empty path becomes `/`).
    pub(crate) fn canonical(&self) -> String {
        let slash = if self.rest.starts_with('/') { "" } else { "/" };
        format!("{}{slash}{}", self.origin_text(), self.rest)
    }

    /// The display form: the origin and the path, never the query.
    pub(crate) fn display(&self) -> String {
        let path = self.rest.split('?').next().unwrap_or_default();
        let slash = if path.starts_with('/') { "" } else { "/" };
        format!("{}{slash}{path}", self.origin_text())
    }
}
