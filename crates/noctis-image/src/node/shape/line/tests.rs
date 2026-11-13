#![allow(clippy::float_cmp, reason = "the series places exact fractions")]

use super::*;

#[test]
fn series_spreads_x_and_scales_y_to_the_finite_range() {
    let line = Line::series([2.0, f64::NAN, 6.0, 4.0, f64::INFINITY], 0);
    let points = &line.points;
    let xs: Vec<f64> = points.iter().map(|p| p.0).collect();
    assert_eq!(xs, [0.0, 0.25, 0.5, 0.75, 1.0]);
    assert_eq!(points[0].1, 0.0);
    assert!(points[1].1.is_nan());
    assert_eq!(points[2].1, 1.0);
    assert_eq!(points[3].1, 0.5);
    assert!(points[4].1.is_nan());
    assert_eq!(drawn(points).count(), 3);
}

#[test]
fn a_flat_or_short_series_sits_at_mid_height() {
    let flat = Area::series([3, 3, 3], 0);
    assert_eq!(flat.points, [(0.0, 0.5), (0.5, 0.5), (1.0, 0.5)]);
    assert_eq!(Line::series([7], 0).points, [(0.0, 0.5)]);
    assert!(Line::series(Vec::<f64>::new(), 0).points.is_empty());
}

#[test]
fn stroke_clamps_and_ignores_non_finite_widths() {
    assert_eq!(Line::new([(0, 0)], 0).stroke_px, DEFAULT_STROKE_PX);
    assert_eq!(Line::new([(0, 0)], 0).stroke_px(-3.0).stroke_px, 0.0);
    assert_eq!(
        Line::new([(0, 0)], 0).stroke_px(1e6).stroke_px,
        MAX_STROKE_PX
    );
    assert_eq!(
        Line::new([(0, 0)], 0)
            .stroke_px(4.0)
            .stroke_px(f32::NAN)
            .stroke_px,
        4.0
    );
    assert_eq!(Line::new([(0, 0)], 0x1FF_FFFF).color, 0xFF_FFFF);
}

#[test]
fn an_area_closes_on_the_bottom_edge() {
    let css = area_polygon(&[(0.0, 0.0), (f64::NAN, 1.0), (0.5, 1.0), (1.0, 0.25)]).unwrap();
    assert_eq!(
        css,
        "polygon(0.000% 100%, 0.000% 100.000%, 50.000% 0.000%, 100.000% 75.000%, 100.000% 100%)"
    );
    assert_eq!(area_polygon(&[(0.0, 0.0), (f64::NAN, 1.0)]), None);
    assert_eq!(area_polygon(&[]), None);
}

#[test]
fn a_line_needs_two_distinct_points_a_stroke_and_a_box() {
    let two = [(0.0, 0.5), (1.0, 0.5)];
    assert!(line_polygon(&two, 2.0, 100.0, 50.0).is_some());
    assert_eq!(line_polygon(&two, 0.0, 100.0, 50.0), None);
    assert_eq!(line_polygon(&two, 2.0, 0.0, 50.0), None);
    assert_eq!(
        line_polygon(&[(0.5, 0.5), (0.5, 0.5)], 2.0, 100.0, 50.0),
        None
    );
    assert_eq!(
        line_polygon(&[(0.5, f64::NAN), (0.5, 0.5)], 2.0, 100.0, 50.0),
        None
    );
}

#[test]
fn a_two_point_line_is_one_rectangle_walked_in_two_halves() {
    let css = line_polygon(&[(0.0, 0.5), (1.0, 0.5)], 4.0, 100.0, 50.0).unwrap();
    assert_eq!(
        css,
        "polygon(0.00px 25.00px, 0.00px 27.00px, 100.00px 27.00px, 100.00px 25.00px, \
         100.00px 23.00px, 0.00px 23.00px, 0.00px 25.00px)"
    );
}

#[test]
fn a_turn_adds_one_bevel_triangle_wound_like_the_rectangles() {
    let css = line_polygon(&[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)], 2.0, 100.0, 100.0).unwrap();
    // The start, two half rectangles forward and two back (3 points
    // each) and one triangle (2 + 1).
    assert_eq!(css.matches("px ").count(), 1 + 4 * 3 + 3);
    let quad = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    assert!(signed_area(&quad) > 0.0);
    // A straight joint adds none.
    let css = line_polygon(&[(0.0, 0.5), (0.5, 0.5), (1.0, 0.5)], 2.0, 100.0, 100.0).unwrap();
    assert_eq!(css.matches("px ").count(), 1 + 4 * 3);
}

#[test]
fn shapes_report_their_point_cap() {
    #[allow(clippy::cast_precision_loss, reason = "small indices")]
    let points: Vec<(f64, f64)> = (0..=MAX_POINTS).map(|i| (i as f64, 0.0)).collect();
    let why = super::super::Shape::Line(Line::new(points.clone(), 0))
        .over_cap()
        .unwrap();
    assert!(
        why.contains("Line") && why.contains("513 points") && why.contains("512"),
        "{why}"
    );
    assert_eq!(
        super::super::Shape::Area(Area::new(points[1..].to_vec(), 0)).over_cap(),
        None
    );
    let mut with_nan = points;
    with_nan[0].1 = f64::NAN;
    assert_eq!(
        super::super::Shape::Area(Area::new(with_nan, 0)).over_cap(),
        None
    );
}
