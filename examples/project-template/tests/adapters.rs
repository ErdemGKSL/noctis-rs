//! Our custom Adapter must pass the same conformance suite as Noctis's memory default
//! (FR-39): missing keys, verbatim keys and values, overwrite, delete, counters, App
//! namespacing, races of 32 concurrent callers (one `put_if_absent` winner, no lost
//! increments) and TTL expiry. The suite panics on the first failing case.

use noctis::App;
use noctis::bytes::Bytes;
use noctis::image::OutputFormat;
use noctis::image::RemoteImage;
use noctis::image::unstable::{RenderJob, Renderer};
use noctis::testkit::conformance;
use noctis_bot_template::adapters::{BoundedCacheStore, FixedRenderer};
use noctis_bot_template::modules;
use noctis_bot_template::modules::profile::ProfileCard;

#[tokio::test]
async fn bounded_cache_store_conforms() {
    conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}

fn card() -> ProfileCard {
    ProfileCard {
        name: "Deniz".into(),
        subtitle: "they/them".into(),
        level_label: "Level 7".into(),
        xp_label: "42 / 100 XP".into(),
        progress: 0.42,
        avatar: RemoteImage::placeholder(),
    }
}

/// The custom Renderer answers its fixed bytes, digested by the job's render key.
#[tokio::test]
async fn fixed_renderer_renders_a_profile_card_job() -> anyhow::Result<()> {
    let renderer = FixedRenderer::new(Bytes::from_static(b"\x89PNG"), OutputFormat::Png);
    let job = RenderJob::of(&card())?;
    let output = renderer.render(job.clone()).await?;
    assert_eq!(output.bytes, Bytes::from_static(b"\x89PNG"));
    assert_eq!(output.digest, job.key(FixedRenderer::ID));
    assert_eq!(output.format, OutputFormat::Png);
    assert_eq!(output.settings_used.size, job.size);
    Ok(())
}

/// The profile Module builds on the custom Renderer alone: no theme, no font.
#[tokio::test]
async fn fixed_renderer_replaces_the_image_plugin() -> anyhow::Result<()> {
    let app = App::builder()
        .module(modules::profile::module())
        .renderer(FixedRenderer::new(
            Bytes::from_static(b"\x89PNG"),
            OutputFormat::Png,
        ))
        .build()?;
    assert!(app.has_renderer());
    Ok(())
}
