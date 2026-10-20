//! Array lengths at the type level: [`Arity`] and the bounds built on it
//! (CV2-06, CV2-07, CV2-10, CV2-15).
//!
//! Each bound is a sealed trait implemented only for the legal lengths, so an
//! array of the wrong length fails `cargo check` with the rule id. The bounds
//! use Discord's default limits; a lowered `Limits` is enforced by
//! [`validate`](crate::validate). Lengths known only at runtime go through the
//! `try_*` constructors instead.

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

/// The number of buttons an [`ActionRow`](crate::ActionRow) may hold: 1 to 5
/// (CV2-06). `ActionRow::try_buttons` is the runtime path.
///
/// ```
/// use noctis_ui::{Arity, RowButtons};
///
/// fn legal<const N: usize>() where Arity<N>: RowButtons {}
/// legal::<1>();
/// legal::<5>();
/// ```
///
/// ```compile_fail
/// use noctis_ui::{Arity, RowButtons};
///
/// fn legal<const N: usize>() where Arity<N>: RowButtons {}
/// legal::<0>(); // an empty row: does not compile
/// ```
///
/// ```compile_fail
/// use noctis_ui::{Arity, RowButtons};
///
/// fn legal<const N: usize>() where Arity<N>: RowButtons {}
/// legal::<6>(); // six buttons: does not compile
/// ```
#[diagnostic::on_unimplemented(
    message = "[CV2-06] an Action Row holds 1 to 5 buttons",
    label = "not 1 to 5 buttons",
    note = "`{Self}` is the number of buttons; for a length known only at runtime use ActionRow::try_buttons(buttons, &limits)"
)]
pub trait RowButtons: sealed::Sealed {}

/// The number of Text Displays a [`Section`](crate::Section) may hold: 1 to
/// 3 (CV2-07). `Section::try_new` is the runtime path.
///
/// ```
/// use noctis_ui::{Arity, SectionTexts};
///
/// fn legal<const N: usize>() where Arity<N>: SectionTexts {}
/// legal::<1>();
/// legal::<3>();
/// ```
///
/// ```compile_fail
/// use noctis_ui::{Arity, SectionTexts};
///
/// fn legal<const N: usize>() where Arity<N>: SectionTexts {}
/// legal::<0>(); // no text: does not compile
/// ```
///
/// ```compile_fail
/// use noctis_ui::{Arity, SectionTexts};
///
/// fn legal<const N: usize>() where Arity<N>: SectionTexts {}
/// legal::<4>(); // four texts: does not compile
/// ```
#[diagnostic::on_unimplemented(
    message = "[CV2-07] a Section holds 1 to 3 Text Displays",
    label = "not 1 to 3 Text Displays",
    note = "`{Self}` is the number of texts; for a length known only at runtime use Section::try_new(texts, accessory, &limits)"
)]
pub trait SectionTexts: sealed::Sealed {}

/// The number of items a [`MediaGallery`](crate::MediaGallery) may hold: 1
/// to 10 (CV2-10). `MediaGallery::try_new` is the runtime path.
///
/// ```
/// use noctis_ui::{Arity, GalleryItems};
///
/// fn legal<const N: usize>() where Arity<N>: GalleryItems {}
/// legal::<1>();
/// legal::<10>();
/// ```
///
/// ```compile_fail
/// use noctis_ui::{Arity, GalleryItems};
///
/// fn legal<const N: usize>() where Arity<N>: GalleryItems {}
/// legal::<0>(); // an empty gallery: does not compile
/// ```
///
/// ```compile_fail
/// use noctis_ui::{Arity, GalleryItems};
///
/// fn legal<const N: usize>() where Arity<N>: GalleryItems {}
/// legal::<11>(); // eleven items: does not compile
/// ```
#[diagnostic::on_unimplemented(
    message = "[CV2-10] a Media Gallery holds 1 to 10 items",
    label = "not 1 to 10 items",
    note = "`{Self}` is the number of items; for a length known only at runtime use MediaGallery::try_new(items, &limits)"
)]
pub trait GalleryItems: sealed::Sealed {}

macro_rules! arity {
    (sealed: $($n:literal),* $(,)?) => {$(
        impl sealed::Sealed for Arity<$n> {}
    )*};
    ($trait:ident: $($n:literal),* $(,)?) => {$(
        impl $trait for Arity<$n> {}
    )*};
}

arity!(sealed: 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
arity!(LegacyRows: 0, 1, 2, 3, 4, 5);
arity!(RowButtons: 1, 2, 3, 4, 5);
arity!(SectionTexts: 1, 2, 3);
arity!(GalleryItems: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
