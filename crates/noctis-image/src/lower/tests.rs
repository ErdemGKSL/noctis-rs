//! Lowering a Noctis tree into takumi nodes.

use takumi::prelude::NodeKind;

use super::*;
use crate::{StaticImage, Tag, div, img, text};

/// `lower` without static images.
fn plain(node: &Node) -> Result<takumi::prelude::Node, RenderError> {
    lower(node, &StaticImages::default(), "t")
}

#[test]
fn element_becomes_a_container_with_its_children() {
    let tree = Node::element(Tag::H1)
        .child("a")
        .child(Node::element(Tag::Span).child(1_u8));
    let lowered = plain(&tree).unwrap();
    let NodeKind::Container { children } = &lowered.kind else {
        panic!("{:?}", lowered.kind);
    };
    assert_eq!(children.len(), 2);
    assert!(matches!(&children[0].kind, NodeKind::Text(t) if t.text == "a"));
    assert!(matches!(&children[1].kind, NodeKind::Container { children } if children.len() == 1));
}

#[test]
fn text_becomes_a_text_node_never_parsed() {
    let lowered = plain(&Node::text("<b tw=\"hidden\">")).unwrap();
    assert!(matches!(&lowered.kind, NodeKind::Text(t) if t.text == "<b tw=\"hidden\">"));
}

#[test]
fn lowered_trees_compare_equal_to_hand_built_takumi_trees() {
    let tree = div(Classes::dynamic("flex p-4")).child(text("x", Classes::default()));
    let expected = takumi::prelude::Node::container([takumi::prelude::Node::text("x")])
        .with_tw(TailwindValues::from_str("flex p-4").unwrap());
    assert_eq!(plain(&tree).unwrap().to_html(), expected.to_html());
}

#[test]
fn blank_classes_and_default_style_set_nothing() {
    for classes in ["", "  \t"] {
        let lowered = plain(&div(Classes::dynamic(classes))).unwrap();
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
    assert!(plain(&div(Classes::default()).style(style)).is_ok());
    // Styles apply to text nodes too.
    assert!(plain(&Node::text("t").style(Style::new().color_rgb(1))).is_ok());
}

/// A 1x1 PNG.
fn png() -> &'static [u8] {
    png_of([255, 0, 0, 255])
}

/// A 1x1 PNG of `rgba`.
fn png_of(rgba: [u8; 4]) -> &'static [u8] {
    let bitmap = takumi::prelude::Bitmap::from_raw(1, 1, rgba.to_vec()).unwrap();
    let mut out = Vec::new();
    takumi::write_image(&bitmap, &mut out, takumi::prelude::OutputFormat::Png).unwrap();
    out.leak()
}

#[test]
fn img_02_image_becomes_an_image_node_over_the_decoded_source() {
    let logo = StaticImage::new("logo", png());
    let images = StaticImages::decode(&[logo]).unwrap();
    let lowered = lower(&img(logo, Classes::dynamic("w-4")), &images, "card").unwrap();
    assert!(matches!(
        &lowered.kind,
        NodeKind::Image(data) if matches!(data.src, ImageSourceInput::Loaded(_))
            && data.width.is_none() && data.height.is_none()
    ));
    let source = images.get(&logo).unwrap().clone();
    let expected = takumi::prelude::Node::image(ImageData {
        src: ImageSourceInput::Loaded(source),
        width: None,
        height: None,
    })
    .with_tw(TailwindValues::from_str("w-4").unwrap());
    assert_eq!(lowered.to_html(), expected.to_html());
}

#[test]
fn img_02_unregistered_image_is_unknown_image() {
    let logo = StaticImage::new("logo", png());
    let other = StaticImage::new("logo", png_of([0, 0, 255, 255]));
    let images = StaticImages::decode(&[logo]).unwrap();
    for (missing, tree) in [
        (StaticImage::new("other", png()), div(Classes::default())),
        (other, div(Classes::default())),
    ] {
        let tree = tree.child(Node::image(missing));
        match lower(&tree, &images, "card") {
            Err(RenderError::UnknownImage { id, image }) => {
                assert_eq!((id.as_str(), image.as_str()), ("card", missing.id()));
            }
            other => panic!("expected UnknownImage, got {other:?}"),
        }
    }
}
