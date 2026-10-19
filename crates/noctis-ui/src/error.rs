//! [`UiError`]: what a component, attachment or message builder can reject.

use noctis_codec::RouteId;

/// An error from a `noctis-ui` builder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UiError {
    /// An attachment extension is empty, longer than 10 characters, or has a
    /// character outside ASCII letters and digits.
    #[error("invalid attachment extension `{ext}`: expected 1 to 10 ASCII letters or digits")]
    InvalidExtension {
        /// The rejected extension.
        ext: String,
    },
    /// A Ref slot was finished on the `custom_id` of another Route.
    #[error("ref slot of route `{actual}` finished on a custom_id of route `{expected}`")]
    SlotMismatch {
        /// The Route of the `custom_id` being finished.
        expected: RouteId,
        /// The Route the slot belongs to.
        actual: RouteId,
    },
    /// An edit would turn a Components V2 message back into a legacy one.
    #[error(
        "a Components V2 message cannot be edited back into a legacy message (CV2-01: the IS_COMPONENTS_V2 flag cannot be removed)"
    )]
    V2ToLegacy,
}
