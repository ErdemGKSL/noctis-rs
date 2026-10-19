//! Text Display (type 10) and Separator (type 14).

use std::borrow::Cow;

use serde::Serializer;
use serde::ser::SerializeMap;

use crate::wire::{Ids, Wire, head, serialize_via_wire};

/// A Text Display (type 10): Markdown text.
///
/// Placement: top level, a Container, or one of a Section's texts.
///
/// ```
/// use noctis_ui::TextDisplay;
///
/// let text = TextDisplay::new("## Hi");
/// assert_eq!(
///     serde_json::to_value(&text).unwrap(),
///     serde_json::json!({"type": 10, "content": "## Hi"})
/// );
/// ```
///
/// Numeric ids are assigned by [`Components`](crate::Components); no API sets
/// one:
///
/// ```compile_fail,E0599
/// use noctis_ui::TextDisplay;
///
/// let _ = TextDisplay::new("a").id(5); // [CV2-14]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDisplay {
    content: Cow<'static, str>,
}

impl TextDisplay {
    /// A Text Display showing `content` (Markdown).
    pub fn new(content: impl Into<Cow<'static, str>>) -> Self {
        Self {
            content: content.into(),
        }
    }

    /// The Markdown content.
    pub fn content(&self) -> &str {
        &self.content
    }
}

impl Wire for TextDisplay {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 10, ids)?;
        map.serialize_entry("content", &self.content)?;
        map.end()
    }
}

/// A Separator's padding: `Small` (1, the default) or `Large` (2) (CV2-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Spacing {
    /// Small padding (`1`).
    #[default]
    Small,
    /// Large padding (`2`).
    Large,
}

impl Spacing {
    const fn value(self) -> u8 {
        match self {
            Self::Small => 1,
            Self::Large => 2,
        }
    }
}

/// A Separator (type 14): vertical padding with an optional divider line.
///
/// Placement: top level or a Container. The default has a divider and small
/// spacing.
///
/// ```
/// use noctis_ui::{Separator, Spacing};
///
/// assert_eq!(Separator::new(), Separator::small());
/// let large = Separator::large().divider(false);
/// assert_eq!(
///     serde_json::to_value(&large).unwrap(),
///     serde_json::json!({"type": 14, "divider": false, "spacing": 2})
/// );
/// assert_eq!(large.spacing(Spacing::Small), Separator::small().divider(false));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Separator {
    divider: bool,
    spacing: Spacing,
}

impl Separator {
    /// A divider with small spacing.
    pub const fn new() -> Self {
        Self::small()
    }

    /// A divider with small spacing.
    pub const fn small() -> Self {
        Self {
            divider: true,
            spacing: Spacing::Small,
        }
    }

    /// A divider with large spacing.
    pub const fn large() -> Self {
        Self {
            divider: true,
            spacing: Spacing::Large,
        }
    }

    /// Whether the divider line shows.
    #[must_use]
    pub const fn divider(self, divider: bool) -> Self {
        Self { divider, ..self }
    }

    /// The padding size.
    #[must_use]
    pub const fn spacing(self, spacing: Spacing) -> Self {
        Self { spacing, ..self }
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}

impl Wire for Separator {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 14, ids)?;
        map.serialize_entry("divider", &self.divider)?;
        map.serialize_entry("spacing", &self.spacing.value())?;
        map.end()
    }
}

serialize_via_wire!(TextDisplay, Separator);
