//! The node model: builders, `IntoNodes`, `Classes` and `Style`.

use std::borrow::Cow;

use super::style::Dimension;
use super::*;

fn kids(node: &Node) -> &[Node] {
    node.child_nodes()
}

#[test]
fn element_builders_and_later_calls_win() {
    let node = Node::element(Tag::P)
        .classes(Classes::dynamic("a"))
        .classes(Classes::__literal("b"))
        .style(Style::new().gap_px(1))
        .style(Style::new().gap_px(2));
    assert_eq!(node.class_list().as_str(), "b");
    assert_eq!(node.style, Style::new().gap_px(2));
    assert_eq!(
        node.kind,
        Kind::Element {
            tag: Tag::P,
            children: Vec::new()
        }
    );
}

#[test]
fn text_nodes_take_classes_and_style_but_no_children() {
    let node = Node::text("hi")
        .classes(Classes::__literal("font-bold"))
        .style(Style::new().color_rgb(0x00FF_0000))
        .child("ignored")
        .children(["also", "ignored"]);
    assert_eq!(node.kind, Kind::Text(Cow::Borrowed("hi")));
    assert_eq!(node.class_list().as_str(), "font-bold");
    assert_eq!(node.style.color, Some(0x00FF_0000));
    assert!(kids(&node).is_empty());
}

#[test]
fn free_functions() {
    let classes = Classes::__literal("flex");
    assert_eq!(
        div(classes.clone()),
        Node::element(Tag::Div).classes(classes.clone())
    );
    assert_eq!(
        span(classes.clone()),
        Node::element(Tag::Span).classes(classes.clone())
    );
    assert_eq!(text("x", classes.clone()), Node::text("x").classes(classes));
}

#[test]
fn into_nodes_text_kinds() {
    let owned = String::from("owned");
    let node = div(Classes::default())
        .child("lit")
        .child(owned.clone())
        .child(&owned)
        .child(Cow::<'static, str>::Borrowed("cow"))
        .child(42u8)
        .child(-7i64)
        .child(1.5f32)
        .child(2.25f64)
        .child(usize::MAX);
    let texts: Vec<Node> = [
        "lit".to_owned(),
        "owned".to_owned(),
        "owned".to_owned(),
        "cow".to_owned(),
        "42".to_owned(),
        "-7".to_owned(),
        "1.5".to_owned(),
        "2.25".to_owned(),
        usize::MAX.to_string(),
    ]
    .into_iter()
    .map(Node::text)
    .collect();
    assert_eq!(kids(&node), texts.as_slice());
}

#[test]
fn into_nodes_option_and_vec() {
    let missing: Option<Node> = None;
    let node = div(Classes::default())
        .child(missing)
        .child(Some(Node::text("a")))
        .child(Some("b"))
        .child(Vec::<Node>::new())
        .child(vec![Node::text("c"), Node::text("d")]);
    assert_eq!(
        kids(&node),
        [
            Node::text("a"),
            Node::text("b"),
            Node::text("c"),
            Node::text("d")
        ]
    );
}

#[test]
fn children_appends_in_order() {
    let node = div(Classes::default()).children(vec![Some("a"), None, Some("b")]);
    assert_eq!(kids(&node), [Node::text("a"), Node::text("b")]);
    let mut nodes = Vec::new();
    Node::__push(&mut nodes, "x");
    Node::__push(&mut nodes, None::<u8>);
    Node::__push(&mut nodes, 3u16);
    assert_eq!(nodes, [Node::text("x"), Node::text("3")]);
}

#[test]
fn user_text_stays_text() {
    let evil = "<b tw=\"hidden\">";
    let node = div(Classes::default()).child(evil);
    assert_eq!(kids(&node), [Node::text(evil)]);
    assert!(kids(&kids(&node)[0]).is_empty());
}

#[test]
fn classes_literal_dynamic_and_join() {
    const CONST: Classes = Classes::__literal("flex");
    let lit = Classes::__literal("p-4");
    assert!(lit.is_checked());
    assert_eq!(lit.as_str(), "p-4");
    let dynamic = Classes::dynamic("x");
    assert!(!dynamic.is_checked());
    let joined = lit.clone().and(dynamic);
    assert_eq!(joined.as_str(), "p-4 x");
    assert!(!joined.is_checked());
    let both = lit.clone().and(Classes::__literal("m-2"));
    assert_eq!(both.as_str(), "p-4 m-2");
    assert!(both.is_checked());
    assert_eq!(Classes::dynamic("a").and(lit).as_str(), "a p-4");
    let default = Classes::default();
    assert_eq!(default.as_str(), "");
    assert!(default.is_checked());
    assert_eq!(CONST.as_str(), "flex");
}

#[test]
fn style_percent_clamps_and_ignores_non_finite() {
    let pct = |v: f32| Style::new().width_pct(v).width;
    assert_eq!(pct(-1.0), Some(Dimension::Percent(0.0)));
    assert_eq!(pct(150.0), Some(Dimension::Percent(100.0)));
    assert_eq!(pct(42.5), Some(Dimension::Percent(42.5)));
    assert_eq!(pct(f32::NAN), None);
    assert_eq!(pct(f32::INFINITY), None);
    let kept = Style::new().width_pct(10.0).width_pct(f32::NAN);
    assert_eq!(kept.width, Some(Dimension::Percent(10.0)));
    let h = Style::new().height_pct(101.0).height_pct(f32::NEG_INFINITY);
    assert_eq!(h.height, Some(Dimension::Percent(100.0)));
}

#[test]
fn style_opacity_clamps() {
    let op = |v: f32| Style::new().opacity(v).opacity;
    assert_eq!(op(1.5), Some(1.0));
    assert_eq!(op(-0.1), Some(0.0));
    assert_eq!(op(0.25), Some(0.25));
    assert_eq!(op(f32::NAN), None);
    assert_eq!(
        Style::new().opacity(0.5).opacity(f32::NAN).opacity,
        Some(0.5)
    );
}

#[test]
fn style_rgb_uses_the_low_24_bits() {
    let s = Style::new()
        .bg_rgb(0x1_5865F2)
        .color_rgb(0xFF00_0000)
        .border_color_rgb(0x00AB_CDEF);
    assert_eq!(s.bg, Some(0x0058_65F2));
    assert_eq!(s.color, Some(0));
    assert_eq!(s.border_color, Some(0x00AB_CDEF));
}

#[test]
fn style_rotate_and_translate() {
    assert_eq!(Style::new().rotate_deg(-720.0).rotate, Some(-720.0));
    assert_eq!(
        Style::new()
            .rotate_deg(10.0)
            .rotate_deg(f32::NEG_INFINITY)
            .rotate,
        Some(10.0)
    );
    assert_eq!(Style::new().translate_px(-3, 4).translate, Some((-3, 4)));
}

#[test]
fn style_later_setter_wins() {
    const CONST: Style = Style::new().width_px(3).opacity(2.0);
    assert_eq!(
        Style::new().width_px(5).width_pct(50.0).width,
        Some(Dimension::Percent(50.0))
    );
    assert_eq!(
        Style::new().height_pct(5.0).height_px(7).height,
        Some(Dimension::Px(7))
    );
    assert_eq!(Style::new().gap_px(1).gap_px(2).gap, Some(2));
    assert_eq!(Style::new(), Style::default());
    assert_eq!(CONST.opacity, Some(1.0));
}
