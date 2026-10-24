//! [`LocaleSource`]: runtime overlays of content strings.

use crate::{BoxFuture, PortError};

/// Loads runtime overlays of content strings, so translators can fix or
/// add content text without a redeploy.
///
/// `App::run` calls [`load`](Self::load) at startup, before the startup
/// Publish and before any shard connects, once per locale that has a file
/// in the App's merged locale catalogs, one after the other in
/// `Locale::ALL` order. An App without catalogs never calls it, and the
/// CLI Publish modes (`--publish*`) never load overlays. There is no
/// reload: restart the App to pick up edited overlays.
///
/// Overlays apply to content keys only: a key must be a content key of a
/// merged catalog, and its value must use exactly the compiled placeholder
/// set (order and repetition do not matter). Keys of the reserved
/// families `cmd.*`, `cmdword.*`, `ctxcmd.*` and `noctis.*` are rejected,
/// because published command texts change only with a redeploy. A failed
/// load, or any rejected pair, stops startup with a typed error and
/// installs nothing. An accepted value acts as if written in that
/// locale's file of the key's catalog, process-wide.
pub trait LocaleSource: Send + Sync + 'static {
    /// Loads the overlay for `locale`, a Discord locale code such as `"en-US"`.
    ///
    /// Each pair is (message key, value); an empty `Vec` means no overlay.
    ///
    /// # Errors
    ///
    /// A [`PortError`] when the overlay cannot be read; it fails startup.
    fn load<'a>(
        &'a self,
        locale: &'a str,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>>;
}
