//! The per-interaction allocation budget (Story 14.3, NFR-9, AD-25): a dhat
//! counting allocator measures the heap blocks and bytes one pre-parsed
//! interaction `Inbound` costs from `InboundSink::push` on the App's lane
//! sender to its handler's start, and a counting `RefStore` counts the
//! Payload port calls in the same window. Cases:
//!
//! - `component_inline`: a click on a Button with an inline `u64` Payload;
//!   no `RefStore` op;
//! - `component_by_ref`: a click on a `by_ref` Button (its Ref minted by a
//!   `/mint` reply beforehand); exactly one `RefStore` op, a `get`;
//! - `command`: a slash command with an integer option; no `RefStore` op;
//! - `component_inline_big` / `command_big`: twins of `component_inline` /
//!   `command` whose handler keeps a `[u8; BIG]` alive across an `.await`.
//!   Each must cost the same blocks as its small twin and between `BIG` and
//!   `2 · BIG` more bytes: the handler future is allocated once, inside a
//!   block that exists anyway (a second boxing or a by-value copy of the
//!   future into another allocation would add another `BIG`).
//!
//! Each handler's first statement snapshots dhat's totals and the
//! `RefStore` op counts, so a window ends exactly at handler start; the
//! handler then acks (`defer_update` / `defer_reply`), and the next window
//! starts only after that ack has returned. The `Inbound` is built before
//! each window; each case runs 3 warm-up windows, then reports the smallest
//! cost of 7. The op counts must hold in every window, warm-ups included.
//!
//! Each block count must equal its recorded constant in [`RECORDED`]: a
//! change that adds an allocation fails here; one that removes one lowers
//! the constant in the same commit.
//!
//! dhat counts the whole process and allows one profiler per process, so
//! this binary has no test harness (`harness = false`) and runs one App on
//! a one-worker runtime, without a tracing subscriber, a metrics recorder
//! or the Discord cache.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a harness-less test: it panics with the failing case and reports on stdout"
)]

use std::hint::black_box;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use noctis::__private::codec::{Parts, encode};
use noctis::DiscordCacheConfig;
use noctis::ports::memory::MemoryRefStore;
use noctis::ports::{BoxFuture, PortError, RefStore};
use noctis::prelude::*;
use noctis::testkit::{TestApp, dhat};
use noctis::ui::{ActionRow, Components, TextDisplay};
use noctis_core::__testing::{Inbound, InboundKind, InboundSink as _, LaneSender, RawJson};
use serde_json::{Value, json};
use twilight_model::application::interaction::Interaction;
use twilight_model::gateway::ShardId;
use twilight_model::guild::Permissions;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// The recorded block count of each case, push to handler start (Story
/// 14.3, measured with the handler future boxed once through a `map`
/// combinator). Each block:
///
/// - `component_inline` (12): the interaction's job task (the worker's
///   spawned, observed pipeline future); the built-in locale cell `Arc` of
///   the pre-stage-4 notices; the attachment file-limit parse of the raw
///   JSON (`responder::file_limit`, one `Vec`); the stage-4 locale cell
///   `Arc`; the `RateRun` `Arc`; the unknown-components parse of the source
///   message (`source_unknowns`, one `Vec`); the placeholder locale cell
///   `Arc` of `CtxInner::detached_parts` (dropped by the struct update);
///   the source-message parse (`SourceMessage::from_interaction`, one
///   `Vec`); the responder state (`Shared::declared`); the context parts
///   `Arc`; the boxed handler future; the auto-defer timer task
///   (`pipeline::arm`).
/// - `component_by_ref` (16): those 12, plus the Ref resolve: the
///   namespaced key `String` (`<app_id>:ref:<ref>`), the `Namespaced`
///   port's boxed `get` future, the `MemoryRefStore`'s boxed `get` future,
///   and the labeled `noctis_ref_lookups_total` counter key
///   (`count_lookup`).
/// - `command` (88): the interaction's job task; the command path `String`
///   of the Route resolve; the attachment file-limit parse; the stage-4
///   locale cell `Arc`; the `RateRun` `Arc`; the placeholder locale cells
///   of `CtxInner::detached_parts` and `rate_limited_parts`; the responder
///   state; the rate-limit hook's context parts `Arc`; the auto-defer timer
///   task; 71 blocks of the `serde_json::Value` tree `CommandInput::from_json`
///   parses the whole raw interaction into, plus 4 of the typed option list
///   (its path `String`, the options array, the leaf option `Vec` and the
///   option name `String`); the boxed start closure (`pipeline::start`);
///   the `CommandCtx` parts `Arc`; the boxed handler future.
///
/// The `_big` twins must equal their small twin (asserted separately).
const RECORDED: [(&str, u64); 3] = [
    ("component_inline", 12),
    ("component_by_ref", 16),
    ("command", 88),
];

