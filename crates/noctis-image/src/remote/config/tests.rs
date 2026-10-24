//! `RemoteConfig` defaults, ranges and the origin grammar (IMG-03).

use super::*;

fn url(text: &str) -> ParsedUrl {
    ParsedUrl::parse(text).unwrap()
}

fn allowed(config: &RemoteConfig, text: &str) -> bool {
    config
        .origins()
        .iter()
        .any(|origin| origin.matches(&url(text)))
}

#[test]
fn img_03_defaults() {
    let config = RemoteConfig::new();
    assert_eq!(config, RemoteConfig::default());
    assert_eq!(config.timeout_value(), Duration::from_secs(5));
    assert_eq!(config.max_bytes_value(), 8_388_608);
    assert_eq!(config.concurrency_value(), 8);
    assert_eq!(config.max_per_job_value(), 16);
    assert_eq!(config.max_pixels_value(), 4_194_304);
    assert_eq!(config.cache_ttl_value(), Duration::from_hours(24));
    assert_eq!(config.fallback_image(), None);
    assert_eq!(config.cdn(), "https://cdn.discordapp.com");
    let origins: Vec<String> = config.origins().iter().map(ToString::to_string).collect();
    assert_eq!(
        origins,
        ["https://cdn.discordapp.com", "https://media.discordapp.net"]
    );
    assert!(config.validate().is_empty());
}

#[test]
fn img_03_origins_accepted_and_deduped() {
    let mut config = RemoteConfig::new();
    for origin in [
        "https://i.imgur.com",
        "https://i.imgur.com/",
        "https://h:8443",
        "http://127.0.0.1:8080",
        "http://[::1]:1",
        "http://localhost:3000",
        "https://cdn.discordapp.com:443",
    ] {
        assert!(Origin::parse(origin).is_ok(), "{origin}");
        config = config.allow_origin(origin);
    }
    assert!(config.validate().is_empty());
    let origins: Vec<String> = config.origins().iter().map(ToString::to_string).collect();
    assert_eq!(
        origins,
        [
            "https://cdn.discordapp.com",
            "https://media.discordapp.net",
            "https://i.imgur.com",
            "https://h:8443",
            "http://127.0.0.1:8080",
            "http://[::1]:1",
            "http://localhost:3000",
        ]
    );
}

#[test]
fn img_03_invalid_origins_are_violations() {
    let cases = [
        ("", "it is empty"),
        ("i.imgur.com", "it must start with `https://`"),
        (
            "ftp://x",
            "the scheme must be `https` (or `http` for a loopback host)",
        ),
        ("https://h/p", "it must not have a path"),
        ("https://h?q", "it must not have a query or fragment"),
        ("https://u@h", "it must not have user info"),
        (
            "https://*.h",
            "the host must be ASCII letters, digits, `.` and `-` (no wildcard), or an IP literal",
        ),
        (
            "http://example.com",
            "plain http is only for loopback hosts",
        ),
    ];
    let mut config = RemoteConfig::new();
    for (origin, _) in cases {
        config = config.allow_origin(origin);
    }
    let expected: Vec<String> = cases
        .iter()
        .map(|(origin, reason)| format!("[IMG-03] remote origin `{origin}` is invalid: {reason}"))
        .collect();
    assert_eq!(config.validate(), expected);
    // Invalid entries never reach the allow-list.
    assert_eq!(config.origins().len(), 2);
    let cdn = RemoteConfig::new().discord_cdn("http://cdn.example");
    assert_eq!(
        cdn.validate(),
        [
            "[IMG-03] remote origin `http://cdn.example` is invalid: plain http is only for loopback hosts"
        ]
    );
}

#[test]
fn img_03_matching_is_exact() {
    let config = RemoteConfig::new();
    assert!(allowed(&config, "https://cdn.discordapp.com/x"));
    assert!(allowed(&config, "https://cdn.discordapp.com:443/x"));
    assert!(allowed(&config, "HTTPS://CDN.DISCORDAPP.COM/x"));
    for text in [
        "https://cdn.discordapp.com:444/x",
        "http://cdn.discordapp.com/x",
        "https://cdn.discordapp.com.evil.com/x",
        "https://example.com/x.png",
    ] {
        assert!(!allowed(&config, text), "{text}");
    }
    let local = RemoteConfig::new().discord_cdn("http://127.0.0.1:9000/");
    assert_eq!(local.cdn(), "http://127.0.0.1:9000");
    assert!(allowed(&local, "http://127.0.0.1:9000/avatars/1/x.webp"));
    assert!(!allowed(&local, "http://127.0.0.1:9001/x"));
    assert!(!allowed(&local, "http://localhost:9000/x"));
}

#[test]
fn img_03_config_ranges() {
    let base = RemoteConfig::new;
    let inside = [
        base().timeout(Duration::from_millis(100)),
        base().timeout(Duration::from_mins(1)),
        base().max_bytes(1024),
        base().max_bytes(67_108_864),
        base().concurrency(1),
        base().concurrency(256),
        base().max_per_job(1),
        base().max_per_job(64),
        base().max_pixels(1),
        base().max_pixels(16_777_216),
        base().cache_ttl(Duration::from_secs(1)),
        base().cache_ttl(Duration::from_hours(30 * 24)),
    ];
    for config in inside {
        assert!(config.validate().is_empty(), "{config:?}");
    }
    let outside = [
        (
            base().timeout(Duration::from_millis(99)),
            "timeout 99ms",
            "100ms..=60s",
        ),
        (
            base().timeout(Duration::from_secs(61)),
            "timeout 61s",
            "100ms..=60s",
        ),
        (base().max_bytes(1023), "max_bytes 1023", "1024..=67108864"),
        (
            base().max_bytes(67_108_865),
            "max_bytes 67108865",
            "1024..=67108864",
        ),
        (base().concurrency(0), "concurrency 0", "1..=256"),
        (base().concurrency(257), "concurrency 257", "1..=256"),
        (base().max_per_job(0), "max_per_job 0", "1..=64"),
        (base().max_per_job(65), "max_per_job 65", "1..=64"),
        (base().max_pixels(0), "max_pixels 0", "1..=16777216"),
        (
            base().max_pixels(16_777_217),
            "max_pixels 16777217",
            "1..=16777216",
        ),
        (
            base().cache_ttl(Duration::ZERO),
            "cache_ttl 0ns",
            "1s..=2592000s (30 days)",
        ),
        (
            base().cache_ttl(Duration::from_secs(30 * 86_400 + 1)),
            "cache_ttl 2592001s",
            "1s..=2592000s (30 days)",
        ),
    ];
    for (config, value, range) in outside {
        assert_eq!(
            config.validate(),
            [format!(
                "[IMG-03] remote {value} is out of range: use {range}"
            )]
        );
    }
}
