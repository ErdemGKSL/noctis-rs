//! [`RemoteConfig`]: the App's allow-list, limits, cache TTL and fallback
//! for remote images, and the [`Origin`] grammar of its allow-list.

use std::fmt;
use std::ops::RangeInclusive;
use std::time::Duration;

use super::{AuthorityError, HostPort, ParsedUrl, Scheme};
use crate::StaticImage;

/// The origins every App allows: the Discord CDN and its media proxy.
pub(crate) const DEFAULT_ORIGINS: [&str; 2] =
    ["https://cdn.discordapp.com", "https://media.discordapp.net"];
/// The default origin avatar and icon URLs are built on.
const DEFAULT_CDN: &str = "https://cdn.discordapp.com";

const TIMEOUT: RangeInclusive<Duration> = Duration::from_millis(100)..=Duration::from_mins(1);
const MAX_BYTES: RangeInclusive<u64> = 1024..=67_108_864;
const CONCURRENCY: RangeInclusive<usize> = 1..=256;
const MAX_PER_JOB: RangeInclusive<usize> = 1..=64;
const MAX_PIXELS: RangeInclusive<u64> = 1..=16_777_216;
const CACHE_TTL: RangeInclusive<Duration> = Duration::from_secs(1)..=Duration::from_hours(30 * 24);

/// How an App resolves the remote images its Templates declare (IMG-03);
/// set with `ImageConfig::remote`.
///
/// - The allow-list: `https://cdn.discordapp.com` and
///   `https://media.discordapp.net`, plus every [`allow_origin`](Self::allow_origin)
///   and the [`discord_cdn`](Self::discord_cdn) origin. An origin is
///   `scheme://host[:port]` (one trailing `/` allowed), `https`, or `http`
///   for `127.0.0.1`, `[::1]` and `localhost` only; scheme, host and
///   effective port must match exactly. Redirects are never followed.
/// - `timeout` (100 ms..=60 s, default 5 s): the permit wait, the request
///   and the body of one image.
/// - `max_bytes` (1024..=67108864, default 8 MiB): the largest body.
/// - `concurrency` (1..=256, default 8): images fetched and decoded at once
///   per App.
/// - `max_per_job` (1..=64, default 16): the distinct remote images one job
///   may declare; more is `RenderError::OverCap` with
///   `RenderCap::RemoteImages`.
/// - `max_pixels` (1..=16777216, default 4194304): the largest image,
///   checked from its header before it is decoded.
/// - `cache_ttl` (1 s..=30 days, default 1 day): how long fetched bytes stay
///   in the App's `AssetStore`.
/// - `fallback` (default a built-in 1x1 `#475569` pixel): what an image
///   that cannot be used draws.
///
/// Out-of-range values and invalid origins fail `App::build` with
/// `[IMG-03]` plugin violations.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_image::RemoteConfig;
///
/// let config = RemoteConfig::new()
///     .allow_origin("https://i.imgur.com")
///     .timeout(Duration::from_secs(2))
///     .max_bytes(4 * 1024 * 1024)
///     .concurrency(4)
///     .max_per_job(8)
///     .cache_ttl(Duration::from_secs(3600));
/// assert_ne!(config, RemoteConfig::default());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfig {
    origins: Vec<String>,
    discord_cdn: String,
    timeout: Duration,
    max_bytes: u64,
    concurrency: usize,
    max_per_job: usize,
    max_pixels: u64,
    cache_ttl: Duration,
    fallback: Option<StaticImage>,
}

impl RemoteConfig {
    /// The defaults: the Discord CDN allow-list, a 5 s timeout, 8 MiB,
    /// 8 at once, 16 per job, 4 MP, a 1 day cache TTL and the built-in
    /// fallback.
    pub fn new() -> Self {
        Self {
            origins: Vec::new(),
            discord_cdn: DEFAULT_CDN.to_owned(),
            timeout: Duration::from_secs(5),
            max_bytes: 8_388_608,
            concurrency: 8,
            max_per_job: 16,
            max_pixels: 4_194_304,
            cache_ttl: Duration::from_hours(24),
            fallback: None,
        }
    }