/// The array the `_big` twins keep alive across an `.await`.
const BIG: usize = 16_384;
/// Warm-up windows per case.
const WARM_UPS: usize = 3;
/// Measured windows per case (the smallest costs).
const WINDOWS: usize = 7;
/// The bound on one window's waits.
const WAIT: Duration = Duration::from_secs(10);

/// The `RefStore` ops, in [`OPS`] order.
const OP_NAMES: [&str; 5] = ["put_if_absent", "get", "take", "delete", "touch"];
const PUT_IF_ABSENT: usize = 0;
const GET: usize = 1;
const TAKE: usize = 2;
const DELETE: usize = 3;
const TOUCH: usize = 4;

/// `RefStore` calls since the window began, per op.
static OPS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
/// [`OPS`] as the last handler start saw them.
static START_OPS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
/// dhat's totals as the last handler start saw them.
static START_BLOCKS: AtomicU64 = AtomicU64::new(0);
static START_BYTES: AtomicU64 = AtomicU64::new(0);
/// Handler starts so far.
static STARTS: AtomicUsize = AtomicUsize::new(0);
/// Handlers whose ack has returned so far.
static DONE: AtomicUsize = AtomicUsize::new(0);

/// The first statement of every measured handler: snapshots the heap
/// totals and the op counts, then counts the start. Never allocates.
fn started() {
    let stats = dhat::HeapStats::get();
    START_BLOCKS.store(stats.total_blocks, Ordering::Release);
    START_BYTES.store(stats.total_bytes, Ordering::Release);
    for (snapshot, count) in START_OPS.iter().zip(&OPS) {
        snapshot.store(count.load(Ordering::Acquire), Ordering::Release);
    }
    STARTS.fetch_add(1, Ordering::AcqRel);
}

/// Counts the end of a handler's ack.
fn done() {
    DONE.fetch_add(1, Ordering::AcqRel);
}

/// The App's `RefStore`: a `MemoryRefStore` counting every call per op.
struct CountingRefStore(MemoryRefStore);

fn count(op: usize) {
    OPS[op].fetch_add(1, Ordering::AcqRel);
}

impl RefStore for CountingRefStore {
    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        count(PUT_IF_ABSENT);
        self.0.put_if_absent(key, value, ttl)
    }

    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        count(GET);
        self.0.get(key)
    }

    fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        count(TAKE);
        self.0.take(key)
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        count(DELETE);
        self.0.delete(key)
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: Duration) -> BoxFuture<'a, Result<bool, PortError>> {
        count(TOUCH);
        self.0.touch(key, ttl)
    }
}

/// The inline component.
#[derive(Button)]
#[button("tap", defaults(style = Primary, label = "Tap"))]
struct Tap {
    seq: u64,
}

impl Tap {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        started();
        black_box(self.seq);
        let responded = r.defer_update().await?.done();
        done();
        Ok(responded)
    }
}

/// [`Tap`]'s twin holding `[u8; BIG]` across an `.await`.
#[derive(Button)]
#[button("tapbig", defaults(style = Primary, label = "Big"))]
struct TapBig {
    seq: u64,
}

