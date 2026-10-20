//! Story 5.6 Matrix 6, 25-38 and 40: the worker set.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use noctis_gateway::{InboundSink, LaneConfig, LaneSender, RawJson};
use serde_json::json;
use tokio::sync::{Notify, Semaphore};
use tokio::time::Instant;
use twilight_model::gateway::ShardId;
use twilight_model::gateway::payload::incoming::{
    GuildDelete, MessageCreate, MessageDelete, UnavailableGuild, UserUpdate,
};

use super::*;
use crate::test_support::{Capture, handler, message_delete, noop};
use crate::{App, AppBuilder, EventHandler, HandlerError, Module, Runtime};

/// Guild ids in different sub-queues.
const GA: u64 = 1;
const GB: u64 = 2;

/// The bound on every wait.
const WAIT: Duration = Duration::from_secs(10);

/// A shared, ordered log of handler steps.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn push(&self, line: impl Into<String>) {
        self.0.lock().unwrap().push(line.into());
    }

    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn position(&self, line: &str) -> usize {
        self.lines()
            .iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("{line} missing from {:?}", self.lines()))
    }

    fn count(&self, line: &str) -> usize {
        self.lines().iter().filter(|l| *l == line).count()
    }
}

/// A running drain over a built App.
struct Harness {
    app: App,
    sender: Option<LaneSender>,
    drain: JoinHandle<()>,
}

impl Harness {
    fn start(builder: &mut AppBuilder) -> Self {
        builder.runtime(&Runtime::for_test());
        let app = builder.build().unwrap();
        let (sender, receiver) = noctis_gateway::lanes(app.lane_config());
        let drain = tokio::spawn(drain(receiver, app.workers()));
        Self {
            app,
            sender: Some(sender),
            drain,
        }
    }

    fn push_on(&self, shard: ShardId, kind: InboundKind) {
        self.sender.as_ref().unwrap().push(Inbound {
            shard,
            received_at: Instant::now(),
            kind,
        });
    }

    fn push(&self, event: Event) {
        self.push_on(ShardId::ONE, InboundKind::Event(Box::new(event)));
    }

    fn push_interaction(&self, id: u64) {
        let d = json!({
            "id": id.to_string(),
            "application_id": "1",
            "type": 1,
            "token": "t",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
        });
        let raw = RawJson::new(bytes::Bytes::from(d.to_string()));
        self.push_on(
            ShardId::ONE,
            InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw),
        );
    }

    /// Drops the sender and waits for the drain to return.
    async fn close(mut self) {
        drop(self.sender.take());
        tokio::time::timeout(WAIT, self.drain)
            .await
            .unwrap()
            .unwrap();
    }
}

/// Polls `cond` until it holds, within [`WAIT`].
async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// A `USER_UPDATE`.
fn user_update(n: u64) -> Event {
    let user = serde_json::from_value(json!({
        "id": n.to_string(),
        "username": "u",
        "discriminator": "0001",
        "avatar": null,
        "bot": false,
        "mfa_enabled": false,
    }))
    .unwrap();
    Event::UserUpdate(UserUpdate(user))
}

