//! The [`AssetFetcher`]: the only non-Discord HTTP client of the workspace
//! (AD-15; deps-check `asset-http`). One per App, owned by its Renderer.
//!
//! Every image is checked against the App's origin allow-list, fetched with
//! a `GET` that never follows redirects, capped in time, bytes and pixels,
//! sniffed (PNG, JPEG or WebP) and decoded on tokio's blocking pool, all
//! under one per-App semaphore permit; fetched bytes are cached in the App's
//! `AssetStore`.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use futures_util::future::join_all;
use http_body_util::{BodyExt, Empty, LengthLimitError, Limited};
use hyper::header::{CONTENT_LENGTH, HeaderMap, USER_AGENT};
use hyper::{Request, Uri};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use image::{ExtendedColorType, ImageEncoder};
use noctis_ports::{AssetStore, RenderCap, RenderError};
use takumi::prelude::ImageSource;
use tokio::sync::Semaphore;
use tokio::time::{Instant, timeout_at};

use super::{Declared, Origin, ParsedUrl, RemoteConfig, RemoteImage};
use crate::assets::{self, sniff};

/// The `user-agent` of every request; nothing personal goes in a header.
const USER_AGENT_VALUE: &str = concat!("noctis/", env!("CARGO_PKG_VERSION"));
/// The built-in fallback pixel: opaque `#475569`.
const FALLBACK_RGB: [u8; 3] = [0x47, 0x55, 0x69];

type HttpClient = Client<HttpsConnector<HttpConnector>, Empty<Bytes>>;

/// Fetches, checks, decodes and caches the remote images of one App.
pub(crate) struct AssetFetcher {
    client: HttpClient,
    origins: Vec<Origin>,
    /// The CDN origin avatar and icon URLs are built on, without a `/`.
    cdn: String,
    timeout: Duration,
    max_bytes: u64,
    max_pixels: u64,
    max_per_job: usize,
    cache_ttl: Duration,
    permits: Semaphore,
    fallback: ImageSource,
}

impl fmt::Debug for AssetFetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AssetFetcher")
            .field(
                "origins",
                &self
                    .origins
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )
            .field("cdn", &self.cdn)
            .field("timeout", &self.timeout)
            .field("max_bytes", &self.max_bytes)
            .field("max_pixels", &self.max_pixels)
            .field("max_per_job", &self.max_per_job)
            .finish_non_exhaustive()
    }
}

/// The rustls config of every fetcher: ring, the safe default protocol
/// versions and the platform verifier, built once per process (loading the
/// platform roots is slow). It never reads a process default provider.
fn tls_config() -> Result<rustls::ClientConfig, String> {
    static CONFIG: OnceLock<Result<rustls::ClientConfig, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            use rustls_platform_verifier::BuilderVerifierExt;

            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let builder = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|err| err.to_string())?
                .with_platform_verifier()
                .map_err(|err| err.to_string())?;
            Ok(builder.with_no_client_auth())
        })
        .clone()
}

/// The built-in fallback: a 1x1 opaque `#475569` PNG, decoded.
fn builtin_fallback() -> Result<ImageSource, String> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&FALLBACK_RGB, 1, 1, ExtendedColorType::Rgb8)
        .map_err(|err| format!("[IMG-03] the built-in fallback could not be encoded: {err}"))?;
    ImageSource::from_bytes(&png)
        .map_err(|err| format!("[IMG-03] the built-in fallback could not be decoded: {err}"))
}

impl AssetFetcher {
    /// The fetcher of an App configured with `config` (already validated).
    ///
    /// # Errors
    ///
    /// The `[IMG-02]` message of a bad fallback image, or `[IMG-03] the TLS
    /// verifier is unavailable: …`.
    pub(crate) fn new(config: &RemoteConfig) -> Result<Self, String> {
        let fallback = match config.fallback_image() {
            Some(image) => assets::decode(&image)?,
            None => builtin_fallback()?,
        };
        let tls = tls_config()
            .map_err(|err| format!("[IMG-03] the TLS verifier is unavailable: {err}"))?;
        // `https_or_http`: plain http reaches only allow-listed loopback
        // origins (the origin grammar refuses any other http host).
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        let mut builder = Client::builder(TokioExecutor::new());
        builder.pool_timer(TokioTimer::new());
        Ok(Self {
            client: builder.build(connector),
            origins: config.origins(),
            cdn: config.cdn(),
            timeout: config.timeout_value(),
            max_bytes: config.max_bytes_value(),
            max_pixels: config.max_pixels_value(),
            max_per_job: config.max_per_job_value(),
            cache_ttl: config.cache_ttl_value(),
            permits: Semaphore::new(config.concurrency_value()),
            fallback,
        })
    }

