//! Image Templates for Noctis.
//!
//! - `emoji` (takumi-free, always built, hidden): the Discord custom emoji
//!   token matcher (IMG-04), shared by the Renderer and `view!`.
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
//!   - fonts ([`Font`], IMG-01): registered in priority order with
//!     `ImageConfig::font` into a process-scoped font registry, with
//!     family overrides and Unicode ranges; the fallback chain is the
//!     families in order, then the bundled last-resort `"Inter"` (feature
//!     `default-font`, OFL-1.1, see `NOTICE`). Templates without any font
//!     fail `App::build` with `RenderError::NoFont`;
//!   - static images ([`StaticImage`], IMG-02): PNG, JPEG or WebP
//!     `&'static` bytes registered per App with `ImageConfig::image`,
//!     decoded once, drawn with [`img`] (`<img src={..} />`);
//!   - [`ImageConfig`], [`ImagePlugin`] and [`AppBuilderImageExt`]
//!     (`.image(..)`, `.theme(..)`), which install that Renderer.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.

#[doc(hidden)]
pub mod emoji;
pub mod tw;

#[cfg(feature = "render")]
mod assets;
#[cfg(feature = "render")]
mod budget;
#[cfg(feature = "render")]
mod caps;
#[cfg(feature = "render")]
mod check;
#[cfg(feature = "render")]
mod fonts;
#[cfg(feature = "render")]
mod lower;
#[cfg(feature = "render")]
mod node;
#[cfg(feature = "render")]
mod plugin;
#[cfg(feature = "render")]
mod pool;
#[cfg(feature = "render")]
mod remote;
#[cfg(feature = "render")]
mod renderer;
#[cfg(feature = "render")]
mod template;

#[cfg(feature = "render")]
pub use assets::StaticImage;
#[cfg(feature = "render")]
pub use caps::RenderCaps;
#[cfg(feature = "render")]
pub use check::{ClassCheck, ClassCheckMode};
#[cfg(feature = "render")]
pub use fonts::Font;
#[cfg(feature = "render")]
pub use node::{Classes, ImageSrc, IntoNodes, Node, Style, Tag, div, img, span, text};
#[cfg(feature = "render")]
pub use plugin::{AppBuilderImageExt, ImageConfig, ImagePlugin};
#[cfg(feature = "render")]
pub use remote::{AssetField, DeclaredImages, TemplateAssets};
#[cfg(feature = "render")]
pub use remote::{RemoteConfig, RemoteImage, RemoteUrlError};
#[cfg(feature = "render")]
pub use template::{
    GifOutput, GifOutputFormat, TemplateView, ViewFn, decode_view, erased_view, require_gif_output,
};