/// A `MESSAGE_CREATE` in guild `guild`.
fn message_create(guild: u64) -> Event {
    let message = serde_json::from_value(json!({
        "id": "10",
        "channel_id": "2",
        "guild_id": guild.to_string(),
        "author": { "id": "3", "username": "a", "discriminator": "0001", "avatar": null },
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
    Event::MessageCreate(Box::new(MessageCreate(message)))
}

/// A `MessageDelete` handler that logs `{name}:{id}`.
fn logging(name: &'static str, ordered: bool, log: &Log) -> EventHandler {
    let log = log.clone();
    handler(name, ordered, move |_, e: MessageDelete| {
        let log = log.clone();
        async move {
            log.push(format!("{name}:{}", e.id));
            Ok(())
        }
    })
}

/// Matrix 38: the sub-queue hash.
#[test]
fn subqueue_hash() {
    assert_eq!(subqueue(None), 0);
    assert_eq!(subqueue(Some(Id::new(42))), subqueue(Some(Id::new(42))));
    for id in [1, u64::MAX] {
        assert!(subqueue(Some(Id::new(id))) < SUBQUEUES);
    }
    assert!((0_u64.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) < 64);
    assert_ne!(subqueue(Some(Id::new(GA))), subqueue(Some(Id::new(GB))));
}

/// Matrix 25 and 6: every concurrent handler runs once with the payload;
/// the same descriptor twice runs twice.
#[tokio::test]
async fn concurrent_handlers_both_run() {
    let log = Log::default();
    let twice = logging("h2", false, &log);
    let h = Harness::start(
        App::builder().module(
            Module::new("m")
                .add(logging("h1", false, &log))
                .add(twice.clone())
                .add(twice),
        ),
    );
    h.push(message_delete(7, Some(GA)));
    until("both handlers", || log.lines().len() == 3).await;
    assert_eq!(log.count("h1:7"), 1);
    assert_eq!(log.count("h2:7"), 2);
    h.close().await;
}

/// Matrix 26: the ordered handler ends before the concurrent one starts.
#[tokio::test(start_paused = true)]
async fn ordered_runs_first() {
    let log = Log::default();
    let o = {
        let log = log.clone();
        handler("o", true, move |_, _: MessageDelete| {
            let log = log.clone();
            async move {
                log.push("o-start");
                tokio::time::sleep(Duration::from_secs(1)).await;
                log.push("o-end");
                Ok(())
            }
        })
    };
    let c = {
        let log = log.clone();
        handler("c", false, move |_, _: MessageDelete| {
            let log = log.clone();
            async move {
                log.push("c-start");
                Ok(())
            }
        })
    };
    let h = Harness::start(App::builder().module(Module::new("m").add(c).add(o)));
    h.push(message_delete(1, Some(GA)));
    until("c", || log.count("c-start") == 1).await;
    assert_eq!(log.lines(), ["o-start", "o-end", "c-start"]);
    h.close().await;
}

/// An ordered `MessageDelete` handler logging start/end; message 1 sleeps
/// 1 s, and a message in `blocked_guild` waits for `gate`.
fn ordered_logger(log: &Log, gate: &Arc<Notify>, blocked_guild: Option<u64>) -> EventHandler {
    let log = log.clone();
    let gate = Arc::clone(gate);
    handler("o", true, move |ctx, e: MessageDelete| {
        let log = log.clone();
        let gate = Arc::clone(&gate);
        async move {
            log.push(format!("start:{}", e.id));
            if e.id.get() == 1 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if blocked_guild.is_some() && ctx.guild_id().map(Id::get) == blocked_guild {
                gate.notified().await;
            }
            log.push(format!("end:{}", e.id));
            Ok(())
        }
    })
}

/// Matrix 27: events of one guild run their ordered handlers in arrival
/// order, one after another.
#[tokio::test(start_paused = true)]
async fn per_guild_order() {
    let log = Log::default();
    let gate = Arc::new(Notify::new());
    let h = Harness::start(
        App::builder().module(Module::new("m").add(ordered_logger(&log, &gate, None))),
    );
    h.push(message_delete(1, Some(GA)));
    h.push(message_delete(2, Some(GA)));
    until("both", || log.count("end:2") == 1).await;
    assert_eq!(log.lines(), ["start:1", "end:1", "start:2", "end:2"]);
    h.close().await;
}

/// Matrix 28: a blocked guild does not block another sub-queue.
#[tokio::test]
async fn guild_isolation() {
    let log = Log::default();
    let gate = Arc::new(Notify::new());
    let h = Harness::start(App::builder().module(Module::new("m").add(ordered_logger(
        &log,
        &gate,
        Some(GA),
    ))));
    h.push(message_delete(5, Some(GA)));
    until("GA started", || log.count("start:5") == 1).await;
    h.push(message_delete(6, Some(GB)));
    until("GB handled", || log.count("end:6") == 1).await;
    assert_eq!(log.count("end:5"), 0);
    gate.notify_one();
    until("GA released", || log.count("end:5") == 1).await;
    h.close().await;
}

/// Matrix 29: guildless events share sub-queue 0 and run in order.
#[tokio::test(start_paused = true)]
async fn no_guild_is_sequential() {
    let log = Log::default();
    let o = {
        let log = log.clone();
        handler("o", true, move |ctx, e: UserUpdate| {
            let log = log.clone();
            async move {
                assert_eq!(ctx.guild_id(), None);
                log.push(format!("start:{}", e.id));
                if e.id.get() == 1 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                log.push(format!("end:{}", e.id));
                Ok(())
            }
        })
    };
    let h = Harness::start(App::builder().module(Module::new("m").add(o)));
    assert_eq!(subqueue(user_update(1).guild_id()), 0);
    h.push(user_update(1));
    h.push(user_update(2));
    until("both", || log.count("end:2") == 1).await;
    assert_eq!(log.lines(), ["start:1", "end:1", "start:2", "end:2"]);
    h.close().await;
}

/// Matrix 30: ordered handlers of one event run in fold order.
#[tokio::test(start_paused = true)]
async fn ordered_sequence() {
    let log = Log::default();
    let step = |name: &'static str, sleep: bool| {
        let log = log.clone();
        handler(name, true, move |_, _: MessageDelete| {
            let log = log.clone();
            async move {
                log.push(format!("{name}-start"));
                if sleep {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                log.push(format!("{name}-end"));
                Ok(())
            }
        })
    };
    let h = Harness::start(
        App::builder().module(
            Module::new("m")
                .add(step("o1", true))
                .add(step("o2", false)),
        ),
    );
    h.push(message_delete(1, None));
    until("o2", || log.count("o2-end") == 1).await;
    assert_eq!(log.lines(), ["o1-start", "o1-end", "o2-start", "o2-end"]);
    h.close().await;
}

/// Matrix 31: at most `handler_concurrency` events in flight.
#[tokio::test(start_paused = true)]
async fn concurrency_bound() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Semaphore::new(0));
    let h = {
        let (in_flight, max, done, release) = (
            Arc::clone(&in_flight),
            Arc::clone(&max),
            Arc::clone(&done),
            Arc::clone(&release),
        );
        handler("h", false, move |_, _: MessageDelete| {
            let (in_flight, max, done, release) = (
                Arc::clone(&in_flight),
                Arc::clone(&max),
                Arc::clone(&done),
                Arc::clone(&release),
            );
            async move {
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(now, Ordering::SeqCst);
                release.acquire().await?.forget();
                in_flight.fetch_sub(1, Ordering::SeqCst);
                done.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
    };
    let harness = Harness::start(
        App::builder()
            .handler_concurrency(2)
            .module(Module::new("m").add(h)),
    );
    for n in 1..=5 {
        harness.push(message_delete(n, Some(n)));
    }
    until("two in flight", || in_flight.load(Ordering::SeqCst) == 2).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(in_flight.load(Ordering::SeqCst), 2);
    assert_eq!(done.load(Ordering::SeqCst), 0);
    release.add_permits(5);
    until("all five", || done.load(Ordering::SeqCst) == 5).await;
    assert_eq!(max.load(Ordering::SeqCst), 2);
    harness.close().await;
}

/// Matrix 32: an interaction is taken while every permit is busy.
#[tokio::test]
async fn interaction_bypasses_permits() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let gate = Arc::new(Notify::new());
    let blocker = {
        let (log, gate) = (log.clone(), Arc::clone(&gate));
        handler("h", false, move |_, e: MessageDelete| {
            let (log, gate) = (log.clone(), Arc::clone(&gate));
            async move {
                log.push(format!("start:{}", e.id));
                if e.id.get() == 1 {
                    gate.notified().await;
                }
                log.push(format!("end:{}", e.id));
                Ok(())
            }
        })
    };
    let h = Harness::start(
        App::builder()
            .handler_concurrency(1)
            .module(Module::new("m").add(blocker)),
    );
    h.push(message_delete(1, Some(GA)));
    until("M1 started", || log.count("start:1") == 1).await;
    h.push(message_delete(2, Some(GA)));
    tokio::time::sleep(Duration::from_millis(20)).await;
    h.push_interaction(9);
    until("I1 traced", || {
        !capture
            .lines_with(&["no handler registered", "kind=Interaction"])
            .is_empty()
    })
    .await;
    assert_eq!(log.lines(), ["start:1"]);
    gate.notify_one();
    until("M2", || log.count("end:2") == 1).await;
    h.close().await;
}

/// Matrix 33: an `Err` is logged with its fields; siblings still run.
#[tokio::test]
async fn handler_error_is_logged() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let failing = handler("bot::fails", false, |_, _: MessageDelete| async {
        Err(HandlerError::new(std::io::Error::other("disk full")))
    });
    let h = Harness::start(
        App::builder().module(
            Module::new("audit")
                .add(failing)
                .add(logging("ok", false, &log)),
        ),
    );
    h.push(message_delete(3, Some(GA)));
    until("error log", || {
        !capture.lines_with(&["event handler failed"]).is_empty()
    })
    .await;
    until("sibling", || log.count("ok:3") == 1).await;
    let lines = capture.lines_with(&["event handler failed"]);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    for needle in [
        "ERROR",
        "noctis::event",
        "module=\"audit\"",
        "handler=\"bot::fails\"",
        "event_type=\"MESSAGE_DELETE\"",
        "error=disk full",
    ] {
        assert!(line.contains(needle), "{needle} missing from {line}");
    }
    h.close().await;
}

/// Matrix 34: panics are logged with their message; the next event runs.
#[tokio::test]
async fn panics_are_logged() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let panicking = {
        let log = log.clone();
        handler("bot::panics", false, move |_, e: MessageDelete| {
            let log = log.clone();
            async move {
                let s = String::from("dynamic message");
                match e.id.get() {
                    1 => panic!("boom"),
                    2 => panic!("{}", s),
                    3 => std::panic::panic_any(5_u8),
                    _ => log.push(format!("ok:{}", e.id)),
                }
                Ok(())
            }
        })
    };
    let h = Harness::start(App::builder().module(Module::new("m").add(panicking)));
    for n in 1..=4 {
        h.push(message_delete(n, Some(GA)));
    }
    until("next event", || log.count("ok:4") == 1).await;
    until("three panics", || {
        capture.lines_with(&["event handler panicked"]).len() == 3
    })
    .await;
    let lines = capture.lines_with(&["event handler panicked"]);
    for message in [
        "panic=\"boom\"",
        "panic=\"dynamic message\"",
        "panic=\"<non-string panic>\"",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(message)),
            "{message} missing from {lines:?}"
        );
    }
    assert!(lines.iter().all(|l| l.contains("handler=\"bot::panics\"")));
    h.close().await;
}

