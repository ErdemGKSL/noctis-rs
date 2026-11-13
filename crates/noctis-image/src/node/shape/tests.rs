#![allow(clippy::float_cmp, reason = "the setters store exact clamped values")]

use super::*;

#[test]
fn pie_css_is_proportional_and_ends_at_360() {
    let pie = Pie::new([(1, 0xFF_0000), (0, 0x00_FF00), (3, 0x00_00FF)]).start_deg(-90.0);
    let css = Shape::Pie(pie).css().0;
    assert!(
        css.ends_with(
            "background-image: conic-gradient(from 270.000deg, #ff0000 0.000deg 90.000deg, \
             #0000ff 90.000deg 360.000deg)"
        ),
        "{css}"
    );
}

#[test]
fn an_empty_pie_has_no_background() {
    let css = Shape::Pie(Pie::new([(0.0, 1), (f64::NAN, 2)])).css().0;
    assert_eq!(css, BOX);
    assert_eq!(Shape::Pie(Pie::new([(0.0, 1)])).slice_count(), 0);
}

#[test]
fn arc_setters_clamp_and_ignore_non_finite_values() {
    let arc = Arc::new(1.5, 0x1FF_FFFF)
        .thickness_pct(150.0)
        .sweep_deg(-5.0)
        .start_deg(f32::NAN);
    assert_eq!(arc.value, 1.0);
    assert_eq!(arc.color, 0xFF_FFFF);
    assert_eq!(arc.thickness_pct, 100.0);
    assert_eq!(arc.sweep_deg, 0.0);
    assert_eq!(arc.start_deg, 0.0);
    assert_eq!(Arc::new(f32::NAN, 0).value, 0.0);
    assert_eq!(Arc::new(-1.0, 0).value, 0.0);
    let arc = Arc::new(0.5, 0)
        .thickness_pct(f32::INFINITY)
        .sweep_deg(f32::NAN);
    assert_eq!(arc.thickness_pct, DEFAULT_THICKNESS_PCT);
    assert_eq!(arc.sweep_deg, 360.0);
}

#[test]
fn an_arc_has_a_value_a_track_and_a_hole() {
    let arc = Arc::new(0.25, 0xFF_0000).track(0x00_FF00).sweep_deg(270.0);
    assert_eq!(Shape::Arc(arc.clone()).slice_count(), 2);
    let (css, caps) = Shape::Arc(arc).css();
    assert!(
        css.contains(
            "conic-gradient(from 0.000deg, #ff0000 0.000deg 67.500deg, \
             #00ff00 67.500deg 270.000deg, transparent 270.000deg 360deg)"
        ),
        "{css}"
    );
    assert!(
        css.contains("radial-gradient(circle closest-side, transparent 79.000%, #000 80.000%)"),
        "{css}"
    );
    assert!(caps.is_empty());
}

#[test]
fn round_caps_sit_on_the_middle_radius_at_both_ends() {
    let arc = Arc::new(0.5, 0xFF_0000).round_caps(true);
    let (_, caps) = Shape::Arc(arc).css();
    // Thickness 20 % of the radius = 10 % of the box; middle radius 45 %.
    assert_eq!(caps.len(), 2);
    assert!(
        caps[0].contains("left: 45.000%; top: 0.000%; width: 10.000%"),
        "{}",
        caps[0]
    );
    assert!(
        caps[1].contains("left: 45.000%; top: 90.000%;"),
        "{}",
        caps[1]
    );
    assert!(caps[1].ends_with("background-color: #ff0000"));
    let (_, none) = Shape::Arc(Arc::new(0.0, 0).round_caps(true)).css();
    assert!(none.is_empty());
}

#[test]
fn a_shape_converts_into_a_node_with_its_classes_and_style() {
    let node: Node = Pie::new([(1u8, 0)])
        .classes(Classes::__literal("size-8"))
        .style(Style::new().opacity(0.5))
        .into();
    assert_eq!(node.class_list().as_str(), "size-8");
    assert_eq!(node.style_values(), &Style::new().opacity(0.5));
    assert!(node.child_nodes().is_empty());
    let mut nodes = Vec::new();
    Node::__push(&mut nodes, Arc::new(0.5, 0));
    assert!(matches!(
        nodes[0].kind(),
        super::super::Kind::Shape(Shape::Arc(_))
    ));
}
