//! FR-46 engine tests (Story 13.4 runtime matrix R1-R23, R30): stage 5 and
//! `ctx.rate_limits()` over the memory `RateLimitStore`.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use noctis_ports::memory::MemoryRateLimitStore;
use noctis_ports::{BoxFuture, PortError, RateLimitStore};
use twilight_model::id::Id;

use super::*;
use crate::namespace::{AppId, Namespaced};
use crate::test_support::Capture;

const NAME: &str = "bot::profile::Profile";

const USER3: RateLimit = RateLimit::__declare(RateScope::User, 3, 10, false);
const USER1_1S: RateLimit = RateLimit::__declare(RateScope::User, 1, 1, false);
const GUILD1: RateLimit = RateLimit::__declare(RateScope::Guild, 1, 10, false);
const MEMBER1: RateLimit = RateLimit::__declare(RateScope::Member, 1, 60, false);
const CHANNEL1: RateLimit = RateLimit::__declare(RateScope::Channel, 1, 60, false);
const MESSAGE1: RateLimit = RateLimit::__declare(RateScope::Message, 1, 60, false);
const MANUAL1: RateLimit = RateLimit::__declare(RateScope::User, 1, 60, true);

fn ids(user: u64, guild: Option<u64>, channel: Option<u64>, message: Option<u64>) -> RateIds {
    RateIds {
        user: Id::new(user),
        guild: guild.map(Id::new),
        channel: channel.map(Id::new),
        message: message.map(Id::new),
    }
}

fn store() -> Arc<dyn RateLimitStore> {
    Arc::new(MemoryRateLimitStore::new())
}

fn run_on(store: &Arc<dyn RateLimitStore>, limits: &'static [RateLimit], ids: RateIds) -> RateRun {
    RateRun::new(Arc::clone(store), NAME, limits, ids)
}

fn leak(limits: Vec<RateLimit>) -> &'static [RateLimit] {
    Box::leak(limits.into_boxed_slice())
}

fn is_hit(stage: &Stage5) -> Option<RateLimitHit> {
    match stage {
        Stage5::Hit(hit) => Some(*hit),
        Stage5::Pass => None,
    }
}

async fn count(store: &Arc<dyn RateLimitStore>, key: &str) -> Option<u64> {
    store.get(key).await.unwrap().and_then(|raw| counter(&raw))
}

fn near(actual: Duration, expected: Duration) {
    let diff = actual.abs_diff(expected);
    assert!(
        diff <= Duration::from_millis(1_500),
        "{actual:?} vs {expected:?}"
    );
}

/// The key format: `n|e/<scope>/<segment>/<NAME>`, with the fallbacks.
#[test]
fn int_14_key_segments() {
    let s = store();
    let guild = run_on(&s, &[], ids(5, Some(7), Some(9), Some(11)));
    assert_eq!(guild.segment(RateScope::User).as_deref(), Some("5"));
    assert_eq!(guild.segment(RateScope::Member).as_deref(), Some("7.5"));
    assert_eq!(guild.segment(RateScope::Channel).as_deref(), Some("9"));
    assert_eq!(guild.segment(RateScope::Guild).as_deref(), Some("7"));
    assert_eq!(guild.segment(RateScope::Message).as_deref(), Some("11"));
    assert_eq!(
        guild.keys(RateScope::Member, "7.5"),
        (
            format!("n/member/7.5/{NAME}"),
            format!("e/member/7.5/{NAME}")
        )
    );
    // R5: a DM keys member under guild 0 and guild by the DM channel.
    let dm = run_on(&s, &[], ids(5, None, Some(9), None));
    assert_eq!(dm.segment(RateScope::Member).as_deref(), Some("0.5"));
    assert_eq!(dm.segment(RateScope::Guild).as_deref(), Some("c9"));
    assert_eq!(dm.segment(RateScope::Message), None);
    // R6: no channel, no channel or DM guild key.
    let bare = run_on(&s, &[], ids(5, None, None, None));
    assert_eq!(bare.segment(RateScope::Channel), None);
    assert_eq!(bare.segment(RateScope::Guild), None);
}

/// R1, R2: three uses per window per user; a fourth is a hit retried at
/// the window's end; another user is counted apart.
#[tokio::test]
async fn int_14_user_limit() {
    let s = store();
    let a = run_on(&s, &[USER3], ids(1, Some(2), Some(3), None));
    for _ in 0..3 {
        assert!(is_hit(&a.stage5().await).is_none());
    }
    let hit = is_hit(&a.stage5().await).expect("fourth use");
    assert_eq!(hit.scope(), RateScope::User);
    assert_eq!(hit.limit(), USER3);
    near(hit.retry_after(), Duration::from_secs(10));
    let expected = now_ms().div_ceil(1_000) + 10;
    assert!(hit.retry_at_unix_secs().abs_diff(expected) <= 2);
    assert_eq!(
        hit.relative_timestamp(),
        format!("<t:{}:R>", hit.retry_at_unix_secs())
    );
    let b = run_on(&s, &[USER3], ids(4, Some(2), Some(3), None));
    assert!(is_hit(&b.stage5().await).is_none());
}

