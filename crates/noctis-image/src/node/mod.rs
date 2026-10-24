//! The node tree a Template view returns: Noctis-owned and takumi-free.
//!
//! A [`Node`] is an element ([`Tag`], [`Classes`], [`Style`] and children),
//! a text node or an image node (an [`ImageSrc`]: a registered
//! [`StaticImage`] or a declared [`RemoteImage`]). User data enters a tree
//! only as text: text children are never
//! parsed, [`Classes`] come from `tw!` literals or [`Classes::dynamic`], and no
//! constructor turns a string into markup. Dynamic values (widths, colours,
//! percentages) use the typed [`Style`] setters, which override class values
//! of the same property when the Renderer draws the tree.
//!
//! `view!` expands to the builder calls below; they can be used directly:
//!
//! ```
//! use noctis_image::{Classes, Node, Style, Tag, div, text};
//!
//! let name = String::from("<b>Ada</b>");
//! let card = div(Classes::default())
//!     .style(Style::new().width_px(300))
//!     .child(text(name.clone(), Classes::default()))
//!     .child(Node::element(Tag::Span).child(42u8));
//! assert_eq!(card, card.clone());
//! ```

mod classes;
mod into_nodes;
mod style;
#[cfg(test)]
mod tests;

use std::borrow::Cow;

use noctis_ports::Size;

use crate::{RemoteImage, StaticImage};

pub use classes::Classes;
pub use into_nodes::IntoNodes;
pub(crate) use style::Dimension;
pub use style::Style;

use into_nodes::sealed::Sealed as _;

/// An element tag. Tags carry no default styles: every look comes from
/// classes and [`Style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Tag {
    /// `<div>`.
    Div,
    /// `<span>`.
    Span,
    /// `<p>`.
    P,
    /// `<h1>`.
    H1,
    /// `<h2>`.
    H2,
    /// `<h3>`.
    H3,
}

/// One node of a view tree: an element, a text node or an image.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    kind: Kind,
    classes: Classes,
    style: Style,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Element { tag: Tag, children: Vec<Node> },
    Text(Cow<'static, str>),
    Image(ImageSrc),
}

/// What an image node draws: a static image the App registered with
/// `ImageConfig::image`, or a remote image an `#[asset]` field of the
/// Template declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSrc {
    /// A registered [`StaticImage`].
    Static(StaticImage),
    /// A declared [`RemoteImage`].
    Remote(RemoteImage),
}

impl From<StaticImage> for ImageSrc {
    fn from(image: StaticImage) -> Self {
        Self::Static(image)
    }
}

impl From<&StaticImage> for ImageSrc {
    fn from(image: &StaticImage) -> Self {
        Self::Static(*image)
    }
}

impl From<RemoteImage> for ImageSrc {
    fn from(image: RemoteImage) -> Self {
        Self::Remote(image)
    }
}

impl From<&RemoteImage> for ImageSrc {
    fn from(image: &RemoteImage) -> Self {
        Self::Remote(image.clone())
    }
}

impl Node {
    /// An empty `tag` element without classes or style.
    pub fn element(tag: Tag) -> Self {
        Self {
            kind: Kind::Element {
                tag,
                children: Vec::new(),
            },
            classes: Classes::default(),
            style: Style::new(),
        }
    }

