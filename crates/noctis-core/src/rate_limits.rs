//! Declared rate limits on Commands and Routes (FR-46, AD-23 stage 5;
//! INT-14).
//!
//! A kind attribute declares `rate_limit(<scope>, <n>, per = "<dur>"[,
//! manual])` once per scope; the derive emits them as the kind trait's
//! `RATE_LIMITS` const and `#[handler]` binds them onto the descriptor.
//! Stage 5 of the interaction pipelines (and of
//! [`Dispatcher::run_command`](crate::Dispatcher::run_command)) counts each
//! declared limit, in declared order, in the App's namespaced
//! `RateLimitStore`, before the auto-defer, the Middleware and the handler.
//!
//! Each limit of a handler struct keeps two logical keys (the
//! `Namespaced` port adds the `<app>:rate_limit:` prefix):
//!
//! - `n/<scope>/<segment>/<NAME>`: the counter, the port's 8-byte
//!   big-endian `u64`;
//! - `e/<scope>/<segment>/<NAME>`: the window end in unix milliseconds,
//!   written with `noctis_ports::encode_value` (best effort: it only shows
//!   the time remaining and backs `block`/`extend`).
//!
//! `<NAME>` is the descriptor's name (`module_path!()::Struct`), last so it
//! may hold any character. The segments are: user `<user>`; member
//! `<guild>.<user>` (`0.<user>` outside a guild); channel `<channel>`;
//! guild `<guild>` (`c<channel>` outside a guild); message `<message id>`.
//! A scope whose segment cannot be formed is unkeyed: stage 5 skips it.
//!
//! A store error never takes a Command down: stage 5 fails open with one
//! `warn` on `noctis::rate_limit`.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures_util::future::BoxFuture;
use noctis_ports::{PortError, RateLimitStore};
use noctis_rules::Locale;
use noctis_ui::V2Message;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker};

use crate::State;
use crate::command::CtxInner;
use crate::handler::HandlerResult;

/// The schema byte of a stored window end.
const END_SCHEMA: u8 = 1;

/// The longest window, block or extension: one day (the memory store's TTL
/// cap).
pub(crate) const MAX_WINDOW: Duration = Duration::from_hours(24);

/// What a declared rate limit counts per: its key scope (FR-46).
///
/// ```
/// use noctis_core::RateScope;
///
/// assert_eq!(RateScope::Member.as_str(), "member");
/// assert_eq!(RateScope::Guild.to_string(), "guild");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateScope {
    /// The invoking user, everywhere.
    User,
    /// The invoking user in one guild (outside a guild: the user under
    /// guild `0`).
    Member,
    /// The channel the interaction came from (unkeyed without one).
    Channel,
    /// The guild (outside a guild: the DM channel).
    Guild,
    /// The message: a message command's target, a component's source
    /// message, or the source message of a component-opened modal (unkeyed
    /// without one).
    Message,
}

impl RateScope {
    /// The lowercase name, as written in `rate_limit(<scope>, ..)`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Member => "member",
            Self::Channel => "channel",
            Self::Guild => "guild",
            Self::Message => "message",
        }
    }
}

impl fmt::Display for RateScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One declared rate limit: `rate_limit(<scope>, <max_uses>, per =
/// "<window>"[, manual])` on a kind attribute.
///
/// An automatic limit counts every invocation; a `manual` one counts only
/// the handler's `ctx.rate_limits().hit(scope)` calls. Either way, a scope
/// allows `max_uses` uses per window, and the window starts at the first
/// counted use.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_core::{RateLimit, RateScope};
///
/// const LIMIT: RateLimit = RateLimit::__declare(RateScope::User, 3, 10, false);
/// assert_eq!(LIMIT.scope(), RateScope::User);
/// assert_eq!(LIMIT.max_uses(), 3);
/// assert_eq!(LIMIT.window(), Duration::from_secs(10));
/// assert!(!LIMIT.is_manual());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    scope: RateScope,
    max_uses: u32,
    window_secs: u32,
    manual: bool,
}

