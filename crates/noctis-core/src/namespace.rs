//! App ids and port namespacing (FR-71, AD-14).
//!
//! Core wraps each of the seven byte-store ports in a [`Namespaced`] wrapper
//! that prefixes every key with `<app>:<port>:`, or `_shared:<port>:` for a
//! port passed to its setter as [`Shared::new(adapter)`](Shared::new).
//! Adapters store keys verbatim and never add a prefix.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use noctis_ports::{
    AssetStore, BoxFuture, CacheStore, ModuleStateStore, PortError, RateLimitStore, RefStore,
    RenderCache, SessionStore,
};
use twilight_model::id::Id;
use twilight_model::id::marker::ApplicationMarker;

/// The most characters an [`AppId`] may have.
const MAX_LEN: usize = 32;

/// The namespace segment of a [`Shared`] port; `_` is outside the
/// [`AppId`] alphabet, so it never equals an App id.
const SHARED_SEGMENT: &str = "_shared";

/// An App's id: 1-32 characters of `a-z`, `0-9` and `-`
/// (`[a-z0-9-]{1,32}`).
///
/// It is the `app` label of every span, metric and Error Sink report, and
/// the first segment of every port key (`<app>:<port>:<key>`), so two Apps
/// sharing one Adapter never see each other's data. Set it with
/// `AppBuilder::app_id` or `NOCTIS_APP_ID`; by default it is the bot's
/// application id in base36 ([`AppId::from_application_id`]), read from the
/// token.
///
/// ```
/// use noctis_core::{AppId, AppIdError};
///
/// let id = AppId::new("my-bot")?;
/// assert_eq!(id.as_str(), "my-bot");
/// assert_eq!(id.to_string(), "my-bot");
/// assert_eq!(format!("{id:?}"), r#"AppId("my-bot")"#);
/// assert_eq!(AppId::new("My-bot"), Err(AppIdError::InvalidChar { index: 0, ch: 'M' }));
/// # Ok::<(), AppIdError>(())
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct AppId(Arc<str>);

impl AppId {
    /// Validates `id` against `[a-z0-9-]{1,32}`. The input is never trimmed.
    ///
    /// # Errors
    ///
    /// [`AppIdError::Empty`] for `""`, [`AppIdError::TooLong`] for more than
    /// 32 characters, then [`AppIdError::InvalidChar`] for the first
    /// character outside `a-z`, `0-9` and `-`.
    ///
    /// ```
    /// use noctis_core::{AppId, AppIdError};
    ///
    /// assert!(AppId::new("-").is_ok());
    /// assert_eq!(AppId::new(""), Err(AppIdError::Empty));
    /// assert_eq!(AppId::new(&"a".repeat(33)), Err(AppIdError::TooLong { len: 33 }));
    /// ```
    pub fn new(id: &str) -> Result<Self, AppIdError> {
        if id.is_empty() {
            return Err(AppIdError::Empty);
        }
        let len = id.chars().count();
        if len > MAX_LEN {
            return Err(AppIdError::TooLong { len });
        }
        if let Some((index, ch)) = id
            .chars()
            .enumerate()
            .find(|&(_, c)| !matches!(c, 'a'..='z' | '0'..='9' | '-'))
        {
            return Err(AppIdError::InvalidChar { index, ch });
        }
        Ok(Self(Arc::from(id)))
    }

