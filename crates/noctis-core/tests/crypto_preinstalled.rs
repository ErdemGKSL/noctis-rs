//! Matrix 32: a provider installed before Noctis runs is kept.

use std::sync::Arc;

use rustls::crypto::CryptoProvider;

#[tokio::test(flavor = "multi_thread")]
async fn existing_provider_is_kept() {
    let mut provider = rustls::crypto::ring::default_provider();
    provider.cipher_suites.truncate(1);
    provider.install_default().unwrap();
    let before = Arc::clone(CryptoProvider::get_default().unwrap());
    // Nothing listens on port 1: `run` passes the crypto check, then fails
    // reading GET /gateway/bot without touching the network.
    let app = noctis_core::App::builder()
        .token("abc".parse().unwrap())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    assert!(matches!(
        app.run().await,
        Err(noctis_core::Error::Start(
            noctis_core::StartError::GatewayInfo { status: None }
        ))
    ));
    assert!(Arc::ptr_eq(&before, CryptoProvider::get_default().unwrap()));
    assert_eq!(
        CryptoProvider::get_default().unwrap().cipher_suites.len(),
        1
    );
}