impl TapBig {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        started();
        let buf = [0_u8; BIG];
        tokio::task::yield_now().await;
        black_box(&buf);
        black_box(self.seq);
        let responded = r.defer_update().await?.done();
        done();
        Ok(responded)
    }
}

/// The `by_ref` component.
#[derive(Button)]
#[button("stored", by_ref, defaults(style = Primary, label = "Stored"))]
struct Stored {
    n: u64,
}

impl Stored {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        started();
        black_box(self.n);
        let responded = r.defer_update().await?.done();
        done();
        Ok(responded)
    }
}

/// Replies with a [`Stored`] button, storing its Ref (outside any window).
#[derive(SlashCommand)]
#[command("mint", description = "Mints a Stored button")]
struct Mint;

impl Mint {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
        Ok(Components::new()
            .push(TextDisplay::new("stored".to_owned()))
            .push(ActionRow::buttons([Stored { n: 7 }.build()])))
    }
}

/// The command.
#[derive(SlashCommand)]
#[command("seq", description = "Measured command")]
struct SeqCommand {
    #[option(description = "Item")]
    seq: i64,
}

impl SeqCommand {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        started();
        black_box(self.seq);
        let responded = r.defer_reply().await?.done();
        done();
        Ok(responded)
    }
}

/// [`SeqCommand`]'s twin holding `[u8; BIG]` across an `.await`.
#[derive(SlashCommand)]
#[command("seqbig", description = "Measured command, big future")]
struct SeqBig {
    #[option(description = "Item")]
    seq: i64,
}

impl SeqBig {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        started();
        let buf = [0_u8; BIG];
        tokio::task::yield_now().await;
        black_box(&buf);
        black_box(self.seq);
        let responded = r.defer_reply().await?.done();
        done();
        Ok(responded)
    }
}

/// What one window pushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    ComponentInline,
    ComponentByRef,
    Command,
    ComponentInlineBig,
    CommandBig,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::ComponentInline => "component_inline",
            Self::ComponentByRef => "component_by_ref",
            Self::Command => "command",
            Self::ComponentInlineBig => "component_inline_big",
            Self::CommandBig => "command_big",
        }
    }

    /// The `RefStore` ops one window of this case must make before its
    /// handler starts, per op.
    fn expected_ops(self) -> [usize; 5] {
        let mut ops = [0; 5];
        if self == Self::ComponentByRef {
            ops[GET] = 1;
        }
        ops
    }
}

/// The heap blocks and bytes of one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    blocks: u64,
    bytes: u64,
}

/// The guild interaction base: everything but `id`, `token`, `type`,
/// `data` and `message`.
fn interaction_base() -> Value {
    let user = json!({
        "id": "3",
        "username": "alloc",
        "discriminator": "0",
        "avatar": null,
        "global_name": null,
        "bot": false,
    });
    json!({
        "application_id": "1100000000000000000",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "locale": "en-US",
        "app_permissions": Permissions::all().bits().to_string(),
        "channel": { "id": "2", "type": 0 },
        "channel_id": "2",
        "guild_id": "1",
        "guild_locale": "en-US",
        "member": {
            "user": user,
            "roles": [],
            "permissions": Permissions::all().bits().to_string(),
            "joined_at": "2026-01-01T00:00:00.000000+00:00",
            "deaf": false,
            "mute": false,
            "flags": 0,
            "nick": null,
            "avatar": null,
            "pending": false,
            "premium_since": null,
            "communication_disabled_until": null,
        },
        "context": 0,
    })
}

/// The message a component sits on.
fn message() -> Value {
    json!({
        "id": "10",
        "channel_id": "2",
        "guild_id": "1",
        "author": { "id": "3", "username": "alloc", "discriminator": "0", "avatar": null },
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
        "flags": 0,
    })
}

/// Builds the pre-parsed interactions of every case.
struct Fixtures {
    base: Value,
    /// The minted `Stored` button's `custom_id` (a Ref id).
    by_ref: String,
    /// The next interaction id.
    next: u64,
}