    /// Allows `origin` (`scheme://host[:port]`) in addition to the Discord
    /// CDN; duplicates are ignored.
    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        self.origins.push(origin.into());
        self
    }

    /// The origin avatar and icon URLs are built on (default
    /// `https://cdn.discordapp.com`); always allow-listed. Point it at a
    /// media proxy, or at a loopback server in tests.
    #[must_use]
    pub fn discord_cdn(mut self, origin: impl Into<String>) -> Self {
        self.discord_cdn = origin.into();
        self
    }

    /// The time one image may take: permit wait, request and body
    /// (100 ms..=60 s, default 5 s).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The largest body in bytes (1024..=67108864, default 8388608).
    #[must_use]
    pub fn max_bytes(mut self, bytes: u64) -> Self {
        self.max_bytes = bytes;
        self
    }

    /// The images fetched and decoded at once (1..=256, default 8).
    #[must_use]
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// The distinct remote images one job may declare (1..=64, default 16).
    #[must_use]
    pub fn max_per_job(mut self, images: usize) -> Self {
        self.max_per_job = images;
        self
    }

    /// The largest image in pixels (1..=16777216, default 4194304).
    #[must_use]
    pub fn max_pixels(mut self, pixels: u64) -> Self {
        self.max_pixels = pixels;
        self
    }

    /// How long fetched bytes stay in the `AssetStore` (1 s..=30 days,
    /// default 1 day).
    #[must_use]
    pub fn cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// The image drawn instead of one that cannot be used (and for
    /// [`RemoteImage::placeholder`](super::RemoteImage::placeholder));
    /// checked with the IMG-02 rules when the App builds.
    #[must_use]
    pub fn fallback(mut self, image: StaticImage) -> Self {
        self.fallback = Some(image);
        self
    }

    /// Every `[IMG-03]` problem: invalid origins and out-of-range values.
    pub(crate) fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for origin in self.origins.iter().chain([&self.discord_cdn]) {
            if let Err(reason) = Origin::parse(origin) {
                problems.push(format!(
                    "[IMG-03] remote origin `{origin}` is invalid: {reason}"
                ));
            }
        }
        let mut range = |name: &str, ok: bool, value: &dyn fmt::Debug, range: &str| {
            if !ok {
                problems.push(format!(
                    "[IMG-03] remote {name} {value:?} is out of range: use {range}"
                ));
            }
        };
        range(
            "timeout",
            TIMEOUT.contains(&self.timeout),
            &self.timeout,
            "100ms..=60s",
        );
        range(
            "max_bytes",
            MAX_BYTES.contains(&self.max_bytes),
            &self.max_bytes,
            "1024..=67108864",
        );
        range(
            "concurrency",
            CONCURRENCY.contains(&self.concurrency),
            &self.concurrency,
            "1..=256",
        );
        range(
            "max_per_job",
            MAX_PER_JOB.contains(&self.max_per_job),
            &self.max_per_job,
            "1..=64",
        );
        range(
            "max_pixels",
            MAX_PIXELS.contains(&self.max_pixels),
            &self.max_pixels,
            "1..=16777216",
        );
        range(
            "cache_ttl",
            CACHE_TTL.contains(&self.cache_ttl),
            &self.cache_ttl,
            "1s..=2592000s (30 days)",
        );
        problems
    }

    /// The allow-list: the defaults, the added origins and the CDN origin,
    /// parsed and deduplicated; invalid entries (already reported by
    /// [`validate`](Self::validate)) are skipped.
    pub(crate) fn origins(&self) -> Vec<Origin> {
        let mut origins: Vec<Origin> = Vec::new();
        let all = DEFAULT_ORIGINS
            .into_iter()
            .chain(self.origins.iter().map(String::as_str))
            .chain([self.discord_cdn.as_str()]);
        for origin in all.filter_map(|o| Origin::parse(o).ok()) {
            if !origins.contains(&origin) {
                origins.push(origin);
            }
        }
        origins
    }

    /// The CDN origin as `scheme://host[:port]`, without a trailing `/`.
    pub(crate) fn cdn(&self) -> String {
        Origin::parse(&self.discord_cdn)
            .map_or_else(|_| DEFAULT_CDN.to_owned(), |origin| origin.to_string())
    }

    pub(crate) fn timeout_value(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn max_bytes_value(&self) -> u64 {
        self.max_bytes
    }

    pub(crate) fn concurrency_value(&self) -> usize {
        self.concurrency
    }

    pub(crate) fn max_per_job_value(&self) -> usize {
        self.max_per_job
    }

    pub(crate) fn max_pixels_value(&self) -> u64 {
        self.max_pixels
    }

    pub(crate) fn cache_ttl_value(&self) -> Duration {
        self.cache_ttl
    }

    pub(crate) fn fallback_image(&self) -> Option<StaticImage> {
        self.fallback
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// An allow-listed origin: scheme, lowercase host and effective port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    scheme: Scheme,
    host: String,
    port: u16,
}

/// The hosts plain `http` may use.
const LOOPBACK: [&str; 3] = ["127.0.0.1", "[::1]", "localhost"];

impl Origin {
    /// Parses `scheme://host[:port]` with one optional trailing `/`.
    ///
    /// # Errors
    ///
    /// Why `text` is not an origin.
    pub(crate) fn parse(text: &str) -> Result<Self, &'static str> {
        if text.is_empty() {
            return Err("it is empty");
        }
        let Some((scheme, rest)) = text.split_once("://") else {
            return Err("it must start with `https://`");
        };
        let scheme = if scheme.eq_ignore_ascii_case("https") {
            Scheme::Https
        } else if scheme.eq_ignore_ascii_case("http") {
            Scheme::Http
        } else {
            return Err("the scheme must be `https` (or `http` for a loopback host)");
        };
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.contains('/') {
            return Err("it must not have a path");
        }
        if rest.contains('?') || rest.contains('#') {
            return Err("it must not have a query or fragment");
        }
        if rest.contains('@') {
            return Err("it must not have user info");
        }
        let HostPort { host, port } = HostPort::parse(rest).map_err(|err| match err {
            AuthorityError::NoHost => "it has no host",
            AuthorityError::Port => "the port must be 1..=65535",
            AuthorityError::Host => {
                "the host must be ASCII letters, digits, `.` and `-` (no wildcard), or an IP \
                 literal"
            }
        })?;
        if scheme == Scheme::Http && !LOOPBACK.contains(&host.as_str()) {
            return Err("plain http is only for loopback hosts");
        }
        Ok(Self {
            port: port.unwrap_or_else(|| scheme.default_port()),
            scheme,
            host,
        })
    }

    /// Whether `url` is on this origin.
    pub(crate) fn matches(&self, url: &ParsedUrl) -> bool {
        self.scheme == url.scheme && self.host == url.authority.host && self.port == url.port()
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme.as_str(), self.host)?;
        if self.port != self.scheme.default_port() {
            write!(f, ":{}", self.port)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
