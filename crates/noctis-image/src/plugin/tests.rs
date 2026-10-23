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

#[test]
fn budget_defaults_and_setters() {
    let config = ImageConfig::new();
    assert_eq!(config.size_target, DEFAULT_SIZE_TARGET);
    assert_eq!(DEFAULT_SIZE_TARGET, 2_097_152);
    assert!(config.still_fallback);
    let changed = ImageConfig::new().size_target(4096).still_fallback(false);
    assert_eq!((changed.size_target, changed.still_fallback), (4096, false));
    // The size budget is not part of the pool config, so such Apps share a pool.
    assert_eq!(changed.pool_config(), config.pool_config());
}

#[test]
fn caps_default_and_stay_out_of_the_pool_config() {
    let config = ImageConfig::new();
    assert_eq!(config.caps, RenderCaps::new());
    let changed = ImageConfig::new().caps(RenderCaps::new().frames(200));
    assert_eq!(changed.caps, RenderCaps::new().frames(200));
    assert_ne!(changed, config);
    assert_eq!(changed.pool_config(), config.pool_config());
}
