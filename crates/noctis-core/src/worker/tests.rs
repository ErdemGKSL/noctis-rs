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
    assert_eq!(user_update(1).guild_id(), None);
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
    let xp = Module::new("profile/xp")
        .__add(probe)
        .__add(noop::<MessageDelete>("x"));
    let h = Harness::start(
        App::builder()
            .state(7_u32)
            .module(Module::new("profile").module(xp)),
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
                shards: 0..1,
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

mod guild_locales {
    //! Story 11.3 rows 20-24, 27 and 44: the guild record and event
    //! resolution through the worker set.

    use noctis_i18n::Catalog;
    use noctis_rules::Locale;

    use super::*;
    use crate::test_support::{guild_create, guild_create_unavailable, guild_delete, guild_update};
    use crate::{BeforeCtx, Flow, Middleware};

    /// en-US (default) and tr files.
    static EN_TR: Catalog = Catalog::__new(
        "bot/en_tr",
        Locale::EnUs,
        &[(Locale::EnUs, &[]), (Locale::Tr, &[])],
        &[],
    );
    /// en-US (default), de and tr files.
    static EN_DE_TR: Catalog = Catalog::__new(
        "bot/en_de_tr",
        Locale::EnUs,
        &[(Locale::De, &[]), (Locale::EnUs, &[]), (Locale::Tr, &[])],
        &[],
    );

    /// A `MESSAGE_CREATE` handler logging `guild_locale()` and the event's
    /// guild.
    fn recorder(log: &Log) -> EventHandler {
        let log = log.clone();
        handler("h", false, move |ctx: EventCtx, e: MessageCreate| {
            let log = log.clone();
            async move {
                log.push(format!(
                    "{:?}:{}",
                    e.guild_id.map(Id::get),
                    ctx.guild_locale()
                ));
                Ok(())
            }
        })
    }

    /// A `MESSAGE_CREATE` without a guild.
    fn dm_message() -> Event {
        let Event::MessageCreate(mut message) = message_create(1) else {
            unreachable!()
        };
        message.0.guild_id = None;
        Event::MessageCreate(message)
    }

    /// Pushes `events`, then one `MESSAGE_CREATE` per guild (`None`: a
    /// DM), and returns the logged lines.
    async fn run(
        catalog: Catalog,
        module: Module,
        events: Vec<Event>,
        guilds: &[Option<u64>],
        log: &Log,
    ) -> Vec<String> {
        let h = Harness::start(App::builder().locales(catalog).module(module));
        for event in events {
            h.push(event);
        }
        for guild in guilds {
            h.push(guild.map_or_else(dm_message, message_create));
        }
        until("every message", || log.lines().len() == guilds.len()).await;
        h.close().await;
        log.lines()
    }

    /// Rows 20 and 21: a recorded guild's locale; no record or a DM is the
    /// default.
    #[tokio::test]
    async fn event_reads_the_guild_record() {
        let log = Log::default();
        let module = Module::new("m").__add(recorder(&log));
        let lines = run(
            EN_TR,
            module,
            vec![guild_create(9, "tr")],
            &[Some(9), Some(8), None],
            &log,
        )
        .await;
        assert_eq!(lines, ["Some(9):tr", "Some(8):en-US", "None:en-US"]);
    }

    /// Row 22: `GUILD_UPDATE` to a code without a file is the default; with
    /// a file it is that code.
    #[tokio::test]
    async fn guild_update_changes_the_record() {
        let events = || vec![guild_create(9, "tr"), guild_update(9, "de")];
        let log = Log::default();
        let lines = run(
            EN_TR,
            Module::new("m").__add(recorder(&log)),
            events(),
            &[Some(9)],
            &log,
        )
        .await;
        assert_eq!(lines, ["Some(9):en-US"]);
        let log = Log::default();
        let lines = run(
            EN_DE_TR,
            Module::new("m").__add(recorder(&log)),
            events(),
            &[Some(9)],
            &log,
        )
        .await;
        assert_eq!(lines, ["Some(9):de"]);
    }

    /// Rows 23 and 24: an outage keeps the record, a removal drops it; an
    /// unavailable or unparseable create records nothing.
    #[tokio::test]
    async fn deletes_and_unavailable_creates() {
        let log = Log::default();
        let events = vec![
            guild_create(9, "tr"),
            guild_delete(9, Some(true)),
            guild_create(10, "tr"),
            guild_delete(10, None),
            guild_create_unavailable(11),
            guild_create(12, "en-us"),
        ];
        let lines = run(
            EN_TR,
            Module::new("m").__add(recorder(&log)),
            events,
            &[Some(9), Some(10), Some(11), Some(12)],
            &log,
        )
        .await;
        assert_eq!(
            lines,
            [
                "Some(9):tr",
                "Some(10):en-US",
                "Some(11):en-US",
                "Some(12):en-US"
            ]
        );
    }

    /// Unhandled `GUILD_CREATE`s update the record without waiting on a
    /// permit: with the only permit held, a second create after the first
    /// is still observed.
    #[tokio::test]
    async fn unhandled_guild_create_skips_the_permit() {
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut builder = App::builder();
        builder
            .handler_concurrency(1)
            .locales(EN_TR)
            .module(Module::new("m").__add({
                let (log, gate) = (log.clone(), Arc::clone(&gate));
                handler("h", false, move |_, _: MessageCreate| {
                    let (log, gate) = (log.clone(), Arc::clone(&gate));
                    async move {
                        log.push("start");
                        let _permit = gate.acquire().await;
                        Ok(())
                    }
                })
            }));
        let h = Harness::start(&mut builder);
        h.push(message_create(1));
        until("the handler holds the permit", || !log.lines().is_empty()).await;
        h.push(guild_create(9, "tr"));
        h.push(guild_create(10, "tr"));
        let locales = &h.app.shared().locales;
        until("both records", || locales.recorded() == 2).await;
        assert_eq!(locales.guild(Some(Id::new(10))), Some(Locale::Tr));
        gate.add_permits(1);
        h.close().await;
    }

    /// Row 44: an event before-Middleware overrides the Guild Locale.
    #[tokio::test]
    async fn event_middleware_overrides() {
        let log = Log::default();
        let module = Module::new("m")
            .before(Middleware::before(|ctx: BeforeCtx| async move {
                assert_eq!(ctx.guild_locale(), Locale::EnUs);
                ctx.set_guild_locale(Locale::Tr);
                Ok(Flow::Continue)
            }))
            .__add(recorder(&log));
        let lines = run(EN_TR, module, Vec::new(), &[Some(8)], &log).await;
        assert_eq!(lines, ["Some(8):tr"]);
    }

    /// Row 32 (events): the resolver runs once per event handler run and
    /// sees the record.
    #[tokio::test]
    async fn resolver_once_per_event_run() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let log = Log::default();
        let mut builder = App::builder();
        builder
            .locales(EN_TR)
            .locale_resolver(move |r: &crate::LocaleRequest| {
                seen.lock()
                    .unwrap()
                    .push((r.invocation, r.discord_user, r.discord_guild));
                r.resolved
            })
            .module(Module::new("m").__add(recorder(&log)));
        let h = Harness::start(&mut builder);
        h.push(guild_create(9, "tr"));
        h.push(message_create(9));
        until("the message", || log.lines().len() == 1).await;
        h.close().await;
        assert_eq!(
            *calls.lock().unwrap(),
            [(
                Invocation::Event {
                    event_type: "MESSAGE_CREATE"
                },
                None,
                Some(Locale::Tr)
            )]
        );
    }
}

