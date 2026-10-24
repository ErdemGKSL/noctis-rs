//! [`TemplateView`] and the decoder `#[derive(Template)]` erases into
//! [`ErasedView`] (AD-22).

use noctis_ports::{ErasedView, Frame, RenderError, Template, decode_template};

use crate::Node;
use crate::remote::{Declared, DeclaredImages, TemplateAssets};

/// How a Template draws itself: `view` returns the node tree of one frame.
///
/// A still Template receives [`Frame::STILL`]; an animated one is called once
/// per frame with `Frame { index, count }`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` derives `Template` but does not implement `TemplateView`",
    note = "add `impl TemplateView for {Self} {{ fn view(&self, frame: Frame) -> Node {{ view! {{ .. }} }} }}`"
)]
pub trait TemplateView {
    /// The node tree of `frame`.
    fn view(&self, frame: Frame) -> Node;
}

/// The view a Template's [`ErasedView`] holds: it decodes the Template value
/// from [`RenderJob::data`](noctis_ports::RenderJob) and calls
/// [`TemplateView::view`].
///
/// ```
/// # use noctis_image::{Classes, Node, TemplateView, ViewFn, erased_view};
/// # use noctis_ports::{AnimationSpec, ErasedView, Format, Frame, RenderJob, Size, Template};
/// # #[derive(serde::Serialize, serde::Deserialize)]
/// # struct Card { level: u32 }
/// # impl Template for Card {
/// #     const ID: &'static str = "card";
/// #     const VERSION: u32 = 1;
/// #     const SIZE: Size = Size::new(10, 10);
/// #     const FORMAT: Format = Format::Png;
/// #     const ANIMATION: Option<AnimationSpec> = None;
/// #     fn view() -> ErasedView { erased_view::<Self>() }
/// # }
/// # impl TemplateView for Card {
/// #     fn view(&self, _: Frame) -> Node { Node::text(self.level.to_string()) }
/// # }
/// # impl noctis_image::TemplateAssets for Card {}
/// let view = <Card as Template>::view();
/// let decode = view.downcast_ref::<ViewFn>().expect("a derived view");
/// let job = RenderJob::of(&Card { level: 3 })?;
/// assert_eq!(decode.call(&job.data, Frame::STILL)?, Node::text("3"));
/// assert!(!decode.is_animated());
/// assert!(!decode.declares_assets());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ViewFn {
    decode: fn(&[u8], Frame) -> Result<Node, RenderError>,
    animated: bool,
    /// The declared remote images of a Template with `#[asset]` fields.
    assets: Option<DeclareFn>,
}

/// Decodes a Template value and collects its declared remote images.
type DeclareFn = fn(&[u8]) -> Result<Vec<Declared>, RenderError>;

impl ViewFn {
    /// Decodes `data` and returns the view of `frame`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Failed`] with the [`ValueError`](noctis_ports::ValueError)
    /// as its source when `data` does not decode as the Template.
    pub fn call(&self, data: &[u8], frame: Frame) -> Result<Node, RenderError> {
        (self.decode)(data, frame)
    }

    /// Whether the Template is animated ([`Template::ANIMATION`] is `Some`).
    pub fn is_animated(&self) -> bool {
        self.animated
    }

    /// Whether the Template has `#[asset]` fields, so its remote images
    /// are resolved before its job is admitted.
    pub fn declares_assets(&self) -> bool {
        self.assets.is_some()
    }

    /// The declared remote images of `data`; `None` for a Template without
    /// `#[asset]` fields.
    pub(crate) fn assets(&self) -> Option<DeclareFn> {
        self.assets
    }
}

/// The [`ErasedView`] of `T`: a [`ViewFn`] running [`decode_view::<T>`]
/// (and, with `#[asset]` fields, collecting the declared remote images);
/// what `#[derive(Template)]` returns from `Template::view`.
#[doc(hidden)]
pub fn erased_view<T: Template + TemplateView + TemplateAssets>() -> ErasedView {
    ErasedView::new(ViewFn {
        decode: decode_view::<T>,
        animated: T::ANIMATION.is_some(),
        assets: T::DECLARES.then_some(declared_of::<T> as DeclareFn),
    })
}

/// Decodes a `T` from `data` and collects its declared remote images.
fn declared_of<T: Template + TemplateAssets>(data: &[u8]) -> Result<Vec<Declared>, RenderError> {
    let value = decode_template::<T>(data).map_err(RenderError::failed)?;
    let mut out = DeclaredImages::default();
    value.declare(&mut out);
    Ok(out.into_vec())
}

/// Decodes a `T` from `data` and returns its view of `frame`.
///
/// # Errors
///
/// [`RenderError::Failed`] when `data` does not decode as `T`.
#[doc(hidden)]
pub fn decode_view<T: Template + TemplateView>(
    data: &[u8],
    frame: Frame,
) -> Result<Node, RenderError> {
    let value = decode_template::<T>(data).map_err(RenderError::failed)?;
    Ok(value.view(frame))
}

/// The marker type the GIF/APNG gate of `#[derive(Template)]` checks.
#[doc(hidden)]
#[derive(Debug)]
pub enum GifOutputFormat {}

/// Implemented only with the `gif` feature (the facade's `image-gif`), so a
/// Template declaring `Gif`/`Apng` output fails to compile without it.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "GIF and APNG output need the `image-gif` feature of `noctis`",
    label = "animated format `Gif`/`Apng`",
    note = "enable it: noctis = {{ version = \"..\", features = [\"image-gif\"] }}; or use `format = Webp {{ quality: 80 }}` (the default)"
)]
pub trait GifOutput {}

#[cfg(feature = "gif")]
impl GifOutput for GifOutputFormat {}

/// Compiles only when `T` is [`GifOutput`]; the derive calls it in a `const`.
#[doc(hidden)]
pub const fn require_gif_output<T: GifOutput>() {}