impl RateLimit {
    /// A declared limit; what the kind derives emit (`window_secs` is
    /// already checked to `1..=86_400`, `max_uses` to `1..`).
    #[doc(hidden)]
    #[must_use]
    pub const fn __declare(
        scope: RateScope,
        max_uses: u32,
        window_secs: u32,
        manual: bool,
    ) -> Self {
        Self {
            scope,
            max_uses,
            window_secs,
            manual,
        }
    }

    /// The key scope.
    #[must_use]
    pub const fn scope(&self) -> RateScope {
        self.scope
    }

    /// The uses allowed per window.
    #[must_use]
    pub const fn max_uses(&self) -> u32 {
        self.max_uses
    }

    /// The window length.
    #[must_use]
    pub const fn window(&self) -> Duration {
        Duration::from_secs(self.window_secs as u64)
    }

    /// Whether only `ctx.rate_limits().hit(..)` counts uses (`manual`).
    #[must_use]
    pub const fn is_manual(&self) -> bool {
        self.manual
    }
}

/// A limit that refused an invocation: what an `#[on_rate_limited]` method
/// or the App's `rate_limited_handler` receives.
///
/// ```
/// # use std::time::Duration;
/// # use noctis_core::{RateLimit, RateLimitHit, RateScope};
/// # let hit = RateLimitHit::__new(RateLimit::__declare(RateScope::User, 3, 10, false), Duration::from_millis(1_500), 1_000_000);
/// assert_eq!(hit.scope(), RateScope::User);
/// assert_eq!(hit.retry_at_unix_secs(), 1_002);
/// assert_eq!(hit.relative_timestamp(), "<t:1002:R>");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitHit {
    limit: RateLimit,
    retry_after: Duration,
    /// Unix milliseconds at which the scope allows uses again.
    end_ms: u64,
}

impl RateLimitHit {
    /// A hit of `limit`, `retry_after` before uses are allowed again, seen
    /// at `now_ms` (unix milliseconds).
    #[doc(hidden)]
    #[must_use]
    pub fn __new(limit: RateLimit, retry_after: Duration, now_ms: u64) -> Self {
        let retry_ms = u64::try_from(retry_after.as_millis()).unwrap_or(u64::MAX);
        Self {
            limit,
            retry_after,
            end_ms: now_ms.saturating_add(retry_ms),
        }
    }

    /// The scope of the limit that was hit.
    #[must_use]
    pub fn scope(&self) -> RateScope {
        self.limit.scope
    }

    /// The limit that was hit.
    #[must_use]
    pub fn limit(&self) -> RateLimit {
        self.limit
    }

    /// How long until the scope allows uses again.
    #[must_use]
    pub fn retry_after(&self) -> Duration {
        self.retry_after
    }

    /// When the scope allows uses again, in unix seconds (rounded up).
    #[must_use]
    pub fn retry_at_unix_secs(&self) -> u64 {
        self.end_ms.div_ceil(1_000)
    }

    /// The Discord relative timestamp of
    /// [`retry_at_unix_secs`](Self::retry_at_unix_secs): `<t:N:R>`.
    #[must_use]
    pub fn relative_timestamp(&self) -> String {
        format!("<t:{}:R>", self.retry_at_unix_secs())
    }
}

/// Why a [`RateLimits`] call changed nothing.
///
/// ```
/// use noctis_core::{RateLimitError, RateScope};
///
/// let err = RateLimitError::Undeclared { scope: RateScope::Guild };
/// assert_eq!(err.to_string(), "the handler declares no `guild` rate limit");
/// ```
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    /// The handler declares no limit of this scope.
    #[error("the handler declares no `{scope}` rate limit")]
    Undeclared {
        /// The scope asked for.
        scope: RateScope,
    },
    /// The interaction has no key for this scope (no channel, or no
    /// message for `message`).
    #[error("this interaction has no `{scope}` rate-limit key")]
    Unkeyed {
        /// The scope asked for.
        scope: RateScope,
    },
    /// The context has no App (the detached test seams).
    #[error("a detached context has no rate limits")]
    Detached,
    /// The `RateLimitStore` failed.
    #[error("the rate-limit store failed")]
    Store(#[from] PortError),
}

