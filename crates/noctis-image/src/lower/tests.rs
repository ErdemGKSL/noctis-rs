//! Lowering a Noctis tree into takumi nodes.

use takumi::prelude::NodeKind;

use super::*;
use crate::{Tag, div, text};

#[test]
fn element_becomes_a_container_with_its_children() {
    let tree = Node::element(Tag::H1)
        .child("a")
        .child(Node::element(Tag::Span).child(1_u8));
    let lowered = lower(&tree).unwrap();
    let NodeKind::Container { children } = &lowered.kind else {
        panic!("{:?}", lowered.kind);
    };
    assert_eq!(children.len(), 2);
    assert!(matches!(&children[0].kind, NodeKind::Text(t) if t.text == "a"));
    assert!(matches!(&children[1].kind, NodeKind::Container { children } if children.len() == 1));
}

#[test]
fn text_becomes_a_text_node_never_parsed() {
    let lowered = lower(&Node::text("<b tw=\"hidden\">")).unwrap();
    assert!(matches!(&lowered.kind, NodeKind::Text(t) if t.text == "<b tw=\"hidden\">"));
}

#[test]
fn lowered_trees_compare_equal_to_hand_built_takumi_trees() {
    let tree = div(Classes::dynamic("flex p-4")).child(text("x", Classes::default()));
    let expected = takumi::prelude::Node::container([takumi::prelude::Node::text("x")])
        .with_tw(TailwindValues::from_str("flex p-4").unwrap());
    assert_eq!(lower(&tree).unwrap().to_html(), expected.to_html());
}

#[test]
fn blank_classes_and_default_style_set_nothing() {
    for classes in ["", "  \t"] {
        let lowered = lower(&div(Classes::dynamic(classes))).unwrap();
        assert_eq!(
            lowered.to_html(),
            takumi::prelude::Node::container([]).to_html()
        );
    }
}

#[test]
fn tailwind_values_parse_a_literal() {
    let values = tailwind(&Classes::dynamic("w-full bg-red-500")).unwrap();
    assert_eq!(
        values,
        Some(TailwindValues::from_str("w-full bg-red-500").unwrap())
    );
    assert_eq!(tailwind(&Classes::default()).unwrap(), None);
}

#[test]
fn style_lowers_to_declarations() {
    assert_eq!(style_css(&Style::new()), "");
    let style = Style::new()
        .width_pct(42.5)
        .height_px(10)
        .gap_px(4)
        .bg_rgb(0x00_FF00)
        .color_rgb(0x0A_0B0C)
        .border_color_rgb(0x58_65F2)
        .opacity(0.5)
        .translate_px(-3, 4)
        .rotate_deg(-720.0);
    assert_eq!(
        style_css(&style),
        "width: 42.5%; height: 10px; gap: 4px; background-color: #00ff00; color: #0a0b0c; \
         border-color: #5865f2; opacity: 0.5; translate: -3px 4px; rotate: -720deg;"
    );
    let parsed = inline_style(&style).unwrap().unwrap();
    assert!(!parsed.declarations.is_empty());
    assert!(inline_style(&Style::new()).unwrap().is_none());
    assert!(lower(&div(Classes::default()).style(style)).is_ok());
    // Styles apply to text nodes too.
    assert!(lower(&Node::text("t").style(Style::new().color_rgb(1))).is_ok());
}
