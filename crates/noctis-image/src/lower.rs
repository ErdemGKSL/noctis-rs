//! Lowering: a Noctis [`Node`] tree becomes a takumi node tree.
//!
//! An element becomes a container, a text node a takumi text node, an image
//! node a takumi image node over the App's already decoded source (takumi
//! never fetches). Tags set
//! no tag name and no preset (tags carry no default styles). Non-blank
//! classes become takumi Tailwind values; a non-default [`Style`] becomes
//! inline style declarations, which takumi's cascade applies after the
//! (non-`!important`) Tailwind layer, so a `Style` value wins over a class
//! of the same property.

use std::fmt::Write as _;
use std::str::FromStr;

use noctis_ports::RenderError;
use takumi_core::style::TailwindValues;

use takumi::prelude::{ImageData, ImageSourceInput};

use crate::assets::StaticImages;
use crate::node::{Dimension, Kind};
use crate::{Classes, Node, Style};

/// The takumi tree of `node`, a view of the Template `template` drawing the
/// App's static `images`.
///
/// # Errors
///
/// [`RenderError::Failed`] when takumi rejects the classes or style of a
/// node; [`RenderError::UnknownImage`] for an image node whose image is not
/// in `images` (or is, with other bytes).
pub(crate) fn lower(
    node: &Node,
    images: &StaticImages,
    template: &str,
) -> Result<takumi::prelude::Node, RenderError> {
    let lowered = match node.kind() {
        Kind::Element { children, .. } => takumi::prelude::Node::container(
            children
                .iter()
                .map(|child| lower(child, images, template))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Kind::Text(text) => takumi::prelude::Node::text(text.as_ref()),
        Kind::Image(src) => {
            let source = images.get(src).ok_or_else(|| RenderError::UnknownImage {
                id: template.to_owned(),
                image: src.id().to_owned(),
            })?;
            takumi::prelude::Node::image(ImageData {
                src: ImageSourceInput::Loaded(source.clone()),
                width: None,
                height: None,
            })
        }
    };
    let lowered = match tailwind(node.class_list())? {
        Some(values) => lowered.with_tw(values),
        None => lowered,
    };
    Ok(match inline_style(node.style_values())? {
        Some(style) => lowered.with_style(style),
        None => lowered,
    })
}

/// The Tailwind values of `classes`; `None` when they are blank.
fn tailwind(classes: &Classes) -> Result<Option<TailwindValues>, RenderError> {
    let text = classes.as_str();
    if text.trim().is_empty() {
        return Ok(None);
    }
    TailwindValues::from_str(text)
        .map(Some)
        .map_err(RenderError::failed)
}

/// The inline declarations of `style`; `None` for the default style.
fn inline_style(style: &Style) -> Result<Option<takumi::prelude::Style>, RenderError> {
    let css = style_css(style);
    if css.is_empty() {
        return Ok(None);
    }
    takumi::prelude::Style::from_str(&css)
        .map(Some)
        .map_err(RenderError::failed)
}

/// `style` as a CSS declaration list; empty for the default style.
pub(crate) fn style_css(style: &Style) -> String {
    let mut css = String::new();
    // Writing to a `String` cannot fail.
    let mut push = |args: std::fmt::Arguments<'_>| {
        let _ = css.write_fmt(args);
        css.push_str("; ");
    };
    if let Some(width) = style.width {
        push(format_args!("width: {}", dimension(width)));
    }
    if let Some(height) = style.height {
        push(format_args!("height: {}", dimension(height)));
    }
    if let Some(gap) = style.gap {
        push(format_args!("gap: {gap}px"));
    }
    if let Some(bg) = style.bg {
        push(format_args!("background-color: #{bg:06x}"));
    }
    if let Some(color) = style.color {
        push(format_args!("color: #{color:06x}"));
    }
    if let Some(border) = style.border_color {
        push(format_args!("border-color: #{border:06x}"));
    }
    if let Some(opacity) = style.opacity {
        push(format_args!("opacity: {opacity}"));
    }
    if let Some((x, y)) = style.translate {
        push(format_args!("translate: {x}px {y}px"));
    }
    if let Some(rotate) = style.rotate {
        push(format_args!("rotate: {rotate}deg"));
    }
    css.truncate(css.trim_end().len());
    css
}

fn dimension(dimension: Dimension) -> String {
    match dimension {
        Dimension::Px(px) => format!("{px}px"),
        Dimension::Percent(pct) => format!("{pct}%"),
    }
}

#[cfg(test)]
mod tests;
