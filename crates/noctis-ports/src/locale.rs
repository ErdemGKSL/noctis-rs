//! [`LocaleSource`]: runtime overlays of content strings.

use crate::{BoxFuture, PortError};

/// Loads runtime overlays of content strings.
pub trait LocaleSource: Send + Sync + 'static {
    /// Loads the overlay for `locale`, a Discord locale code such as `"en-US"`.
    ///
    /// Each pair is (message key, value); an empty `Vec` means no overlay.
    fn load<'a>(
        &'a self,
        locale: &'a str,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>>;
}
