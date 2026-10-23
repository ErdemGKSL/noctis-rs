//! Image Templates for Noctis.
//!
//! - [`tw`] (takumi-free, always built): the strict Tailwind v4 class grammar
//!   and the Theme file parser, which the macros run at compile time.
//! - Behind `render`:
//!   - the Noctis-owned node tree ([`Node`], [`Tag`], [`Classes`], [`Style`],
//!     [`IntoNodes`]), [`TemplateView`] and its [`ViewFn`] decoder, and the
//!     runtime class check ([`ClassCheck`]);
//!   - the takumi Renderer, which lowers a view's tree into takumi nodes,
//!     applies the App's Theme and encodes the Template's still format (PNG,
//!     JPEG, WebP, lossless WebP) or its animation (animated WebP; GIF and
//!     APNG with the `gif` feature), on a process-scoped, bounded rayon
//!     render pool;
//!   - the size budget: every output is fitted under
//!     `min(RenderJob::upload_budget, ImageConfig::size_target)` by degrading
//!     fps, then size, then quality, then (optionally) falling back to a
//!     still frame; `RenderError::OverBudget` when nothing fits;
//!   - the per-job caps ([`RenderCaps`]), checked before admission
//!     (`RenderError::OverCap`), and the pool's weighted, still-first
//!     admission, which skips jobs whose requester is gone;
//!   - [`ImageConfig`], [`ImagePlugin`] and [`AppBuilderImageExt`]
//!     (`.image(..)`, `.theme(..)`), which install that Renderer.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.

pub mod tw;

#[cfg(feature = "render")]
mod budget;
#[cfg(feature = "render")]
mod caps;
#[cfg(feature = "render")]
mod check;
#[cfg(feature = "render")]
mod lower;
#[cfg(feature = "render")]
mod node;
#[cfg(feature = "render")]
mod plugin;
#[cfg(feature = "render")]
mod pool;
#[cfg(feature = "render")]
mod renderer;
#[cfg(feature = "render")]
mod template;

#[cfg(feature = "render")]
pub use caps::RenderCaps;
#[cfg(feature = "render")]
pub use check::{ClassCheck, ClassCheckMode};
#[cfg(feature = "render")]
pub use node::{Classes, IntoNodes, Node, Style, Tag, div, span, text};
#[cfg(feature = "render")]
pub use plugin::{AppBuilderImageExt, ImageConfig, ImagePlugin};
#[cfg(feature = "render")]
pub use template::{
    GifOutput, GifOutputFormat, TemplateView, ViewFn, decode_view, erased_view, require_gif_output,
};
