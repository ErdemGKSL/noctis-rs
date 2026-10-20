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
