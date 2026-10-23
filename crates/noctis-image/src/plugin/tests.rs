//! `ImageConfig` defaults and setters.

use super::*;

#[test]
fn defaults() {
    let config = ImageConfig::new();
    assert_eq!(config, ImageConfig::default());
    assert!(config.threads >= 1);
    assert!(config.threads <= 256);
    assert_eq!(config.queue_capacity, 64);
    let expected = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
    assert_eq!(config.threads, expected);
}

#[test]
fn setters_replace_one_field() {
    let config = ImageConfig::new().threads(3).queue_capacity(9);
    assert_eq!(
        config.pool_config(),
        PoolConfig {
            threads: 3,
            queue_capacity: 9
        }
    );
    assert_eq!(config.clone().threads(3), config);
}
