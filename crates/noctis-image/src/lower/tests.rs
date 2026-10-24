//! Lowering a Noctis tree into takumi nodes.

use takumi::prelude::{ImageSource, NodeKind};
use twilight_model::id::Id;

use super::*;
use crate::remote::Resolved;
use crate::{RemoteImage, StaticImage, Tag, div, img, text};

/// No resolved remote image; the placeholder draws a red pixel.
fn no_remote() -> Resolved {
    Resolved::empty(ImageSource::from_bytes(png()).unwrap())
}

/// `lower` without static or remote images.
fn plain(node: &Node) -> Result<takumi::prelude::Node, RenderError> {
    lower(node, &StaticImages::default(), &no_remote(), "t")
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
    let parsed = inline_style("", &style).unwrap().unwrap();
    assert!(!parsed.declarations.is_empty());
    assert!(inline_style("", &Style::new()).unwrap().is_none());
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
    let lowered = lower(
        &img(logo, Classes::dynamic("w-4")),
        &images,
        &no_remote(),
        "card",
    )
    .unwrap();
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
        match lower(&tree, &images, &no_remote(), "card") {
            Err(RenderError::UnknownImage { id, image }) => {
                assert_eq!((id.as_str(), image.as_str()), ("card", missing.id()));
            }
            other => panic!("expected UnknownImage, got {other:?}"),
        }
    }
}

#[test]
fn img_03_remote_images_draw_their_resolved_source_or_the_fallback() {
    let avatar = RemoteImage::url("https://cdn.discordapp.com/a.png").unwrap();
    let blue = ImageSource::from_bytes(png_of([0, 0, 255, 255])).unwrap();
    let remote = Resolved::with(
        vec![(avatar.clone(), blue.clone())],
        ImageSource::from_bytes(png()).unwrap(),
    );
    let none = StaticImages::default();
    let lowered = lower(&img(&avatar, Classes::default()), &none, &remote, "card").unwrap();
    let expected = takumi::prelude::Node::image(ImageData {
        src: ImageSourceInput::Loaded(blue),
        width: None,
        height: None,
    });
    assert_eq!(lowered.to_html(), expected.to_html());
    let placeholder = lower(
        &img(RemoteImage::placeholder(), Classes::default()),
        &none,
        &no_remote(),
        "card",
    );
    assert!(placeholder.is_ok());
}

#[test]
fn img_03_undeclared_remote_image_is_an_error() {
    let other = RemoteImage::url("https://cdn.discordapp.com/b.png?x=1").unwrap();
    let tree = div(Classes::default()).child(img(&other, Classes::default()));
    match lower(&tree, &StaticImages::default(), &no_remote(), "card") {
        Err(RenderError::UndeclaredImage { id, image }) => {
            assert_eq!(
                (id.as_str(), image.as_str()),
                ("card", "https://cdn.discordapp.com/b.png")
            );
        }
        other => panic!("expected UndeclaredImage, got {other:?}"),
    }
}

const EMOJI_ID: u64 = 123_456_789_012_345_678;

fn emoji_remote() -> Resolved {
    let pepe = RemoteImage::custom_emoji(Id::new(EMOJI_ID), false);
    Resolved::with(
        vec![(
            pepe,
            ImageSource::from_bytes(png_of([0, 0, 255, 255])).unwrap(),
        )],
        ImageSource::from_bytes(png()).unwrap(),
    )
}

#[test]
fn img_04_text_without_tokens_stays_one_text_node() {
    for text in ["plain", "<:p:1>", "a < b > c"] {
        let lowered = lower(
            &Node::text(text),
            &StaticImages::default(),
            &emoji_remote(),
            "t",
        )
        .unwrap();
        assert_eq!(
            lowered.to_html(),
            takumi::prelude::Node::text(text).to_html(),
            "{text:?}"
        );
    }
}

#[test]
fn img_04_custom_emoji_split_into_inline_runs_and_images() {
    let node = text(
        format!("ab <:pepe:{EMOJI_ID}> cd"),
        Classes::dynamic("text-lg"),
    )
    .style(Style::new().color_rgb(0x00_FF00));
    let lowered = lower(&node, &StaticImages::default(), &emoji_remote(), "t").unwrap();
    let NodeKind::Container { children } = &lowered.kind else {
        panic!("{:?}", lowered.kind);
    };
    assert_eq!(children.len(), 3);
    assert!(matches!(&children[0].kind, NodeKind::Text(t) if t.text == "ab "));
    assert!(matches!(&children[1].kind, NodeKind::Image(_)));
    assert!(matches!(&children[2].kind, NodeKind::Text(t) if t.text == " cd"));
    let html = lowered.to_html();
    assert!(!html.contains("pepe"), "{html}");
    let NodeKind::Image(data) = &children[1].kind else {
        unreachable!()
    };
    let expected = takumi::prelude::Node::container([
        takumi::prelude::Node::text("ab "),
        takumi::prelude::Node::image(data.clone())
            .with_style(takumi::prelude::Style::from_str(EMOJI_STYLE).unwrap()),
        takumi::prelude::Node::text(" cd"),
    ])
    .with_tw(TailwindValues::from_str("text-lg").unwrap())
    .with_style(takumi::prelude::Style::from_str("display: inline; color: #00ff00").unwrap());
    assert_eq!(html, expected.to_html());
}

#[test]
fn img_04_undeclared_custom_emoji_is_an_error() {
    let id = 98_765_432_109_876_543_u64;
    let node = Node::text(format!("<:x1:{id}>"));
    match lower(&node, &StaticImages::default(), &emoji_remote(), "card") {
        Err(RenderError::UndeclaredImage { id: t, image }) => {
            assert_eq!((t.as_str(), image), ("card", format!("emoji/{id}")));
        }
        other => panic!("expected UndeclaredImage, got {other:?}"),
    }
    // The animated flag is part of the lookup; the name is not.
    let animated = Node::text(format!("<a:pepe:{EMOJI_ID}>"));
    assert!(lower(&animated, &StaticImages::default(), &emoji_remote(), "card").is_err());
    let renamed = Node::text(format!("<:PEPE:{EMOJI_ID}>"));
    assert!(lower(&renamed, &StaticImages::default(), &emoji_remote(), "card").is_ok());
}