impl Fixtures {
    /// One pre-parsed interaction of `case`, parsed from the same bytes its
    /// `RawJson` carries.
    fn inbound(&mut self, case: Case) -> Inbound {
        self.next += 1;
        let seq = self.next;
        let (kind, data) = match case {
            Case::ComponentInline => (3, button(encode::<Tap>(&Tap { seq }, &Parts::new()))),
            Case::ComponentInlineBig => {
                (3, button(encode::<TapBig>(&TapBig { seq }, &Parts::new())))
            }
            Case::ComponentByRef => (3, json!({ "custom_id": self.by_ref, "component_type": 2 })),
            Case::Command => (2, command("seq", seq)),
            Case::CommandBig => (2, command("seqbig", seq)),
        };
        let mut d = self.base.clone();
        d["id"] = json!((1_000_000_000_000 + seq).to_string());
        d["token"] = json!(format!("alloc-{seq}"));
        d["type"] = json!(kind);
        d["data"] = data;
        if kind == 3 {
            d["message"] = message();
        }
        let bytes = serde_json::to_vec(&d).unwrap();
        let interaction: Interaction = serde_json::from_slice(&bytes).unwrap();
        Inbound {
            shard: ShardId::ONE,
            received_at: tokio::time::Instant::now(),
            kind: InboundKind::Interaction(Box::new(interaction), RawJson::new(Bytes::from(bytes))),
        }
    }
}

/// A button click's `data` for an encoded inline `custom_id`.
fn button<E: std::fmt::Debug>(custom_id: Result<impl std::fmt::Display, E>) -> Value {
    let custom_id = custom_id.expect("the custom_id encodes");
    json!({ "custom_id": custom_id.to_string(), "component_type": 2 })
}

/// A slash command's `data` with its `seq` option.
fn command(name: &str, seq: u64) -> Value {
    json!({
        "id": "900",
        "name": name,
        "type": 1,
        "options": [{ "name": "seq", "type": 4, "value": seq }],
    })
}

/// Waits until `counter` reaches `target`, panicking after [`WAIT`].
async fn wait_for(counter: &AtomicUsize, target: usize, case: Case, what: &str) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while counter.load(Ordering::Acquire) < target {
        assert!(
            tokio::time::Instant::now() < deadline,
            "case {}: {what} within {WAIT:?}",
            case.name()
        );
        tokio::task::yield_now().await;
    }
}

/// One window: pushes a pre-built interaction of `case` and returns the
/// heap cost up to its handler's start; waits for the handler's ack before
/// returning. Asserts the window's `RefStore` ops.
async fn window(sender: &LaneSender, fixtures: &mut Fixtures, case: Case) -> Cost {
    let item = fixtures.inbound(case);
    let start_target = STARTS.load(Ordering::Acquire) + 1;
    let done_target = DONE.load(Ordering::Acquire) + 1;
    for op in &OPS {
        op.store(0, Ordering::Release);
    }
    let before = dhat::HeapStats::get();
    sender.push(item);
    wait_for(&STARTS, start_target, case, "no handler started").await;
    let cost = Cost {
        blocks: START_BLOCKS.load(Ordering::Acquire) - before.total_blocks,
        bytes: START_BYTES.load(Ordering::Acquire) - before.total_bytes,
    };
    let ops: [usize; 5] = std::array::from_fn(|op| START_OPS[op].load(Ordering::Acquire));
    let expected = case.expected_ops();
    assert_eq!(
        ops,
        expected,
        "case {}: RefStore ops before handler start ({OP_NAMES:?}): expected {expected:?}, \
         measured {ops:?} (AD-25: at most one Payload port call)",
        case.name()
    );
    wait_for(&DONE, done_target, case, "the handler's ack did not return").await;
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    cost
}

/// The smallest cost of [`WINDOWS`] windows after [`WARM_UPS`].
async fn quiet(sender: &LaneSender, fixtures: &mut Fixtures, case: Case) -> Cost {
    for _ in 0..WARM_UPS {
        window(sender, fixtures, case).await;
    }
    let mut best = window(sender, fixtures, case).await;
    for _ in 1..WINDOWS {
        best = best.min(window(sender, fixtures, case).await);
    }
    best
}

