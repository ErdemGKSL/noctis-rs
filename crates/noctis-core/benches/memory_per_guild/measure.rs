//! The bench's App runs: one App against `FakeGateway` per run, live heap
//! read through dhat before and after loading idle guilds (NFR-1, AD-28).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use noctis::DiscordCacheConfig;
use noctis::events::{EventCtx, GuildCreate};
use noctis::ports::memory::MemoryRenderCache;
use noctis::prelude::*;
use noctis::testkit::{TestApp, TestError, dhat};
use serde_json::{Value, json};

use crate::budget::Measurement;

/// Guilds dispatched before waiting for their handlers.
const BATCH: u32 = 250;
/// How long one batch may take to be handled.
const BATCH_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the heap settles before it is read, and after a run.
const SETTLE: Duration = Duration::from_millis(300);
/// The live-heap reads of one settled reading (their median counts).
const READS: usize = 5;
/// The pause between two reads.
const READ_GAP: Duration = Duration::from_millis(50);
/// The warm-up run's guild count.
const WARM_UP_GUILDS: u32 = 100;
/// The `preferred_locale` cycle of the fixture guilds.
const LOCALES: [&str; 4] = ["en-US", "tr", "de", "ja"];

/// Why a measurement failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MeasureError {
    /// The process does not use dhat's global allocator.
    #[error(
        "dhat counts no allocations: install `noctis::testkit::dhat::Alloc` as the global allocator"
    )]
    AllocatorMissing,
    /// A step did not finish in time.
    #[error("timed out waiting for {what}")]
    Timeout { what: &'static str },
    /// A dispatch reached no shard session.
    #[error("a GUILD_CREATE dispatch reached no shard session")]
    NoSession,
    /// The test App failed.
    #[error(transparent)]
    Test(#[from] TestError),
}

/// Starts dhat's (testing-mode) profiler; keep it alive until every read is
/// done (one profiler per process).
///
/// # Errors
///
/// [`MeasureError::AllocatorMissing`] without dhat's global allocator.
pub(crate) fn start_profiler() -> Result<dhat::Profiler, MeasureError> {
    let profiler = dhat::Profiler::builder().testing().build();
    let probe = std::hint::black_box(Box::new([0_u8; 64]));
    let tracked = dhat::HeapStats::get().total_blocks;
    drop(probe);
    if tracked == 0 {
        return Err(MeasureError::AllocatorMissing);
    }
    Ok(profiler)
}

/// Runs the four Apps (warm-up, empty baseline, `guilds` guilds without and
/// with the default Discord cache) and returns their deltas.
///
/// # Errors
///
/// The first run's [`MeasureError`].
pub(crate) async fn measure(guilds: u32) -> Result<Measurement, MeasureError> {
    run(WARM_UP_GUILDS, DiscordCacheConfig::default()).await?;
    let (baseline, _) = run(0, DiscordCacheConfig::none()).await?;
    let (none, _) = run(guilds, DiscordCacheConfig::none()).await?;
    let (default, render_cache_bytes) = run(guilds, DiscordCacheConfig::default()).await?;
    Ok(Measurement {
        guilds,
        baseline,
        none,
        default,
        render_cache_bytes,
    })
}

/// Counts the handled `GUILD_CREATE`s (after the locale record and the
/// Discord cache saw them).
#[derive(EventHandler)]
struct CountGuilds;

impl CountGuilds {
    #[handler]
    async fn handle(self, ctx: EventCtx, _ev: GuildCreate) -> HandlerResult<()> {
        if let Some(counter) = ctx.state::<Arc<AtomicUsize>>() {
            counter.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }
}

/// One App run: the live-heap delta of loading `guilds` guilds under
/// `cache`, and the Renderer cache bytes.
async fn run(guilds: u32, cache: DiscordCacheConfig) -> Result<(i64, u64), MeasureError> {
    let render = Arc::new(MemoryRenderCache::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let before = settled_bytes().await;
    let mut builder = App::builder();
    builder
        .state(Arc::clone(&counter))
        .render_cache(Arc::clone(&render))
        .discord_cache(cache)
        .module(Module::new("bench").add::<CountGuilds>());
    let app = TestApp::build(&mut builder).await?;
    let measured = match load_guilds(&app, guilds, &counter).await {
        Ok(()) => {
            let after = settled_bytes().await;
            Ok((after.saturating_sub(before), render.usage().bytes))
        }
        Err(error) => Err(error),
    };
    let stopped = app.stop().await;
    drop(builder);
    drop(render);
    drop(counter);
    tokio::time::sleep(SETTLE).await;
    let measured = measured?;
    stopped?;
    Ok(measured)
}

/// Dispatches `guilds` fixture guilds in batches, waiting after each until
/// its handlers ran.
async fn load_guilds(
    app: &TestApp,
    guilds: u32,
    counter: &AtomicUsize,
) -> Result<(), MeasureError> {
    let mut start = 0;
    while start < guilds {
        let end = start.saturating_add(BATCH).min(guilds);
        for index in start..end {
            if app.gateway_fake().dispatch("GUILD_CREATE", guild(index)) != 1 {
                return Err(MeasureError::NoSession);
            }
        }
        let target = usize::try_from(end).unwrap_or(usize::MAX);
        let waited = tokio::time::timeout(BATCH_TIMEOUT, async {
            while counter.load(Ordering::Acquire) < target {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        if waited.is_err() {
            return Err(MeasureError::Timeout {
                what: "a batch of GUILD_CREATE events to be handled",
            });
        }
        start = end;
    }
    Ok(())
}

/// The live heap bytes after settling: the median of [`READS`] reads.
async fn settled_bytes() -> i64 {
    tokio::time::sleep(SETTLE).await;
    let mut reads = [0_i64; READS];
    for (index, read) in reads.iter_mut().enumerate() {
        if index > 0 {
            tokio::time::sleep(READ_GAP).await;
        }
        *read = live_bytes();
    }
    reads.sort_unstable();
    reads[READS / 2]
}

/// The live heap bytes dhat counts now.
fn live_bytes() -> i64 {
    i64::try_from(dhat::HeapStats::get().curr_bytes).unwrap_or(i64::MAX)
}

/// The `GUILD_CREATE` payload of fixture guild `index`: two roles and three
/// channels, every list of members, presences and the like empty.
fn guild(index: u32) -> Value {
    let id = 1_000_000 + u64::from(index);
    let channel = |offset: u64, kind: u8, name: &str| {
        json!({
            "id": (id * 10 + offset).to_string(),
            "guild_id": id.to_string(),
            "type": kind,
            "name": name,
            "position": 0,
            "permission_overwrites": [],
        })
    };
    let role = |role_id: u64, name: &str, position: u64| {
        json!({
            "id": role_id.to_string(),
            "name": name,
            "color": 0,
            "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null},
            "hoist": false,
            "managed": false,
            "mentionable": false,
            "permissions": "0",
            "position": position,
            "flags": 0,
        })
    };
    let locale = LOCALES[usize::try_from(index).unwrap_or(0) % LOCALES.len()];
    json!({
        "id": id.to_string(),
        "name": format!("guild-{index}"),
        "owner_id": "1",
        "preferred_locale": locale,
        "afk_timeout": 300,
        "default_message_notifications": 0,
        "explicit_content_filter": 0,
        "features": [],
        "mfa_level": 0,
        "nsfw_level": 0,
        "premium_progress_bar_enabled": false,
        "premium_tier": 0,
        "system_channel_flags": 0,
        "verification_level": 0,
        "emojis": [],
        "stickers": [],
        "channels": [
            channel(2, 4, "Text"),
            channel(3, 0, "general"),
            channel(4, 2, "Voice"),
        ],
        "roles": [role(id, "@everyone", 0), role(id * 10 + 1, "member", 1)],
        "threads": [],
        "members": [],
        "voice_states": [],
        "presences": [],
        "stage_instances": [],
        "guild_scheduled_events": [],
    })
}
