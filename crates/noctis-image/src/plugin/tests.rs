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

static INTER: &[u8] = include_bytes!("../../fonts/InterVariable.woff2");
static JP400: &[u8] =
    include_bytes!("../../tests/fixtures/fonts/noto-sans-jp-119-400-normal.woff2");

fn fonts_config(fonts: Vec<Font>) -> FontsConfig {
    FontsConfig {
        fonts,
        bundled: cfg!(feature = "default-font"),
        emoji: cfg!(feature = "emoji"),
    }
}

fn build(
    runtime: &noctis_core::Runtime,
    config: ImageConfig,
) -> Result<(), noctis_core::BuildError> {
    let mut builder = noctis_core::App::builder();
    builder.runtime(runtime).image(config.threads(1));
    builder.build().map(drop)
}

#[test]
fn fonts_and_images_setters_append_in_order() {
    const LOGO: StaticImage = StaticImage::new("logo", b"");
    let a = Font::from_static(INTER);
    let b = Font::from_static(JP400);
    let config = ImageConfig::new().font(a.clone()).font(b.clone());
    assert_eq!(config.fonts, [a.clone(), b.clone()]);
    assert_ne!(config, ImageConfig::new().font(b).font(a));
    // Fonts are not part of the pool config.
    assert_eq!(config.pool_config(), ImageConfig::new().pool_config());
    assert_eq!(ImageConfig::new().image(LOGO).images, [LOGO]);
}

#[tokio::test]
async fn img_01_equal_fonts_share_one_registry() {
    let runtime = noctis_core::Runtime::for_test();
    let config = || ImageConfig::new().font(Font::from_static(INTER));
    build(&runtime, config()).unwrap();
    build(&runtime, config()).unwrap();
    let first = runtime
        .resource_or_init::<FontRegistry>(fonts_config(vec![Font::from_static(INTER)]))
        .unwrap();
    let second = runtime
        .resource_or_init::<FontRegistry>(fonts_config(vec![Font::from_bytes(INTER.to_vec())]))
        .unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.families().len(), 1);
}

#[tokio::test]
async fn img_01_other_fonts_conflict_on_one_runtime() {
    let runtime = noctis_core::Runtime::for_test();
    build(&runtime, ImageConfig::new().font(Font::from_static(INTER))).unwrap();
    let err = build(&runtime, ImageConfig::new().font(Font::from_static(JP400))).unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [BuildViolation::ResourceConflict(c)] if c.resource.ends_with("FontRegistry")
        ),
        "{err}"
    );
}

#[tokio::test]
async fn img_01_test_runtimes_get_their_own_registry() {
    let one = noctis_core::Runtime::for_test();
    let two = noctis_core::Runtime::for_test();
    build(&one, ImageConfig::new().font(Font::from_static(INTER))).unwrap();
    build(&two, ImageConfig::new().font(Font::from_static(JP400))).unwrap();
    let a = one
        .resource_or_init::<FontRegistry>(fonts_config(vec![Font::from_static(INTER)]))
        .unwrap();
    let b = two
        .resource_or_init::<FontRegistry>(fonts_config(vec![Font::from_static(JP400)]))
        .unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
}

/// The plugin violation messages of a failed build.
fn plugin_violations(err: &noctis_core::BuildError) -> Vec<String> {
    err.violations()
        .iter()
        .map(ToString::to_string)
        .filter(|v| v.contains("[IMG-03]"))
        .collect()
}

#[tokio::test]
async fn img_03_remote_config_is_checked_at_build() {
    let runtime = noctis_core::Runtime::for_test();
    let config = ImageConfig::new().font(Font::from_static(INTER)).remote(
        RemoteConfig::new()
            .allow_origin("https://*.h")
            .allow_origin("http://example.com")
            .concurrency(0)
            .timeout(Duration::from_secs(61)),
    );
    assert_eq!(
        config.remote,
        RemoteConfig::new()
            .allow_origin("https://*.h")
            .allow_origin("http://example.com")
            .concurrency(0)
            .timeout(Duration::from_secs(61))
    );
    let err = build(&runtime, config).unwrap_err();
    let violations = plugin_violations(&err);
    assert_eq!(violations.len(), 4, "{err}");
    for needle in [
        "[IMG-03] remote origin `https://*.h` is invalid: the host must be",
        "[IMG-03] remote origin `http://example.com` is invalid: plain http is only for loopback hosts",
        "[IMG-03] remote concurrency 0 is out of range: use 1..=256",
        "[IMG-03] remote timeout 61s is out of range: use 100ms..=60s",
    ] {
        assert!(
            violations.iter().any(|v| v.contains(needle)),
            "{needle}: {violations:?}"
        );
    }
    // A valid config builds.
    let ok = ImageConfig::new()
        .font(Font::from_static(INTER))
        .remote(RemoteConfig::new().allow_origin("http://127.0.0.1:9"));
    build(&runtime, ok).unwrap();
}

#[tokio::test]
async fn img_03_a_bad_fallback_is_an_img_02_violation() {
    let runtime = noctis_core::Runtime::for_test();
    let config = ImageConfig::new()
        .font(Font::from_static(INTER))
        .remote(RemoteConfig::new().fallback(StaticImage::new("fallback", b"GIF89a")));
    let err = build(&runtime, config).unwrap_err();
    assert!(
        err.to_string().contains(
            "[IMG-02] static image `fallback`: unsupported format: use PNG, JPEG or WebP"
        ),
        "{err}"
    );
}

#[tokio::test]
async fn img_04_max_emoji_is_checked_at_build() {
    let runtime = noctis_core::Runtime::for_test();
    let config = |n| {
        ImageConfig::new()
            .font(Font::from_static(INTER))
            .remote(RemoteConfig::new().max_emoji(n))
    };
    let err = build(&runtime, config(129)).unwrap_err();
    let violations: Vec<String> = err
        .violations()
        .iter()
        .map(ToString::to_string)
        .filter(|v| v.contains("[IMG-04]"))
        .collect();
    assert_eq!(violations.len(), 1, "{err}");
    assert!(
        violations[0].contains("[IMG-04] remote max_emoji 129 is out of range: use 0..=128"),
        "{}",
        violations[0]
    );
    for n in [0, 128] {
        build(&runtime, config(n)).unwrap();
    }
}
