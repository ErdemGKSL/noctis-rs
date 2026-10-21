//! [`Kv`]: the typed key-value handle over the App's namespaced
//! `CacheStore` (FR-38, AD-1).

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use noctis_ports::{CacheStore, PortError, decode_value, encode_value};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Typed key-value data for a bot and its Modules, from `ctx.kv()`.
///
/// It wraps the App's `CacheStore`, so every key lands under
/// `<app>:cache:<key>` (or `_shared:cache:<key>` for a store passed as
/// `Shared::new(..)`). Values are postcard with a leading schema byte
/// ([`schema`](Self::schema), default 0): a value written under another
/// schema reads as a miss. Counters are 8-byte big-endian `u64`s. The store
/// is a cache: it may evict.
///
/// Each method makes exactly one `CacheStore` call. Cheap to clone.
///
/// ```
/// use noctis_core::{ComponentCtx, HandlerResult};
///
/// async fn clicks(ctx: ComponentCtx) -> HandlerResult<u64> {
///     let kv = ctx.kv();
///     kv.put("last", &ctx.user_id().get(), None).await?;
///     Ok(kv.incr("clicks", 1, None).await?)
/// }
/// ```
#[derive(Clone)]
pub struct Kv {
    /// The App's namespaced store; `None` without an App.
    store: Option<Arc<dyn CacheStore>>,
    schema: u8,
}

impl Kv {
    /// A handle over the App's namespaced `store`.
    pub(crate) fn new(store: Arc<dyn CacheStore>) -> Self {
        Self {
            store: Some(store),
            schema: 0,
        }
    }

    /// A handle without an App (the detached `__testing` seams): every op
    /// is [`PortError::Unavailable`].
    pub(crate) fn detached() -> Self {
        Self {
            store: None,
            schema: 0,
        }
    }

    /// The handle of the App behind `app`, or a detached one.
    pub(crate) fn of(app: Option<&Arc<crate::handler::AppShared>>) -> Self {
        app.map_or_else(Self::detached, |app| Self::new(Arc::clone(&app.kv)))
    }

    /// The same handle with the schema byte `schema` for typed ops; bump it
    /// when a stored type changes incompatibly, and old entries read as
    /// misses.
    #[must_use]
    pub fn schema(mut self, schema: u8) -> Self {
        self.schema = schema;
        self
    }

    fn store(&self) -> Result<&Arc<dyn CacheStore>, PortError> {
        self.store
            .as_ref()
            .ok_or_else(|| PortError::unavailable("this context has no App"))
    }

    /// Reads the value under `key`.
    ///
    /// A missing key and a value written under another schema are
    /// `Ok(None)`.
    ///
    /// # Errors
    ///
    /// The store's [`PortError`], or [`PortError::Value`] when the stored
    /// bytes are empty, malformed or have trailing bytes.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, PortError> {
        let Some(raw) = self.store()?.get(key).await? else {
            return Ok(None);
        };
        Ok(decode_value(self.schema, &raw)?)
    }

    /// Stores `value` under `key`, overwriting any value and resetting the
    /// TTL (`None` = no expiry).
    ///
    /// The value is encoded before the call, so the future does not borrow
    /// it (and is `Send` for any `T`).
    ///
    /// # Errors
    ///
    /// [`PortError::Value`] with
    /// [`ValueError::Serialize`](noctis_ports::ValueError::Serialize) when `value`
    /// cannot be serialized (no store call is made), else the store's
    /// [`PortError`].
    pub fn put<'a, T: Serialize + ?Sized>(
        &'a self,
        key: &'a str,
        value: &T,
        ttl: Option<Duration>,
    ) -> impl Future<Output = Result<(), PortError>> + Send + 'a {
        let encoded = encode_value(self.schema, value);
        async move {
            let bytes = encoded?;
            self.store()?.put(key, bytes, ttl).await
        }
    }

    /// Stores `value` under `key` only if no live entry exists: `true` only
    /// for the one caller that inserted, even under concurrency.
    ///
    /// # Errors
    ///
    /// As [`put`](Self::put).
    pub fn put_if_absent<'a, T: Serialize + ?Sized>(
        &'a self,
        key: &'a str,
        value: &T,
        ttl: Option<Duration>,
    ) -> impl Future<Output = Result<bool, PortError>> + Send + 'a {
        let encoded = encode_value(self.schema, value);
        async move {
            let bytes = encoded?;
            self.store()?.put_if_absent(key, bytes, ttl).await
        }
    }

    /// Atomically adds `by` to the counter under `key` and returns the new
    /// value (saturating at `u64::MAX`). A missing counter is created with
    /// `by` and `ttl`; an existing one keeps its expiry.
    ///
    /// # Errors
    ///
    /// The store's [`PortError`]; [`PortError::InvalidValue`] when the value
    /// under `key` is not an 8-byte counter.
    pub async fn incr(&self, key: &str, by: u64, ttl: Option<Duration>) -> Result<u64, PortError> {
        self.store()?.incr_with_ttl(key, by, ttl).await
    }

    /// Reads the counter under `key`: 0 when it is missing.
    ///
    /// # Errors
    ///
    /// The store's [`PortError`]; [`PortError::InvalidValue`] (with the
    /// logical `key`) when the value is not exactly 8 bytes.
    pub async fn counter(&self, key: &str) -> Result<u64, PortError> {
        let Some(raw) = self.store()?.get(key).await? else {
            return Ok(0);
        };
        let bytes: [u8; 8] = raw
            .as_ref()
            .try_into()
            .map_err(|_| PortError::invalid_value(key, "not an 8-byte counter"))?;
        Ok(u64::from_be_bytes(bytes))
    }

    /// Removes the entry under `key`; a missing key is not an error.
    ///
    /// # Errors
    ///
    /// The store's [`PortError`].
    pub async fn delete(&self, key: &str) -> Result<(), PortError> {
        self.store()?.delete(key).await
    }
}

impl fmt::Debug for Kv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kv")
            .field("schema", &self.schema)
            .field("attached", &self.store.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests;