/// Matrix 35: a panicking ordered handler still lets the concurrent ones
/// and the next same-guild event run.
#[tokio::test]
async fn ordered_panic() {
    let log = Log::default();
    let o = handler("o", true, |_, e: MessageDelete| async move {
        assert!(e.id.get() != 1, "ordered boom");
        Ok(())
    });
    let h = Harness::start(
        App::builder().module(
            Module::new("m")
                .add(o)
                .add(logging("c", false, &log))
                .add(logging("o2", true, &log)),
        ),
    );
    h.push(message_delete(1, Some(GA)));
    h.push(message_delete(2, Some(GA)));
    until("both events", || {
        log.count("c:2") == 1 && log.count("c:1") == 1
    })
    .await;
    assert_eq!(log.count("o2:1"), 1);
    assert!(log.position("o2:1") < log.position("o2:2"));
    h.close().await;
}

/// Matrix 36: an event without a handler is traced and its permit freed.
#[tokio::test]
async fn no_handler_releases_permit() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let h = Harness::start(
        App::builder()
            .handler_concurrency(1)
            .module(Module::new("m").add(logging("h", false, &log))),
    );
    h.push(Event::GatewayHeartbeatAck);
    h.push(user_update(1));
    h.push(message_delete(8, None));
    until("handled", || log.count("h:8") == 1).await;
    assert_eq!(
        capture
            .lines_with(&["no handler registered", "UserUpdate"])
            .len(),
        1
    );
    h.close().await;
}

