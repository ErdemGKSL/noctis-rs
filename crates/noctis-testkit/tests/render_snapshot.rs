//! Story 10.11: `assert_render_snapshot!` end to end on a `TestApp`, through
//! the App's `Renderer` port (a fake here; the takumi card lives in the
//! facade's `tests/reference_card.rs`), and `TestApp::renderer`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Arc;

use bytes::Bytes;
use noctis::prelude::*;
use noctis_ports::{
    AnimationSpec, BoxFuture, EncodeSettings, ErasedView, Format, OutputFormat, RenderError,
    RenderJob, RenderOutput, Renderer, Size, Template, TemplateDescriptor,
};
use noctis_testkit::{TestApp, TestError, Tolerance, assert_render_snapshot};
use serde::{Deserialize, Serialize};

/// A hand-written Template (the fake Renderer never draws it).
#[derive(Serialize, Deserialize)]
struct FakeCard {
    shade: u8,
}

impl Template for FakeCard {
    const ID: &'static str = "fake.card";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(4, 2);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

/// Answers a registered `FakeCard` with a 4x2 gray PNG of its shade.
struct Fake;

impl Renderer for Fake {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        Box::pin(async move {
            let card: FakeCard = noctis_ports::decode_template(&job.data)
                .map_err(|_| RenderError::failed("bad data"))?;
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 4, 2);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().unwrap();
                writer
                    .write_image_data(&[card.shade, card.shade, card.shade, 255].repeat(8))
                    .unwrap();
                writer.finish().unwrap();
            }
            Ok(RenderOutput {
                bytes: Bytes::from(bytes),
                digest: [0; 32],
                format: OutputFormat::Png,
                settings_used: EncodeSettings {
                    size: job.size,
                    quality: None,
                    fps: None,
                    frames: 1,
                },
            })
        })
    }

    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        assert!(templates.iter().any(|t| t.id == FakeCard::ID));
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_testapp_snapshots_through_its_renderer() -> Result<(), TestError> {
    let fake: Arc<dyn Renderer> = Arc::new(Fake);
    let mut builder = App::builder();
    builder
        .renderer(Arc::clone(&fake))
        .module(Module::new("cards").template::<FakeCard>());
    let app = TestApp::build(&mut builder).await?;
    // `.renderer(..)` wraps the given `Arc` (Story 15.1): the App holds
    // this Renderer, not a copy.
    assert!(app.renderer().is_some());
    assert_eq!(Arc::strong_count(&fake), 2);
    // The committed `tests/snapshots/fake_card.png` holds shade 120. One
    // channel off by 2 everywhere: within a channel delta of 2 only.
    assert_render_snapshot!(
        app.renderer(),
        "fake_card",
        FakeCard { shade: 122 },
        tolerance = Tolerance::ratio(0.0).channel(2),
    );
    // The exact match runs last, so `NOCTIS_UPDATE_SNAPSHOTS=1` leaves
    // shade 120 stored.
    assert_render_snapshot!(app.renderer(), "fake_card", FakeCard { shade: 120 });
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_testapp_has_no_renderer() -> Result<(), TestError> {
    let app = TestApp::build(&mut App::builder()).await?;
    assert!(app.renderer().is_none());
    app.stop().await
}
