//! The edge's DNS: an async resolver behind a TTL-honouring cache with a
//! single-flight background refresh and stale answers on failure.

use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};
use hickory_resolver::TokioResolver;
use tokio::time::Instant;

/// How long past its TTL a cached answer is still served when the resolver
/// fails.
const STALE_MAX: Duration = Duration::from_hours(1);

/// One resolver answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    /// The addresses, in resolver order.
    pub(crate) addrs: Vec<IpAddr>,
    /// How long the answer may be cached (clamped by [`DnsCache`]).
    pub(crate) ttl: Duration,
}

/// Something that resolves a host name.
pub(crate) trait Resolve: Send + Sync + 'static {
    /// Resolves `host`.
    fn resolve(&self, host: &str) -> BoxFuture<'_, io::Result<Resolved>>;
}

/// The production resolver: hickory over the system configuration, or
/// `getaddrinfo` (with a zero TTL, which the cache raises to its floor)
/// when hickory cannot read it. Built on first use, inside the runtime.
#[derive(Default)]
pub(crate) struct SystemResolver {
    hickory: OnceLock<Option<TokioResolver>>,
}

impl SystemResolver {
    fn hickory(&self) -> Option<&TokioResolver> {
        self.hickory
            .get_or_init(|| match TokioResolver::builder_tokio().and_then(hickory_resolver::ResolverBuilder::build) {
                Ok(resolver) => Some(resolver),
                Err(error) => {
                    tracing::warn!(
                        target: "noctis::http",
                        %error,
                        "the DNS resolver could not read the system configuration; using getaddrinfo"
                    );
                    None
                }
            })
            .as_ref()
    }
}

impl Resolve for SystemResolver {
    fn resolve(&self, host: &str) -> BoxFuture<'_, io::Result<Resolved>> {
        let host = host.to_owned();
        async move {
            if let Some(resolver) = self.hickory() {
                let lookup = resolver
                    .lookup_ip(host.as_str())
                    .await
                    .map_err(io::Error::other)?;
                let ttl = lookup
                    .valid_until()
                    .saturating_duration_since(std::time::Instant::now());
                return Ok(Resolved {
                    addrs: lookup.iter().collect(),
                    ttl,
                });
            }
            let addrs = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .map(|addr| addr.ip())
                .collect();
            Ok(Resolved {
                addrs,
                ttl: Duration::ZERO,
            })
        }
        .boxed()
    }
}

/// A host's addresses, or why there are none (shared between waiters).
type Answer = Result<Vec<IpAddr>, io::ErrorKind>;
/// One lookup that every waiter shares.
type Flight = Shared<BoxFuture<'static, Answer>>;

/// A cached answer.
#[derive(Debug, Clone)]
struct Cached {
    addrs: Vec<IpAddr>,
    fetched_at: Instant,
    /// The clamped TTL.
    ttl: Duration,
    /// The last instant the answer was known valid (`fetched_at + ttl`);
    /// stale answers are served up to [`STALE_MAX`] after it.
    last_success: Instant,
}

#[derive(Default)]
struct Entry {
    cached: Option<Cached>,
    inflight: Option<Flight>,
}

struct CacheInner {
    resolver: Arc<dyn Resolve>,
    floor: Duration,
    cap: Duration,
    entries: Mutex<HashMap<String, Entry>>,
}

/// The TTL cache in front of a [`Resolve`].
///
/// A fresh answer is returned from the cache; from 75 % of its TTL on, one
/// background refresh starts (single-flight). An expired answer makes
/// callers await one shared lookup. A failed or empty lookup serves the
/// cached answer for up to 1 h past its TTL, else it is an error.
#[derive(Clone)]
pub(crate) struct DnsCache {
    inner: Arc<CacheInner>,
}

impl DnsCache {
    pub(crate) fn new(resolver: Arc<dyn Resolve>, floor: Duration, cap: Duration) -> Self {
        Self {
            inner: Arc::new(CacheInner {
                resolver,
                floor,
                cap,
                entries: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The addresses of `host`. An IP literal (IPv6 in brackets or not)
    /// never reaches the resolver.
    pub(crate) async fn lookup(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        if let Some(ip) = ip_literal(host) {
            return Ok(vec![ip]);
        }
        let now = Instant::now();
        let flight = {
            let mut entries = self.inner.lock();
            let entry = entries.entry(host.to_owned()).or_default();
            if let Some(cached) = &entry.cached {
                let age = now.saturating_duration_since(cached.fetched_at);
                if age < cached.ttl {
                    let addrs = cached.addrs.clone();
                    if age >= cached.ttl.mul_f64(0.75) && entry.inflight.is_none() {
                        let flight = self.flight(host);
                        entry.inflight = Some(flight.clone());
                        tokio::spawn(flight);
                    }
                    return Ok(addrs);
                }
            }
            entry
                .inflight
                .get_or_insert_with(|| self.flight(host))
                .clone()
        };
        flight.await.map_err(io::Error::from)
    }

    /// A shared lookup that stores its answer (or applies the stale rule)
    /// when it completes.
    fn flight(&self, host: &str) -> Flight {
        let inner = Arc::clone(&self.inner);
        let host = host.to_owned();
        async move {
            let result = inner.resolver.resolve(&host).await;
            inner.settle(&host, result)
        }
        .boxed()
        .shared()
    }
}

impl CacheInner {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores a lookup's outcome and returns what callers get.
    fn settle(&self, host: &str, result: io::Result<Resolved>) -> Answer {
        let now = Instant::now();
        let mut entries = self.lock();
        let entry = entries.entry(host.to_owned()).or_default();
        entry.inflight = None;
        match result {
            Ok(resolved) if !resolved.addrs.is_empty() => {
                let ttl = resolved.ttl.clamp(self.floor, self.cap);
                entry.cached = Some(Cached {
                    addrs: resolved.addrs.clone(),
                    fetched_at: now,
                    ttl,
                    last_success: now + ttl,
                });
                Ok(resolved.addrs)
            }
            other => {
                let kind = match other {
                    Err(error) => error.kind(),
                    Ok(_) => io::ErrorKind::NotFound,
                };
                tracing::debug!(target: "noctis::http", ?kind, "DNS lookup failed");
                match &entry.cached {
                    Some(cached)
                        if now.saturating_duration_since(cached.last_success) <= STALE_MAX =>
                    {
                        Ok(cached.addrs.clone())
                    }
                    _ => Err(kind),
                }
            }
        }
    }
}

/// `host` as an IP address: `127.0.0.1`, `::1` or `[::1]`.
pub(crate) fn ip_literal(host: &str) -> Option<IpAddr> {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse().ok()
}

#[cfg(test)]
mod tests;
