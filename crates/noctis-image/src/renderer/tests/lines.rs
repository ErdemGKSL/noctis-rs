//! Story 20.9: the pixel proof that takumi-core 0.31.0 clips a box to an
//! inline `clip-path: polygon(..)` (percentages, `calc()` and sub-polygons
//! joined through one point), then [`Line`] and [`Area`] drawn through the
//! Renderer.

use std::str::FromStr as _;
use std::time::Instant;

use super::*;
use crate::shape::{Area, Line, MAX_POINTS};

const RED: [u8; 4] = [0xFF, 0, 0, 0xFF];

/// What a line Template draws.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum Case {
    Line {
        points: Vec<(f64, f64)>,
        stroke: f32,
    },
    Series {
        values: Vec<f64>,
        stroke: f32,
    },
    Area {
        points: Vec<(f64, f64)>,
    },
    /// A line in a hidden box, beside a drawn one.
    Hidden {
        points: Vec<(f64, f64)>,
    },
}

pub(super) fn line_view(case: &Case) -> Node {
    let root = div(Classes::__literal("flex w-full h-full"));
    match case {
        Case::Line { points, stroke } => {
            root.child(Line::new(points.iter().copied(), 0xFF_0000).stroke_px(*stroke))
        }
        Case::Series { values, stroke } => {
            root.child(Line::series(values.iter().copied(), 0xFF_0000).stroke_px(*stroke))
        }
        Case::Area { points } => root.child(Area::new(points.iter().copied(), 0xFF_0000)),
        Case::Hidden { points } => root
            .child(
                div(Classes::__literal("hidden"))
                    .child(Line::new(points.iter().copied(), 0x00_00FF)),
            )
            .child(Line::new(points.iter().copied(), 0xFF_0000).stroke_px(10.0)),
    }
}

async fn draw(case: Case) -> Vec<u8> {
    let out = renderer()
        .render(RenderJob::of(&Lined { case }).unwrap())
        .await
        .unwrap();
    decode_png(&out.bytes).2
}

fn line(points: &[(f64, f64)], stroke: f32) -> Case {
    Case::Line {
        points: points.to_vec(),
        stroke,
    }
}

fn is_clear(px: [u8; 4]) -> bool {
    px[3] == 0
}

/// Draws a 100x100 viewport holding a container with inline `css`
/// straight through takumi.
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
fn takumi_clips_a_box_to_an_inline_percentage_polygon() {
    let px = raw("width: 100px; height: 100px; background-color: #ff0000; \
                  clip-path: polygon(0% 0%, 100% 0%, 0% 100%)");
    // The top-left triangle is drawn, the bottom-right one is clear.
    for (x, y) in [(10, 10), (60, 20), (20, 60)] {
        assert_eq!(pixel(&px, 100, x, y), RED, "({x}, {y})");
    }
    for (x, y) in [(90, 90), (40, 80), (80, 40)] {
        assert!(is_clear(pixel(&px, 100, x, y)), "({x}, {y})");
    }
}

#[test]
fn takumi_fills_sub_polygons_joined_through_one_point() {
    // Two overlapping squares wound the same way and one apart, each
    // entered and left through (0, 0): nonzero fills all three, the
    // joining edges draw nothing and the overlap stays filled.
    let px = raw("width: 100px; height: 100px; background-color: #ff0000; \
                  clip-path: polygon(0px 0px, 10px 10px, 30px 10px, 30px 30px, 10px 30px, \
                  10px 10px, 0px 0px, 20px 20px, 40px 20px, 40px 40px, 20px 40px, 20px 20px, \
                  0px 0px, calc(60% + 0px) 60%, 80% 60%, 80% 80%, 60% 80%, 60% 60%, 0px 0px)");
    for (x, y) in [(15, 15), (25, 25), (35, 35), (70, 70)] {
        assert_eq!(pixel(&px, 100, x, y), RED, "({x}, {y})");
    }
    for (x, y) in [(5, 5), (45, 45), (50, 50), (5, 20), (15, 35)] {
        assert!(is_clear(pixel(&px, 100, x, y)), "({x}, {y})");
    }
}

#[tokio::test]
async fn a_line_is_a_band_of_its_stroke_width() {
    let px = draw(line(&[(0.0, 0.5), (1.0, 0.5)], 10.0)).await;
    // y = 0.5 is the row 50; 10 px is 45..55.
    for y in [46, 50, 54] {
        assert_eq!(pixel(&px, 100, 50, y), RED, "{y}");
    }
    for y in [43, 57, 10, 90] {
        assert!(is_clear(pixel(&px, 100, 50, y)), "{y}");
    }
}

