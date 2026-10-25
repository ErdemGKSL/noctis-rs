//! A custom `Renderer`: answers every render with fixed bytes.
//!
//! Use it as a handler-test double (no font, no render pool, deterministic files), and
//! read it as the skeleton of a remote Renderer: `render` would send the job (Template
//! id, version, postcard `data`, size, format, animation, upload budget) to a render
//! service and return its bytes. Install it with `.renderer(FixedRenderer::new(..))`
//! instead of `.theme(..)`/`.image(..)`; an App has one Renderer, so setting both is a
//! `BuildViolation::DuplicateRenderer`. Templates, views and handlers do not change.
//!
//! The renderer-facing types come from `noctis::image::unstable`: semver does not cover
//! them, so pin the Noctis version when you ship a custom Renderer.
//!
//! - The digest is `job.key(Self::ID)`: a deterministic render key over every job
//!   field, the Noctis version and this renderer's identity. It names the attachment
//!   (`<first 16 hex>.<ext>`), and a caching Renderer uses it as its `RenderCache` key
//!   (the App's cache is handed over in `Renderer::bind`).
//! - `ctx.render` checks the output: empty bytes are `RenderError::Failed`, more than
//!   `job.upload_budget` bytes are `RenderError::OverBudget`.

use noctis::bytes::Bytes;
use noctis::image::unstable::{RenderJob, RenderOutput, Renderer, TemplateDescriptor};
use noctis::image::{EncodeSettings, OutputFormat, RenderError};
use noctis::ports::BoxFuture;

/// Renders every job as the same bytes in one format.
pub struct FixedRenderer {
    bytes: Bytes,
    format: OutputFormat,
}

impl FixedRenderer {
    /// This renderer's identity in the render key: bump it when its output changes.
    pub const ID: &'static [u8] = b"noctis-template.fixed=1";

    /// A Renderer answering `bytes` as `format` (non-empty, within the upload budget).
    pub fn new(bytes: Bytes, format: OutputFormat) -> Self {
        Self { bytes, format }
    }
}

impl Renderer for FixedRenderer {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let output = RenderOutput {
            bytes: self.bytes.clone(),
            digest: job.key(Self::ID),
            format: self.format,
            settings_used: EncodeSettings {
                size: job.size,
                quality: None,
                fps: None,
                frames: 1,
            },
        };
        Box::pin(async move { Ok(output) })
    }

    fn register(&self, _templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}
