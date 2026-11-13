//! Story 20.8: the pixel proof that takumi-core 0.31.0 draws the CSS the
//! shapes lower to, then [`Pie`] and [`Arc`] drawn through the Renderer.

use std::str::FromStr as _;

use super::*;
use crate::shape::{Arc, MAX_SLICES, Pie};

const RED: [u8; 4] = [0xFF, 0, 0, 0xFF];
const BLUE: [u8; 4] = [0, 0, 0xFF, 0xFF];
const GREEN: [u8; 4] = [0, 0xFF, 0, 0xFF];

/// What a shape Template draws.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum Case {
    Pie {
        slices: Vec<(f64, u32)>,
        start: f32,
        sized: bool,
    },
    Arc {
        value: f32,
        track: Option<u32>,
        thickness: f32,
        start: f32,
        sweep: f32,
        caps: bool,
    },
}

pub(super) fn shape_view(case: &Case) -> Node {
    let shape: Node = match case {
        Case::Pie {
            slices,
            start,
            sized,
        } => {
            let pie = Pie::new(slices.iter().copied()).start_deg(*start);
            if *sized {
                pie.classes(Classes::__literal("size-8")).into()
            } else {
                pie.into()
            }
        }
        Case::Arc {
            value,
            track,
            thickness,
            start,
            sweep,
            caps,
        } => {
            let arc = Arc::new(*value, 0xFF_0000)
                .thickness_pct(*thickness)
                .start_deg(*start)
                .sweep_deg(*sweep)
                .round_caps(*caps);
            match track {
                Some(track) => arc.track(*track).into(),
                None => arc.into(),
            }
        }
    };
    div(Classes::__literal("flex w-full h-full")).child(shape)
}

async fn draw(case: Case) -> Vec<u8> {
    let out = renderer()
        .render(RenderJob::of(&Shaped { case }).unwrap())
        .await
        .unwrap();
    decode_png(&out.bytes).2
}

fn pie(slices: &[(f64, u32)]) -> Case {
    Case::Pie {
        slices: slices.to_vec(),
        start: 0.0,
        sized: false,
    }
}

fn ring(value: f32, track: Option<u32>) -> Case {
    Case::Arc {
        value,
        track,
        thickness: 20.0,
        start: 0.0,
        sweep: 360.0,
        caps: false,
    }
}

fn is_clear(px: [u8; 4]) -> bool {
    px[3] == 0
}

/// The pixel at `deg` clockwise from 12 o'clock, `r` px from (50, 50).
fn polar(rgba: &[u8], deg: f64, r: f64) -> [u8; 4] {
    let (sin, cos) = deg.to_radians().sin_cos();
    // Truncating to the pixel grid is the point.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x, y) = ((50.0 + r * sin) as u32, (50.0 - r * cos) as u32);
    pixel(rgba, 100, x, y)
}

/// Draws a 100x100 container with inline `css` straight through takumi.
fn raw(css: &str) -> Vec<u8> {
    let node = takumi::prelude::Node::container(Vec::new())
        .with_style(takumi::prelude::Style::from_str(css).unwrap());
    let fonts = takumi::prelude::Fonts::default();
    let options = RenderOptions::builder()
        .fonts(&fonts)
        .viewport(Viewport::new((100, 100)))
        .node(node)
        .build();
    takumi::render(options).unwrap().as_raw().to_vec()
}

#[test]
fn takumi_draws_inline_conic_gradients_with_degree_stops() {
    let px = raw("width: 100px; height: 100px; background-image: \
                  conic-gradient(from 0deg, #ff0000 0deg 90deg, #0000ff 90deg 360deg)");
    // 0..90 degrees clockwise from 12 o'clock is the top-right quadrant.
    for (x, y) in [(75, 25), (55, 5), (95, 45)] {
        assert_eq!(pixel(&px, 100, x, y), RED, "({x}, {y})");
    }
    for (x, y) in [(25, 25), (75, 75), (25, 75), (45, 5)] {
        assert_eq!(pixel(&px, 100, x, y), BLUE, "({x}, {y})");
    }
    // `from` turns the whole gradient clockwise.
    let px = raw("width: 100px; height: 100px; background-image: \
                  conic-gradient(from 90deg, #ff0000 0deg 90deg, #0000ff 90deg 360deg)");
    assert_eq!(pixel(&px, 100, 75, 75), RED);
    assert_eq!(pixel(&px, 100, 75, 25), BLUE);
}