/// The ids a run's limits are keyed by.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RateIds {
    pub(crate) user: Id<UserMarker>,
    pub(crate) guild: Option<Id<GuildMarker>>,
    pub(crate) channel: Option<Id<ChannelMarker>>,
    pub(crate) message: Option<Id<MessageMarker>>,
}

/// One handler run's declared limits, the ids they are keyed by and the
/// App's namespaced store: what stage 5 counts and `ctx.rate_limits()`
/// changes.
pub(crate) struct RateRun {
    store: Arc<dyn RateLimitStore>,
    handler: &'static str,
    limits: &'static [RateLimit],
    ids: RateIds,
}

impl fmt::Debug for RateRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateRun")
            .field("handler", &self.handler)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

/// The result of stage 5.
#[derive(Debug)]
pub(crate) enum Stage5 {
    /// No declared limit refused the run.
    Pass,
    /// This limit refused it.
    Hit(RateLimitHit),
}

/// Now, in unix milliseconds.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Milliseconds of `d`, saturating.
fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// The 8-byte big-endian counter of `value`, `None` when it is not one.
fn counter(value: &[u8]) -> Option<u64> {
    <[u8; 8]>::try_from(value).ok().map(u64::from_be_bytes)
}

impl RateRun {
    pub(crate) fn new(
        store: Arc<dyn RateLimitStore>,
        handler: &'static str,
        limits: &'static [RateLimit],
        ids: RateIds,
    ) -> Self {
        Self {
            store,
            handler,
            limits,
            ids,
        }
    }

    /// Whether the handler declares any limit.
    pub(crate) fn is_empty(&self) -> bool {
        self.limits.is_empty()
    }

    /// The key segment of `scope`, `None` when it cannot be formed.
    fn segment(&self, scope: RateScope) -> Option<String> {
        let ids = &self.ids;
        match scope {
            RateScope::User => Some(ids.user.to_string()),
            RateScope::Member => Some(format!("{}.{}", ids.guild.map_or(0, Id::get), ids.user)),
            RateScope::Channel => ids.channel.map(|c| c.to_string()),
            RateScope::Guild => match (ids.guild, ids.channel) {
                (Some(guild), _) => Some(guild.to_string()),
                (None, Some(channel)) => Some(format!("c{channel}")),
                (None, None) => None,
            },
            RateScope::Message => ids.message.map(|m| m.to_string()),
        }
    }

    /// The counter and window-end keys of `scope` under `segment`.
    fn keys(&self, scope: RateScope, segment: &str) -> (String, String) {
        let tail = format!("{}/{segment}/{}", scope.as_str(), self.handler);
        (format!("n/{tail}"), format!("e/{tail}"))
    }

    /// The declared limit of `scope` and its keys.
    fn declared(&self, scope: RateScope) -> Result<(RateLimit, String, String), RateLimitError> {
        let limit = self
            .limits
            .iter()
            .find(|l| l.scope == scope)
            .copied()
            .ok_or(RateLimitError::Undeclared { scope })?;
        let segment = self
            .segment(scope)
            .ok_or(RateLimitError::Unkeyed { scope })?;
        let (n, e) = self.keys(scope, &segment);
        Ok((limit, n, e))
    }

    /// The stored window end of `key` in unix ms; absent, undecodable or a
    /// store error reads as `None`.
    async fn window_end(&self, key: &str) -> Option<u64> {
        let raw = self.store.get(key).await.ok()??;
        noctis_ports::decode_value::<u64>(END_SCHEMA, &raw).ok()?
    }

