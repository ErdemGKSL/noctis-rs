//! [`Components`]: the top-level component list of a message, with numeric
//! ids assigned in pre-order (CV2-14).

use serde::{Serialize, Serializer};

use crate::component::TopLevelComponent;
use crate::placement::TopLevel;
use crate::route::CustomId;
use crate::wire::{Ids, Wire, WiredSeq};

/// The components of a message body, in order.
///
/// It serializes as a JSON array and numbers every component in pre-order
/// from 1 (`"id"` right after `"type"`); select options and gallery items
/// get none, and no API sets an id (CV2-14).
///
/// ```
/// use noctis_ui::{Components, Container, Separator, TextDisplay};
///
/// let body = Components::new()
///     .push(Container::new().push(TextDisplay::new("a")))
///     .push(Separator::new());
/// assert_eq!(
///     serde_json::to_value(&body).unwrap(),
///     serde_json::json!([
///         {"type": 17, "id": 1, "components": [{"type": 10, "id": 2, "content": "a"}]},
///         {"type": 14, "id": 3, "divider": true, "spacing": 1}
///     ])
/// );
/// ```
#[derive(Debug, Clone, Default)]
pub struct Components {
    pub(crate) items: Vec<TopLevelComponent>,
}

impl Components {
    /// An empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `component`.
    #[must_use]
    pub fn push<T: TopLevel>(mut self, component: T) -> Self {
        self.items.push(component.into_top());
        self
    }

    /// The number of top-level components.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there is no component.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every Route `custom_id` of the tree, in pre-order; core resolves the
    /// unresolved ones before sending (AD-18).
    #[doc(hidden)]
    pub fn custom_ids_mut(&mut self) -> impl Iterator<Item = &mut CustomId> {
        let mut out = Vec::new();
        for item in &mut self.items {
            item.custom_ids_mut(&mut out);
        }
        out.into_iter()
    }

    /// Replaces every [`Text::Key`](crate::Text::Key) of the tree (Route
    /// button labels, select placeholders) with `Literal(f(key))`.
    pub(crate) fn resolve_keys(&mut self, f: &mut dyn FnMut(noctis_rules::MessageKey) -> String) {
        let mut out = Vec::new();
        for item in &mut self.items {
            item.texts_mut(&mut out);
        }
        for text in out {
            text.resolve(f);
        }
    }
}

impl Serialize for Components {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let ids = Ids::default();
        WiredSeq(&self.items, Some(&ids)).serialize(s)
    }
}
