//! Story 20.9: a line, an area under its line (two stacked nodes), a flat
//! series and a two-point series, as pixel snapshots through `TestApp`, so
//! a takumi upgrade that moves them fails here.

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::image::shape::{Area, Line};
use noctis::image::{Font, Frame, ImageConfig, Node, Template, TemplateView, theme, tw, view};
use noctis::prelude::*;
use noctis::testkit::{TestApp, assert_render_snapshot};
use serde::{Deserialize, Serialize};

const BRAND: u32 = 0x58_65F2;
const FILL: u32 = 0x2E_3470;
const GREEN: u32 = 0x57_F287;

/// Four sparklines over one series.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "lines", version = 1, size = (480, 120), format = Png)]
struct Lines {
    /// The series.
    values: Vec<f64>,
}

impl TemplateView for Lines {
    fn view(&self, _: Frame) -> Node {
        let values = self.values.iter().copied();
        view! {
            <div tw="flex items-center justify-around w-full h-full bg-slate-900">
                <div tw="flex w-24 h-16">{Line::series(values.clone(), GREEN).stroke_px(2.0)}</div>
                <div tw="relative flex w-24 h-16">
                    {Area::series(values.clone(), FILL).classes(tw!("absolute inset-0"))}
                    {Line::series(values, BRAND)
                        .stroke_px(3.0)
                        .classes(tw!("absolute inset-0"))}
                </div>
                <div tw="flex w-24 h-16">{Line::series([4, 4, 4, 4], GREEN).stroke_px(3.0)}</div>
                <div tw="flex w-24 h-16">
                    {Line::new([(0.0, 0.2), (1.0, 0.8)], BRAND).stroke_px(6.0)}
                </div>
            </div>
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lines_match_snapshots() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = App::builder();
    builder
        .theme(theme!())
        .image(ImageConfig::new().font(Font::from_static(include_bytes!(
            "../../noctis-image/fonts/InterVariable.woff2"
        ))))
        .module(Module::new("lines").template::<Lines>());
    let app = TestApp::build(&mut builder).await?;
    assert_render_snapshot!(
        app.renderer(),
        "lines",
        Lines {
            values: vec![3.0, 5.0, 4.0, 8.0, 6.0, 7.0, 2.0, 9.0, 6.5, 8.0],
        },
        tolerance = 0.01
    );
    app.stop().await?;
    Ok(())
}
