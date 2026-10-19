//! Discord JSON for the component model: numeric ids, the shared map head
//! and the pre-order walk over `custom_id`s.

use core::cell::Cell;

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::route::CustomId;

/// The numeric id counter of one [`Components`](crate::Components) body:
/// pre-order, starting at 1 (CV2-14).
#[derive(Debug, Default)]
pub(crate) struct Ids(Cell<u32>);

impl Ids {
    fn next(&self) -> u32 {
        let id = self.0.get().saturating_add(1);
        self.0.set(id);
        id
    }
}

/// A component's JSON, with numeric ids when `ids` is given.
pub(crate) trait Wire {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error>;

    /// Pushes every `custom_id` of the subtree, in pre-order.
    fn custom_ids_mut<'a>(&'a mut self, _out: &mut Vec<&'a mut CustomId>) {}
}

/// Serializes a [`Wire`] value with the given id counter.
pub(crate) struct Wired<'a, T: ?Sized>(pub(crate) &'a T, pub(crate) Option<&'a Ids>);

impl<T: Wire + ?Sized> Serialize for Wired<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.wire(self.1, s)
    }
}

/// Serializes a slice of [`Wire`] values as a JSON array.
pub(crate) struct WiredSeq<'a, T>(pub(crate) &'a [T], pub(crate) Option<&'a Ids>);

impl<T: Wire> Serialize for WiredSeq<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for item in self.0 {
            seq.serialize_element(&Wired(item, self.1))?;
        }
        seq.end()
    }
}

/// Starts a component object: `type`, then `id` when numbering.
pub(crate) fn head<S: Serializer>(
    s: S,
    ty: u8,
    ids: Option<&Ids>,
) -> Result<S::SerializeMap, S::Error> {
    let mut map = s.serialize_map(None)?;
    map.serialize_entry("type", &ty)?;
    if let Some(ids) = ids {
        map.serialize_entry("id", &ids.next())?;
    }
    Ok(map)
}

/// Writes `key` only when `value` is `Some`.
pub(crate) fn opt<M: SerializeMap, T: Serialize + ?Sized>(
    map: &mut M,
    key: &'static str,
    value: Option<&T>,
) -> Result<(), M::Error> {
    match value {
        Some(v) => map.serialize_entry(key, v),
        None => Ok(()),
    }
}

/// Writes `key: true` only when `value` is `true`.
pub(crate) fn flag<M: SerializeMap>(
    map: &mut M,
    key: &'static str,
    value: bool,
) -> Result<(), M::Error> {
    if value {
        map.serialize_entry(key, &true)
    } else {
        Ok(())
    }
}

/// Implements `Serialize` (without numeric ids) through [`Wire`].
macro_rules! serialize_via_wire {
    ($($ty:ty),* $(,)?) => {$(
        impl serde::Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                $crate::wire::Wire::wire(self, None, s)
            }
        }
    )*};
}
pub(crate) use serialize_via_wire;