/// R3: the window expires.
#[tokio::test]
async fn int_14_window_expires() {
    let s = store();
    let a = run_on(&s, &[USER1_1S], ids(1, None, Some(3), None));
    assert!(is_hit(&a.stage5().await).is_none());
    assert!(is_hit(&a.stage5().await).is_some());
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    assert!(is_hit(&a.stage5().await).is_none());
}

/// R4, R5, R6, R7: per-scope keys.
#[tokio::test]
async fn int_14_scope_keys() {
    let s = store();
    // R4: member in two guilds.
    let g1 = run_on(&s, &[MEMBER1], ids(1, Some(10), Some(3), None));
    let g2 = run_on(&s, &[MEMBER1], ids(1, Some(20), Some(3), None));
    assert!(is_hit(&g1.stage5().await).is_none());
    assert!(is_hit(&g2.stage5().await).is_none());
    assert!(is_hit(&g1.stage5().await).is_some());
    // R5: member and guild apply in a DM.
    let limits = leak(vec![MEMBER1]);
    let dm = run_on(&s, limits, ids(1, None, Some(30), None));
    assert!(is_hit(&dm.stage5().await).is_none());
    assert!(is_hit(&dm.stage5().await).is_some());
    let dm = run_on(&s, &[GUILD1], ids(1, None, Some(30), None));
    assert!(is_hit(&dm.stage5().await).is_none());
    assert!(is_hit(&dm.stage5().await).is_some());
    // R6: an unkeyed scope is skipped with one `debug`.
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let bare = run_on(&s, &[CHANNEL1], ids(1, None, None, None));
    for _ in 0..3 {
        assert!(is_hit(&bare.stage5().await).is_none());
    }
    let lines = capture.lines_with(&[
        "DEBUG",
        "noctis::rate_limit",
        "scope=\"channel\"",
        "unkeyed",
    ]);
    assert_eq!(lines.len(), 3, "{}", capture.text());
    // R7: message scope per message.
    let m1 = run_on(&s, &[MESSAGE1], ids(1, Some(2), Some(3), Some(100)));
    let m2 = run_on(&s, &[MESSAGE1], ids(1, Some(2), Some(3), Some(200)));
    assert!(is_hit(&m1.stage5().await).is_none());
    assert!(is_hit(&m2.stage5().await).is_none());
    assert!(is_hit(&m1.stage5().await).is_some());
    // R8: a message scope without a message is unkeyed.
    let slash = run_on(&s, &[MESSAGE1], ids(1, Some(2), Some(3), None));
    assert!(is_hit(&slash.stage5().await).is_none());
    assert!(is_hit(&slash.stage5().await).is_none());
}

/// R10: every limit counts in order; evaluation stops at the first hit.
#[tokio::test]
async fn int_14_limits_in_order() {
    let s = store();
    let both = run_on(&s, &[USER3, GUILD1], ids(1, Some(2), Some(3), None));
    assert!(is_hit(&both.stage5().await).is_none());
    assert_eq!(count(&s, &format!("n/user/1/{NAME}")).await, Some(1));
    assert_eq!(count(&s, &format!("n/guild/2/{NAME}")).await, Some(1));
    let hit = is_hit(&both.stage5().await).expect("guild hit");
    assert_eq!(hit.scope(), RateScope::Guild);
    assert_eq!(count(&s, &format!("n/user/1/{NAME}")).await, Some(2));
    // Guild first: the user limit after the hit is not counted.
    let s = store();
    let both = run_on(&s, &[GUILD1, USER3], ids(1, Some(2), Some(3), None));
    assert!(is_hit(&both.stage5().await).is_none());
    assert!(is_hit(&both.stage5().await).is_some());
    assert_eq!(count(&s, &format!("n/user/1/{NAME}")).await, Some(1));
}

/// R11, R12: a manual limit counts only `hit`.
#[tokio::test]
async fn int_14_manual_limit() {
    let s = store();
    let run = Arc::new(run_on(&s, &[MANUAL1], ids(1, Some(2), Some(3), None)));
    for _ in 0..5 {
        assert!(is_hit(&run.stage5().await).is_none());
    }
    let limits = RateLimits::of(Some(&run));
    assert_eq!(limits.hit(RateScope::User).await.unwrap(), 1);
    let hit = is_hit(&run.stage5().await).expect("charged");
    near(hit.retry_after(), Duration::from_mins(1));
}

