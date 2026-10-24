//! Lowering: a Noctis [`Node`] tree becomes a takumi node tree.
//!
//! An element becomes a container, a text node a takumi text node, an image
//! node a takumi image node over an already decoded source: the App's
//! static image, or the job's resolved remote image (takumi never fetches). Tags set
//! no tag name and no preset (tags carry no default styles). Non-blank
//! classes become takumi Tailwind values; a non-default [`Style`] becomes
//! inline style declarations, which takumi's cascade applies after the
//! (non-`!important`) Tailwind layer, so a `Style` value wins over a class
//! of the same property.
//!
//! A text node holding Discord custom emoji tokens (IMG-04) becomes an
//! inline container carrying the node's classes and style, whose children
//! are inline text runs and `1.375em` inline images of the job's resolved
//! emoji; the markup itself is never drawn.

use std::fmt::Write as _;
use std::str::FromStr;

use noctis_ports::RenderError;
use takumi_core::style::TailwindValues;
use twilight_model::id::Id;

use takumi::prelude::{ImageData, ImageSourceInput};

use crate::assets::StaticImages;
use crate::emoji::{self, Piece};
use crate::node::{Dimension, ImageSrc, Kind};
use crate::remote::{RemoteImage, Resolved};
use crate::{Classes, Node, Style};

/// The inline style of a text node that holds custom emoji.
const INLINE_TEXT: &str = "display: inline";
/// The inline style of one custom emoji, sized like Discord's inline emoji.
const EMOJI_STYLE: &str = "display: inline-block; width: 1.375em; height: 1.375em; \
                           vertical-align: -0.3em; object-fit: contain";

/// The takumi tree of `node`, a view of the Template `template` drawing the
/// App's static `images` and the job's resolved `remote` images.
///
/// # Errors
///
/// [`RenderError::Failed`] when takumi rejects the classes or style of a
/// node; [`RenderError::UnknownImage`] for a static image that is not in
/// `images` (or is, with other bytes); [`RenderError::UndeclaredImage`] for
/// a remote image (other than the placeholder) or a custom emoji that is not
/// in `remote`.
pub(crate) fn lower(
    node: &Node,
    images: &StaticImages,
    remote: &Resolved,
    template: &str,
) -> Result<takumi::prelude::Node, RenderError> {
    // The declarations that go before the node's own style.
    let mut base_css = "";
    let lowered = match node.kind() {
        Kind::Element { children, .. } => takumi::prelude::Node::container(
            children
                .iter()
                .map(|child| lower(child, images, remote, template))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Kind::Text(text) => {
            let text = text.as_ref();
            if emoji::split(text).all(|piece| matches!(piece, Piece::Text(_))) {
                takumi::prelude::Node::text(text)
            } else {
                base_css = INLINE_TEXT;
                inline_text(text, remote, template)?
            }
        }
        Kind::Image(src) => {
            let source = match src {
                ImageSrc::Static(image) => {
                    images.get(image).ok_or_else(|| RenderError::UnknownImage {
                        id: template.to_owned(),
                        image: image.id().to_owned(),
                    })?
                }
                ImageSrc::Remote(image) => {
                    remote
                        .get(image)
                        .ok_or_else(|| RenderError::UndeclaredImage {
                            id: template.to_owned(),
                            image: image.to_string(),
                        })?
                }
            };
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
    Ok(match inline_style(base_css, node.style_values())? {
        Some(style) => lowered.with_style(style),
        None => lowered,
    })
}

/// The children of a text node holding custom emoji: text runs and inline
/// images of the resolved emoji (looked up by id and `a` flag; the name is
/// ignored), in an unstyled container.
fn inline_text(
    text: &str,
    remote: &Resolved,
    template: &str,
) -> Result<takumi::prelude::Node, RenderError> {
    let emoji_style = takumi::prelude::Style::from_str(EMOJI_STYLE).map_err(RenderError::failed)?;
    let children = emoji::split(text)
        .map(|piece| match piece {
            Piece::Text(run) => Ok(takumi::prelude::Node::text(run)),
            Piece::Emoji(e) => {
                let image = RemoteImage::custom_emoji(Id::from(e.id), e.animated);
                let source = remote
                    .get(&image)
                    .ok_or_else(|| RenderError::UndeclaredImage {
                        id: template.to_owned(),
                        image: image.to_string(),
                    })?;
                Ok(takumi::prelude::Node::image(ImageData {
                    src: ImageSourceInput::Loaded(source.clone()),
                    width: None,
                    height: None,
                })
                .with_style(emoji_style.clone()))
            }
        })
        .collect::<Result<Vec<_>, RenderError>>()?;
    Ok(takumi::prelude::Node::container(children))
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

/// The inline declarations `base` followed by those of `style`; `None`
/// when both are empty.
fn inline_style(base: &str, style: &Style) -> Result<Option<takumi::prelude::Style>, RenderError> {
    let own = style_css(style);
    let css = match (base.is_empty(), own.is_empty()) {
        (true, true) => return Ok(None),
        (true, false) => own,
        (false, true) => base.to_owned(),
        (false, false) => format!("{base}; {own}"),
    };
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