    /// The application id in lowercase base36 (`0-9a-z`, most significant
    /// digit first, at most 13 characters): the default App id.
    ///
    /// ```
    /// use noctis_core::AppId;
    /// use twilight_model::id::Id;
    ///
    /// assert_eq!(AppId::from_application_id(Id::new(36)).as_str(), "10");
    /// assert_eq!(
    ///     AppId::from_application_id(Id::new(1_234_567_890_123_456_789)).as_str(),
    ///     "9do1sj396nf9",
    /// );
    /// ```
    pub fn from_application_id(id: Id<ApplicationMarker>) -> Self {
        let mut digits = Vec::with_capacity(13);
        let mut rest = id.get();
        loop {
            // `rest % 36` is a base36 digit, so both conversions succeed.
            let digit = u32::try_from(rest % 36)
                .ok()
                .and_then(|d| char::from_digit(d, 36))
                .unwrap_or('0');
            digits.push(digit);
            rest /= 36;
            if rest == 0 {
                break;
            }
        }
        Self(digits.into_iter().rev().collect::<String>().into())
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The App id when none is set and none derives from the token.
pub(crate) fn fallback_app_id() -> AppId {
    AppId(Arc::from("default"))
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AppId").field(&self.as_str()).finish()
    }
}

impl FromStr for AppId {
    type Err = AppIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Why an [`AppId`] was rejected.
///
/// ```
/// use noctis_core::AppIdError;
///
/// let err = AppIdError::InvalidChar { index: 1, ch: '_' };
/// assert!(err.to_string().contains("[a-z0-9-]{1,32}"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppIdError {
    /// The id is empty.
    #[error("App id is empty: use 1-32 characters matching [a-z0-9-]{{1,32}}, such as `my-bot`")]
    Empty,
    /// The id has more than 32 characters.
    #[error("App id has {len} characters: shorten it to at most 32 matching [a-z0-9-]{{1,32}}")]
    TooLong {
        /// The number of characters.
        len: usize,
    },
    /// The id has a character outside `a-z`, `0-9` and `-`.
    #[error("App id has `{ch}` at index {index}: use only a-z, 0-9 and - ([a-z0-9-]{{1,32}})")]
    InvalidChar {
        /// The character index of the first invalid character.
        index: usize,
        /// The invalid character.
        ch: char,
    },
}

/// Escapes one namespace segment: `%` becomes `%25`, then `:` becomes
/// `%3A`; every other character is kept.
pub(crate) fn escape_segment(segment: &str) -> Cow<'_, str> {
    if !segment.contains(['%', ':']) {
        return Cow::Borrowed(segment);
    }
    Cow::Owned(segment.replace('%', "%25").replace(':', "%3A"))
}

mod sealed {
    /// Seals [`NamespacedPort`](super::NamespacedPort).
    pub trait Port {}

    /// Seals [`IntoPort`](super::IntoPort) per port.
    pub trait PortInput<P: ?Sized> {}
}

/// A byte-store port that core namespaces: one of the seven `dyn Port`
/// types (`dyn RefStore`, `dyn SessionStore`, `dyn ModuleStateStore`,
/// `dyn RateLimitStore`, `dyn CacheStore`, `dyn RenderCache` and
/// `dyn AssetStore`). Sealed.
pub trait NamespacedPort: sealed::Port + Send + Sync + 'static {
    /// The port segment of the key prefix (`cache` for `dyn CacheStore`).
    const SEGMENT: &'static str;
}

/// A byte-store port with every key prefixed by its namespace:
/// `<app>:<port>:` ([`Namespaced::new`]) or `_shared:<port>:`
/// ([`Namespaced::shared`]).
///
/// Every op forwards exactly one call to the inner port with the key
/// `prefix + key`; the logical key is appended verbatim, and values, TTLs,
/// return values and errors pass through unchanged.
///
/// ```
/// use std::sync::Arc;
///
/// use noctis_core::{AppId, Namespaced};
/// use noctis_ports::CacheStore;
/// use noctis_ports::memory::MemoryCacheStore;
///
/// let inner: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new());
/// let ns = Namespaced::new(&AppId::new("bot-a")?, inner);
/// assert_eq!(ns.prefix(), "bot-a:cache:");
/// # Ok::<(), noctis_core::AppIdError>(())
/// ```
pub struct Namespaced<P: ?Sized + NamespacedPort> {
    prefix: Box<str>,
    inner: Arc<P>,
}

impl<P: ?Sized + NamespacedPort> Namespaced<P> {
    /// `inner` under the App's namespace: `<app>:<port>:`.
    pub fn new(app: &AppId, inner: Arc<P>) -> Self {
        Self::with_namespace(app.as_str(), inner)
    }

    /// `inner` under the shared namespace: `_shared:<port>:`, the same for
    /// every App.
    pub fn shared(inner: Arc<P>) -> Self {
        Self::with_namespace(SHARED_SEGMENT, inner)
    }

    fn with_namespace(namespace: &str, inner: Arc<P>) -> Self {
        let prefix = format!(
            "{}:{}:",
            escape_segment(namespace),
            escape_segment(P::SEGMENT)
        );
        Self {
            prefix: prefix.into_boxed_str(),
            inner,
        }
    }

    /// The key prefix, such as `"bot-a:cache:"`.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The full key the inner port sees for `key`.
    fn key(&self, key: &str) -> String {
        let mut full = String::with_capacity(self.prefix.len() + key.len());
        full.push_str(&self.prefix);
        full.push_str(key);
        full
    }
}

