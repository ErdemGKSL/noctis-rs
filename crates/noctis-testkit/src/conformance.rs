//! Adapter conformance suites (FR-39, AD-14): one async function per port
//! that proves a custom Adapter keeps the semantics core relies on.
//!
//! | Suite | Port | Time |
//! | --- | --- | --- |
//! | [`ref_store`] | `RefStore` | ~3 s |
//! | [`session_store`] | `SessionStore` | ~3 s |
//! | [`module_state_store`] | `ModuleStateStore` | ~3 s |
//! | [`rate_limit_store`] | `RateLimitStore` | ~3 s |
//! | [`cache_store`] | `CacheStore` | ~3 s |
//! | [`render_cache`] | `RenderCache` | ~3 s |
//! | [`asset_store`] | `AssetStore` | ~3 s |
//! | [`identify_queue`] | `IdentifyQueue` | ~5 s |
//! | [`discord_cache`] | `DiscordCache` | < 1 s |
//!
//! ```
//! use noctis_ports::memory::MemoryCacheStore;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! // In a test: #[tokio::test] async fn my_store_conforms() { .. }
//! noctis_testkit::conformance::cache_store(MemoryCacheStore::new).await;
//! # }
//! ```
//!
//! # The factory
//!
//! Every suite takes a factory closure and calls it once per case. Each call
//! must return a fresh, empty store: cases use fixed keys and rely on that
//! emptiness. For a Redis or SQL Adapter, hand out a fresh key prefix,
//! database or table per call. A byte-store suite writes at most 64
//! distinct keys per instance, so a bounded Adapter of at least that
//! capacity never evicts during a run.
//!
//! [`identify_queue`]'s factory must give at least 2 buckets and a queue
//! that granted no Lease in the last 5 s. [`discord_cache`]'s factory takes
//! the [`DiscordCacheResources`](noctis_ports::memory::DiscordCacheResources)
//! to cache, as core passes the configured set to a custom backend.
//!
//! # Failures
//!
//! A suite returns `()` when every case passes and panics on the first
//! failing case with `` conformance::<suite>: case `<case>` failed: <detail> ``.
//! An unexpected `Err(PortError)` fails the case, and the detail carries the
//! error's `Display`. A panic inside a racing task is re-raised with the
//! same prefix.
//!
//! # Clock and runtime
//!
//! The suites use the real clock: run them inside `#[tokio::test]` (either
//! flavor) with the time driver enabled, and never on a paused tokio clock
//! (`start_paused`). A byte-store suite writes every short-TTL entry (1 s)
//! first, then waits once for 2.5 s, which tolerates backends with
//! second-resolution TTLs.
//!
//! # Cases
//!
//! Byte stores (each case runs when the port has the ops it needs), in
//! this order: `missing_key`, `roundtrip_values`, `key_shapes`,
//! `overwrite`, `put_if_absent_live`, `take_once`, `delete_live`,
//! `touch_live`, `incr_basic`, `incr_saturates`, `incr_non_counter`,
//! `version_monotonic`, `namespacing`, then `race_put_if_absent`,
//! `race_take`, `race_incr` (32 concurrent callers), then
//! `ttl_live_before_expiry`, `ttl_get_expires`, `ttl_put_resets`,
//! `ttl_put_if_absent_expired`, `ttl_take_expired`, `ttl_touch`,
//! `ttl_incr_create_only`.
//!
//! `IdentifyQueue`: `lease_bucket`, `bucket_spacing`, `buckets_parallel`.
//!
//! `DiscordCache`: `empty`, `guild_create_all`, `unknown_ids`,
//! `owned_reads`, `updates_in_order`, `disabled_guild`, `disabled_channel`,
//! `disabled_role`, `disabled_member`, `disabled_user`, `disabled_message`,
//! `channel_only`. Each `disabled_*` case configures every resource but
//! one and expects that resource's getter to be `None` and every other
//! getter `Some`; `disabled_user` also turns off `MEMBER`, which caches
//! its users.

use std::any::Any;
use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;

use noctis_ports::PortError;
use tokio::sync::Barrier;

mod bytes;
mod discord;
mod identify;

pub use self::bytes::{
    asset_store, cache_store, module_state_store, rate_limit_store, ref_store, render_cache,
    session_store,
};
pub use self::discord::discord_cache;
pub use self::identify::identify_queue;

/// One case of one suite: the context of every failure message.
#[derive(Clone, Copy, Debug)]
struct Case {
    suite: &'static str,
    name: &'static str,
}

impl Case {
    const fn new(suite: &'static str, name: &'static str) -> Self {
        Self { suite, name }
    }

    /// Panics with the suite's failure format.
    fn fail(self, detail: impl Display) -> ! {
        panic!(
            "conformance::{}: case `{}` failed: {detail}",
            self.suite, self.name
        )
    }

    /// Fails unless `ok`, with the detail built lazily.
    fn check(self, ok: bool, detail: impl FnOnce() -> String) {
        if !ok {
            self.fail(detail());
        }
    }

    /// Unwraps a port result; an `Err` fails the case.
    fn ok<T>(self, op: impl Display, result: Result<T, PortError>) -> T {
        result.unwrap_or_else(|err| self.fail(format_args!("{op} returned Err: {err}")))
    }
}

/// The text of a panic payload.
fn panic_text(payload: &(dyn Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<&str>() {
        text
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text
    } else {
        "<non-string panic payload>"
    }
}

/// Spawns `callers` tasks on `shared`, releases them together through a
/// barrier, and returns their results in caller order.
///
/// `task` must return a lazy future (an `async move` block), so nothing runs
/// before the barrier. A task's `Err` or panic fails the case.
async fn race<S, T, F, Fut>(case: Case, shared: &Arc<S>, callers: usize, task: F) -> Vec<T>
where
    S: ?Sized + Send + Sync + 'static,
    T: Send + 'static,
    F: Fn(Arc<S>, usize) -> Fut,
    Fut: Future<Output = Result<T, PortError>> + Send + 'static,
{
    let gate = Arc::new(Barrier::new(callers));
    let handles: Vec<_> = (0..callers)
        .map(|caller| {
            let gate = Arc::clone(&gate);
            let work = task(Arc::clone(shared), caller);
            tokio::spawn(async move {
                gate.wait().await;
                work.await
            })
        })
        .collect();
    let mut results = Vec::with_capacity(callers);
    for (caller, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(result) => results.push(case.ok(format_args!("racing caller {caller}"), result)),
            Err(err) if err.is_panic() => {
                let payload = err.into_panic();
                case.fail(format_args!(
                    "racing caller {caller} panicked: {}",
                    panic_text(payload.as_ref())
                ));
            }
            Err(err) => case.fail(format_args!("racing caller {caller} was cancelled: {err}")),
        }
    }
    results
}
