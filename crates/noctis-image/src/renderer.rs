//! The takumi [`Renderer`]: one per App, bound to its Theme, rendering on the
//! process-scoped [`RenderPool`].

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use bytes::Bytes;
use noctis_ports::{
    BoxFuture, EncodeSettings, Format, Frame, RenderError, RenderJob, RenderOutput, Renderer,
    TemplateDescriptor,
};
use takumi::prelude::{Fonts, Quality, RenderOptions, StyleSheet, Viewport};

use crate::lower::lower;
use crate::pool::RenderPool;
use crate::tw::Theme;
use crate::{ClassCheck, ClassCheckMode, ViewFn};

/// The registered Templates: id → (version, view).
type Views = HashMap<&'static str, (u32, ViewFn)>;

/// The per-App takumi Renderer.
pub(crate) struct TakumiRenderer {
    pool: Arc<RenderPool>,
    shared: Arc<Shared>,
    views: RwLock<Views>,
}

/// What every render job of one Renderer reads.
struct Shared {
    theme: Theme,
    /// The `@theme { … }` stylesheet built once from `theme`.
    stylesheet: Arc<StyleSheet>,
    check: ClassCheck,
    /// No fonts are registered yet (Story 10.7).
    fonts: Fonts,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TakumiRenderer>();
};

/// The takumi stylesheet of `theme`: `@theme { --<prefix><token>: <value>; … }`
/// over every namespace's tokens. takumi reads `@theme` as `:root`, so theme
/// utilities (`bg-brand`) resolve their `var(--color-brand)` through it.
///
/// # Errors
///
/// takumi's parse error when a value is not valid CSS for it.
pub(crate) fn theme_stylesheet(
    theme: &Theme,
) -> Result<StyleSheet, takumi_core::error::StyleSheetParseError> {
    let mut css = String::from("@theme {\n");
    for (name, value) in theme.custom_properties() {
        css.push_str("  ");
        css.push_str(&name);
        css.push_str(": ");
        css.push_str(value);
        css.push_str(";\n");
    }
    css.push('}');
    StyleSheet::parse_list([css])
}

impl TakumiRenderer {
    /// A Renderer drawing with `theme` (already parsed into `stylesheet`)
    /// on `pool`; `mode` is Deny on test Runtimes, Warn otherwise.
    pub(crate) fn new(
        pool: Arc<RenderPool>,
        theme: Theme,
        stylesheet: StyleSheet,
        mode: ClassCheckMode,
    ) -> Self {
        Self {
            pool,
            shared: Arc::new(Shared {
                theme,
                stylesheet: Arc::new(stylesheet),
                check: ClassCheck::new(mode),
                fonts: Fonts::default(),
            }),
            views: RwLock::new(HashMap::new()),
        }
    }

    /// The view of `job`, checked against the registered version and the
    /// still-only support of this Renderer.
    fn view_of(&self, job: &RenderJob) -> Result<ViewFn, RenderError> {
        let views = self.views.read().unwrap_or_else(PoisonError::into_inner);
        let view = match views.get(job.template_id) {
            Some(&(version, view)) if version == job.template_version => view,
            _ => {
                return Err(RenderError::UnknownTemplate {
                    id: job.template_id.to_owned(),
                    version: job.template_version,
                });
            }
        };
        if job.animation.is_some() {
            return Err(RenderError::Unsupported {
                id: job.template_id.to_owned(),
                why: "animated Templates are not rendered yet".into(),
            });
        }
        Ok(view)
    }
}

impl std::fmt::Debug for TakumiRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let views = self.views.read().unwrap_or_else(PoisonError::into_inner);
        let mut ids: Vec<&str> = views.keys().copied().collect();
        ids.sort_unstable();
        f.debug_struct("TakumiRenderer")
            .field("pool", &self.pool)
            .field("templates", &ids)
            .finish_non_exhaustive()
    }
}

impl Renderer for TakumiRenderer {
    /// Looks the Template up, then admits the job to the pool before it
    /// returns the future, so admission order is call order.
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let admitted = self.view_of(&job).and_then(|view| {
            let shared = Arc::clone(&self.shared);
            self.pool.try_submit(move || shared.draw(view, &job))
        });
        Box::pin(async move {
            admitted?
                .await
                .map_err(|_| RenderError::failed("render pool dropped the job"))?
        })
    }

    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        let mut views = Views::with_capacity(templates.len());
        for descriptor in templates {
            let Some(view) = descriptor.view.downcast_ref::<ViewFn>() else {
                return Err(RenderError::Unsupported {
                    id: descriptor.id.to_owned(),
                    why: "its view was not made by `#[derive(Template)]`".into(),
                });
            };
            if view.is_animated() {
                return Err(RenderError::Unsupported {
                    id: descriptor.id.to_owned(),
                    why: "animated Templates are not rendered yet".into(),
                });
            }
            views.insert(descriptor.id, (descriptor.version, *view));
        }
        *self.views.write().unwrap_or_else(PoisonError::into_inner) = views;
        Ok(())
    }
}

impl Shared {
    /// Renders `job` with `view`; runs on a pool thread.
    fn draw(&self, view: ViewFn, job: &RenderJob) -> Result<RenderOutput, RenderError> {
        let node = view.call(&job.data, Frame::STILL)?;
        self.check
            .check(&node, &self.theme)
            .map_err(RenderError::failed)?;
        let root = lower(&node)?;
        let (width, height) = (job.size.width(), job.size.height());
        let options = RenderOptions::builder()
            .viewport(Viewport::new((width, height)))
            .fonts(&self.fonts)
            .node(root)
            .stylesheet(Arc::clone(&self.stylesheet))
            .build();
        let bitmap = takumi::render(options).map_err(RenderError::failed)?;
        if (bitmap.width(), bitmap.height()) != (width, height) {
            return Err(RenderError::failed(format!(
                "takumi drew {}x{} pixels for a {width}x{height} Template",
                bitmap.width(),
                bitmap.height()
            )));
        }
        let (format, quality) = encoding(job.format);
        let mut bytes = Vec::new();
        takumi::write_image(&bitmap, &mut bytes, format).map_err(RenderError::failed)?;
        let digest = *blake3::hash(&bytes).as_bytes();
        Ok(RenderOutput {
            bytes: Bytes::from(bytes),
            digest,
            format: job.format.into(),
            settings_used: EncodeSettings {
                size: job.size,
                quality,
                fps: None,
                frames: 1,
            },
        })
    }
}

/// takumi's output format for `format`, and the quality it encodes with.
fn encoding(format: Format) -> (takumi::prelude::OutputFormat, Option<u8>) {
    use takumi::prelude::OutputFormat;

    match format {
        Format::Png => (OutputFormat::Png, None),
        Format::Jpeg { quality } => (
            OutputFormat::Jpeg {
                quality: Quality::new(quality),
            },
            Some(quality),
        ),
        Format::Webp { quality } => (
            OutputFormat::WebP {
                quality: Quality::new(quality),
            },
            Some(quality),
        ),
        Format::WebpLossless => (OutputFormat::WebPLossless, None),
    }
}

#[cfg(test)]
mod tests;