/// Story 15.2: an event from a guild the App's `GuildAccessList` refuses
/// runs none of its handlers (ordered or concurrent), frees its permit and
/// is one `trace`; guildless events and served guilds run.
#[tokio::test]
async fn guild_not_served_runs_no_handler() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
    tracing::callsite::rebuild_interest_cache();
    let log = Log::default();
    let h = Harness::start(
        App::builder()
            .handler_concurrency(1)
            .guild_access(noctis_ports::memory::GuildAccess::deny([Id::new(GA)]))
            .module(
                Module::new("m")
                    .__add(logging("c", false, &log))
                    .__add(logging("o", true, &log)),
            ),
    );
    h.push(message_delete(7, Some(GA)));
    h.push(message_delete(8, None));
    h.push(message_delete(9, Some(GB)));
    until("served events", || log.lines().len() == 4).await;
    assert_eq!(log.count("c:7") + log.count("o:7"), 0);
    for line in ["c:8", "o:8", "c:9", "o:9"] {
        assert_eq!(log.count(line), 1, "{line}");
    }
    assert_eq!(
        capture
            .lines_with(&["TRACE", "noctis::dispatch", "guild not served"])
            .len(),
        1,
        "{}",
        capture.text()
    );
    h.close().await;
}

/// Story 16.2: one drain per lane partition and on-demand per-guild
/// ordered FIFOs.
mod partitions {
    use std::num::NonZeroUsize;

    use super::*;

    /// A drain over `n` lane partitions whose worker set and shutdown
    /// controls the test holds.
    struct Multi {
        _app: App,
        sender: Option<LaneSender>,
        workers: Arc<Workers>,
        controls: Shutdown,
        drain: Option<AbortOnDrop>,
    }

    impl Multi {
        fn start(builder: &mut AppBuilder, partitions: usize) -> Self {
            builder.runtime(&Runtime::for_test());
            let app = builder.build().unwrap();
            let controls = Shutdown::default();
            let (sender, receiver) = noctis_gateway::lanes_with(
                app.lane_config(),
                NonZeroUsize::new(partitions).unwrap(),
            );
            let workers = Arc::new(app.workers(controls.clone()));
            let drain = AbortOnDrop(tokio::spawn(drain_shared(receiver, Arc::clone(&workers))));
            Self {
                _app: app,
                sender: Some(sender),
                workers,
                controls,
                drain: Some(drain),
            }
        }

        fn push_on(&self, shard: u32, kind: InboundKind) {
            self.sender.as_ref().unwrap().push(Inbound {
                shard: ShardId::new(shard, 16),
                received_at: Instant::now(),
                kind,
            });
        }

        fn push(&self, shard: u32, event: Event) {
            self.push_on(shard, InboundKind::Event(Box::new(event)));
        }

        /// Waits for the drain to return (after the senders dropped or the
        /// shutdown fired).
        async fn join(&mut self) {
            let drain = self.drain.take().unwrap();
            let abort = drain.0.abort_handle();
            until("the drain returned", || abort.is_finished()).await;
            drop(drain);
        }
    }