/// R13: `block` refuses for its duration and keeps a later live end.
#[tokio::test]
async fn int_14_block() {
    let s = store();
    let run = Arc::new(run_on(&s, &[USER3], ids(1, None, Some(3), None)));
    let limits = RateLimits::of(Some(&run));
    limits
        .block(RateScope::User, Duration::from_secs(30))
        .await
        .unwrap();
    let hit = is_hit(&run.stage5().await).expect("blocked");
    near(hit.retry_after(), Duration::from_secs(30));
    limits
        .block(RateScope::User, Duration::from_secs(5))
        .await
        .unwrap();
    let hit = is_hit(&run.stage5().await).expect("still blocked");
    near(hit.retry_after(), Duration::from_secs(30));
    // A manual limit is blocked too.
    let manual = Arc::new(run_on(&s, &[MANUAL1], ids(9, None, Some(3), None)));
    RateLimits::of(Some(&manual))
        .block(RateScope::User, Duration::from_secs(30))
        .await
        .unwrap();
    assert!(is_hit(&manual.stage5().await).is_some());
}

/// R14, R15, R16: `extend` adds to the live end; zero writes nothing; a
/// day is the most.
#[tokio::test]
async fn int_14_extend_zero_and_clamp() {
    let s = store();
    let run = Arc::new(run_on(&s, &[USER3], ids(1, None, Some(3), None)));
    let limits = RateLimits::of(Some(&run));
    limits
        .extend(RateScope::User, Duration::from_secs(10))
        .await
        .unwrap();
    limits
        .extend(RateScope::User, Duration::from_secs(10))
        .await
        .unwrap();
    let hit = is_hit(&run.stage5().await).expect("extended");
    near(hit.retry_after(), Duration::from_secs(20));

    let fresh = Arc::new(run_on(&s, &[USER3], ids(2, None, Some(3), None)));
    let limits = RateLimits::of(Some(&fresh));
    limits.block(RateScope::User, Duration::ZERO).await.unwrap();
    limits
        .extend(RateScope::User, Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(s.get(&format!("n/user/2/{NAME}")).await.unwrap(), None);
    assert_eq!(s.get(&format!("e/user/2/{NAME}")).await.unwrap(), None);

    limits
        .block(RateScope::User, Duration::from_hours(48))
        .await
        .unwrap();
    let hit = is_hit(&fresh.stage5().await).expect("blocked a day");
    near(hit.retry_after(), MAX_WINDOW);
    limits
        .extend(RateScope::User, Duration::from_hours(1))
        .await
        .unwrap();
    let hit = is_hit(&fresh.stage5().await).expect("still a day");
    near(hit.retry_after(), MAX_WINDOW);
}

/// R17: `reset` clears the count.
#[tokio::test]
async fn int_14_reset() {
    let s = store();
    let run = Arc::new(run_on(&s, &[USER1_1S], ids(1, None, Some(3), None)));
    assert!(is_hit(&run.stage5().await).is_none());
    assert!(is_hit(&run.stage5().await).is_some());
    RateLimits::of(Some(&run))
        .reset(RateScope::User)
        .await
        .unwrap();
    assert!(is_hit(&run.stage5().await).is_none());
}

/// R18, R19, R20: undeclared, unkeyed and detached calls write nothing.
#[tokio::test]
async fn int_14_dynamic_errors() {
    let s = store();
    let run = Arc::new(run_on(&s, &[USER3, CHANNEL1], ids(1, None, None, None)));
    let limits = RateLimits::of(Some(&run));
    assert!(matches!(
        limits.hit(RateScope::Guild).await,
        Err(RateLimitError::Undeclared {
            scope: RateScope::Guild
        })
    ));
    assert!(matches!(
        limits
            .block(RateScope::Member, Duration::from_secs(1))
            .await,
        Err(RateLimitError::Undeclared { .. })
    ));
    assert!(matches!(
        limits
            .extend(RateScope::Channel, Duration::from_secs(1))
            .await,
        Err(RateLimitError::Unkeyed {
            scope: RateScope::Channel
        })
    ));
    assert!(matches!(
        limits.reset(RateScope::Message).await,
        Err(RateLimitError::Undeclared { .. })
    ));
    let detached = RateLimits::of(None);
    assert!(matches!(
        detached.hit(RateScope::User).await,
        Err(RateLimitError::Detached)
    ));
    assert!(matches!(
        detached.reset(RateScope::User).await,
        Err(RateLimitError::Detached)
    ));
    assert_eq!(
        RateLimitError::Unkeyed {
            scope: RateScope::Channel
        }
        .to_string(),
        "this interaction has no `channel` rate-limit key"
    );
}

/// A store whose every op fails.
struct Failing;

impl RateLimitStore for Failing {
    fn incr_with_ttl<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        Box::pin(async { Err(PortError::unavailable("down")) })
    }

    fn get<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        Box::pin(async { Err(PortError::unavailable("down")) })
    }

    fn put<'a>(
        &'a self,
        _: &'a str,
        _: Bytes,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async { Err(PortError::unavailable("down")) })
    }
}