    /// Writes the window end `end_ms`, living `ttl`.
    async fn put_end(&self, key: &str, end_ms: u64, ttl: Duration) -> Result<(), PortError> {
        let value = noctis_ports::encode_value(END_SCHEMA, &end_ms)?;
        self.store.put(key, value, Some(ttl)).await
    }

    /// The hit of `limit` at `now`: the stored window end, else a whole
    /// window.
    async fn hit_of(&self, limit: RateLimit, end_key: &str, now: u64) -> RateLimitHit {
        let retry = match self.window_end(end_key).await {
            Some(end) if end > now => Duration::from_millis(end - now),
            _ => limit.window(),
        };
        RateLimitHit::__new(limit, retry, now)
    }

    /// Stage 5 (AD-23): counts each declared limit in order and stops at
    /// the first one that refuses the run. An unkeyed scope is skipped
    /// (`debug`); a store error skips its limit (fail open, `warn`).
    pub(crate) async fn stage5(&self) -> Stage5 {
        for &limit in self.limits {
            let scope = limit.scope;
            let Some(segment) = self.segment(scope) else {
                tracing::debug!(
                    target: "noctis::rate_limit",
                    handler = self.handler,
                    scope = scope.as_str(),
                    "unkeyed rate-limit scope skipped"
                );
                continue;
            };
            let (n_key, e_key) = self.keys(scope, &segment);
            let now = now_ms();
            let max = u64::from(limit.max_uses);
            let hit = if limit.manual {
                match self.store.get(&n_key).await {
                    Ok(None) => false,
                    Ok(Some(raw)) => {
                        if let Some(count) = counter(&raw) {
                            count >= max
                        } else {
                            tracing::warn!(
                                target: "noctis::rate_limit",
                                handler = self.handler,
                                scope = scope.as_str(),
                                "malformed rate-limit counter read as 0"
                            );
                            false
                        }
                    }
                    Err(error) => {
                        fail_open(self.handler, scope, &error);
                        continue;
                    }
                }
            } else {
                match self
                    .store
                    .incr_with_ttl(&n_key, 1, Some(limit.window()))
                    .await
                {
                    Ok(1) => {
                        let window = limit.window();
                        if let Err(error) = self
                            .put_end(&e_key, now.saturating_add(millis(window)), window)
                            .await
                        {
                            fail_open(self.handler, scope, &error);
                        }
                        1 > max
                    }
                    Ok(count) => count > max,
                    Err(error) => {
                        fail_open(self.handler, scope, &error);
                        continue;
                    }
                }
            };
            if hit {
                return Stage5::Hit(self.hit_of(limit, &e_key, now).await);
            }
        }
        Stage5::Pass
    }

    /// Counts one use of `scope` (see [`RateLimits::hit`]).
    async fn hit(&self, scope: RateScope) -> Result<u64, RateLimitError> {
        let (limit, n_key, e_key) = self.declared(scope)?;
        let window = limit.window();
        let now = now_ms();
        let count = self.store.incr_with_ttl(&n_key, 1, Some(window)).await?;
        if count == 1 {
            self.put_end(&e_key, now.saturating_add(millis(window)), window)
                .await?;
        }
        Ok(count)
    }

    /// Blocks `scope` until `end_ms` (clamped to a day from `now`): the
    /// counter is set to the limit, the window end to `end_ms`.
    async fn block_until(
        &self,
        limit: RateLimit,
        n_key: &str,
        e_key: &str,
        now: u64,
        end_ms: u64,
    ) -> Result<(), RateLimitError> {
        let end = end_ms.min(now.saturating_add(millis(MAX_WINDOW)));
        let Some(ttl) = end.checked_sub(now).filter(|&ms| ms > 0) else {
            return Ok(());
        };
        let ttl = Duration::from_millis(ttl);
        let count = Bytes::copy_from_slice(&u64::from(limit.max_uses).to_be_bytes());
        self.store.put(n_key, count, Some(ttl)).await?;
        self.put_end(e_key, end, ttl).await?;
        Ok(())
    }