/// Measures every case on one App.
async fn measure() -> Vec<(Case, Cost)> {
    let mut builder = App::builder();
    builder
        .discord_cache(DiscordCacheConfig::none())
        .ref_store(CountingRefStore(MemoryRefStore::new()))
        .module(
            Module::new("alloc")
                .add::<Tap>()
                .add::<TapBig>()
                .add::<Stored>()
                .add::<Mint>()
                .add::<SeqCommand>()
                .add::<SeqBig>(),
        );
    let app = TestApp::build(&mut builder).await.unwrap();
    let sender = app.lane_sender().expect("the run's lane sender");
    let minted = app.command("mint").run().await.unwrap();
    let by_ref = minted
        .button("Stored")
        .unwrap()
        .custom_id()
        .expect("the Stored button has a custom_id")
        .to_owned();
    assert_eq!(
        OPS[PUT_IF_ABSENT].load(Ordering::Acquire),
        1,
        "the /mint reply stores one Ref"
    );
    let mut fixtures = Fixtures {
        base: interaction_base(),
        by_ref,
        next: 0,
    };
    let mut measured = Vec::new();
    for case in [
        Case::ComponentInline,
        Case::ComponentByRef,
        Case::Command,
        Case::ComponentInlineBig,
        Case::CommandBig,
    ] {
        measured.push((case, quiet(&sender, &mut fixtures, case).await));
    }
    drop(sender);
    app.stop().await.unwrap();
    measured
}

/// The cost of `case` in `measured`.
fn cost_of(measured: &[(Case, Cost)], case: Case) -> Cost {
    measured
        .iter()
        .find_map(|(measured_case, cost)| (*measured_case == case).then_some(*cost))
        .expect("every case is measured")
}

/// Asserts that `big` costs as many blocks as `small` and `BIG..2·BIG` more
/// bytes: its handler future, `BIG` bytes larger, is allocated once.
fn assert_one_future(measured: &[(Case, Cost)], small: Case, big: Case) {
    let small_cost = cost_of(measured, small);
    let big_cost = cost_of(measured, big);
    assert_eq!(
        big_cost.blocks,
        small_cost.blocks,
        "case {}: {} blocks, {} has {} (AD-25: one boxed handler future per interaction)",
        big.name(),
        big_cost.blocks,
        small.name(),
        small_cost.blocks
    );
    let extra = big_cost.bytes.saturating_sub(small_cost.bytes);
    let big_bytes = u64::try_from(BIG).unwrap();
    assert!(
        big_cost.bytes >= small_cost.bytes && (big_bytes..2 * big_bytes).contains(&extra),
        "case {}: {} bytes, {} has {}: {extra} more, expected {BIG}..{} (AD-25: the handler \
         future is allocated once)",
        big.name(),
        big_cost.bytes,
        small.name(),
        small_cost.bytes,
        2 * BIG
    );
}

fn main() {
    let _profiler = dhat::Profiler::builder()
        .testing()
        .file_name(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/dhat-interaction-alloc.json"
        ))
        .build();
    let probe = black_box(Box::new([0_u8; 64]));
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

    for (case, cost) in &measured {
        let ops = case.expected_ops().iter().sum::<usize>();
        println!(
            "case {}: blocks {}, bytes {}, ref_store ops {ops}",
            case.name(),
            cost.blocks,
            cost.bytes
        );
    }
    for (name, recorded) in RECORDED {
        let blocks = measured
            .iter()
            .find_map(|(case, cost)| (case.name() == name).then_some(cost.blocks))
            .expect("every recorded case is measured");
        assert_eq!(
            blocks, recorded,
            "case {name}: recorded {recorded}, measured {blocks} (a removed allocation \
             updates RECORDED in the same commit)"
        );
    }
    assert_one_future(&measured, Case::ComponentInline, Case::ComponentInlineBig);
    assert_one_future(&measured, Case::Command, Case::CommandBig);
}