/// R21: a store error fails open with one `warn` (no ids); a dynamic call
/// returns it.
#[tokio::test]
async fn int_14_store_error_fails_open() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let failing: Arc<dyn RateLimitStore> = Arc::new(Failing);
    let run = Arc::new(run_on(
        &failing,
        &[USER3, MANUAL1],
        ids(424_242, None, Some(3), None),
    ));
    assert!(is_hit(&run.stage5().await).is_none());
    let lines = capture.lines_with(&["WARN", "noctis::rate_limit", "limit not applied"]);
    assert_eq!(lines.len(), 2, "{}", capture.text());
    assert!(!capture.text().contains("424242"), "{}", capture.text());
    assert!(matches!(
        RateLimits::of(Some(&run)).hit(RateScope::User).await,
        Err(RateLimitError::Store(_))
    ));
}

/// R22: a counter key holding another value.
#[tokio::test]
async fn int_14_malformed_counter() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let s = store();
    s.put(&format!("n/user/1/{NAME}"), Bytes::from_static(b"x"), None)
        .await
        .unwrap();
    let auto = run_on(&s, &[USER3], ids(1, None, Some(3), None));
    assert!(is_hit(&auto.stage5().await).is_none());
    assert_eq!(
        capture
            .lines_with(&["WARN", "noctis::rate_limit", "limit not applied"])
            .len(),
        1,
        "{}",
        capture.text()
    );
    let manual = run_on(&s, &[MANUAL1], ids(1, None, Some(3), None));
    assert!(is_hit(&manual.stage5().await).is_none());
    assert_eq!(
        capture
            .lines_with(&["WARN", "noctis::rate_limit", "malformed rate-limit counter"])
            .len(),
        1,
        "{}",
        capture.text()
    );
}

/// R23: a hit without a window-end record retries after a whole window.
#[tokio::test]
async fn int_14_missing_window_end() {
    let s = store();
    s.put(
        &format!("n/user/1/{NAME}"),
        Bytes::copy_from_slice(&3_u64.to_be_bytes()),
        Some(Duration::from_secs(5)),
    )
    .await
    .unwrap();
    let run = run_on(&s, &[USER3], ids(1, None, Some(3), None));
    let hit = is_hit(&run.stage5().await).expect("hit");
    assert_eq!(hit.retry_after(), Duration::from_secs(10));
}

/// R30: one store under two Apps: the same id shares counters, another id
/// does not.
#[tokio::test]
async fn int_14_cluster_sharing() {
    let inner = store();
    let ns = |app: &str| -> Arc<dyn RateLimitStore> {
        Arc::new(Namespaced::new(
            &AppId::new(app).unwrap(),
            Arc::clone(&inner),
        ))
    };
    let (a1, a2, b) = (ns("bot-a"), ns("bot-a"), ns("bot-b"));
    let ids = ids(1, Some(2), Some(3), None);
    let first = run_on(&a1, &[GUILD1], ids);
    let second = run_on(&a2, &[GUILD1], ids);
    let other = run_on(&b, &[GUILD1], ids);
    assert!(is_hit(&first.stage5().await).is_none());
    assert!(is_hit(&second.stage5().await).is_some());
    assert!(is_hit(&other.stage5().await).is_none());
    assert!(
        inner
            .get(&format!("bot-a:rate_limit:n/guild/2/{NAME}"))
            .await
            .unwrap()
            .is_some()
    );
}

#[test]
fn int_14_hit_rounds_up() {
    let hit = RateLimitHit::__new(USER3, Duration::from_millis(1), 1_000);
    assert_eq!(hit.retry_at_unix_secs(), 2);
    let hit = RateLimitHit::__new(USER3, Duration::from_secs(1), 1_000);
    assert_eq!(hit.retry_at_unix_secs(), 2);
    assert_eq!(RateScope::Message.to_string(), "message");
    assert_eq!(GUILD1.window(), Duration::from_secs(10));
    assert!(MANUAL1.is_manual());
}