/// Matrix 37: `drain` returns once every sender has dropped.
#[tokio::test]
async fn drain_ends_when_senders_drop() {
    let h = Harness::start(&mut App::builder());
    h.push(Event::Resumed);
    h.push(Event::GatewayHeartbeatAck);
    h.close().await;
}

/// Matrix 37: dropping `AbortOnDrop` aborts a pending drain task.
#[tokio::test]
async fn abort_on_drop_aborts_pending_drain() {
    let app = App::builder().build().unwrap();
    let (sender, receiver) = noctis_gateway::lanes(LaneConfig::default());
    let handle = tokio::spawn(drain(receiver, app.workers()));
    let abort = handle.abort_handle();
    tokio::task::yield_now().await;
    assert!(!abort.is_finished());
    drop(AbortOnDrop(handle));
    until("abort", || abort.is_finished()).await;
    drop(sender);
}

/// Matrix 40: the context a handler receives.
#[tokio::test]
async fn event_ctx_contents() {
    let seen = Arc::new(Mutex::new(None));
    let probe = {
        let seen = Arc::clone(&seen);
        handler("probe", false, move |ctx: EventCtx, msg: MessageCreate| {
            let seen = Arc::clone(&seen);
            async move {
                *seen.lock().unwrap() = Some((ctx, msg.id.get()));
                Ok(())
            }
        })
    };
    let h = Harness::start(
        App::builder().state(7_u32).module(
            Module::new("profile/xp")
                .add(probe)
                .add(noop::<MessageDelete>("x")),
        ),
    );
    h.push_on(
        ShardId::new(2, 4),
        InboundKind::Event(Box::new(message_create(GA))),
    );
    until("probe", || seen.lock().unwrap().is_some()).await;
    let (ctx, id) = seen.lock().unwrap().take().unwrap();
    assert_eq!(id, 10);
    assert_eq!(ctx.shard(), 2);
    assert_eq!(ctx.guild_id(), Some(Id::new(GA)));
    assert_eq!(ctx.module(), "profile/xp");
    assert_eq!(*ctx.state::<u32>().unwrap(), 7);
    assert!(ctx.state::<u64>().is_none());
    assert!(Arc::ptr_eq(ctx.http(), h.app.http()));
    let debug = format!("{ctx:?}");
    assert!(
        debug.contains("profile/xp") && debug.contains("u32"),
        "{debug}"
    );
    assert!(!debug.contains("Client"), "{debug}");
    h.close().await;
}

