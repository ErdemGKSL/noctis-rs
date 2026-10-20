//! Matrix 32: in a fresh process with no provider, building installs `ring`'s,
//! and a second ensure is a no-op.

use std::sync::Arc;

use rustls::crypto::CryptoProvider;

#[tokio::test]
async fn build_installs_ring_provider_once() {
    assert!(CryptoProvider::get_default().is_none());
    noctis_core::App::builder().build().unwrap();
    let installed = Arc::clone(CryptoProvider::get_default().unwrap());
    noctis_core::App::builder().build().unwrap();
    assert!(Arc::ptr_eq(
        &installed,
        CryptoProvider::get_default().unwrap()
    ));
}