impl<P: ?Sized + NamespacedPort> fmt::Debug for Namespaced<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Namespaced")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// Forwards one op to `inner` with the prefixed key.
macro_rules! namespaced_op {
    (get) => {
        fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.get(&key).await })
        }
    };
    (put) => {
        fn put<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<(), PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.put(&key, value, ttl).await })
        }
    };
    (put_if_absent) => {
        fn put_if_absent<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<bool, PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.put_if_absent(&key, value, ttl).await })
        }
    };
    (take) => {
        fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.take(&key).await })
        }
    };
    (delete) => {
        fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.delete(&key).await })
        }
    };
    (touch) => {
        fn touch<'a>(
            &'a self,
            key: &'a str,
            ttl: Duration,
        ) -> BoxFuture<'a, Result<bool, PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.touch(&key, ttl).await })
        }
    };
    (incr_with_ttl) => {
        fn incr_with_ttl<'a>(
            &'a self,
            key: &'a str,
            by: u64,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<u64, PortError>> {
            let key = self.key(key);
            Box::pin(async move { self.inner.incr_with_ttl(&key, by, ttl).await })
        }
    };
    (version) => {
        fn version(&self) -> BoxFuture<'_, Result<u64, PortError>> {
            self.inner.version()
        }
    };
}

/// Implements `NamespacedPort` and `IntoPort` for each byte-store port, and
/// the port itself for its `Namespaced` wrapper.
macro_rules! namespaced_ports {
    ($($port:ident = $segment:literal: $($op:ident),+;)*) => {$(
        impl sealed::Port for dyn $port {}

        impl NamespacedPort for dyn $port {
            const SEGMENT: &'static str = $segment;
        }

        impl $port for Namespaced<dyn $port> {
            $(namespaced_op!($op);)+
        }

        impl<T: $port> sealed::PortInput<dyn $port> for T {}

        impl<T: $port> IntoPort<dyn $port> for T {
            fn into_port(self) -> (Arc<dyn $port>, bool) {
                (Arc::new(self), false)
            }
        }

        impl<T: $port> sealed::PortInput<dyn $port> for Shared<T> {}

        impl<T: $port> IntoPort<dyn $port> for Shared<T> {
            fn into_port(self) -> (Arc<dyn $port>, bool) {
                (Arc::new(self.0), true)
            }
        }
    )*};
}

namespaced_ports! {
    RefStore = "ref": put_if_absent, get, take, delete, touch;
    SessionStore = "session": get, put, delete;
    ModuleStateStore = "module_state": get, put, version;
    RateLimitStore = "rate_limit": incr_with_ttl, get, put;
    CacheStore = "cache": get, put, put_if_absent, delete, incr_with_ttl;
    RenderCache = "render_cache": get, put;
    AssetStore = "asset": get, put;
}

/// Marks a byte-store Adapter as shared between Apps: pass
/// `Shared::new(adapter)` to the normal port setter, and its keys get the
/// prefix `_shared:<port>:` instead of `<app>:<port>:`, so every App that
/// shares it reads and writes the same entries.
///
/// It implements no port trait; only the seven byte-store setters accept
/// it.
///
/// ```
/// use std::sync::Arc;
///
/// use noctis_core::Shared;
/// use noctis_ports::memory::MemoryCacheStore;
///
/// let store = Arc::new(MemoryCacheStore::new());
/// let mut a = noctis_core::App::builder();
/// a.app_id("bot-a").cache_store(Shared::new(Arc::clone(&store)));
/// let mut b = noctis_core::App::builder();
/// b.app_id("bot-b").cache_store(Shared::new(store));
/// ```
#[derive(Debug, Clone)]
pub struct Shared<P>(P);

impl<P> Shared<P> {
    /// Marks `port` as shared.
    pub fn new(port: P) -> Self {
        Self(port)
    }

    /// The wrapped port.
    pub fn into_inner(self) -> P {
        self.0
    }
}

/// What a byte-store port setter accepts: any implementation of the port
/// `P` (namespaced by the App id), or one wrapped in [`Shared`] (namespaced
/// as `_shared`). Sealed.
pub trait IntoPort<P: ?Sized>: sealed::PortInput<P> {
    /// The port, and whether it is shared.
    #[doc(hidden)]
    fn into_port(self) -> (Arc<P>, bool);
}

/// Wraps a resolved port (from [`IntoPort::into_port`]) in its namespace.
pub(crate) fn namespace<P: ?Sized + NamespacedPort>(
    app: &AppId,
    (inner, shared): (Arc<P>, bool),
) -> Arc<Namespaced<P>> {
    Arc::new(if shared {
        Namespaced::shared(inner)
    } else {
        Namespaced::new(app, inner)
    })
}

#[cfg(test)]
mod tests;