    /// The fallback drawn for the placeholder and for every image that
    /// cannot be used.
    pub(crate) fn fallback(&self) -> &ImageSource {
        &self.fallback
    }

    /// `declared` deduplicated by image (a field is required when any of
    /// its duplicates is), without placeholders, checked against
    /// `max_per_job` before anything is fetched.
    ///
    /// # Errors
    ///
    /// `RenderError::OverCap` with `RenderCap::RemoteImages`.
    pub(crate) fn plan(
        &self,
        template: &str,
        declared: Vec<Declared>,
    ) -> Result<Vec<Declared>, RenderError> {
        let mut unique: Vec<Declared> = Vec::new();
        let mut index: HashMap<RemoteImage, usize> = HashMap::new();
        for item in declared {
            if item.image.is_placeholder() {
                continue;
            }
            if let Some(&at) = index.get(&item.image) {
                unique[at].required |= item.required;
                continue;
            }
            index.insert(item.image.clone(), unique.len());
            unique.push(item);
        }
        if unique.len() > self.max_per_job {
            return Err(RenderError::OverCap {
                id: template.to_owned(),
                cap: RenderCap::RemoteImages,
                limit: u64::try_from(self.max_per_job).unwrap_or(u64::MAX),
                actual: u64::try_from(unique.len()).unwrap_or(u64::MAX),
            });
        }
        Ok(unique)
    }

    /// Resolves every image of `plan` concurrently for a job of `template`
    /// that is `animated`. A failed image logs one warning and draws the
    /// fallback, unless its field is required.
    ///
    /// # Errors
    ///
    /// `RenderError::RemoteImage` for the first required image that could
    /// not be used.
    pub(crate) async fn resolve(
        &self,
        store: Option<&dyn AssetStore>,
        template: &str,
        plan: Vec<Declared>,
        animated: bool,
    ) -> Result<Resolved, RenderError> {
        let results = join_all(
            plan.iter()
                .map(|item| self.fetch(store, template, &item.image, animated)),
        )
        .await;
        let mut images = HashMap::with_capacity(plan.len());
        let mut failure = None;
        for (item, result) in plan.into_iter().zip(results) {
            let source = match result {
                Ok(source) => source,
                Err(reason) => {
                    tracing::warn!(
                        target: "noctis::image",
                        template,
                        field = %item.field,
                        image = %item.image,
                        reason = %reason,
                        "remote image unavailable; drawing the fallback"
                    );
                    if item.required {
                        failure.get_or_insert_with(|| RenderError::RemoteImage {
                            id: template.to_owned(),
                            image: item.image.to_string(),
                            reason,
                        });
                        continue;
                    }
                    self.fallback.clone()
                }
            };
            images.insert(item.image, source);
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(Resolved {
                images,
                fallback: self.fallback.clone(),
            }),
        }
    }

    /// One image: allow-list, permit, cache, fetch, check and decode.
    async fn fetch(
        &self,
        store: Option<&dyn AssetStore>,
        template: &str,
        image: &RemoteImage,
        animated: bool,
    ) -> Result<ImageSource, String> {
        let Some(target) = image.target(&self.cdn, animated)? else {
            return Ok(self.fallback.clone());
        };
        let url = ParsedUrl::parse(&target.url).map_err(|err| err.to_string())?;
        if !self.origins.iter().any(|origin| origin.matches(&url)) {
            return Err(format!(
                "origin `{}` is not allow-listed",
                url.origin_text()
            ));
        }
        let deadline = Instant::now() + self.timeout;
        let timed_out = || format!("timed out after {:?}", self.timeout);
        let _permit = timeout_at(deadline, self.permits.acquire())
            .await
            .map_err(|_| timed_out())?
            .map_err(|_| "the fetcher is closed".to_owned())?;
        if let Some(store) = store {
            match store.get(&target.key).await {
                Ok(Some(bytes)) => {
                    // Bytes that no longer pass the checks count as a miss.
                    if let Ok(source) = self.decode(bytes).await {
                        return Ok(source);
                    }
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    target: "noctis::image",
                    template,
                    image = %image,
                    error = %error,
                    "remote image cache read failed; fetching"
                ),
            }
        }
        let bytes = timeout_at(deadline, self.get(&target.url))
            .await
            .map_err(|_| timed_out())??;
        let source = self.decode(bytes.clone()).await?;
        if let Some(store) = store
            && let Err(error) = store.put(&target.key, bytes, Some(self.cache_ttl)).await
        {
            tracing::warn!(
                target: "noctis::image",
                template,
                image = %image,
                error = %error,
                "remote image cache write failed"
            );
        }
        Ok(source)
    }

