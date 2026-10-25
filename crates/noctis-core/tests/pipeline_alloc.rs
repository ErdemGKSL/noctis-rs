//! Allocation counts of the event pipeline (Story 16.1, Epic 16): a dhat
//! counting allocator measures the heap blocks and bytes one pre-parsed
//! `Inbound` costs from `InboundSink::push` on the App's lane sender to its
//! handlers' starts, for four cases:
//!
//! - `unhandled`: a `MESSAGE_CREATE` no handler takes (the window of a
//!   `MESSAGE_CREATE` followed by a `MESSAGE_DELETE`, minus the window of a
//!   `MESSAGE_DELETE` alone);
//! - `one_concurrent`: a `MESSAGE_DELETE` with one concurrent no-op handler;
//! - `two_concurrent`: a `MESSAGE_REACTION_REMOVE_ALL` with two;
//! - `ordered`: a `CHANNEL_PINS_UPDATE` with one ordered handler.
//!
//! Each block count must equal its recorded constant in [`RECORDED`]. A
//! change that removes an allocation lowers the constant in the same commit
//! (Stories 16.2 and 16.3); one that adds an allocation fails here.
//!
//! dhat counts the whole process and allows one profiler per process, so
//! this binary has no test harness (`harness = false`) and runs one App on
//! a one-worker runtime, without a tracing subscriber, a metrics recorder
//! or the Discord cache. The `Inbound` is built before each window; each
//! case runs 3 warm-up windows, then reports the smallest of 7.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a harness-less test: it panics with the failing case and reports on stdout"
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use noctis::DiscordCacheConfig;
use noctis::events::{ChannelPinsUpdate, EventCtx, MessageDelete, ReactionRemoveAll};
use noctis::prelude::*;
use noctis::testkit::{TestApp, dhat};
use noctis_core::__testing::{Inbound, InboundKind, InboundSink as _, LaneSender};
use serde_json::json;
use twilight_model::gateway::ShardId;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming;
use twilight_model::id::Id;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// The recorded block count of each case (Story 16.1 baseline; Story 16.2
/// raised `ordered` from 4 to 5: an ordered event of an idle guild spawns
/// that guild's on-demand runner task instead of reusing one of 64
/// long-lived sub-queue tasks).
const RECORDED: [(&str, u64); 4] = [
    ("unhandled", 0),
    ("one_concurrent", 7),
    ("two_concurrent", 12),
    ("ordered", 5),
];

/// Warm-up windows per case.
const WARM_UPS: usize = 3;
/// Measured windows per case (the smallest counts).
const WINDOWS: usize = 7;
/// The bound on one window's handler wait.
const WAIT: Duration = Duration::from_secs(10);

/// Handler starts so far; incrementing it never allocates.
static STARTS: AtomicUsize = AtomicUsize::new(0);

#[derive(EventHandler)]
struct OnDelete;

impl OnDelete {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _ev: MessageDelete) -> HandlerResult<()> {
        STARTS.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(EventHandler)]
struct OnClearA;

impl OnClearA {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _ev: ReactionRemoveAll) -> HandlerResult<()> {
        STARTS.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(EventHandler)]
struct OnClearB;

impl OnClearB {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _ev: ReactionRemoveAll) -> HandlerResult<()> {
        STARTS.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(EventHandler)]
#[event(ordered)]
struct OnPins;

impl OnPins {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _ev: ChannelPinsUpdate) -> HandlerResult<()> {
        STARTS.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

/// The heap blocks and bytes of one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    blocks: u64,
    bytes: u64,
}