#[test]
fn takumi_draws_inline_radial_gradient_masks() {
    let px = raw(
        "width: 100px; height: 100px; background-color: #ff0000; mask-image: \
                  radial-gradient(circle closest-side, transparent 60%, #000 60% 100%, \
                  transparent 100%)",
    );
    // Clear inside 30 px and outside 50 px, drawn between.
    for (x, y) in [(50, 50), (50, 25), (2, 2)] {
        assert!(is_clear(pixel(&px, 100, x, y)), "({x}, {y})");
    }
    for (x, y) in [(50, 10), (50, 1), (90, 50)] {
        assert_eq!(pixel(&px, 100, x, y), RED, "({x}, {y})");
    }
}

#[tokio::test]
async fn a_pie_draws_proportional_slices_clockwise_from_12() {
    let px = draw(pie(&[(1.0, 0xFF_0000), (3.0, 0x00_00FF)])).await;
    assert_eq!(polar(&px, 45.0, 30.0), RED);
    for deg in [135.0, 225.0, 315.0] {
        assert_eq!(polar(&px, deg, 30.0), BLUE, "{deg}");
    }
    // A circle filling its box: the corners stay clear.
    assert!(is_clear(pixel(&px, 100, 3, 3)));
    assert!(is_clear(pixel(&px, 100, 96, 96)));
}

#[tokio::test]
async fn start_deg_turns_a_pie() {
    let px = draw(Case::Pie {
        slices: vec![(1.0, 0xFF_0000), (3.0, 0x00_00FF)],
        start: 90.0,
        sized: false,
    })
    .await;
    assert_eq!(polar(&px, 135.0, 30.0), RED);
    assert_eq!(polar(&px, 45.0, 30.0), BLUE);
}

#[tokio::test]
async fn a_pie_skips_zero_negative_and_non_finite_values() {
    let px = draw(pie(&[
        (0.0, 0x00_FF00),
        (-1.0, 0x00_FF00),
        (f64::NAN, 0x00_FF00),
        (f64::INFINITY, 0x00_FF00),
        (2.0, 0xFF_0000),
    ]))
    .await;
    for deg in [10.0, 100.0, 200.0, 300.0] {
        assert_eq!(polar(&px, deg, 30.0), RED, "{deg}");
    }
}

#[tokio::test]
async fn an_all_zero_pie_draws_nothing() {
    let px = draw(pie(&[(0.0, 0xFF_0000), (0.0, 0x00_00FF)])).await;
    assert!(px.chunks_exact(4).all(|p| p[3] == 0));
    let px = draw(pie(&[])).await;
    assert!(px.chunks_exact(4).all(|p| p[3] == 0));
}