    fn ping(id: u64) -> InboundKind {
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
        InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw)
    }

    /// The 16.1-era sub-queue hash, to pick guild ids that used to share a
    /// sub-queue.
    fn old_subqueue(id: u64) -> u64 {
        id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58
    }

    #[test]
    fn lane_partitions_follow_the_runtime() {
        assert_eq!(crate::app::lane_partitions().get(), 1);
        for (workers, expected) in [(1, 1), (3, 3), (70, 64)] {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .build()
                .unwrap();
            let got = runtime.block_on(async { crate::app::lane_partitions().get() });
            assert_eq!(got, expected, "{workers} workers");
        }
        let current = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(
            current.block_on(async { crate::app::lane_partitions().get() }),
            1
        );
    }

    /// `run` splits its lanes into one partition per runtime worker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn run_lanes_use_lane_partitions() {
        let mut builder = App::builder();
        builder.runtime(&Runtime::for_test());
        let app = builder.build().unwrap();
        assert_eq!(app.run_lanes().1.into_partitions().len(), 4);
    }

    /// Interactions still in a drain's local batch at cancel count as
    /// interactions; batched, queued and waiting events as events.
    #[test]
    fn discarded_counts_batched_interactions() {
        let (sender, receiver) =
            noctis_gateway::lanes_with(LaneConfig::default(), NonZeroUsize::MIN);
        let partition = receiver.into_partitions().remove(0);
        let inbound = |kind| Inbound {
            shard: ShardId::new(0, 16),
            received_at: Instant::now(),
            kind,
        };
        sender.push(inbound(InboundKind::Event(Box::new(message_delete(
            1, None,
        )))));
        sender.push(inbound(ping(1)));
        let batch = VecDeque::from([
            inbound(ping(2)),
            inbound(InboundKind::Event(Box::new(message_delete(2, None)))),
            inbound(ping(3)),
        ]);
        let discarded = Discarded::of(&partition, &batch, 1);
        assert_eq!(discarded.interactions, 3);
        assert_eq!(discarded.events, 3);
    }

    /// Two guilds that shared a 16.1 sub-queue no longer block each other.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn colliding_guilds_are_isolated() {
        let collide = (2..u64::MAX)
            .find(|id| old_subqueue(*id) == old_subqueue(GA))
            .unwrap();
        let log = Log::default();
        let gate = Arc::new(Notify::new());
        let m = Multi::start(
            App::builder().module(Module::new("m").__add(ordered_logger(&log, &gate, Some(GA)))),
            4,
        );
        m.push(0, message_delete(5, Some(GA)));
        until("GA started", || log.count("start:5") == 1).await;
        m.push(0, message_delete(6, Some(collide)));
        m.push(1, message_delete(7, Some(collide)));
        until("collider handled", || {
            log.count("end:6") == 1 && log.count("end:7") == 1
        })
        .await;
        assert_eq!(log.count("end:5"), 0);
        gate.notify_one();
        until("GA released", || log.count("end:5") == 1).await;
        until("map empty", || m.workers.subqueues() == 0).await;
    }

    /// Ordered handlers of one guild start in arrival order, each after the
    /// previous ended, while other shards' partitions run beside it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_guild_order_across_partitions() {
        let log = Log::default();
        let o = {
            let log = log.clone();
            handler("o", true, move |ctx, e: MessageDelete| {
                let log = log.clone();
                async move {
                    log.push(format!("start:{}:{}", ctx.guild_id().unwrap(), e.id));
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    log.push(format!("end:{}:{}", ctx.guild_id().unwrap(), e.id));
                    Ok(())
                }
            })
        };
        let m = Multi::start(App::builder().module(Module::new("m").__add(o)), 4);
        for id in 1..=20 {
            for shard in 0..4_u32 {
                let guild = u64::from(shard) + 10;
                m.push(shard, message_delete(id, Some(guild)));
            }
        }
        until("all ended", || {
            log.lines().iter().filter(|l| l.starts_with("end:")).count() == 80
        })
        .await;
        let lines = log.lines();
        for guild in 10..14 {
            let own: Vec<_> = lines
                .iter()
                .filter(|l| l.contains(&format!(":{guild}:")))
                .cloned()
                .collect();
            let expected: Vec<_> = (1..=20)
                .flat_map(|id| [format!("start:{guild}:{id}"), format!("end:{guild}:{id}")])
                .collect();
            assert_eq!(own, expected);
        }
        until("map empty", || m.workers.subqueues() == 0).await;
    }

    /// The per-guild map holds at most `handler_concurrency` guilds and is
    /// empty, with no runner left, once the work is done.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn ordered_map_is_bounded() {
        let started = Arc::new(AtomicUsize::new(0));
        let ended = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let o = {
            let (started, ended, release) = (
                Arc::clone(&started),
                Arc::clone(&ended),
                Arc::clone(&release),
            );
            handler("o", true, move |_, _: MessageDelete| {
                let (started, ended, release) = (
                    Arc::clone(&started),
                    Arc::clone(&ended),
                    Arc::clone(&release),
                );
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    release.acquire().await?.forget();
                    ended.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
        };
        let m = Multi::start(
            App::builder()
                .handler_concurrency(3)
                .module(Module::new("m").__add(o)),
            4,
        );
        for guild in 1..=10_u64 {
            m.push(
                u32::try_from(guild % 4).unwrap(),
                message_delete(guild + 100, Some(guild)),
            );
        }
        until("three started", || started.load(Ordering::SeqCst) == 3).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(started.load(Ordering::SeqCst), 3);
        assert_eq!(m.workers.subqueues(), 3);
        release.add_permits(10);
        until("all ended", || ended.load(Ordering::SeqCst) == 10).await;
        until("map empty", || m.workers.subqueues() == 0).await;
        until("no runner", || m.controls.tracker.is_empty()).await;
    }

    /// An interaction queued behind a 10 000-event flood is handed to its
    /// pipeline before the event after it starts.
    #[tokio::test(flavor = "current_thread")]
    async fn interaction_before_later_event_under_flood() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let seen = Arc::new(Mutex::new(None));
        let h = {
            let (seen, capture) = (Arc::clone(&seen), capture.clone());
            handler("h", false, move |_, e: MessageDelete| {
                let (seen, capture) = (Arc::clone(&seen), capture.clone());
                async move {
                    if e.id.get() == 99_999 {
                        let traced = !capture
                            .lines_with(&["no handler registered", "kind=Interaction"])
                            .is_empty();
                        *seen.lock().unwrap() = Some(traced);
                    }
                    Ok(())
                }
            })
        };
        let m = Multi::start(App::builder().module(Module::new("m").__add(h)), 4);
        for id in 1..=10_000 {
            m.push(2, user_update(id));
        }
        m.push_on(2, ping(9));
        m.push(2, message_delete(99_999, Some(GA)));
        until("E started", || seen.lock().unwrap().is_some()).await;
        assert_eq!(*seen.lock().unwrap(), Some(true));
    }

    /// The drain's guild check: pushes `ping(9)` on shard 0 when guild 1
    /// is checked.
    struct PushOnCheck(Mutex<Option<LaneSender>>);

    impl noctis_ports::GuildAccessList for PushOnCheck {
        fn allows(&self, guild: Id<GuildMarker>) -> bool {
            if guild.get() == 1
                && let Some(sender) = self.0.lock().unwrap().take()
            {
                sender.push(Inbound {
                    shard: ShardId::new(0, 16),
                    received_at: Instant::now(),
                    kind: ping(9),
                });
            }
            true
        }
    }

    /// An interaction pushed while the drain handles a batch's handled
    /// event is served before the batch's later events.
    #[tokio::test(flavor = "current_thread")]
    async fn interaction_before_later_batched_event() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let access = Arc::new(PushOnCheck(Mutex::new(None)));
        let mut m = Multi::start(
            App::builder()
                .guild_access(Arc::clone(&access))
                .module(Module::new("m").__add(noop::<MessageDelete>("h"))),
            1,
        );
        m.push(0, message_delete(1, Some(1)));
        m.push(0, user_update(2));
        m.push(0, user_update(3));
        *access.0.lock().unwrap() = m.sender.take();
        m.join().await;
        let text = capture.text();
        let at = |needle: &str| {
            text.lines()
                .position(|line| line.contains("no handler registered") && line.contains(needle))
        };
        let interaction = at("kind=Interaction").expect("the interaction was traced");
        let later = at("kind=UserUpdate").expect("the later events were traced");
        assert!(interaction < later, "{text}");
    }

    /// At most `handler_concurrency` events hold permits with 4 drains.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrency_bound_with_partitions() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicUsize::new(0));
        let h = {
            let (in_flight, max, done) =
                (Arc::clone(&in_flight), Arc::clone(&max), Arc::clone(&done));
            handler("h", false, move |_, _: MessageDelete| {
                let (in_flight, max, done) =
                    (Arc::clone(&in_flight), Arc::clone(&max), Arc::clone(&done));
                async move {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    done.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
        };
        let m = Multi::start(
            App::builder()
                .handler_concurrency(2)
                .module(Module::new("m").__add(h)),
            4,
        );
        for id in 1..=10_u64 {
            m.push(u32::try_from(id % 4).unwrap(), message_delete(id, Some(id)));
        }
        until("all ten", || done.load(Ordering::SeqCst) == 10).await;
        assert_eq!(max.load(Ordering::SeqCst), 2);
    }

    /// `shutdown` drains every partition until empty.
    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_drains_all_partitions() {
        let log = Log::default();
        let mut m = Multi::start(
            App::builder().module(Module::new("m").__add(logging("h", false, &log))),
            4,
        );
        for id in 1..=12_u64 {
            m.push(u32::try_from(id % 4).unwrap(), message_delete(id, Some(id)));
        }
        m.controls.shutdown.cancel();
        m.join().await;
        until("every item", || log.lines().len() == 12).await;
        for id in 1..=12 {
            assert_eq!(log.count(&format!("h:{id}")), 1);
        }
    }

    /// `cancel` discards the queued items of every partition with one
    /// summary warning.
    #[tokio::test(flavor = "current_thread")]
    async fn cancel_warns_once_with_totals() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let mut m = Multi::start(
            App::builder().module(Module::new("m").__add(logging("h", false, &log))),
            4,
        );
        for id in 1..=8_u64 {
            m.push(u32::try_from(id % 4).unwrap(), message_delete(id, Some(id)));
        }
        m.push_on(3, ping(1));
        m.controls.shutdown.cancel();
        m.controls.cancel.cancel();
        m.join().await;
        assert!(log.lines().is_empty());
        let lines = capture.lines_with(&["lanes discarded at shutdown"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("WARN"), "{}", lines[0]);
        assert!(lines[0].contains("interactions=1"), "{}", lines[0]);
        assert!(lines[0].contains("events=8"), "{}", lines[0]);
    }

    /// Batched and permit-waiting events of several partitions count in the
    /// one summary.
    #[tokio::test(flavor = "current_thread")]
    async fn cancel_counts_batched_and_waiting() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let gate = Arc::new(Semaphore::new(0));
        let mut m = Multi::start(
            App::builder()
                .handler_concurrency(1)
                .module(Module::new("ops").__add(shutdown_gated(&log, &gate))),
            4,
        );
        m.push(0, message_delete(1, Some(GA)));
        until("first started", || log.count("start:1") == 1).await;
        // Partition 0: 2 waits for a permit, 3 stays batched; partitions 1
        // and 2 each wait for a permit.
        m.push(0, message_delete(2, Some(GA)));
        m.push(0, message_delete(3, Some(GA)));
        m.push(1, message_delete(4, Some(GB)));
        m.push(2, message_delete(5, Some(GB)));
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        m.controls.shutdown.cancel();
        m.controls.cancel.cancel();
        m.join().await;
        let lines = capture.lines_with(&["lanes discarded at shutdown"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("interactions=0"), "{}", lines[0]);
        assert!(lines[0].contains("events=4"), "{}", lines[0]);
        assert_eq!(log.count("start:2"), 0);
    }

    fn shutdown_gated(log: &Log, gate: &Arc<Semaphore>) -> EventHandler {
        let log = log.clone();
        let gate = Arc::clone(gate);
        handler("b", false, move |_, e: MessageDelete| {
            let log = log.clone();
            let gate = Arc::clone(&gate);
            async move {
                log.push(format!("start:{}", e.id));
                let _permit = gate.acquire().await;
                Ok(())
            }
        })
    }

    /// Dropping the supervisor's `AbortOnDrop` aborts every partition drain.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn abort_aborts_partition_drains() {
        let mut m = Multi::start(&mut App::builder(), 4);
        // The test, `drain_shared` and the four partition drains.
        until("drains started", || Arc::strong_count(&m.workers) == 6).await;
        drop(m.drain.take());
        until("drains aborted", || Arc::strong_count(&m.workers) == 1).await;
        drop(m.sender.take());
    }
}

