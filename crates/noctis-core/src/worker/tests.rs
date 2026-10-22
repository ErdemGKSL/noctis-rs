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
        let drain = tokio::spawn(drain(receiver, app.workers(Shutdown::default())));
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
                .__add(logging("h1", false, &log))
                .__add(twice.clone())
                .__add(twice),
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
    let h = Harness::start(App::builder().module(Module::new("m").__add(c).__add(o)));
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
        App::builder().module(Module::new("m").__add(ordered_logger(&log, &gate, None))),
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
    let h = Harness::start(App::builder().module(Module::new("m").__add(ordered_logger(
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
    let h = Harness::start(App::builder().module(Module::new("m").__add(o)));
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
                .__add(step("o1", true))
                .__add(step("o2", false)),
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
            .module(Module::new("m").__add(h)),
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
            .module(Module::new("m").__add(blocker)),
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

/// A type-2 (`APPLICATION_COMMAND`) interaction for the global `name`
/// (id 100, token `tok`, application 7, user 3).
fn command_interaction(name: &str) -> InboundKind {
    let d = json!({
        "id": "100",
        "application_id": "7",
        "type": 2,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": { "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null },
        "data": { "id": "9", "name": name, "type": 1, "options": [] },
    });
    let raw = RawJson::new(bytes::Bytes::from(d.to_string()));
    InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw)
}

/// A type-2 interaction runs its Command and replies while every worker
/// permit is held by a blocked event handler.
#[tokio::test]
async fn command_interaction_bypasses_permits() {
    let server = crate::edge::test_support::TestServer::start_with(
        false,
        crate::responder::tests::discord(),
    )
    .await;
    let log = Log::default();
    let gate = Arc::new(Notify::new());
    let blocker = {
        let (log, gate) = (log.clone(), Arc::clone(&gate));
        handler("h", false, move |_, e: MessageDelete| {
            let (log, gate) = (log.clone(), Arc::clone(&gate));
            async move {
                log.push(format!("start:{}", e.id));
                gate.notified().await;
                log.push(format!("end:{}", e.id));
                Ok(())
            }
        })
    };
    let command = {
        let log = log.clone();
        crate::CommandHandler::__new_plain(
            "m::ping",
            &["ping"],
            "d",
            move |_: crate::CommandCtx, _: crate::CommandResponder| {
                log.push("ping");
                async { Ok(crate::responder::tests::v2("pong")) }
            },
        )
    };
    let h = Harness::start(
        App::builder()
            .handler_concurrency(1)
            .http_proxy(server.authority())
            .module(Module::new("m").__add(blocker).__add(command)),
    );
    h.push(message_delete(1, Some(GA)));
    until("M1 started", || log.count("start:1") == 1).await;
    h.push_on(ShardId::ONE, command_interaction("ping"));
    until("callback", || !server.seen().is_empty()).await;
    assert_eq!(log.lines(), ["start:1", "ping"]);
    let seen = server.seen();
    assert_eq!(seen[0].path, "/api/v10/interactions/100/tok/callback");
    gate.notify_one();
    until("M1", || log.count("end:1") == 1).await;
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
                .__add(failing)
                .__add(logging("ok", false, &log)),
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
    let h = Harness::start(App::builder().module(Module::new("m").__add(panicking)));
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
                .__add(o)
                .__add(logging("c", false, &log))
                .__add(logging("o2", true, &log)),
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
            .module(Module::new("m").__add(logging("h", false, &log))),
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
    let handle = tokio::spawn(drain(receiver, app.workers(Shutdown::default())));
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
    let h =
        Harness::start(
            App::builder().state(7_u32).module(
                Module::new("profile/xp")
                    .__add(probe)
                    .__add(noop::<MessageDelete>("x")),
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
    let h = Harness::start(App::builder().module(Module::new("m").__add(outage).__add(deleted)));
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
                .__add(sync_panic)
                .__add(logging("after", true, &log)),
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

// ---- Story 5.7: spans, handler metrics and Error Sinks ----

mod observability {
    use metrics_util::MetricKind;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use tracing_subscriber::fmt::format::FmtSpan;

    use super::*;
    use crate::test_support::{Recording, Seen};
    use crate::{ErrorOrigin, ErrorReport, ErrorSink};

    /// The handler of Matrix 8: `bot::h` in Module `audit`.
    fn failing() -> EventHandler {
        handler("bot::h", false, |_, _: MessageDelete| async {
            Err(HandlerError::new(std::io::Error::other("disk full")))
        })
    }

    fn sink_app(recording: &Arc<Recording>, module: Module) -> Harness {
        Harness::start(
            App::builder()
                .error_sink(ErrorOrigin::Event, Arc::clone(recording))
                .module(module),
        )
    }

    fn on_shard_3(h: &Harness, event: Event) {
        h.push_on(ShardId::new(3, 4), InboundKind::Event(Box::new(event)));
    }

    /// Matrix 8: an `Err` reaches the custom sink with every context
    /// field; siblings run; `LogSink` stays silent.
    #[tokio::test]
    async fn error_reaches_custom_sink() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let recording = Arc::new(Recording::default());
        let log = Log::default();
        let h = sink_app(
            &recording,
            Module::new("audit")
                .__add(failing())
                .__add(logging("ok", false, &log)),
        );
        on_shard_3(&h, message_delete(3, Some(GA)));
        until("report", || recording.seen().len() == 1).await;
        until("sibling", || log.count("ok:3") == 1).await;
        assert_eq!(
            recording.seen(),
            [Seen {
                origin: ErrorOrigin::Event,
                kind: "error:disk full".to_owned(),
                app: "default".to_owned(),
                shard: Some(3),
                guild_id: Some(GA),
                module: Some("audit".to_owned()),
                handler: Some("bot::h".to_owned()),
                event_type: Some("MESSAGE_DELETE".to_owned()),
            }]
        );
        assert!(capture.lines_with(&["event handler failed"]).is_empty());
        h.close().await;
    }

    /// Matrix 13: every panic form reaches the sink; the next event runs.
    #[tokio::test]
    async fn panics_reach_custom_sink() {
        let recording = Arc::new(Recording::default());
        let log = Log::default();
        let panicking = {
            let log = log.clone();
            EventHandler::__new(
                "bot::p",
                false,
                false,
                move |_: EventCtx, e: MessageDelete| {
                    assert!(e.id.get() != 4, "boom");
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
                },
            )
        };
        let h = sink_app(&recording, Module::new("m").__add(panicking));
        for n in 1..=5 {
            h.push(message_delete(n, Some(GA)));
        }
        until("next event", || log.count("ok:5") == 1).await;
        until("four panics", || recording.seen().len() == 4).await;
        let mut kinds: Vec<_> = recording.seen().into_iter().map(|s| s.kind).collect();
        kinds.sort();
        assert_eq!(
            kinds,
            [
                "panic:<non-string panic>",
                "panic:boom",
                "panic:boom",
                "panic:dynamic message"
            ]
        );
        h.close().await;
    }

    /// Matrix 14: a guildless event has no `guild_id` anywhere.
    #[tokio::test]
    async fn guildless_report_and_log() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let recording = Arc::new(Recording::default());
        let fails = handler("bot::user", false, |_, _: UserUpdate| async {
            Err(HandlerError::new(std::io::Error::other("nope")))
        });
        let h = sink_app(&recording, Module::new("m").__add(fails.clone()));
        h.push(user_update(5));
        until("report", || recording.seen().len() == 1).await;
        assert_eq!(recording.seen()[0].guild_id, None);
        assert_eq!(
            recording.seen()[0].event_type.as_deref(),
            Some("USER_UPDATE")
        );
        h.close().await;

        let h = Harness::start(App::builder().module(Module::new("m").__add(fails)));
        h.push(user_update(5));
        until("log", || {
            !capture.lines_with(&["event handler failed"]).is_empty()
        })
        .await;
        let line = &capture.lines_with(&["event handler failed"])[0];
        assert!(!line.contains("guild_id"), "{line}");
        h.close().await;
    }

    /// Matrix 15 and 16: duration samples for every run, errors for `Err`
    /// and panics only.
    #[tokio::test]
    async fn handler_metrics() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _metrics = metrics::set_default_local_recorder(&recorder);
        let recording = Arc::new(Recording::default());
        let log = Log::default();
        let mixed = {
            let log = log.clone();
            handler("bot::mixed", false, move |_, e: MessageDelete| {
                let log = log.clone();
                async move {
                    match e.id.get() {
                        1 => Err(HandlerError::new(std::io::Error::other("x"))),
                        2 => panic!("boom"),
                        n => {
                            log.push(format!("ok:{n}"));
                            Ok(())
                        }
                    }
                }
            })
        };
        let h = sink_app(&recording, Module::new("m").__add(mixed.clone()));
        h.push(message_delete(3, Some(GA)));
        // The duration is recorded in the same poll that ran the handler.
        until("ok run", || log.count("ok:3") == 1).await;
        h.close().await;
        let snap = snapshotter.snapshot().into_vec();
        let samples = |snap: &[(metrics_util::CompositeKey, _, _, DebugValue)]| {
            snap.iter()
                .find(|(k, ..)| k.key().name() == "noctis_handler_duration_seconds")
                .map_or_else(Vec::new, |(.., v)| match v {
                    DebugValue::Histogram(values) => values.iter().map(|v| v.0).collect(),
                    other => panic!("{other:?}"),
                })
        };
        assert_eq!(samples(&snap).len(), 1, "Matrix 15");
        assert!(
            !snap
                .iter()
                .any(|(k, ..)| k.key().name() == "noctis_handler_errors_total"),
            "Matrix 15: {snap:?}"
        );

        let h = sink_app(
            &recording,
            Module::new("m").__add(handler(
                "bot::mixed",
                false,
                |_, e: MessageDelete| async move {
                    match e.id.get() {
                        1 => Err(HandlerError::new(std::io::Error::other("x"))),
                        2 => panic!("boom"),
                        _ => Ok(()),
                    }
                },
            )),
        );
        for n in 1..=3 {
            h.push(message_delete(n, Some(GA)));
        }
        until("two reports", || recording.seen().len() == 2).await;
        h.close().await;
        // Every handler has ended once the reports are in and the drain is
        // closed; give the last Ok run its sample.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let snap = snapshotter.snapshot().into_vec();
        let durations: Vec<f64> = samples(&snap);
        assert_eq!(durations.len(), 3, "{snap:?}");
        assert!(durations.iter().all(|d| *d >= 0.0));
        let errors = snap
            .iter()
            .find(|(k, ..)| {
                k.kind() == MetricKind::Counter && k.key().name() == "noctis_handler_errors_total"
            })
            .unwrap();
        let labels: Vec<_> = errors
            .0
            .key()
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect();
        assert_eq!(
            labels,
            [
                ("app".to_owned(), "default".to_owned()),
                ("kind".to_owned(), "event".to_owned())
            ]
        );
        assert!(matches!(errors.3, DebugValue::Counter(2)), "{:?}", errors.3);
    }

    /// Panics on every report.
    struct Panicking(AtomicUsize);

    impl ErrorSink for Panicking {
        fn report(&self, _: ErrorReport<'_>) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("sink broke");
        }
    }

    /// Matrix 17: a panicking sink never stops the drain or the handlers.
    #[tokio::test]
    async fn panicking_sink_keeps_workers_running() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let sink = Arc::new(Panicking(AtomicUsize::new(0)));
        let log = Log::default();
        let h = Harness::start(
            App::builder()
                .error_sink(ErrorOrigin::Event, Arc::clone(&sink))
                .module(
                    Module::new("audit")
                        .__add(failing())
                        .__add(logging("ok", false, &log)),
                ),
        );
        h.push(message_delete(1, Some(GA)));
        until("first", || log.count("ok:1") == 1).await;
        h.push(message_delete(2, Some(GA)));
        until("second", || log.count("ok:2") == 1).await;
        until("two sink panics", || {
            capture.lines_with(&["error sink panicked"]).len() == 2
        })
        .await;
        assert_eq!(sink.0.load(Ordering::SeqCst), 2);
        for line in capture.lines_with(&["error sink panicked"]) {
            for needle in [
                "noctis::error_sink",
                "origin=\"event\"",
                "panic=\"sink broke\"",
            ] {
                assert!(line.contains(needle), "{needle} missing from {line}");
            }
        }
        h.close().await;
    }

    /// A subscriber that also logs span creation.
    fn span_subscriber(capture: &Capture) -> impl tracing::Subscriber + Send + Sync + 'static {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_span_events(FmtSpan::NEW)
            .with_writer(capture.clone())
            .finish()
    }

    /// Matrix 27: the `noctis.event` span and the failure line inside it.
    #[tokio::test]
    async fn event_span_fields() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(span_subscriber(&capture));
        let h = Harness::start(App::builder().module(Module::new("audit").__add(failing())));
        on_shard_3(&h, message_delete(3, Some(GA)));
        until("failure", || {
            !capture.lines_with(&["event handler failed"]).is_empty()
        })
        .await;
        h.close().await;
        let fields = format!(
            "noctis.event{{app=\"default\" shard=3 guild_id={GA} module=\"audit\" handler=\"bot::h\" event_type=\"MESSAGE_DELETE\"}}"
        );
        let created = capture.lines_with(&[&fields, "new"]);
        assert_eq!(created.len(), 1, "{}", capture.text());
        let failure = &capture.lines_with(&["event handler failed"])[0];
        assert!(failure.contains(&fields), "{failure}");
    }

    /// Matrix 27: logs inside a handler body, before and after an
    /// `.await`, carry the `noctis.event` span.
    #[tokio::test]
    async fn handler_logs_inside_event_span() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(span_subscriber(&capture));
        let inside = handler("bot::h", false, |_, _: MessageDelete| async {
            tracing::info!("inside before");
            tokio::task::yield_now().await;
            tracing::info!("inside after");
            Ok(())
        });
        let h = Harness::start(App::builder().module(Module::new("audit").__add(inside)));
        on_shard_3(&h, message_delete(3, Some(GA)));
        until("both lines", || {
            capture.lines_with(&["inside after"]).len() == 1
        })
        .await;
        h.close().await;
        let fields = format!(
            "noctis.event{{app=\"default\" shard=3 guild_id={GA} module=\"audit\" handler=\"bot::h\" event_type=\"MESSAGE_DELETE\"}}"
        );
        for needle in ["inside before", "inside after"] {
            let lines = capture.lines_with(&[needle]);
            assert_eq!(lines.len(), 1, "{}", capture.text());
            assert!(lines[0].contains(&fields), "{}", lines[0]);
        }
    }

    /// Matrix 28: the `noctis.interaction` span, with and without a guild.
    #[tokio::test]
    async fn interaction_span_fields() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(span_subscriber(&capture));
        let h = Harness::start(&mut App::builder());
        let interaction = |id: u64, guild: Option<u64>| {
            let mut d = json!({
                "id": id.to_string(),
                "application_id": "1",
                "type": 1,
                "token": "t",
                "version": 1,
                "authorizing_integration_owners": {},
                "entitlements": [],
            });
            if let Some(guild) = guild {
                d["guild_id"] = json!(guild.to_string());
            }
            let raw = RawJson::new(bytes::Bytes::from(d.to_string()));
            InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw)
        };
        h.push_on(ShardId::new(2, 4), interaction(1, Some(GA)));
        h.push_on(ShardId::new(2, 4), interaction(2, None));
        until("two spans", || {
            capture.lines_with(&["noctis.interaction", "new"]).len() == 2
        })
        .await;
        h.close().await;
        let spans = capture.lines_with(&["noctis.interaction", "new"]);
        assert!(
            spans[0].contains(&format!(
                "noctis.interaction{{app=\"default\" shard=2 guild_id={GA}}}"
            )),
            "{}",
            spans[0]
        );
        assert!(
            spans[1].contains("noctis.interaction{app=\"default\" shard=2}"),
            "{}",
            spans[1]
        );
    }

    /// Matrix 28: `module` and `path` are declared on the span.
    #[test]
    fn interaction_span_declares_module_and_path() {
        #[derive(Default)]
        struct Fields(Mutex<Vec<Vec<String>>>);
        impl tracing::Subscriber for Fields {
            fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                if span.metadata().name() == "noctis.interaction" {
                    let names = span
                        .metadata()
                        .fields()
                        .iter()
                        .map(|f| f.name().to_owned())
                        .collect();
                    self.0.lock().unwrap().push(names);
                }
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, _: &tracing::Event<'_>) {}
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }
        let fields = Arc::new(Fields::default());
        let dispatch = tracing::Dispatch::from(Arc::clone(&fields));
        tracing::dispatcher::with_default(&dispatch, || {
            let d = json!({
                "id": "1",
                "application_id": "1",
                "type": 1,
                "token": "t",
                "version": 1,
                "authorizing_integration_owners": {},
                "entitlements": [],
            });
            let raw = RawJson::new(bytes::Bytes::from(d.to_string()));
            interaction_unhandled(
                Inbound {
                    shard: ShardId::ONE,
                    received_at: Instant::now(),
                    kind: InboundKind::Interaction(
                        Box::new(serde_json::from_value(d).unwrap()),
                        raw,
                    ),
                },
                "default",
            );
        });
        assert_eq!(
            *fields.0.lock().unwrap(),
            [["app", "shard", "guild_id", "module", "path"]]
        );
    }
}