#[tokio::test]
async fn a_shape_is_a_centred_circle_of_the_smaller_side() {
    let out = renderer()
        .render(
            RenderJob::of(&ShapedWide {
                case: pie(&[(1.0, 0xFF_0000)]),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    let (w, _, px) = decode_png(&out.bytes);
    // A 60 px circle centred in 100x60: x 20..80.
    for (x, y) in [(50, 30), (22, 30), (77, 30), (50, 2)] {
        assert_eq!(pixel(&px, w, x, y), RED, "({x}, {y})");
    }
    for (x, y) in [(18, 30), (82, 30), (5, 5)] {
        assert!(is_clear(pixel(&px, w, x, y)), "({x}, {y})");
    }
}

#[tokio::test]
async fn shape_classes_size_it() {
    let px = draw(Case::Pie {
        slices: vec![(1.0, 0xFF_0000)],
        start: 0.0,
        sized: true,
    })
    .await;
    // `size-8` (32 px) in the parent's flex row, at the top left.
    assert_eq!(pixel(&px, 100, 16, 16), RED);
    assert!(is_clear(pixel(&px, 100, 40, 16)));
    assert!(is_clear(pixel(&px, 100, 50, 50)));
}

#[tokio::test]
async fn a_full_ring_has_a_transparent_centre() {
    let px = draw(ring(1.0, None)).await;
    // Radius 50, thickness 20 % of it: drawn between 40 and 50 px.
    for deg in [0.0, 90.0, 180.0, 270.0] {
        assert_eq!(polar(&px, deg, 45.0), RED, "{deg}");
        assert!(is_clear(polar(&px, deg, 35.0)), "{deg}");
    }
    assert!(is_clear(pixel(&px, 100, 50, 50)));
    assert!(is_clear(pixel(&px, 100, 2, 2)));
}

#[tokio::test]
async fn an_arc_draws_its_value_over_its_track() {
    let px = draw(ring(0.5, Some(0x00_FF00))).await;
    assert_eq!(polar(&px, 90.0, 45.0), RED);
    assert_eq!(polar(&px, 270.0, 45.0), GREEN);
    assert!(is_clear(pixel(&px, 100, 50, 50)));
}

#[tokio::test]
async fn an_arc_value_clamps_to_0_1() {
    let empty = draw(ring(-0.5, Some(0x00_FF00))).await;
    for deg in [10.0, 180.0, 350.0] {
        assert_eq!(polar(&empty, deg, 45.0), GREEN, "{deg}");
    }
    let full = draw(ring(7.0, Some(0x00_FF00))).await;
    for deg in [10.0, 180.0, 350.0] {
        assert_eq!(polar(&full, deg, 45.0), RED, "{deg}");
    }
    let none = draw(ring(f32::NAN, None)).await;
    assert!(none.chunks_exact(4).all(|p| p[3] == 0));
}

fn gauge(value: f32, caps: bool) -> Case {
    Case::Arc {
        value,
        track: None,
        thickness: 20.0,
        start: 225.0,
        sweep: 270.0,
        caps,
    }
}

#[tokio::test]
async fn a_270_degree_gauge_leaves_its_bottom_open() {
    let px = draw(Case::Arc {
        value: 0.5,
        track: Some(0x00_FF00),
        thickness: 20.0,
        start: 225.0,
        sweep: 270.0,
        caps: false,
    })
    .await;
    // Value 225..360, track 0..135, the gap 135..225.
    assert_eq!(polar(&px, 270.0, 45.0), RED);
    assert_eq!(polar(&px, 90.0, 45.0), GREEN);
    assert!(is_clear(polar(&px, 180.0, 45.0)));
}

#[tokio::test]
async fn round_caps_are_circles_at_the_value_ends() {
    // Just before the start (220 degrees) and just after the end
    // (5 degrees), on the ring's middle radius.
    let square = draw(gauge(0.5, false)).await;
    assert!(is_clear(polar(&square, 220.0, 45.0)));
    assert!(is_clear(polar(&square, 5.0, 45.0)));
    let round = draw(gauge(0.5, true)).await;
    assert_eq!(polar(&round, 220.0, 45.0), RED);
    assert_eq!(polar(&round, 5.0, 45.0), RED);
    // A cap is round: well past the end it is clear again.
    assert!(is_clear(polar(&round, 15.0, 45.0)));
    // No caps for a value of 0.
    let zero = draw(gauge(0.0, true)).await;
    assert!(zero.chunks_exact(4).all(|p| p[3] == 0));
}

#[tokio::test]
async fn more_slices_than_the_cap_are_unsupported() {
    let slices: Vec<_> = (0..MAX_SLICES).map(|_| (1.0, 0xFF_0000)).collect();
    draw(pie(&slices)).await;
    let mut slices = slices;
    slices.push((1.0, 0x00_00FF));
    let err = renderer()
        .render(RenderJob::of(&Shaped { case: pie(&slices) }).unwrap())
        .await
        .unwrap_err();
    let RenderError::Unsupported { id, why } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(id, "shaped");
    assert!(
        why.contains("Pie") && why.contains("65") && why.contains("64"),
        "{why}"
    );
}

#[test]
fn shape_data_is_part_of_the_render_key_through_the_template_data() {
    let r = renderer();
    let key = |slices: &[(f64, u32)]| {
        r.key(&RenderJob::of(&Shaped { case: pie(slices) }).unwrap())
            .0
    };
    let a = key(&[(1.0, 0xFF_0000), (3.0, 0x00_00FF)]);
    assert_eq!(a, key(&[(1.0, 0xFF_0000), (3.0, 0x00_00FF)]));
    assert_ne!(a, key(&[(2.0, 0xFF_0000), (3.0, 0x00_00FF)]));
    assert_ne!(a, key(&[(1.0, 0xFF_0001), (3.0, 0x00_00FF)]));
}