    /// `GET url`: a 2xx body of at most `max_bytes`; a redirect is a failure.
    async fn get(&self, url: &str) -> Result<Bytes, String> {
        let uri: Uri = url
            .parse()
            .map_err(|err| format!("the URL is not valid: {err}"))?;
        let request = Request::get(uri)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .body(Empty::new())
            .map_err(|err| format!("the request could not be built: {err}"))?;
        let response = self
            .client
            .request(request)
            .await
            .map_err(|err| error_chain(&err))?;
        let status = response.status();
        if status.is_redirection() {
            return Err("redirect not followed".to_owned());
        }
        if !status.is_success() {
            return Err(format!("HTTP {}", status.as_u16()));
        }
        let over = || format!("over the byte cap of {} bytes", self.max_bytes);
        if content_length(response.headers()).is_some_and(|len| len > self.max_bytes) {
            return Err(over());
        }
        let limit = usize::try_from(self.max_bytes).unwrap_or(usize::MAX);
        let body = Limited::new(response.into_body(), limit)
            .collect()
            .await
            .map_err(|err| {
                if err.downcast_ref::<LengthLimitError>().is_some() {
                    over()
                } else {
                    error_chain(&*err)
                }
            })?;
        Ok(body.to_bytes())
    }

    /// Sniffs, size-checks and decodes `bytes` on tokio's blocking pool.
    async fn decode(&self, bytes: Bytes) -> Result<ImageSource, String> {
        let max_pixels = self.max_pixels;
        tokio::task::spawn_blocking(move || check_and_decode(&bytes, max_pixels))
            .await
            .map_err(|_| "could not be decoded: the decoder panicked".to_owned())?
    }
}

/// The `content-length` header, when it is a number.
fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers.get(CONTENT_LENGTH)?.to_str().ok()?.parse().ok()
}

/// `error` and its sources, joined with `: `.
fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        text.push_str(": ");
        text.push_str(&next.to_string());
        source = next.source();
    }
    text
}

/// PNG, JPEG or WebP only; the header's `w*h` at most `max_pixels`, checked
/// before decoding.
fn check_and_decode(bytes: &[u8], max_pixels: u64) -> Result<ImageSource, String> {
    if sniff(bytes).is_none() {
        return Err("unsupported format: use PNG, JPEG or WebP".to_owned());
    }
    let corrupt = |why: &dyn fmt::Display| format!("could not be decoded: {why}");
    let (width, height) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| corrupt(&err))?
        .into_dimensions()
        .map_err(|err| corrupt(&err))?;
    if u64::from(width) * u64::from(height) > max_pixels {
        return Err(format!("is {width}x{height}: over {max_pixels} pixels"));
    }
    ImageSource::from_bytes(bytes).map_err(|err| corrupt(&err))
}

/// The decoded remote images of one job, by image; the placeholder draws
/// the fallback.
#[derive(Clone)]
pub(crate) struct Resolved {
    images: HashMap<RemoteImage, ImageSource>,
    fallback: ImageSource,
}

impl Resolved {
    /// No resolved image: only the placeholder draws.
    pub(crate) fn empty(fallback: ImageSource) -> Self {
        Self {
            images: HashMap::new(),
            fallback,
        }
    }

    /// A map of `images` (tests).
    #[cfg(test)]
    pub(crate) fn with(images: Vec<(RemoteImage, ImageSource)>, fallback: ImageSource) -> Self {
        Self {
            images: images.into_iter().collect(),
            fallback,
        }
    }

    /// The source `image` draws; `None` when the job did not declare it.
    pub(crate) fn get(&self, image: &RemoteImage) -> Option<&ImageSource> {
        if image.is_placeholder() {
            return Some(&self.fallback);
        }
        self.images.get(image)
    }
}

impl fmt::Debug for Resolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut images: Vec<String> = self.images.keys().map(ToString::to_string).collect();
        images.sort_unstable();
        f.debug_struct("Resolved")
            .field("images", &images)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