    /// A text node: `text` is drawn as is, never parsed as markup or classes.
    pub fn text(text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind: Kind::Text(text.into()),
            classes: Classes::default(),
            style: Style::new(),
        }
    }

    /// An image node drawing `src`: a [`StaticImage`] the App must register
    /// with `ImageConfig::image` (else the render fails with
    /// `RenderError::UnknownImage`), or a [`RemoteImage`] an `#[asset]`
    /// field declares (else `RenderError::UndeclaredImage`; the placeholder
    /// always draws). Without a size it takes the image's own; classes and
    /// [`Style`] size it like any node.
    pub fn image(src: impl Into<ImageSrc>) -> Self {
        Self {
            kind: Kind::Image(src.into()),
            classes: Classes::default(),
            style: Style::new(),
        }
    }

    /// Sets the classes; a later call replaces them. Applies to text and
    /// image nodes too.
    #[must_use]
    pub fn classes(mut self, classes: Classes) -> Self {
        self.classes = classes;
        self
    }

    /// Sets the style; a later call replaces it. Applies to text and image
    /// nodes too.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Appends `child` (0..n nodes, see [`IntoNodes`]). A no-op on a text
    /// or image node.
    #[must_use]
    pub fn child(mut self, child: impl IntoNodes) -> Self {
        if let Kind::Element { children, .. } = &mut self.kind {
            child.push_into(children);
        }
        self
    }

    /// Appends every item of `children`, in order. A no-op on a text or
    /// image node.
    #[must_use]
    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: IntoNodes,
    {
        if let Kind::Element { children: own, .. } = &mut self.kind {
            for child in children {
                child.push_into(own);
            }
        }
        self
    }

    /// Appends `item` to `nodes`; what `view!` expands `if` and `for`
    /// children to.
    #[doc(hidden)]
    pub fn __push(nodes: &mut Vec<Node>, item: impl IntoNodes) {
        item.push_into(nodes);
    }

    /// The node's classes.
    pub(crate) fn class_list(&self) -> &Classes {
        &self.classes
    }

    /// Whether the node is an element (with its tag and children), text or
    /// an image.
    pub(crate) fn kind(&self) -> &Kind {
        &self.kind
    }

    /// The node's style.
    pub(crate) fn style_values(&self) -> &Style {
        &self.style
    }

    /// The element's children; empty for a text or image node.
    pub(crate) fn child_nodes(&self) -> &[Node] {
        match &self.kind {
            Kind::Element { children, .. } => children,
            Kind::Text(_) | Kind::Image(_) => &[],
        }
    }

    /// The explicit root size this node declares that differs from
    /// `expected`, as `(width, height)` with `expected`'s side where only
    /// the other one differs; `None` when every declared side matches.
    ///
    /// The declared sides are the [`Style`] pixel width and height and each
    /// class without a variant (one leading or trailing `!` stripped) of the
    /// form `w-[Npx]`, `h-[Npx]` or `size-[Npx]`; spacing and percentage
    /// utilities are relative and ignored.
    pub(crate) fn root_size_mismatch(&self, expected: Size) -> Option<(u32, u32)> {
        let mut widths = Vec::new();
        let mut heights = Vec::new();
        if let Some(Dimension::Px(px)) = self.style.width {
            widths.push(px);
        }
        if let Some(Dimension::Px(px)) = self.style.height {
            heights.push(px);
        }
        for class in self.classes.as_str().split_whitespace() {
            if class.contains(':') {
                continue;
            }
            let class = class
                .strip_prefix('!')
                .or_else(|| class.strip_suffix('!'))
                .unwrap_or(class);
            let Some((prefix, px)) = arbitrary_px(class) else {
                continue;
            };
            match prefix {
                "w" => widths.push(px),
                "h" => heights.push(px),
                _ => {
                    widths.push(px);
                    heights.push(px);
                }
            }
        }
        let width = widths.into_iter().find(|&w| w != expected.width());
        let height = heights.into_iter().find(|&h| h != expected.height());
        if width.is_none() && height.is_none() {
            return None;
        }
        Some((
            width.unwrap_or(expected.width()),
            height.unwrap_or(expected.height()),
        ))
    }
}

/// `w-[Npx]`, `h-[Npx]` or `size-[Npx]` with `N` = `[0-9]{1,9}`, as
/// (prefix, N).
fn arbitrary_px(class: &str) -> Option<(&str, u32)> {
    let (prefix, rest) = class.split_once("-[")?;
    if !matches!(prefix, "w" | "h" | "size") {
        return None;
    }
    let digits = rest.strip_suffix("px]")?;
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((prefix, digits.parse().ok()?))
}

/// A `<div>` with `classes`.
pub fn div(classes: Classes) -> Node {
    Node::element(Tag::Div).classes(classes)
}

/// A `<span>` with `classes`.
pub fn span(classes: Classes) -> Node {
    Node::element(Tag::Span).classes(classes)
}

/// A text node with `classes`; `text` is never parsed.
pub fn text(text: impl Into<Cow<'static, str>>, classes: Classes) -> Node {
    Node::text(text).classes(classes)
}

/// An `<img>` drawing `src` (a [`StaticImage`] or a [`RemoteImage`], by
/// value or by reference) with `classes`; what `view!` expands
/// `<img src={..} />` to.
pub fn img(src: impl Into<ImageSrc>, classes: Classes) -> Node {
    Node::image(src).classes(classes)
}
