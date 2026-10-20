//! Typed errors of App building and startup (AD-13). No message ever contains
//! the bot token.

use std::any::type_name;
use std::fmt;
use std::time::Duration;

use crate::{HttpProxyError, Plugin, ResourceConflict, TokenError};

/// Every error `App::from_env`, `build` and `run` return.
///
/// ```no_run
/// use noctis_core::{App, Error};
///
/// # #[tokio::main] async fn main() -> Result<(), Error> {
/// let app = App::from_env()?.build()?;
/// app.run().await?;
/// # Ok(()) }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A `NOCTIS_*` environment variable is invalid.
    #[error(transparent)]
    Env(#[from] EnvError),
    /// `AppBuilder::build` found violations.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// `App::run` could not start.
    #[error(transparent)]
    Start(#[from] StartError),
}

/// Every violation one `AppBuilder::build` found, in the order recorded:
/// setters, plugins' reports, then the build checks. Never empty.
///
/// ```
/// use noctis_core::{App, BuildViolation};
///
/// // Outside a tokio runtime, and with an out-of-range timeout.
/// let err = App::builder()
///     .http_timeout(std::time::Duration::ZERO)
///     .build()
///     .unwrap_err();
/// assert_eq!(err.violations().len(), 2);
/// assert!(matches!(err.violations()[1], BuildViolation::NoTokioRuntime));
/// assert!(err.to_string().starts_with("2 App-build violation(s)\n  - "));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", Bullets(violations))]
pub struct BuildError {
    violations: Vec<BuildViolation>,
}

/// `"{n} App-build violation(s)"`, then one `"\n  - {violation}"` per entry.
struct Bullets<'a>(&'a [BuildViolation]);

impl fmt::Display for Bullets<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} App-build violation(s)", self.0.len())?;
        self.0.iter().try_for_each(|v| write!(f, "\n  - {v}"))
    }
}

impl BuildError {
    /// `violations` must not be empty.
    pub(crate) fn new(violations: Vec<BuildViolation>) -> Self {
        debug_assert!(!violations.is_empty(), "a BuildError is never empty");
        Self { violations }
    }

    /// The violations, in recorded order.
    ///
    /// ```
    /// let err = noctis_core::App::builder().build().unwrap_err();
    /// assert_eq!(err.violations().len(), 1);
    /// ```
    pub fn violations(&self) -> &[BuildViolation] {
        &self.violations
    }

    /// The violations, by value.
    ///
    /// ```
    /// let err = noctis_core::App::builder().build().unwrap_err();
    /// assert_eq!(err.into_violations().len(), 1);
    /// ```
    pub fn into_violations(self) -> Vec<BuildViolation> {
        self.violations
    }
}

/// One App-build violation.
///
/// ```
/// use noctis_core::BuildViolation;
///
/// let v = BuildViolation::NoTokioRuntime;
/// assert!(v.to_string().contains("#[tokio::main]"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildViolation {
    /// `build()` ran outside a tokio runtime.
    #[error(
        "no tokio runtime: call `build()` inside a tokio runtime, e.g. from `#[tokio::main]` \
         or `tokio::runtime::Runtime::block_on`"
    )]
    NoTokioRuntime,
    /// No rustls crypto provider was set and `ring`'s could not be installed.
    #[error(
        "no rustls crypto provider could be installed: install one before building with \
         `rustls::crypto::ring::default_provider().install_default()`"
    )]
    CryptoProvider,
    /// Two `.state(..)` values of one type; the first is kept.
    #[error(
        "state type `{type_name}` was registered twice: register one value per type, or wrap \
         the second in a newtype"
    )]
    DuplicateState {
        /// The state's type name.
        type_name: &'static str,
    },
    /// Two plugins of one type; the first is kept.
    #[error(
        "plugin `{type_name}` was added twice: add it once; before `build`, \
         change the added one with `AppBuilder::plugin_mut`"
    )]
    DuplicatePlugin {
        /// The plugin's type name.
        type_name: &'static str,
    },
    /// A process resource was requested with a conflicting configuration.
    #[error(transparent)]
    ResourceConflict(#[from] ResourceConflict),
    /// `http_timeout` is outside 1 ms..=300 s.
    #[error("HTTP timeout {actual:?} is out of range: use a value in 1 ms..=300 s")]
    InvalidHttpTimeout {
        /// The rejected timeout.
        actual: Duration,
    },
    /// `http_proxy` is not `host[:port]` or `http://host[:port]`; the message
    /// never contains the value.
    #[error("invalid HTTP proxy: {reason}")]
    InvalidHttpProxy {
        /// Why the proxy was rejected.
        reason: HttpProxyError,
    },
    /// A plugin reported a problem with [`AppBuilder::report`](crate::AppBuilder::report).
    #[error("plugin `{plugin}`: {message}")]
    Plugin {
        /// The plugin's type name.
        plugin: &'static str,
        /// What is wrong and how to fix it.
        message: String,
    },
}

impl BuildViolation {
    /// A [`BuildViolation::Plugin`] naming plugin `P`.
    ///
    /// ```
    /// use noctis_core::{AppBuilder, BuildViolation, Plugin};
    ///
    /// struct Fonts;
    /// impl Plugin for Fonts {
    ///     fn install(&self, app: &mut AppBuilder) {
    ///         app.report(BuildViolation::plugin::<Self>("no font configured: call .font(..)"));
    ///     }
    /// }
    /// ```
    pub fn plugin<P: Plugin>(message: impl Into<String>) -> Self {
        Self::Plugin {
            plugin: type_name::<P>(),
            message: message.into(),
        }
    }
}

/// Why `App::run` could not start.
///
/// ```
/// use noctis_core::StartError;
///
/// assert!(StartError::NotMultiThread.to_string().contains("new_multi_thread"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StartError {
    /// `App::run` was polled outside a tokio runtime.
    #[error(
        "`App::run` was called outside a tokio runtime: run it from `#[tokio::main]` \
         (multi-thread)"
    )]
    NoRuntime,
    /// The current tokio runtime is not the multi-thread flavor.
    #[error(
        "`App::run` needs a multi-thread tokio runtime: use #[tokio::main] (multi-thread) or \
         Builder::new_multi_thread()"
    )]
    NotMultiThread,
    /// No rustls crypto provider was set and `ring`'s could not be installed.
    #[error(
        "no rustls crypto provider could be installed: install one before `App::run` with \
         `rustls::crypto::ring::default_provider().install_default()`"
    )]
    CryptoProvider,
    /// The App has no bot token.
    #[error("no bot token: set NOCTIS_TOKEN and use `App::from_env()`, or call `.token(..)`")]
    MissingToken,
}

/// A `NOCTIS_*` environment variable `App::from_env` could not use. The
/// message never contains the value.
///
/// ```
/// use noctis_core::{EnvError, TokenError};
///
/// let err = EnvError::InvalidToken { var: "NOCTIS_TOKEN", source: TokenError::Empty };
/// assert!(err.to_string().contains("NOCTIS_TOKEN"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EnvError {
    /// The variable is not valid Unicode.
    #[error("environment variable `{var}` is not valid Unicode: set it to the plain value")]
    NotUnicode {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable holds an invalid bot token.
    #[error(
        "environment variable `{var}` holds an invalid bot token: copy the token again from the \
         Discord Developer Portal"
    )]
    InvalidToken {
        /// The variable's name.
        var: &'static str,
        /// Why the token was rejected.
        #[source]
        source: TokenError,
    },
}