// ---- Story 5.8: shutdown, cancellation (Matrix 16-26) ----

mod shutdown {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use tokio::sync::mpsc;

    use super::*;
    use crate::app::{ShardStatus, Watch, wind_down};
    use crate::lifecycle::Lifecycle;
    use crate::test_support::{Recording, Seen};
    use crate::{ErrorOrigin, ErrorReport, ErrorSink};

    /// A drain whose shutdown controls the test holds.
    struct Running {
        app: App,
        sender: LaneSender,
        controls: Shutdown,
        drain: AbortOnDrop,
        recording: Arc<Recording>,
    }

    impl Running {
        /// Builds the App (with a recording `Event` sink) and spawns the
        /// drain; on a current-thread runtime the drain only runs at the
        /// next yield.
        fn start(builder: &mut AppBuilder) -> Self {
            let recording = Arc::new(Recording::default());
            builder
                .runtime(&Runtime::for_test())
                .error_sink(ErrorOrigin::Event, Arc::clone(&recording));
            Self::start_with(builder, recording)
        }

        fn start_with(builder: &mut AppBuilder, recording: Arc<Recording>) -> Self {
            let app = builder.build().unwrap();
            let controls = Shutdown::default();
            let (sender, receiver) = noctis_gateway::lanes(app.lane_config());
            let drain = AbortOnDrop(tokio::spawn(drain(receiver, app.workers(controls.clone()))));
            Self {
                app,
                sender,
                controls,
                drain,
                recording,
            }
        }

