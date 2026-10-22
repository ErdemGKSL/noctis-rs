//! [`TemplateView`] and the decoder `#[derive(Template)]` erases into
//! [`ErasedView`] (AD-22).

use noctis_ports::{ErasedView, Frame, RenderError, Template, decode_template};

use crate::Node;

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
/// let view = <Card as Template>::view();
/// let decode = view.downcast_ref::<ViewFn>().expect("a derived view");
/// let job = RenderJob::of(&Card { level: 3 })?;
/// assert_eq!(decode.call(&job.data, Frame::STILL)?, Node::text("3"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ViewFn(fn(&[u8], Frame) -> Result<Node, RenderError>);

impl ViewFn {
    /// Decodes `data` and returns the view of `frame`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Failed`] with the [`ValueError`](noctis_ports::ValueError)
    /// as its source when `data` does not decode as the Template.
    pub fn call(&self, data: &[u8], frame: Frame) -> Result<Node, RenderError> {
        (self.0)(data, frame)
    }
}

/// The [`ErasedView`] of `T`: a [`ViewFn`] running [`decode_view::<T>`];
/// what `#[derive(Template)]` returns from `Template::view`.
#[doc(hidden)]
pub fn erased_view<T: Template + TemplateView>() -> ErasedView {
    ErasedView::new(ViewFn(decode_view::<T>))
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
