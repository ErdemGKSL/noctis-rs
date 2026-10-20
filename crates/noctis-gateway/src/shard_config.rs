//! Building a twilight shard [`Config`] from App settings (AD-21, AD-24).

use tokio::runtime::Handle;
use twilight_gateway::queue::Queue;
use twilight_gateway::{Config, ConfigBuilder, Intents};

/// Why [`shard_config`] could not build a shard configuration. Messages never
/// contain the token or the gateway URL.
///
/// ```
/// use noctis_gateway::ShardConfigError;
///
/// assert!(ShardConfigError::NoRuntime.to_string().contains("tokio runtime"));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShardConfigError {
    /// Called outside a tokio runtime (twilight spawns a queue task).
    #[error("no tokio runtime: build shard configurations inside a tokio runtime")]
    NoRuntime,
    /// The gateway URL is not a `ws://` or `wss://` URL twilight can connect to.
    #[error(
        "invalid gateway URL: use `ws://host[:port]` or `wss://host[:port]`, e.g. through \
         `AppBuilder::gateway_url`"
    )]
    InvalidGatewayUrl,
    /// The TLS connector could not be created.
    #[error("the gateway TLS connector could not be created: check the platform's certificates")]
    Tls(#[source] tokio_websockets::Error),
}

/// Builds the twilight shard [`Config`] for `token`, `intents`, an optional
/// gateway URL (twilight's `proxy_url`) and an identify `queue`.
///
/// `token` may carry a `Bot ` prefix; twilight adds it when missing. A
/// rustls crypto provider must be installed, as `AppBuilder::build` does.
///
/// # Errors
///
/// - [`ShardConfigError::NoRuntime`] outside a tokio runtime;
/// - [`ShardConfigError::InvalidGatewayUrl`] when `gateway_url` does not start
///   with `ws://`/`wss://` or does not parse as a connect URI;
/// - [`ShardConfigError::Tls`] when the TLS connector cannot be created.
///
/// ```no_run
/// use std::time::Duration;
///
/// use twilight_gateway::{queue::InMemoryQueue, Intents};
///
/// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
/// let queue = InMemoryQueue::new(1, 1000, Duration::from_secs(5), 1000);
/// let config = noctis_gateway::shard_config("token", Intents::GUILDS, None, queue)?;
/// assert_eq!(config.token(), "Bot token");
/// # Ok(()) }
/// ```
pub fn shard_config<Q: Queue>(
    token: &str,
    intents: Intents,
    gateway_url: Option<&str>,
    queue: Q,
) -> Result<Config<Q>, ShardConfigError> {
    if Handle::try_current().is_err() {
        return Err(ShardConfigError::NoRuntime);
    }
    if let Some(url) = gateway_url {
        check_gateway_url(url)?;
    }
    tokio_websockets::Connector::new().map_err(ShardConfigError::Tls)?;
    let mut builder = ConfigBuilder::new(token.to_owned(), intents);
    if let Some(url) = gateway_url {
        builder = builder.proxy_url(url.to_owned());
    }
    Ok(builder.queue(queue).build())
}

/// Checks `url` the way twilight uses it at connect time.
fn check_gateway_url(url: &str) -> Result<(), ShardConfigError> {
    let has_scheme = ["ws://", "wss://"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    });
    if !has_scheme {
        return Err(ShardConfigError::InvalidGatewayUrl);
    }
    tokio_websockets::client::Builder::new()
        .uri(&format!("{url}/?v=10&encoding=json"))
        .map(drop)
        .map_err(|_| ShardConfigError::InvalidGatewayUrl)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use twilight_gateway::queue::InMemoryQueue;

    use super::*;

    /// Installs `ring` as the rustls provider, as `AppBuilder::build` does.
    fn crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn queue() -> InMemoryQueue {
        InMemoryQueue::new(1, 1000, Duration::from_secs(5), 1000)
    }

    /// A queue that needs no runtime (`InMemoryQueue::new` spawns a task).
    #[derive(Debug)]
    struct NeverQueue;

    impl Queue for NeverQueue {
        fn enqueue(&self, _: u32) -> tokio::sync::oneshot::Receiver<()> {
            tokio::sync::oneshot::channel().1
        }
    }

    #[test]
    fn no_runtime() {
        let err = shard_config("t", Intents::GUILDS, None, NeverQueue).unwrap_err();
        assert!(matches!(err, ShardConfigError::NoRuntime));
    }

    #[tokio::test]
    async fn without_url() {
        crypto();
        let intents = Intents::GUILDS | Intents::GUILD_MESSAGES;
        let config = shard_config("t", intents, None, queue()).unwrap();
        assert_eq!(config.proxy_url(), None);
        assert_eq!(config.intents(), intents);
        assert_eq!(config.token(), "Bot t");
    }

    #[tokio::test]
    async fn with_url() {
        crypto();
        let config = shard_config("t", Intents::GUILDS, Some("ws://127.0.0.1:1"), queue()).unwrap();
        assert_eq!(config.proxy_url(), Some("ws://127.0.0.1:1"));
    }

    #[tokio::test]
    async fn invalid_url() {
        crypto();
        for url in ["ws://a b", "", "http://p", "w", "ws:/x"] {
            let err = shard_config("SECRET", Intents::GUILDS, Some(url), queue()).unwrap_err();
            assert!(
                matches!(err, ShardConfigError::InvalidGatewayUrl),
                "{url:?}"
            );
            assert!(!err.to_string().contains("SECRET"));
        }
    }
}
