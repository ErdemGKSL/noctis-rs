//! Story 20.7: one picture drawn with the typed `Style` setters (insets,
//! radius, border width, scale and z-index) as a pixel snapshot through
//! `TestApp`, so a takumi upgrade that moves them fails here.

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::image::{Font, Frame, ImageConfig, Node, Style, Template, TemplateView, theme, view};
use noctis::prelude::*;
use noctis::testkit::{TestApp, assert_render_snapshot};
use serde::{Deserialize, Serialize};

/// A level track with a marker at `value` and a badge drawn over it.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "style.setters", version = 1, size = (240, 80), format = Png)]
struct Track {
    /// Where the marker sits, `0.0..=1.0`.
    value: f32,
}

impl TemplateView for Track {
    fn view(&self, _: Frame) -> Node {
        let pct = self.value * 100.0;
        view! {
            <div tw="relative flex w-full h-full bg-slate-900">
                <div tw="absolute bg-slate-700"
                     style={Style::new().left_px(20).right_px(20).top_px(36).bottom_px(36).rounded_px(4)} />
                <div tw="absolute bg-brand"
                     style={Style::new().left_px(20).top_px(36).bottom_px(36).width_pct(pct * 0.8).rounded_px(4)} />
                <div tw="absolute w-4 h-4 bg-white rounded-sm"
                     style={Style::new()
                         .left_pct(pct)
                         .top_px(32)
                         .translate_px(-8, 0)
                         .rotate_deg(45.0)
                         .scale(1.5)
                         .border_px(2)
                         .border_color_rgb(0x58_65F2)
                         .z_index(2)} />
                <div tw="absolute w-10 h-6 bg-emerald-500"
                     style={Style::new().right_px(8).top_px(8).rounded_px(12).z_index(1)} />
            </div>
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn style_setters_match_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = App::builder();
    builder
        .theme(theme!())
        .image(ImageConfig::new().font(Font::from_static(include_bytes!(
            "../../noctis-image/fonts/InterVariable.woff2"
        ))))
        .module(Module::new("style").template::<Track>());
    let app = TestApp::build(&mut builder).await?;
    assert_render_snapshot!(
        app.renderer(),
        "style_setters",
        Track { value: 0.6 },
        tolerance = 0.01
    );
    app.stop().await?;
    Ok(())
}
