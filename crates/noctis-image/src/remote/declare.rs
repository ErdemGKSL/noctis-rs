//! What `#[derive(Template)]` and `#[derive(Assets)]` emit for `#[asset]`
//! fields: the hidden [`TemplateAssets`] and [`Assets`] impls and the
//! [`AssetField`] / [`RequiredAssetField`] calls that collect
//! [`DeclaredImages`], named by path (`rows[3].avatar`).

use std::borrow::Cow;

use super::RemoteImage;

/// The remote images a Template value declares; `#[derive(Template)]`
/// implements it for every Template (with `DECLARES = true` and a
/// [`declare`](Self::declare) body when it has `#[asset]` fields). A
/// hand-written Template adds `impl TemplateAssets for X {}`.
#[doc(hidden)]
pub trait TemplateAssets {
    /// Whether the Template has `#[asset]` fields; `false` keeps the
    /// Renderer's synchronous admission.
    const DECLARES: bool = false;

    /// Pushes every declared image of `self` into `out`.
    fn declare(&self, out: &mut DeclaredImages) {
        let _ = out;
    }
}

/// The images collected from one Template value, in field order.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct DeclaredImages {
    items: Vec<Declared>,
}

impl DeclaredImages {
    /// The collected images.
    pub(crate) fn into_vec(self) -> Vec<Declared> {
        self.items
    }

    fn push(&mut self, field: Cow<'static, str>, required: bool, image: &RemoteImage) {
        self.items.push(Declared {
            field,
            required,
            image: image.clone(),
        });
    }
}

/// One declared image: its field (`avatar`, `frames[1]`, `0`), whether the
/// field is `#[asset(required)]`, and the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) field: Cow<'static, str>,
    pub(crate) required: bool,
    pub(crate) image: RemoteImage,
}

/// Plain Template data that holds remote images in its own `#[asset]` /
/// `#[asset(required)]` fields, such as a leaderboard row with its avatar;
/// implemented by `#[derive(Assets)]` (IMG-03).
///
/// A Template (or another `Assets` struct) declares those images by holding
/// the struct in an `#[asset]` field as `T`, `Option<T>` or `Vec<T>`. Each
/// inner field keeps its own `required` setting, and logs and errors name the
/// path (`rows[3].avatar`). `#[asset(required)]` on the outer field is a
/// compile error: the inner fields decide.
pub trait Assets {
    /// Pushes every declared image of `self`, named under `path`.
    #[doc(hidden)]
    fn declare_assets(&self, path: &str, out: &mut DeclaredImages);
}

mod sealed {
    use super::RemoteImage;

    pub trait SealedRequired {}

    impl SealedRequired for RemoteImage {}
    impl SealedRequired for Option<RemoteImage> {}
    impl SealedRequired for Vec<RemoteImage> {}
}

/// The field types `#[asset]` accepts: [`RemoteImage`],
/// `Option<RemoteImage>` and `Vec<RemoteImage>`, and an [`Assets`] struct
/// `T` as `T`, `Option<T>` or `Vec<T>`. `#[derive(Assets)]` implements it
/// for `T` itself (not a blanket impl, so another field type gets the
/// `[IMG-03]` message below); nothing else should.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[IMG-03] `#[asset]` fields are `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>`, or a `#[derive(Assets)]` struct `T` as `T`, `Option<T>` or `Vec<T>`",
    label = "not a remote image field",
    note = "declare remote images as `#[asset] avatar: RemoteImage` (`#[asset(required)]` fails the render instead of drawing the fallback); a struct that holds them derives `noctis::image::Assets`"
)]
pub trait AssetField {
    /// Pushes the field's images: `field` is its path in logs.
    fn declare(&self, field: &str, required: bool, out: &mut DeclaredImages);
}

/// The field types `#[asset(required)]` accepts: [`RemoteImage`],
/// `Option<RemoteImage>` and `Vec<RemoteImage>`; an [`Assets`] struct's own
/// fields decide. Sealed.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[IMG-03] `#[asset(required)]` fields are `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>`",
    label = "not a remote image field",
    note = "on a field that holds a `#[derive(Assets)]` struct write `#[asset]`: the struct's own `#[asset]` / `#[asset(required)]` fields decide"
)]
pub trait RequiredAssetField: AssetField + sealed::SealedRequired {
    /// Pushes the field's images as required.
    fn declare_required(&self, field: &str, out: &mut DeclaredImages) {
        self.declare(field, true, out);
    }
}

impl RequiredAssetField for RemoteImage {}
impl RequiredAssetField for Option<RemoteImage> {}
impl RequiredAssetField for Vec<RemoteImage> {}

impl AssetField for RemoteImage {
    fn declare(&self, field: &str, required: bool, out: &mut DeclaredImages) {
        out.push(Cow::Owned(field.to_owned()), required, self);
    }
}

impl AssetField for Option<RemoteImage> {
    fn declare(&self, field: &str, required: bool, out: &mut DeclaredImages) {
        if let Some(image) = self {
            image.declare(field, required, out);
        }
    }
}

impl AssetField for Vec<RemoteImage> {
    fn declare(&self, field: &str, required: bool, out: &mut DeclaredImages) {
        for (index, image) in self.iter().enumerate() {
            image.declare(&format!("{field}[{index}]"), required, out);
        }
    }
}

impl<T: Assets> AssetField for Option<T> {
    fn declare(&self, field: &str, _required: bool, out: &mut DeclaredImages) {
        if let Some(value) = self {
            value.declare_assets(field, out);
        }
    }
}

impl<T: Assets> AssetField for Vec<T> {
    fn declare(&self, field: &str, _required: bool, out: &mut DeclaredImages) {
        for (index, value) in self.iter().enumerate() {
            value.declare_assets(&format!("{field}[{index}]"), out);
        }
    }
}

/// The path of the field `name` inside the `Assets` value at `path`
/// (`rows[3]` and `avatar` give `rows[3].avatar`); what
/// `#[derive(Assets)]` passes to each field.
#[doc(hidden)]
#[must_use]
pub fn asset_path(path: &str, name: &str) -> String {
    format!("{path}.{name}")
}