    /// See [`RateLimits::block`].
    async fn block(&self, scope: RateScope, duration: Duration) -> Result<(), RateLimitError> {
        let (limit, n_key, e_key) = self.declared(scope)?;
        if duration.is_zero() {
            return Ok(());
        }
        let now = now_ms();
        let wanted = now.saturating_add(millis(duration));
        let end = match self.window_end(&e_key).await {
            Some(live) if live > wanted => live,
            _ => wanted,
        };
        self.block_until(limit, &n_key, &e_key, now, end).await
    }

    /// See [`RateLimits::extend`].
    async fn extend(&self, scope: RateScope, duration: Duration) -> Result<(), RateLimitError> {
        let (limit, n_key, e_key) = self.declared(scope)?;
        if duration.is_zero() {
            return Ok(());
        }
        let now = now_ms();
        let base = self
            .window_end(&e_key)
            .await
            .filter(|&end| end > now)
            .unwrap_or(now);
        let end = base.saturating_add(millis(duration));
        self.block_until(limit, &n_key, &e_key, now, end).await
    }

    /// See [`RateLimits::reset`].
    async fn reset(&self, scope: RateScope) -> Result<(), RateLimitError> {
        let (limit, n_key, e_key) = self.declared(scope)?;
        let window = limit.window();
        let now = now_ms();
        self.store
            .put(&n_key, Bytes::from_static(&[0; 8]), Some(window))
            .await?;
        self.put_end(&e_key, now.saturating_add(millis(window)), window)
            .await?;
        Ok(())
    }
}

/// The fail-open `warn` of a store error in stage 5 (no ids).
fn fail_open(handler: &'static str, scope: RateScope, error: &PortError) {
    tracing::warn!(
        target: "noctis::rate_limit",
        handler,
        scope = scope.as_str(),
        %error,
        "rate-limit store failed: limit not applied"
    );
}

/// The handler's own access to its declared rate limits:
/// `ctx.rate_limits()` on [`CommandCtx`](crate::CommandCtx),
/// [`ComponentCtx`](crate::ComponentCtx) and
/// [`ModalSubmitCtx`](crate::ModalSubmitCtx).
///
/// Every call names a scope the handler declares with `rate_limit(..)`;
/// the next invocation's stage 5 sees the change. A `manual` limit is
/// counted only by [`hit`](Self::hit).
///
/// ```
/// use noctis_core::{CommandCtx, HandlerResult, RateScope};
///
/// async fn after_claim(ctx: &CommandCtx) -> HandlerResult<()> {
///     // Only a successful claim is charged.
///     ctx.rate_limits().hit(RateScope::User).await?;
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct RateLimits(Option<Arc<RateRun>>);

impl fmt::Debug for RateLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RateLimits").field(&self.0).finish()
    }
}

impl RateLimits {
    pub(crate) fn of(run: Option<&Arc<RateRun>>) -> Self {
        Self(run.cloned())
    }

    fn run(&self) -> Result<&RateRun, RateLimitError> {
        self.0.as_deref().ok_or(RateLimitError::Detached)
    }

    /// Counts one use of `scope` and returns the window's count; the first
    /// use starts the window.
    ///
    /// # Errors
    ///
    /// [`RateLimitError::Undeclared`], [`Unkeyed`](RateLimitError::Unkeyed),
    /// [`Detached`](RateLimitError::Detached) or
    /// [`Store`](RateLimitError::Store).
    pub async fn hit(&self, scope: RateScope) -> Result<u64, RateLimitError> {
        self.run()?.hit(scope).await
    }

    /// Refuses `scope` for `duration` from now (at most a day); a later
    /// live block is kept. `Duration::ZERO` changes nothing.
    ///
    /// # Errors
    ///
    /// As [`hit`](Self::hit).
    pub async fn block(&self, scope: RateScope, duration: Duration) -> Result<(), RateLimitError> {
        self.run()?.block(scope, duration).await
    }

