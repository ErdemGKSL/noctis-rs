//! Lowering: a Noctis [`Node`] tree becomes a takumi node tree.
//!
//! An element becomes a container, a text node a takumi text node. Tags set
//! no tag name and no preset (tags carry no default styles). Non-blank
//! classes become takumi Tailwind values; a non-default [`Style`] becomes
//! inline style declarations, which takumi's cascade applies after the
//! (non-`!important`) Tailwind layer, so a `Style` value wins over a class
//! of the same property.

use std::fmt::Write as _;
use std::str::FromStr;

use noctis_ports::RenderError;
use takumi_core::style::TailwindValues;

use crate::node::{Dimension, Kind};
use crate::{Classes, Node, Style};

/// The takumi tree of `node`.
///
/// # Errors
///
/// [`RenderError::Failed`] when takumi rejects the classes or style of a
/// node.
pub(crate) fn lower(node: &Node) -> Result<takumi::prelude::Node, RenderError> {
    let lowered = match node.kind() {
        Kind::Element { children, .. } => takumi::prelude::Node::container(
            children.iter().map(lower).collect::<Result<Vec<_>, _>>()?,
        ),
        Kind::Text(text) => takumi::prelude::Node::text(text.as_ref()),
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
