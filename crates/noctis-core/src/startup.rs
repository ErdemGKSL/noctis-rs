//! Startup safety checks shared by `build()` and `App::run` (AD-21).

use rustls::crypto::CryptoProvider;
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::StartError;

/// No rustls crypto provider is installed and `ring`'s could not be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CryptoProviderUnavailable;

/// Checks that a tokio runtime is current and, with `multi_thread_required`,
/// that it is the multi-thread flavor.
pub(crate) fn check_runtime(multi_thread_required: bool) -> Result<(), StartError> {
    let handle = Handle::try_current().map_err(|_| StartError::NoRuntime)?;
    if multi_thread_required && handle.runtime_flavor() != RuntimeFlavor::MultiThread {
        return Err(StartError::NotMultiThread);
    }
    Ok(())
}

/// Makes sure a process rustls crypto provider exists, installing `ring`'s
/// when none is set. An installed provider is never replaced.
pub(crate) fn ensure_crypto_provider() -> Result<(), CryptoProviderUnavailable> {
    if CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    match rustls::crypto::ring::default_provider().install_default() {
        Ok(()) => Ok(()),
        // Another thread installed one first.
        Err(_) if CryptoProvider::get_default().is_some() => Ok(()),
        Err(_) => Err(CryptoProviderUnavailable),
    }
}

/// The process signals that stop [`App::run`](crate::App::run): SIGINT and
/// SIGTERM on unix, Ctrl-C elsewhere. Registered eagerly by
/// [`Signals::install`], so a signal that arrives during startup is not
/// lost.
pub(crate) struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl Signals {
    /// Registers the handlers. tokio panics when the runtime has no IO
    /// driver, so the registration runs under `catch_unwind`; an error or a
    /// panic is [`StartError::SignalHandler`].
    pub(crate) fn install() -> Result<Self, StartError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(Self::register))
            .map_err(|_| StartError::SignalHandler)?
            .map_err(|error| {
                tracing::error!(target: "noctis::app", %error, "signal handler registration failed");
                StartError::SignalHandler
            })
    }

    #[cfg(unix)]
    fn register() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    #[cfg(not(unix))]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "same signature as the unix registration"
    )]
    fn register() -> std::io::Result<Self> {
        Ok(Self {})
    }

    /// Resolves when the first signal arrives.
    #[cfg(unix)]
    pub(crate) async fn recv(mut self) {
        let which = tokio::select! {
            _ = self.interrupt.recv() => "SIGINT",
            _ = self.terminate.recv() => "SIGTERM",
        };
        tracing::debug!(target: "noctis::app", signal = which, "shutdown signal received");
    }

    /// Resolves when Ctrl-C arrives.
    #[cfg(not(unix))]
    pub(crate) async fn recv(self) {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(target: "noctis::app", %error, "Ctrl-C handler failed");
            std::future::pending::<()>().await;
        }
    }
}