/// `panic_message` covers every payload shape.
#[test]
fn panic_messages() {
    let s: Box<dyn Any + Send> = Box::new("a");
    assert_eq!(panic_message(&*s), "a");
    let s: Box<dyn Any + Send> = Box::new(String::from("b"));
    assert_eq!(panic_message(&*s), "b");
    let s: Box<dyn Any + Send> = Box::new(5_i32);
    assert_eq!(panic_message(&*s), "<non-string panic>");
}

/// A `GUILD_DELETE` of guild `GA`.
fn guild_delete(unavailable: Option<bool>) -> Event {
    Event::GuildDelete(GuildDelete {
        id: Id::new(GA),
        unavailable,
    })
}

/// An outage `GUILD_DELETE` runs both the `UnavailableGuild` and the
/// `GuildDelete` handlers; a plain one only the `GuildDelete` handler.
#[tokio::test]
async fn guild_delete_reaches_unavailable_guild_handlers() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let outage = {
        let log = log.clone();
        handler("outage", false, move |_, e: UnavailableGuild| {
            let log = log.clone();
            async move {
                log.push(format!("outage:{}", e.id));
                Ok(())
            }
        })
    };
    let deleted = {
        let log = log.clone();
        handler("deleted", false, move |_, e: GuildDelete| {
            let log = log.clone();
            async move {
                log.push(format!("deleted:{}:{:?}", e.id, e.unavailable));
                Ok(())
            }
        })
    };
    let h = Harness::start(App::builder().module(Module::new("m").add(outage).add(deleted)));
    h.push(guild_delete(Some(true)));
    until("outage", || {
        log.count("outage:1") == 1 && log.count("deleted:1:Some(true)") == 1
    })
    .await;
    h.push(guild_delete(None));
    until("plain delete", || log.count("deleted:1:None") == 1).await;
    h.close().await;
    assert_eq!(log.count("outage:1"), 1);
    assert_eq!(log.lines().len(), 3);
    assert!(capture.lines_with(&["ERROR"]).is_empty());
}

/// A handler whose constructing call panics (before it returns a future)
/// is logged, and the next same-guild event is still handled.
#[tokio::test]
async fn ordered_sync_panic_is_caught() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let log = Log::default();
    let sync_panic = EventHandler::__new(
        "bot::sync_panic",
        true,
        false,
        |_: EventCtx, e: MessageDelete| {
            assert!(e.id.get() != 1, "sync boom");
            async { Ok(()) }
        },
    );
    let h = Harness::start(
        App::builder().module(
            Module::new("m")
                .add(sync_panic)
                .add(logging("after", true, &log)),
        ),
    );
    h.push(message_delete(1, Some(GA)));
    h.push(message_delete(2, Some(GA)));
    until("next event", || log.count("after:2") == 1).await;
    let lines = capture.lines_with(&["event handler panicked"]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("sync boom"), "{}", lines[0]);
    assert!(lines[0].contains("handler=\"bot::sync_panic\""));
    assert_eq!(log.count("after:1"), 1);
    h.close().await;
}