    /// Refuses `scope` for `duration` more: from the current live window
    /// end, else from now (at most a day from now). `Duration::ZERO`
    /// changes nothing.
    ///
    /// # Errors
    ///
    /// As [`hit`](Self::hit).
    pub async fn extend(&self, scope: RateScope, duration: Duration) -> Result<(), RateLimitError> {
        self.run()?.extend(scope, duration).await
    }

    /// Clears `scope`'s count: a fresh window starts now.
    ///
    /// # Errors
    ///
    /// As [`hit`](Self::hit).
    pub async fn reset(&self, scope: RateScope) -> Result<(), RateLimitError> {
        self.run()?.reset(scope).await
    }
}

/// The context of an `#[on_rate_limited]` method or the App's
/// `rate_limited_handler`: who was refused, where, in which locales.
///
/// Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct RateLimitedCtx(Arc<CtxInner>);

impl RateLimitedCtx {
    pub(crate) fn new(parts: Arc<CtxInner>) -> Self {
        Self(parts)
    }

    /// The id of the Module the limited handler was added to.
    pub fn module(&self) -> &'static str {
        self.0.module
    }

    /// The invoked Command Path, menu name or Route id.
    pub fn path(&self) -> &'static str {
        self.0.path
    }

    /// The invoking user.
    pub fn user_id(&self) -> Id<UserMarker> {
        self.0.user_id
    }

    /// The guild, if any.
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.0.guild_id
    }

    /// The channel, if Discord sent it.
    pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.0.channel_id
    }

    /// The resolved User Locale: the notice is ephemeral, so write it in
    /// this locale.
    pub fn user_locale(&self) -> Locale {
        self.0.locales().user
    }

    /// The resolved Guild Locale.
    pub fn guild_locale(&self) -> Locale {
        self.0.locales().guild
    }

    /// The state registered for `T` with `AppBuilder::state`, if any.
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        self.0.app.as_ref()?.states.get::<T>()
    }
}

impl fmt::Debug for RateLimitedCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimitedCtx")
            .field("module", &self.0.module)
            .field("path", &self.0.path)
            .field("guild_id", &self.0.guild_id)
            .field("channel_id", &self.0.channel_id)
            .field("user_id", &self.0.user_id)
            .finish_non_exhaustive()
    }
}

/// An erased rate-limited override: the struct's `#[on_rate_limited]`
/// method or the App's `rate_limited_handler`.
pub(crate) type RateLimitedFn = dyn Fn(RateLimitedCtx, RateLimitHit) -> BoxFuture<'static, HandlerResult<V2Message>>
    + Send
    + Sync;

/// Erases the override `f`.
pub(crate) fn erase_rate_limited<F, Fut>(f: F) -> Arc<RateLimitedFn>
where
    F: Fn(RateLimitedCtx, RateLimitHit) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<V2Message>> + Send + 'static,
{
    Arc::new(move |ctx, hit| Box::pin(f(ctx, hit)))
}

/// A bound `#[on_rate_limited]` method; what the attribute's hidden
/// `__noctis_on_rate_limited()` returns.
#[doc(hidden)]
pub struct __RateLimitedHook(pub(crate) Arc<RateLimitedFn>);

impl __RateLimitedHook {
    /// Binds `f`: `async fn (ctx: RateLimitedCtx, hit: RateLimitHit) ->
    /// HandlerResult<V2Message>`.
    #[doc(hidden)]
    pub fn __new<F, Fut>(f: F) -> Self
    where
        F: Fn(RateLimitedCtx, RateLimitHit) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<V2Message>> + Send + 'static,
    {
        Self(erase_rate_limited(f))
    }
}

impl fmt::Debug for __RateLimitedHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("__RateLimitedHook")
    }
}

#[cfg(test)]
mod tests;
