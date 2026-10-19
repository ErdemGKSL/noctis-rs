//! Array lengths at the type level: [`Arity`] and the bounds built on it
//! (CV2-15).

/// A type-level array length, used only in bounds such as
/// `Arity<N>: LegacyRows`; it is never built at runtime.
///
/// ```
/// use noctis_ui::{Arity, LegacyRows};
///
/// fn legal<const N: usize>() where Arity<N>: LegacyRows {}
/// legal::<5>();
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Arity<const N: usize>;

// The arity bounds share the placement traits' private supertrait, which only
// the legal `Arity` values (and Noctis component types) implement.
use crate::placement::sealed;

/// The number of Action Rows a [`LegacyMessage`](crate::LegacyMessage) may
/// hold: 0 to 5 (CV2-15).
///
/// ```compile_fail
/// use noctis_ui::{Arity, LegacyRows};
///
/// fn legal<const N: usize>() where Arity<N>: LegacyRows {}
/// legal::<6>(); // six rows: does not compile
/// ```
#[diagnostic::on_unimplemented(
    message = "[CV2-15] a LegacyMessage holds at most 5 Action Rows",
    label = "more than 5 Action Rows",
    note = "`{Self}` is the number of rows; send the rest in another message, or build a V2Message"
)]
pub trait LegacyRows: sealed::Sealed {}

macro_rules! legacy_rows {
    ($($n:literal),* $(,)?) => {$(
        impl sealed::Sealed for Arity<$n> {}
        impl LegacyRows for Arity<$n> {}
    )*};
}

legacy_rows!(0, 1, 2, 3, 4, 5);