// ---- Story 16.3: per-event and per-handler overhead ----

mod overhead {
    use std::sync::atomic::AtomicU64;

    use metrics::{
        Counter, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder, SharedString,
        Unit,
    };
    use noctis_i18n::Catalog;
    use noctis_rules::Locale;
    use tokio::sync::Barrier;
    use twilight_model::gateway::event::EventType;

    use super::*;
    use crate::test_support::{Recording, guild_create, guild_update};
    use crate::{BeforeCtx, ErrorOrigin, Flow, Middleware};

    /// A value that counts its clones and knows whether it is the
    /// original.
    struct Tracked {
        clones: Arc<AtomicUsize>,
        original: bool,
    }

    impl Clone for Tracked {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::SeqCst);
            Self {
                clones: Arc::clone(&self.clones),
                original: false,
            }
        }
    }

    /// What each handler gets: `"orig"`, `"clone"` or `"-"` (nothing).
    fn label(value: Option<&Tracked>) -> &'static str {
        match value {
            Some(t) if t.original => "orig",
            Some(_) => "clone",
            None => "-",
        }
    }

    /// Shares one value among handlers with filters `accepts`; what each
    /// gets and the clone count.
    fn share(accepts: &[bool]) -> (Vec<&'static str>, usize) {
        let clones = Arc::new(AtomicUsize::new(0));
        let value = Tracked {
            clones: Arc::clone(&clones),
            original: true,
        };
        let mut share = Share::new(Some(value), accepts.len(), |i, _| accepts[i]);
        let got = accepts
            .iter()
            .enumerate()
            .map(|(i, &a)| label(share.take(i, |_| a).as_ref()))
            .collect();
        (got, clones.load(Ordering::SeqCst))
    }

    /// The Share rows of the Matrix: the last accepting handler gets the
    /// original, earlier accepting ones a clone, rejecting ones nothing.
    #[test]
    fn share_rows() {
        assert_eq!(share(&[]), (vec![], 0));
        assert_eq!(share(&[true]), (vec!["orig"], 0));
        assert_eq!(
            share(&[true, true, true]),
            (vec!["clone", "clone", "orig"], 2)
        );
        assert_eq!(share(&[false, true]), (vec!["-", "orig"], 0));
        assert_eq!(share(&[true, false, true]), (vec!["clone", "-", "orig"], 1));
        assert_eq!(share(&[true, false]), (vec!["orig", "-"], 0));
        assert_eq!(share(&[false, false]), (vec!["-", "-"], 0));
    }

    /// The ordered + concurrent rows: the ordered part shares over every
    /// handler, the concurrent job carries the rest.
    fn split(ordered: &[bool], concurrent: &[bool]) -> (Vec<&'static str>, usize) {
        let clones = Arc::new(AtomicUsize::new(0));
        let value = Tracked {
            clones: Arc::clone(&clones),
            original: true,
        };
        let all: Vec<bool> = ordered.iter().chain(concurrent).copied().collect();
        let mut share = Share::new(Some(value), all.len(), |i, _| all[i]);
        let mut got: Vec<_> = ordered
            .iter()
            .enumerate()
            .map(|(i, &a)| label(share.take(i, |_| a).as_ref()))
            .collect();
        let mut rest = Share::new(share.into_rest(), concurrent.len(), |i, _| concurrent[i]);
        got.extend(
            concurrent
                .iter()
                .enumerate()
                .map(|(i, &a)| label(rest.take(i, |_| a).as_ref())),
        );
        (got, clones.load(Ordering::SeqCst))
    }

    #[test]
    fn share_ordered_then_concurrent() {
        assert_eq!(split(&[true], &[false]), (vec!["orig", "-"], 0));
        assert_eq!(
            split(&[false], &[true, true]),
            (vec!["-", "clone", "orig"], 1)
        );
        assert_eq!(
            split(&[true], &[true, false]),
            (vec!["clone", "orig", "-"], 1)
        );
    }

    /// An `UnavailableGuild` and a `GuildDelete` handler.
    fn delete_handlers(log: &Log) -> (EventHandler, EventHandler) {
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
                    log.push(format!("deleted:{}", e.id));
                    Ok(())
                }
            })
        };
        (outage, deleted)
    }

    /// Acceptance 2: a plain `GUILD_DELETE` is never cloned (the
    /// `GuildDelete` handler gets the original) and the `UnavailableGuild`
    /// run still passes its stages without a call.
    #[tokio::test]
    async fn rejected_handler_gets_no_clone() {
        let recording = Arc::new(Recording::default());
        let log = Log::default();
        let (outage, deleted) = delete_handlers(&log);
        let h = Harness::start(
            App::builder()
                .error_sink(ErrorOrigin::Event, Arc::clone(&recording))
                .module(Module::new("m").__add(outage).__add(deleted)),
        );
        let entries = &h
            .app
            .registry()
            .handlers(EventType::GuildDelete)
            .unwrap()
            .concurrent;
        let clones = Arc::new(AtomicUsize::new(0));
        let event = guild_delete(None);
        let tracked = Tracked {
            clones: Arc::clone(&clones),
            original: true,
        };
        let mut share = Share::new(Some(tracked), entries.len(), |i, _| {
            entries[i].handler.accepts(&event)
        });
        let got: Vec<_> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| label(share.take(i, |_| entry.handler.accepts(&event)).as_ref()))
            .collect();
        assert_eq!((got, clones.load(Ordering::SeqCst)), (vec!["-", "orig"], 0));

        // A rejected run under a fired cancel token still reports
        // `Cancelled`; without it, it ends silently.
        let app = Arc::clone(h.app.shared());
        let cancel = CancellationToken::new();
        run_entry(&entries[0], 0, Some(Id::new(GA)), None, &app, &cancel).await;
        assert!(recording.seen().is_empty());
        cancel.cancel();
        run_entry(&entries[0], 0, Some(Id::new(GA)), None, &app, &cancel).await;
        let seen = recording.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].kind, "cancelled");
        assert_eq!(seen[0].handler.as_deref(), Some("outage"));

        h.push(guild_delete(None));
        until("deleted", || log.count("deleted:1") == 1).await;
        h.close().await;
        assert_eq!(log.lines(), ["deleted:1"]);
    }

    /// A rejected handler whose Module is disabled still traces "module
    /// disabled here" in its span, like an accepting one.
    #[tokio::test]
    async fn rejected_handler_keeps_stage_3_trace() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        let log = Log::default();
        let (outage, deleted) = delete_handlers(&log);
        let h =
            Harness::start(App::builder().module(Module::new("m").__add(outage).__add(deleted)));
        h.app
            .modules()
            .set_enabled("m", Id::new(GA), false)
            .await
            .unwrap();
        tracing::callsite::rebuild_interest_cache();
        h.push(guild_delete(None));
        until("both traces", || {
            capture.lines_with(&["module disabled here"]).len() == 2
        })
        .await;
        h.close().await;
        assert_eq!(
            capture
                .lines_with(&["module disabled here", "handler=\"outage\""])
                .len(),
            1
        );
        assert_eq!(
            capture
                .lines_with(&["module disabled here", "handler=\"deleted\""])
                .len(),
            1
        );
        assert!(log.lines().is_empty());
    }

    /// The last accepting handler's Module is disabled: the earlier one
    /// runs on its clone, the event is dropped with the disabled run.
    #[tokio::test]
    async fn last_acceptor_disabled() {
        let log = Log::default();
        let h = Harness::start(
            App::builder()
                .module(Module::new("on").__add(logging("a", false, &log)))
                .module(Module::new("off").__add(logging("b", false, &log))),
        );
        h.app
            .modules()
            .set_enabled("off", Id::new(GA), false)
            .await
            .unwrap();
        h.push(message_delete(5, Some(GA)));
        until("a", || log.count("a:5") == 1).await;
        h.close().await;
        assert_eq!(log.lines(), ["a:5"]);
    }

    /// `count` concurrent handlers on one event all start before any ends
    /// (a barrier of `count`).
    async fn all_start(count: usize) {
        let barrier = Arc::new(Barrier::new(count));
        let passed = Log::default();
        let waiting = {
            let (barrier, passed) = (Arc::clone(&barrier), passed.clone());
            handler("wait", false, move |_, _: MessageDelete| {
                let (barrier, passed) = (Arc::clone(&barrier), passed.clone());
                async move {
                    barrier.wait().await;
                    passed.push("passed");
                    Ok(())
                }
            })
        };
        let mut module = Module::new("m");
        for _ in 0..count {
            module = module.__add(waiting.clone());
        }
        let mut builder = App::builder();
        builder.handler_concurrency(1).module(module);
        let h = Harness::start(&mut builder);
        h.push(message_delete(1, Some(GA)));
        until("every handler", || passed.count("passed") == count).await;
        h.close().await;
    }

    /// One handler (inline), two and 31 (`join_all` switches to a
    /// `FuturesOrdered` above 30): every handler runs, all at once.
    #[tokio::test]
    async fn concurrent_counts_all_start() {
        all_start(1).await;
        all_start(2).await;
        all_start(31).await;
    }

    /// Counts registrations and records into atomics.
    #[derive(Default)]
    struct Registrations {
        histograms: Mutex<Vec<Key>>,
        counters: Mutex<Vec<Key>>,
        samples: Arc<Samples>,
        errors: Arc<AtomicU64>,
    }

    #[derive(Default)]
    struct Samples(AtomicUsize);

    impl HistogramFn for Samples {
        fn record(&self, _: f64) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Registrations {
        fn histograms(&self) -> Vec<Key> {
            self.histograms.lock().unwrap().clone()
        }

        fn counters(&self) -> Vec<Key> {
            self.counters.lock().unwrap().clone()
        }

        fn samples(&self) -> usize {
            self.samples.0.load(Ordering::SeqCst)
        }

        fn errors(&self) -> u64 {
            self.errors.load(Ordering::SeqCst)
        }
    }

    impl Recorder for Registrations {
        fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

        fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

        fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

        fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
            self.counters.lock().unwrap().push(key.clone());
            if key.name() == "noctis_handler_errors_total" {
                Counter::from_arc(Arc::clone(&self.errors))
            } else {
                Counter::noop()
            }
        }

        fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
            Gauge::noop()
        }

        fn register_histogram(&self, key: &Key, _: &Metadata<'_>) -> Histogram {
            self.histograms.lock().unwrap().push(key.clone());
            Histogram::from_arc(Arc::clone(&self.samples))
        }
    }

    /// `key`'s labels, sorted.
    fn labels(key: &Key) -> Vec<(String, String)> {
        let mut labels: Vec<_> = key
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect();
        labels.sort();
        labels
    }

    /// Acceptance 1: 100 runs with 3 errors and 2 panics register the
    /// duration histogram once and the error counter once, with 100
    /// samples and 5 errors.
    #[tokio::test]
    async fn handler_metrics_register_once() {
        let recorder = Registrations::default();
        let _metrics = metrics::set_default_local_recorder(&recorder);
        let mixed = handler("bot::mixed", false, |_, e: MessageDelete| async move {
            match e.id.get() {
                1..=3 => Err(HandlerError::new(std::io::Error::other("x"))),
                4 | 5 => panic!("boom"),
                _ => Ok(()),
            }
        });
        let mut builder = App::builder();
        builder.app_id("a").module(Module::new("m").__add(mixed));
        let h = Harness::start(&mut builder);
        for n in 1..=100 {
            h.push(message_delete(n, Some(GA)));
        }
        until("every run", || {
            recorder.samples() == 100 && recorder.errors() == 5
        })
        .await;
        h.close().await;
        let histograms = recorder.histograms();
        assert_eq!(histograms.len(), 1, "{histograms:?}");
        assert_eq!(histograms[0].name(), "noctis_handler_duration_seconds");
        let expected = [
            ("app".to_owned(), "a".to_owned()),
            ("kind".to_owned(), "event".to_owned()),
        ];
        assert_eq!(labels(&histograms[0]), expected);
        let errors: Vec<_> = recorder
            .counters()
            .into_iter()
            .filter(|k| k.name() == "noctis_handler_errors_total")
            .collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(labels(&errors[0]), expected);
    }

    /// Two Apps register one handle each, under their own `app` label.
    #[tokio::test]
    async fn two_apps_two_handles() {
        let recorder = Registrations::default();
        let _metrics = metrics::set_default_local_recorder(&recorder);
        for (app, n) in [("a", 1), ("b", 2)] {
            let log = Log::default();
            let mut builder = App::builder();
            builder
                .app_id(app)
                .module(Module::new("m").__add(logging("h", false, &log)));
            let h = Harness::start(&mut builder);
            for _ in 0..3 {
                h.push(message_delete(9, Some(GA)));
            }
            until("three runs", || recorder.samples() == 3 * n).await;
            h.close().await;
        }
        let apps: Vec<_> = recorder
            .histograms()
            .iter()
            .map(|key| labels(key)[0].1.clone())
            .collect();
        assert_eq!(apps, ["a", "b"]);
        assert!(recorder.counters().is_empty());
    }

    /// en-US (default), de and tr files.
    static EN_DE_TR: Catalog = Catalog::__new(
        "bot/overhead",
        Locale::EnUs,
        &[(Locale::De, &[]), (Locale::EnUs, &[]), (Locale::Tr, &[])],
        &[],
    );

    /// A `MESSAGE_CREATE` handler keeping every context it gets.
    fn keeper(ctxs: &Arc<Mutex<Vec<EventCtx>>>) -> EventHandler {
        let ctxs = Arc::clone(ctxs);
        handler("keep", false, move |ctx: EventCtx, _: MessageCreate| {
            ctxs.lock().unwrap().push(ctx);
            async { Ok(()) }
        })
    }

    /// A `MESSAGE_CREATE` without a guild.
    fn dm() -> Event {
        let Event::MessageCreate(mut message) = message_create(1) else {
            unreachable!()
        };
        message.0.guild_id = None;
        Event::MessageCreate(message)
    }

    /// Pushes `event` and waits for the `n`th kept context.
    async fn kept(
        h: &Harness,
        ctxs: &Arc<Mutex<Vec<EventCtx>>>,
        event: Event,
        n: usize,
    ) -> EventCtx {
        h.push(event);
        until("the context", || ctxs.lock().unwrap().len() == n).await;
        ctxs.lock().unwrap()[n - 1].clone()
    }

    /// The locale reuse rows: one shared cell per guild record locale
    /// without a resolver or event Middleware.
    #[tokio::test]
    async fn locale_cells_are_reused() {
        let ctxs = Arc::new(Mutex::new(Vec::new()));
        let mut builder = App::builder();
        builder
            .locales(EN_DE_TR)
            .module(Module::new("m").__add(keeper(&ctxs)));
        let h = Harness::start(&mut builder);
        h.push(guild_create(9, "de"));
        let first = kept(&h, &ctxs, message_create(9), 1).await;
        let second = kept(&h, &ctxs, message_create(9), 2).await;
        assert!(Arc::ptr_eq(first.locale_cell(), second.locale_cell()));
        assert_eq!(first.guild_locale(), Locale::De);
        // The record changes: another cell.
        h.push(guild_update(9, "tr"));
        let third = kept(&h, &ctxs, message_create(9), 3).await;
        assert!(!Arc::ptr_eq(first.locale_cell(), third.locale_cell()));
        assert_eq!(third.guild_locale(), Locale::Tr);
        // Guildless and a guild without a record share the `None` slot.
        let guildless = kept(&h, &ctxs, dm(), 4).await;
        let unrecorded = kept(&h, &ctxs, message_create(8), 5).await;
        assert!(Arc::ptr_eq(
            guildless.locale_cell(),
            unrecorded.locale_cell()
        ));
        assert_eq!(guildless.guild_locale(), Locale::EnUs);
        // A record without a file: its own cell, the default locale.
        h.push(guild_create(10, "fr"));
        let fr = kept(&h, &ctxs, message_create(10), 6).await;
        let fr_again = kept(&h, &ctxs, message_create(10), 7).await;
        assert!(Arc::ptr_eq(fr.locale_cell(), fr_again.locale_cell()));
        assert_eq!(fr.guild_locale(), Locale::EnUs);
        h.close().await;
    }

    /// With event Middleware every run has its own cell: an override in
    /// run 1 never reaches run 2.
    #[tokio::test]
    async fn middleware_runs_get_their_own_cells() {
        let ctxs = Arc::new(Mutex::new(Vec::new()));
        let runs = Arc::new(AtomicUsize::new(0));
        let module = Module::new("m")
            .before(Middleware::before({
                let runs = Arc::clone(&runs);
                move |ctx: BeforeCtx| {
                    let first = runs.fetch_add(1, Ordering::SeqCst) == 0;
                    async move {
                        if first {
                            ctx.set_guild_locale(Locale::Tr);
                        }
                        Ok(Flow::Continue)
                    }
                }
            }))
            .__add(keeper(&ctxs));
        let mut builder = App::builder();
        builder.locales(EN_DE_TR).module(module);
        let h = Harness::start(&mut builder);
        let first = kept(&h, &ctxs, message_create(8), 1).await;
        let second = kept(&h, &ctxs, message_create(8), 2).await;
        h.close().await;
        assert!(!Arc::ptr_eq(first.locale_cell(), second.locale_cell()));
        assert_eq!(first.guild_locale(), Locale::Tr);
        assert_eq!(second.guild_locale(), Locale::EnUs);
    }

    /// With a `LocaleResolver` it runs once per run, each with its own
    /// cell.
    #[tokio::test]
    async fn resolver_runs_get_their_own_cells() {
        let ctxs = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mut builder = App::builder();
        builder
            .locales(EN_DE_TR)
            .locale_resolver(move |r: &crate::LocaleRequest| {
                seen.fetch_add(1, Ordering::SeqCst);
                r.resolved
            })
            .module(Module::new("m").__add(keeper(&ctxs)));
        let h = Harness::start(&mut builder);
        let mut runs = Vec::new();
        for n in 1..=3 {
            runs.push(kept(&h, &ctxs, message_create(8), n).await);
        }
        h.close().await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(!Arc::ptr_eq(runs[0].locale_cell(), runs[1].locale_cell()));
        assert!(!Arc::ptr_eq(runs[1].locale_cell(), runs[2].locale_cell()));
    }
}
