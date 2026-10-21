//! [`RefOps`]: Ref deletions a component or modal handler returns with its
//! response (FR-17, AD-18).

use noctis_codec::Ref;
use noctis_ports::{PortError, RefStore};

/// The Refs a component or modal handler asks core to delete once its
/// response was sent: return `(message, RefOps)` (or `(Responded, RefOps)`)
/// instead of the bare response.
///
/// Core deletes each listed Ref from the App's `RefStore`, in order, only
/// after the response was sent successfully (for `Responded`, as soon as
/// the handler returned `Ok`). A failed send, an error or a panic deletes
/// nothing. A failed delete is one warning and does not fail the
/// interaction. The step-flow Cancel case: delete the clicked Ref, read with
/// `ctx.current_ref()`.
///
/// ```
/// use noctis_codec::Ref;
/// use noctis_core::RefOps;
///
/// let (a, b) = (Ref::mint()?, Ref::mint()?);
/// let ops = RefOps::new().delete(a).delete(a).delete_all([b]);
/// assert_eq!(ops.refs(), &[a, b]);
/// assert_eq!(ops.len(), 2);
/// assert!(RefOps::new().is_empty());
/// # Ok::<(), noctis_codec::RefError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefOps {
    delete: Vec<Ref>,
}

impl RefOps {
    /// No operation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Deletes `r` after the response is sent; a Ref already listed is
    /// ignored.
    #[must_use]
    pub fn delete(mut self, r: Ref) -> Self {
        if !self.delete.contains(&r) {
            self.delete.push(r);
        }
        self
    }

    /// Deletes every Ref of `refs` after the response is sent (duplicates
    /// ignored).
    #[must_use]
    pub fn delete_all(self, refs: impl IntoIterator<Item = Ref>) -> Self {
        refs.into_iter().fold(self, Self::delete)
    }

    /// The Refs to delete, in order.
    pub fn refs(&self) -> &[Ref] {
        &self.delete
    }

    /// How many Refs are deleted.
    pub fn len(&self) -> usize {
        self.delete.len()
    }

    /// Whether there is nothing to delete.
    pub fn is_empty(&self) -> bool {
        self.delete.is_empty()
    }
}

/// Deletes the Refs of `ops` from `refs` in order; each failure is one
/// `warn` on `noctis::refs` that never names the Ref (a `PortError` may
/// carry the key, so only its kind is logged).
pub(crate) async fn apply(refs: &dyn RefStore, ops: &RefOps) {
    for r in ops.refs() {
        if let Err(error) = refs.delete(r.as_str()).await {
            tracing::warn!(
                target: "noctis::refs",
                error = error_kind(&error),
                "could not delete a Ref after the response"
            );
        }
    }
}

/// The kind of `error`, without the key or value it may carry.
pub(crate) fn error_kind(error: &PortError) -> &'static str {
    match error {
        PortError::Backend(_) => "backend",
        PortError::InvalidValue { .. } => "invalid_value",
        PortError::Unavailable { .. } => "unavailable",
        PortError::Value(_) => "value",
        _ => "other",
    }
}