        fn push(&self, event: Event) {
            self.sender.push(Inbound {
                shard: ShardId::ONE,
                received_at: Instant::now(),
                kind: InboundKind::Event(Box::new(event)),
            });
        }

        fn push_interaction(&self) {
            let d = json!({
                "id": "9",
                "application_id": "1",
                "type": 1,
                "token": "t",
                "version": 1,
                "authorizing_integration_owners": {},
                "entitlements": [],
            });
            let raw = RawJson::new(bytes::Bytes::from(d.to_string()));
            self.sender.push(Inbound {
                shard: ShardId::ONE,
                received_at: Instant::now(),
                kind: InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw),
            });
        }

        /// Runs core's wind-down with `grace`, bounded by [`WAIT`].
        async fn wind_down(&mut self, grace: Duration) {
            let (_tx, mut events) = mpsc::unbounded_channel();
            let status = ShardStatus {
                app: Arc::from("default"),
                total: 1,
            };
            let lifecycle = Lifecycle::new();
            let mut watch = Watch {
                events: &mut events,
                status: &status,
                lifecycle: &lifecycle,
            };
            tokio::time::timeout(
                WAIT,
                wind_down(grace, &self.controls, &mut self.drain, &mut watch),
            )
            .await
            .unwrap();
        }

        fn kinds(&self) -> Vec<String> {
            self.recording.seen().into_iter().map(|s| s.kind).collect()
        }
    }

    /// A `MessageDelete` handler that logs `start:{id}`, waits for `gate`,
    /// then logs `end:{id}`.
    fn gated(name: &'static str, ordered: bool, log: &Log, gate: &Arc<Semaphore>) -> EventHandler {
        let log = log.clone();
        let gate = Arc::clone(gate);
        handler(name, ordered, move |_, e: MessageDelete| {
            let log = log.clone();
            let gate = Arc::clone(&gate);
            async move {
                log.push(format!("start:{}", e.id));
                let _permit = gate.acquire().await;
                log.push(format!("end:{}", e.id));
                Ok(())
            }
        })
    }

    /// Matrix 16: an empty drain returns at once and tracks nothing.
    #[tokio::test]
    async fn empty_drain() {
        let mut run = Running::start(
            App::builder().module(Module::new("m").__add(noop::<MessageDelete>("h"))),
        );
        let started = Instant::now();
        run.wind_down(Duration::from_secs(5)).await;
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(run.drain.0.is_finished());
        assert!(run.controls.tracker.is_empty());
        assert!(!run.controls.cancel.is_cancelled());
        drop(run.app);
    }

    /// Matrix 17: queued events finish within the grace period.
    #[tokio::test]
    async fn finish_in_grace() {
        let log = Log::default();
        let slow = {
            let log = log.clone();
            handler("h", false, move |_, e: MessageDelete| {
                let log = log.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    log.push(format!("ok:{}", e.id));
                    Ok(())
                }
            })
        };
        let mut run = Running::start(App::builder().module(Module::new("m").__add(slow)));
        for id in 1..=3 {
            run.push(message_delete(id, Some(GA)));
        }
        run.wind_down(Duration::from_secs(1)).await;
        assert_eq!(log.lines().len(), 3, "{:?}", log.lines());
        assert!(run.kinds().is_empty(), "{:?}", run.kinds());
        assert!(!run.controls.cancel.is_cancelled());
    }

    /// Matrix 18: a handler blocked past the deadline is cancelled once,
    /// with every context field, one error count and one duration sample.
    #[tokio::test]
    async fn deadline_cancels_running_handler() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _metrics = metrics::set_default_local_recorder(&recorder);
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut run = Running::start(
            App::builder().module(Module::new("ops").__add(gated("bot::b", false, &log, &gate))),
        );
        run.sender.push(Inbound {
            shard: ShardId::new(0, 1),
            received_at: Instant::now(),
            kind: InboundKind::Event(Box::new(message_delete(5, Some(3)))),
        });
        until("B started", || log.count("start:5") == 1).await;
        run.wind_down(Duration::from_millis(100)).await;
        assert_eq!(
            run.recording.seen(),
            [Seen {
                origin: ErrorOrigin::Event,
                kind: "cancelled".to_owned(),
                app: "default".to_owned(),
                shard: Some(0),
                guild_id: Some(3),
                module: Some("ops".to_owned()),
                handler: Some("bot::b".to_owned()),
                event_type: Some("MESSAGE_DELETE".to_owned()),
            }]
        );
        assert_eq!(log.count("end:5"), 0);
        assert_eq!(gate.available_permits(), 0);
        assert!(run.controls.tracker.is_empty());
        let snap = snapshotter.snapshot().into_vec();
        let find = |name: &str| {
            snap.iter()
                .find(|(k, ..)| k.key().name() == name)
                .map(|(.., v)| v)
        };
        assert!(
            matches!(
                find("noctis_handler_errors_total"),
                Some(DebugValue::Counter(1))
            ),
            "{snap:?}"
        );
        assert!(
            matches!(find("noctis_handler_duration_seconds"), Some(DebugValue::Histogram(v)) if v.len() == 1),
            "{snap:?}"
        );
    }

    /// Matrix 17: the App shutdown cancels an in-flight Command, which
    /// reports `Cancelled` to the Command Error Sink.
    #[tokio::test]
    async fn shutdown_cancels_command() {
        let recording = Arc::new(Recording::default());
        let started = Arc::new(Notify::new());
        let command = {
            let started = Arc::clone(&started);
            crate::CommandHandler::__new_plain(
                "m::slow",
                &["slow"],
                "d",
                move |_: crate::CommandCtx, _: crate::CommandResponder| {
                    let started = Arc::clone(&started);
                    async move {
                        started.notify_one();
                        std::future::pending::<()>().await;
                        Ok(crate::responder::tests::v2("never"))
                    }
                },
            )
        };
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .error_sink(ErrorOrigin::Command, Arc::clone(&recording))
            .module(Module::new("m").__add(command));
        let mut run = Running::start_with(&mut builder, recording);
        run.sender.push(Inbound {
            shard: ShardId::ONE,
            received_at: Instant::now(),
            kind: command_interaction("slow"),
        });
        tokio::time::timeout(WAIT, started.notified())
            .await
            .unwrap();
        run.wind_down(Duration::from_millis(100)).await;
        let seen = run.recording.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].origin, ErrorOrigin::Command);
        assert_eq!(seen[0].kind, "cancelled");
        assert_eq!(seen[0].handler.as_deref(), Some("m::slow"));
        assert!(run.controls.tracker.is_empty());
    }

    /// Matrix 19: an ordered job queued behind the blocked one is cancelled
    /// too, and its handler body never runs.
    #[tokio::test]
    async fn ordered_queued_is_cancelled() {
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut run = Running::start(
            App::builder().module(Module::new("ops").__add(gated("bot::b", true, &log, &gate))),
        );
        run.push(message_delete(1, Some(GA)));
        run.push(message_delete(2, Some(GA)));
        until("first started", || log.count("start:1") == 1).await;
        run.wind_down(Duration::from_millis(100)).await;
        assert_eq!(run.kinds(), ["cancelled", "cancelled"]);
        assert_eq!(log.count("start:2"), 0, "{:?}", log.lines());
    }

    /// The concurrent part spawned after ordered handlers runs on the
    /// tracker: the wind-down waits for it and cancels it at the deadline.
    #[tokio::test]
    async fn ordered_then_concurrent_cancelled() {
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut run = Running::start(
            App::builder().module(
                Module::new("ops")
                    .__add(logging("o", true, &log))
                    .__add(gated("bot::c", false, &log, &gate)),
            ),
        );
        run.push(message_delete(1, Some(GA)));
        until("C started", || log.count("start:1") == 1).await;
        run.wind_down(Duration::from_millis(100)).await;
        assert_eq!(run.kinds(), ["cancelled"]);
        let seen = run.recording.seen();
        assert_eq!(seen[0].handler.as_deref(), Some("bot::c"), "{seen:?}");
        assert!(run.controls.tracker.is_empty());
    }

    /// Matrix 20: only the blocked one of two concurrent handlers is
    /// cancelled.
    #[tokio::test]
    async fn concurrent_mix() {
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut run = Running::start(
            App::builder().module(
                Module::new("ops")
                    .__add(gated("bot::blocked", false, &log, &gate))
                    .__add(logging("fast", false, &log)),
            ),
        );
        run.push(message_delete(1, Some(GA)));
        until("both started", || {
            log.count("start:1") == 1 && log.count("fast:1") == 1
        })
        .await;
        run.wind_down(Duration::from_millis(100)).await;
        let seen = run.recording.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].kind, "cancelled");
        assert_eq!(seen[0].handler.as_deref(), Some("bot::blocked"));
    }

    /// Matrix 21: runs ending around the deadline report at most once and
    /// never both Ok and Cancelled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_outcome_per_run() {
        let log = Log::default();
        let racing = {
            let log = log.clone();
            handler("h", false, move |_, e: MessageDelete| {
                let log = log.clone();
                async move {
                    // 99..=101 ms: around the 100 ms deadline.
                    let jitter = e.id.get() % 3;
                    tokio::time::sleep(Duration::from_millis(99 + jitter)).await;
                    log.push(format!("ok:{}", e.id));
                    Ok(())
                }
            })
        };
        let mut run = Running::start(App::builder().module(Module::new("m").__add(racing)));
        for id in 1..=200 {
            run.push(message_delete(id, Some(GA)));
        }
        run.wind_down(Duration::from_millis(100)).await;
        let lines = log.lines();
        let unique: std::collections::HashSet<_> = lines.iter().collect();
        assert_eq!(unique.len(), lines.len(), "an Ok logged twice");
        let cancelled = run.kinds().iter().filter(|k| *k == "cancelled").count();
        assert_eq!(run.kinds().len(), cancelled, "{:?}", run.kinds());
        assert_eq!(lines.len() + cancelled, 200);
    }

    /// Matrix 22: zero grace runs no handler and discards the lanes with
    /// one summary warning, without sink reports.
    #[tokio::test]
    async fn zero_grace_discards_lanes() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let mut run = Running::start(
            App::builder().module(Module::new("m").__add(logging("h", false, &log))),
        );
        run.push(message_delete(1, Some(GA)));
        run.push(message_delete(2, Some(GA)));
        run.push_interaction();
        run.wind_down(Duration::ZERO).await;
        assert!(log.lines().is_empty());
        assert!(run.kinds().is_empty());
        let lines = capture.lines_with(&["WARN", "lanes discarded at shutdown"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("interactions=1"), "{}", lines[0]);
        assert!(lines[0].contains("events=2"), "{}", lines[0]);
    }

    /// Matrix 23: an event waiting for a permit is counted, not reported.
    #[tokio::test]
    async fn permit_wait_is_discarded() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut run = Running::start(
            App::builder()
                .handler_concurrency(1)
                .module(Module::new("ops").__add(gated("bot::b", false, &log, &gate))),
        );
        run.push(message_delete(1, Some(GA)));
        until("B started", || log.count("start:1") == 1).await;
        run.push(message_delete(2, Some(GA)));
        // Let the drain (same thread) pop the second event into the
        // permit wait.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        run.wind_down(Duration::from_millis(100)).await;
        assert_eq!(run.kinds(), ["cancelled"]);
        assert_eq!(log.count("start:2"), 0);
        let lines = capture.lines_with(&["lanes discarded at shutdown"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("interactions=0"), "{}", lines[0]);
        assert!(lines[0].contains("events=1"), "{}", lines[0]);
    }

    /// Matrix 24: an `Err` or a panic during the grace period is reported
    /// as such, not as `Cancelled`.
    #[tokio::test]
    async fn err_and_panic_in_grace() {
        let gate = Arc::new(Semaphore::new(0));
        let failing = {
            let gate = Arc::clone(&gate);
            handler("bot::f", false, move |_, e: MessageDelete| {
                let gate = Arc::clone(&gate);
                async move {
                    let _permit = gate.acquire().await;
                    if e.id.get() == 1 {
                        Err(HandlerError::new(std::io::Error::other("x")))
                    } else {
                        panic!("x")
                    }
                }
            })
        };
        let mut run = Running::start(App::builder().module(Module::new("m").__add(failing)));
        run.push(message_delete(1, Some(GA)));
        run.push(message_delete(2, Some(GA)));
        tokio::task::yield_now().await;
        let opener = {
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                gate.add_permits(2);
            })
        };
        run.wind_down(Duration::from_secs(5)).await;
        opener.await.unwrap();
        let mut kinds = run.kinds();
        kinds.sort();
        assert_eq!(kinds, ["error:x", "panic:x"]);
    }

    /// Panics on every report.
    struct Panicking;

    impl ErrorSink for Panicking {
        fn report(&self, _: ErrorReport<'_>) {
            panic!("sink broke");
        }
    }

    /// Matrix 25: a sink panicking on `Cancelled` is logged; the sequence
    /// completes.
    #[tokio::test]
    async fn panicking_sink_on_cancelled() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .error_sink(ErrorOrigin::Event, Panicking)
            .module(Module::new("ops").__add(gated("bot::b", false, &log, &gate)));
        let mut run = Running::start_with(&mut builder, Arc::new(Recording::default()));
        run.push(message_delete(1, Some(GA)));
        until("B started", || log.count("start:1") == 1).await;
        run.wind_down(Duration::from_millis(100)).await;
        assert_eq!(capture.lines_with(&["error sink panicked"]).len(), 1);
        assert!(run.controls.tracker.is_empty());
    }

    /// Matrix 26: a handler blocking its thread cannot be cancelled; the
    /// wind-down warns after the cancel wait and still returns.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stuck_handler_warns() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let stuck = {
            let log = log.clone();
            handler("bot::stuck", false, move |_, _: MessageDelete| {
                let log = log.clone();
                async move {
                    log.push("start");
                    std::thread::sleep(Duration::from_secs(2));
                    Ok(())
                }
            })
        };
        let mut run = Running::start(App::builder().module(Module::new("m").__add(stuck)));
        run.push(message_delete(1, Some(GA)));
        until("stuck started", || log.count("start") == 1).await;
        let started = Instant::now();
        run.wind_down(Duration::from_millis(100)).await;
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1900), "{elapsed:?}");
        let lines = capture.lines_with(&["handlers still running after cancellation"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("running=1"), "{}", lines[0]);
    }

    /// Matrix 41 (unit level): firing `cancel` without a shutdown drops a
    /// running handler's future, releasing what it holds.
    #[tokio::test]
    async fn cancel_drops_handler_future() {
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let run = Running::start(
            App::builder().module(Module::new("ops").__add(gated("bot::b", false, &log, &gate))),
        );
        run.push(message_delete(1, Some(GA)));
        until("B started", || log.count("start:1") == 1).await;
        assert_eq!(Arc::strong_count(&gate), 3);
        run.controls.cancel.cancel();
        until("future dropped", || Arc::strong_count(&gate) == 2).await;
        assert_eq!(run.kinds(), ["cancelled"]);
    }
}