#[tokio::test]
async fn a_diagonal_band_keeps_its_width_in_a_wide_box() {
    // (0, 0) to (1, 1) in 100x60: the band's normals are in pixels, so its
    // width is the stroke, measured across the line.
    let out = renderer()
        .render(
            RenderJob::of(&LinedWide {
                case: line(&[(0.0, 0.0), (1.0, 1.0)], 10.0),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    let (w, _, px) = decode_png(&out.bytes);
    let (dx, dy) = (100.0_f64, -60.0_f64);
    let len = dx.hypot(dy);
    // The unit normal of the drawn line, in pixels.
    let (nx, ny) = (-dy / len, dx / len);
    let at = |d: f64| {
        let (x, y) = (50.0 + nx * d, 30.0 + ny * d);
        // Truncating to the pixel grid is the point.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        pixel(&px, w, x as u32, y as u32)
    };
    for d in [-3.5, 0.0, 3.5] {
        assert_eq!(at(d), RED, "{d}");
    }
    for d in [-7.0, 7.0] {
        assert!(is_clear(at(d)), "{d}");
    }
}

#[tokio::test]
async fn joints_are_bevelled() {
    // A V with its vertex at (50, 80): the bevel fills the outer corner up
    // to 3.2 px below the vertex, and no miter goes further.
    let px = draw(line(&[(0.0, 0.8), (0.5, 0.2), (1.0, 0.8)], 10.0)).await;
    assert_eq!(pixel(&px, 100, 50, 81), RED);
    assert_eq!(pixel(&px, 100, 50, 76), RED);
    assert!(is_clear(pixel(&px, 100, 50, 85)));
    assert!(is_clear(pixel(&px, 100, 50, 60)));
}

#[tokio::test]
async fn an_area_fills_under_its_line() {
    let px = draw(Case::Area {
        points: vec![(0.0, 0.0), (1.0, 1.0)],
    })
    .await;
    for (x, y) in [(90, 50), (50, 90), (60, 45)] {
        assert_eq!(pixel(&px, 100, x, y), RED, "({x}, {y})");
    }
    for (x, y) in [(10, 50), (50, 10), (40, 55)] {
        assert!(is_clear(pixel(&px, 100, x, y)), "({x}, {y})");
    }
}

#[tokio::test]
async fn fewer_than_two_finite_points_draw_nothing() {
    for case in [
        line(&[(0.5, 0.5)], 10.0),
        line(&[(0.0, f64::NAN), (1.0, 0.5)], 10.0),
        line(&[(0.5, 0.5), (0.5, 0.5)], 10.0),
        line(&[], 10.0),
        Case::Area {
            points: vec![(f64::INFINITY, 0.5), (1.0, 0.5)],
        },
    ] {
        let px = draw(case.clone()).await;
        assert!(px.chunks_exact(4).all(|p| p[3] == 0), "{case:?}");
    }
}

#[tokio::test]
async fn non_finite_points_are_skipped() {
    let px = draw(line(&[(0.0, 0.5), (0.5, f64::NAN), (1.0, 0.5)], 10.0)).await;
    for x in [5, 50, 95] {
        assert_eq!(pixel(&px, 100, x, 50), RED, "{x}");
    }
}

#[tokio::test]
async fn a_flat_series_is_drawn_at_mid_height() {
    let px = draw(Case::Series {
        values: vec![3.0, 3.0, 3.0],
        stroke: 4.0,
    })
    .await;
    assert_eq!(pixel(&px, 100, 50, 50), RED);
    assert!(is_clear(pixel(&px, 100, 50, 45)));
}

#[tokio::test]
async fn a_series_spans_its_box() {
    let px = draw(Case::Series {
        values: vec![1.0, 9.0],
        stroke: 6.0,
    })
    .await;
    // From the bottom-left corner to the top-right one.
    assert_eq!(pixel(&px, 100, 50, 50), RED);
    assert_eq!(pixel(&px, 100, 1, 98), RED);
    assert_eq!(pixel(&px, 100, 98, 1), RED);
    assert!(is_clear(pixel(&px, 100, 10, 10)));
}

#[tokio::test]
async fn a_hidden_line_does_not_move_the_others() {
    let px = draw(Case::Hidden {
        points: vec![(0.0, 0.5), (1.0, 0.5)],
    })
    .await;
    assert_eq!(pixel(&px, 100, 50, 50), RED);
    assert!(px.chunks_exact(4).all(|p| p[2] == 0));
}

#[tokio::test]
async fn more_points_than_the_cap_are_unsupported() {
    #[allow(clippy::cast_precision_loss)]
    let points: Vec<_> = (0..MAX_POINTS).map(|i| (i as f64 / 511.0, 0.5)).collect();
    draw(line(&points, 2.0)).await;
    let mut points = points;
    points.push((1.0, 0.0));
    let err = renderer()
        .render(
            RenderJob::of(&Lined {
                case: line(&points, 2.0),
            })
            .unwrap(),
        )
        .await
        .unwrap_err();
    let RenderError::Unsupported { id, why } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(id, "lined");
    assert!(
        why.contains("Line") && why.contains("513") && why.contains("512"),
        "{why}"
    );
}

/// The timing note of the story: a 512-point zigzag line (the cap, a joint
/// at every point) in a 934x282 frame, lowered, measured and drawn, stays
/// inside the reference card's p99 budget (50 ms, NFR-3) per frame.
#[tokio::test]
async fn a_512_point_line_stays_inside_the_frame_budget() {
    let values: Vec<f64> = (0..MAX_POINTS)
        .map(|i| if i % 2 == 0 { 0.0 } else { 1.0 })
        .collect();
    let r = renderer();
    let mut times = Vec::new();
    for round in 0..7_u32 {
        let mut values = values.clone();
        // Distinct data, so no render hits the render cache.
        values[0] = f64::from(round);
        let job = RenderJob::of(&LinedCard {
            case: Case::Series {
                values,
                stroke: 2.0,
            },
        })
        .unwrap();
        let start = Instant::now();
        r.render(job).await.unwrap();
        times.push(start.elapsed());
    }
    times.sort();
    let median = times[times.len() / 2];
    eprintln!("512-point line, 934x282 PNG: median {median:?} of {times:?}");
    assert!(median < Duration::from_millis(50), "{median:?}");
}