/// One pre-parsed event of `kind`.
fn inbound(kind: &str) -> Inbound {
    let event = match kind {
        "create" => {
            let message = serde_json::from_value(json!({
                "id": "10",
                "channel_id": "2",
                "guild_id": "1",
                "author": { "id": "3", "username": "a", "discriminator": "0", "avatar": null },
                "content": "",
                "timestamp": "2026-01-01T00:00:00.000000+00:00",
                "edited_timestamp": null,
                "tts": false,
                "mention_everyone": false,
                "mentions": [],
                "mention_roles": [],
                "attachments": [],
                "embeds": [],
                "pinned": false,
                "type": 0,
            }))
            .unwrap();
            Event::MessageCreate(Box::new(incoming::MessageCreate(message)))
        }
        "delete" => Event::MessageDelete(MessageDelete {
            id: Id::new(10),
            channel_id: Id::new(2),
            guild_id: Some(Id::new(1)),
        }),
        "clear" => Event::ReactionRemoveAll(ReactionRemoveAll {
            channel_id: Id::new(2),
            guild_id: Some(Id::new(1)),
            message_id: Id::new(10),
        }),
        "pins" => Event::ChannelPinsUpdate(ChannelPinsUpdate {
            channel_id: Id::new(2),
            guild_id: Some(Id::new(1)),
            last_pin_timestamp: None,
        }),
        other => panic!("unknown fixture {other}"),
    };
    Inbound {
        shard: ShardId::ONE,
        received_at: tokio::time::Instant::now(),
        kind: InboundKind::Event(Box::new(event)),
    }
}

/// Pushes `kinds` (built before the window) and waits for `starts` handler
/// starts; the heap cost in between.
async fn window(sender: &LaneSender, kinds: &[&str], starts: usize) -> Cost {
    let items: Vec<Inbound> = kinds.iter().map(|kind| inbound(kind)).collect();
    let target = STARTS.load(Ordering::Acquire) + starts;
    let before = dhat::HeapStats::get();
    for item in items {
        sender.push(item);
    }
    let deadline = tokio::time::Instant::now() + WAIT;
    while STARTS.load(Ordering::Acquire) < target {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no handler started for {kinds:?}"
        );
        tokio::task::yield_now().await;
    }
    tokio::task::yield_now().await;
    let after = dhat::HeapStats::get();
    Cost {
        blocks: after.total_blocks - before.total_blocks,
        bytes: after.total_bytes - before.total_bytes,
    }
}

/// The smallest cost of [`WINDOWS`] windows after [`WARM_UPS`].
async fn quiet(sender: &LaneSender, kinds: &[&str], starts: usize) -> Cost {
    for _ in 0..WARM_UPS {
        window(sender, kinds, starts).await;
    }
    let mut best = window(sender, kinds, starts).await;
    for _ in 1..WINDOWS {
        best = best.min(window(sender, kinds, starts).await);
    }
    best
}

/// Measures every case on one App.
async fn measure() -> Vec<(&'static str, Cost)> {
    let mut builder = App::builder();
    builder.discord_cache(DiscordCacheConfig::none()).module(
        Module::new("alloc")
            .add::<OnDelete>()
            .add::<OnClearA>()
            .add::<OnClearB>()
            .add::<OnPins>(),
    );
    let app = TestApp::build(&mut builder).await.unwrap();
    let sender = app.lane_sender().expect("the run's lane sender");

    let delete = quiet(&sender, &["delete"], 1).await;
    let both = quiet(&sender, &["create", "delete"], 1).await;
    let unhandled = Cost {
        blocks: both.blocks.saturating_sub(delete.blocks),
        bytes: both.bytes.saturating_sub(delete.bytes),
    };
    let two = quiet(&sender, &["clear"], 2).await;
    let ordered = quiet(&sender, &["pins"], 1).await;

    drop(sender);
    app.stop().await.unwrap();
    vec![
        ("unhandled", unhandled),
        ("one_concurrent", delete),
        ("two_concurrent", two),
        ("ordered", ordered),
    ]
}

fn main() {
    let _profiler = dhat::Profiler::builder()
        .testing()
        .file_name(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/dhat-pipeline-alloc.json"
        ))
        .build();
    let probe = std::hint::black_box(Box::new([0_u8; 64]));
    let tracked = dhat::HeapStats::get().total_blocks;
    drop(probe);
    assert!(tracked > 0, "dhat counts no allocations");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    // Spawned, so the measured task runs on the one worker with the App.
    let measured = runtime.block_on(async { tokio::spawn(measure()).await.unwrap() });
    drop(runtime);

    for (name, cost) in &measured {
        println!("case {name}: blocks {}, bytes {}", cost.blocks, cost.bytes);
    }
    for (name, recorded) in RECORDED {
        let measured = measured
            .iter()
            .find_map(|(case, cost)| (*case == name).then_some(cost.blocks))
            .expect("every recorded case is measured");
        assert_eq!(
            measured, recorded,
            "case {name}: recorded {recorded}, measured {measured} (a removed allocation \
             updates RECORDED in the same commit)"
        );
    }
}
