//! Story 20.8: a pie, a full ring and a 270° gauge with round caps, and the
//! edge values (an empty and a full arc, a pie with a tiny slice), as pixel
//! snapshots through `TestApp`, so a takumi upgrade that moves them fails
//! here.

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::image::shape::{Arc, Pie};
use noctis::image::{Font, Frame, ImageConfig, Node, Template, TemplateView, theme, view};
use noctis::prelude::*;
use noctis::testkit::{TestApp, assert_render_snapshot};
use serde::{Deserialize, Serialize};

const BRAND: u32 = 0x58_65F2;
const GREEN: u32 = 0x57_F287;
const TRACK: u32 = 0x31_3338;

/// A pie, a full ring and a gauge with its value written in the hole.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "shapes", version = 1, size = (360, 120), format = Png)]
struct Shapes {
    /// The pie's slices.
    slices: Vec<(u32, u32)>,
    /// The ring's and the gauge's value, `0.0..=1.0`.
    value: f32,
}

impl TemplateView for Shapes {
    fn view(&self, _: Frame) -> Node {
        let label = format!("{:.0}%", self.value * 100.0);
        view! {
            <div tw="flex items-center justify-around w-full h-full bg-slate-900">
                <div tw="flex size-24">{Pie::new(self.slices.iter().copied())}</div>
                <div tw="flex size-24">{Arc::new(self.value, GREEN).track(TRACK)}</div>
                <div tw="relative flex items-center justify-center size-24">
                    {Arc::new(self.value, BRAND)
                        .track(TRACK)
                        .start_deg(225.0)
                        .sweep_deg(270.0)
                        .thickness_pct(24.0)
                        .round_caps(true)}
                    <span tw="absolute text-lg text-white">{label}</span>
                </div>
            </div>
        }
    }
}

/// The edge values: an empty arc, a full arc and a pie with a tiny slice.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "shapes.edges", version = 1, size = (300, 100), format = Png)]
struct Edges {}

impl TemplateView for Edges {
    fn view(&self, _: Frame) -> Node {
        view! {
            <div tw="flex items-center justify-around w-full h-full bg-slate-900">
                <div tw="flex size-20">
                    {Arc::new(0.0, BRAND).track(TRACK).round_caps(true)}
                </div>
                <div tw="flex size-20">
                    {Arc::new(1.0, BRAND).track(TRACK).round_caps(true)}
                </div>
                <div tw="flex size-20">
                    {Pie::new([(0.002, 0xFF_FFFF), (1.0, BRAND), (0.0, GREEN)])}
                </div>
            </div>
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn shapes_match_snapshots() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = App::builder();
    builder
        .theme(theme!())
        .image(ImageConfig::new().font(Font::from_static(include_bytes!(
            "../../noctis-image/fonts/InterVariable.woff2"
        ))))
        .module(
            Module::new("shapes")
                .template::<Shapes>()
                .template::<Edges>(),
        );
    let app = TestApp::build(&mut builder).await?;
    assert_render_snapshot!(
        app.renderer(),
        "shapes",
        Shapes {
            slices: vec![(5, BRAND), (3, GREEN), (2, 0xFE_E75C), (1, 0xED_4245)],
            value: 0.62,
        },
        tolerance = 0.01
    );
    assert_render_snapshot!(app.renderer(), "shapes_edges", Edges {}, tolerance = 0.01);
    app.stop().await?;
    Ok(())
}