/// An interaction of `kind` with id `id` and `data` (application 7, token
/// `tok`, user 3).
fn interaction_of(id: u64, kind: u8, data: Option<serde_json::Value>) -> InboundKind {
    let mut d = json!({
        "id": id.to_string(),
        "application_id": "7",
        "type": kind,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": { "id": "3", "username": "u", "discriminator": "0000", "avatar": null, "global_name": null },
    });
    if let Some(data) = data {
        d["data"] = data;
    }
    let raw = RawJson::new(bytes::Bytes::from(d.to_string()));
    InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw)
}

/// Story 8.4: the hidden run observer fires exactly once per interaction,
/// for a command, an autocomplete, a component, a modal submit and an
/// unhandled ping, each with its interaction id.
#[tokio::test]
async fn run_observer_fires_once_per_interaction() {
    let server = crate::edge::test_support::TestServer::start_with(
        false,
        crate::responder::tests::discord(),
    )
    .await;
    let seen: Arc<Mutex<Vec<u64>>> = Arc::default();
    let mut builder = App::builder();
    builder.http_proxy(server.authority());
    let sink = Arc::clone(&seen);
    crate::__testing::observe_runs(
        &mut builder,
        Arc::new(move |id| sink.lock().unwrap().push(id.get())),
    );
    let h = Harness::start(&mut builder);
    h.push_on(
        ShardId::ONE,
        interaction_of(
            11,
            2,
            Some(json!({ "id": "9", "name": "nope", "type": 1, "options": [] })),
        ),
    );
    h.push_on(
        ShardId::ONE,
        interaction_of(
            12,
            4,
            Some(json!({ "id": "9", "name": "nope", "type": 1, "options": [] })),
        ),
    );
    h.push_on(
        ShardId::ONE,
        interaction_of(
            13,
            3,
            Some(json!({ "custom_id": "0nope:1", "component_type": 2 })),
        ),
    );
    h.push_on(
        ShardId::ONE,
        interaction_of(
            14,
            5,
            Some(json!({ "custom_id": "0nope:1", "components": [] })),
        ),
    );
    h.push_on(ShardId::ONE, interaction_of(15, 1, None));
    until("five run ends", || seen.lock().unwrap().len() == 5).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut ids = seen.lock().unwrap().clone();
    ids.sort_unstable();
    assert_eq!(ids, [11, 12, 13, 14, 15]);
    h.close().await;
}

/// Story 8.4: the observer still fires when the run's task panics.
#[tokio::test]
async fn run_observer_fires_on_a_panicking_run() {
    let seen: Arc<Mutex<Vec<u64>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let end = RunEnd {
        observer: Arc::new(move |id| sink.lock().unwrap().push(id.get())),
        id: Id::new(21),
    };
    let task = tokio::spawn(observe(
        async {
            panic!("pipeline panicked");
        },
        Some(end),
    ));
    assert!(task.await.is_err());
    assert_eq!(*seen.lock().unwrap(), [21]);
}
