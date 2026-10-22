//! Image Templates for Noctis.
//!
//! - [`tw`] (takumi-free, always built): the strict Tailwind v4 class grammar
//!   and the Theme file parser, which the macros run at compile time.
//! - Behind `render`: the Noctis-owned node tree ([`Node`], [`Tag`],
//!   [`Classes`], [`Style`], [`IntoNodes`]), [`TemplateView`] and its
//!   [`ViewFn`] decoder, and the runtime class check ([`ClassCheck`]); the
//!   takumi renderer, `ImagePlugin`, render pool, fonts and `AssetFetcher`
//!   follow in later stories.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.

pub mod tw;

#[cfg(feature = "render")]
mod check;
#[cfg(feature = "render")]
mod node;
#[cfg(feature = "render")]
mod template;

#[cfg(feature = "render")]
pub use check::{ClassCheck, ClassCheckMode};
#[cfg(feature = "render")]
pub use node::{Classes, IntoNodes, Node, Style, Tag, div, span, text};
#[cfg(feature = "render")]
pub use template::{TemplateView, ViewFn, decode_view, erased_view};
