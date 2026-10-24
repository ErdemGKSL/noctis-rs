//! What `#[derive(Template)]` emits for `#[asset]` fields: the hidden
//! [`TemplateAssets`] impl and the sealed [`AssetField`] calls that collect
//! [`DeclaredImages`].

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

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::RemoteImage {}
    impl Sealed for Option<super::RemoteImage> {}
    impl Sealed for Vec<super::RemoteImage> {}
}

/// The field types `#[asset]` accepts: [`RemoteImage`],
/// `Option<RemoteImage>` and `Vec<RemoteImage>`. Sealed.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[IMG-03] `#[asset]` fields are `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>`",
    label = "not a remote image field",
    note = "declare remote images as `#[asset] avatar: RemoteImage` (`#[asset(required)]` fails the render instead of drawing the fallback)"
)]
pub trait AssetField: sealed::Sealed {
    /// Pushes the field's images: `field` names it in logs.
    fn declare(&self, field: &'static str, required: bool, out: &mut DeclaredImages);
}

impl AssetField for RemoteImage {
    fn declare(&self, field: &'static str, required: bool, out: &mut DeclaredImages) {
        out.push(Cow::Borrowed(field), required, self);
    }
}

impl AssetField for Option<RemoteImage> {
    fn declare(&self, field: &'static str, required: bool, out: &mut DeclaredImages) {
        if let Some(image) = self {
            out.push(Cow::Borrowed(field), required, image);
        }
    }
}

impl AssetField for Vec<RemoteImage> {
    fn declare(&self, field: &'static str, required: bool, out: &mut DeclaredImages) {
        for (index, image) in self.iter().enumerate() {
            out.push(Cow::Owned(format!("{field}[{index}]")), required, image);
        }
    }
}
