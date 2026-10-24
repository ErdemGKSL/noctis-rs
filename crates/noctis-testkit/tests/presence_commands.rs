//! Presence and gateway commands end to end (Story 15.3): an App against
//! `FakeDiscordHttp` and `FakeGateway`; IDENTIFY carries the presence and
//! `large_threshold`, `app.presence()` sends op 3 to every shard or one, a
//! re-identify restores the runtime presence, and Request Guild Members
//! (op 8) goes to the owning shard and is answered by chunks or
//! `RATE_LIMITED` (GW-07).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis::events::{EventCtx, MemberChunk};
use noctis::gateway::{
    GatewayError, GatewayHandle, GuildMembers, LargeThreshold, MemberRequest, Presence,
};
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::GuildMarker;
use noctis::{Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, GatewayBot, RecordedFrame, SessionStartLimit,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// What the handlers saw.
#[derive(Default)]
struct Probe(Mutex<Vec<String>>);

impl Probe {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(EventHandler)]
struct OnChunk;

impl OnChunk {
    #[handler]
    async fn handle(self, ctx: EventCtx, chunk: MemberChunk) -> HandlerResult<()> {
        if let Some(probe) = ctx.state::<Arc<Probe>>() {
            probe
                .0
                .lock()
                .unwrap()
                .push(format!("chunk:{}", chunk.nonce.unwrap_or_default()));
        }
        Ok(())
    }
}

/// A running App against both fakes.
struct Running {
    gw: FakeGateway,
    _http: FakeDiscordHttp,
    gateway: GatewayHandle,
    probe: Arc<Probe>,
    stop: Option<oneshot::Sender<()>>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    /// Runs `shards` of `total` (all at once: `max_concurrency` 16) with
    /// `configure` applied, and waits until every shard has a session.
    async fn start(
        total: u32,
        shards: Range<u32>,
        configure: impl FnOnce(&mut AppBuilder),
    ) -> Self {
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards: total,
            session_start_limit: SessionStartLimit {
                max_concurrency: 16,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        })
        .await
        .unwrap();
        let gw = FakeGateway::start().await.unwrap();
        let probe = Arc::new(Probe::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .shard_total(total)
            .shard_range(shards.clone())
            .state(Arc::clone(&probe));
        configure(&mut builder);
        let app = builder.build().unwrap();
        let gateway = app.gateway();
        assert_eq!(
            gateway.presence().set(Presence::online()),
            Err(GatewayError::NotRunning),
            "P10: not running before `run`"
        );
        let (stop, stopped) = oneshot::channel();
        let run = tokio::spawn(app.run_until(async {
            let _ = stopped.await;
        }));
        let running = Self {
            gw,
            _http: http,
            gateway,
            probe,
            stop: Some(stop),
            run,
        };
        running.wait_sessions(shards.len()).await;
        running
    }

    /// Waits until the fake holds `n` sessions, probing with an unhandled
    /// `TYPING_START`.
    async fn wait_sessions(&self, n: usize) {
        let deadline = Instant::now() + WAIT;
        while self.gw.dispatch("TYPING_START", json!({})) < n {
            assert!(Instant::now() < deadline, "fewer than {n} sessions");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Stops the App and waits for `run_until` to return.
    async fn stop(mut self) -> GatewayHandle {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(WAIT, &mut self.run)
            .await
            .expect("run_until returned")
            .unwrap()
            .unwrap();
        self.gateway
    }

    fn ops(&self, op: u64) -> Vec<RecordedFrame> {
        self.gw
            .frames()
            .into_iter()
            .filter(|f| f.op() == Some(op))
            .collect()
    }

    /// The shard id each connection identified as.
    fn shard_of(&self, connection: u64) -> Option<u64> {
        self.ops(2)
            .iter()
            .find(|f| f.connection == connection)
            .and_then(|f| f.json()?["d"]["shard"][0].as_u64())
    }

    /// Waits until `n` frames match `pred`; returns them.
    async fn wait_frames(
        &self,
        n: usize,
        pred: impl Fn(&RecordedFrame) -> bool,
    ) -> Vec<RecordedFrame> {
        let deadline = Instant::now() + WAIT;
        loop {
            let found: Vec<_> = self.gw.frames().into_iter().filter(|f| pred(f)).collect();
            if found.len() >= n {
                return found;
            }
            assert!(
                Instant::now() < deadline,
                "only {} of {n} frames",
                found.len()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Starts `request` and returns its task and the op 8 frame it sent.
    async fn request(
        &self,
        request: MemberRequest,
    ) -> (JoinHandle<Result<GuildMembers, GatewayError>>, Value) {
        let before = self.ops(8).len();
        let task = tokio::spawn(self.gateway.request_guild_members(request));
        let frames = self.wait_frames(before + 1, |f| f.op() == Some(8)).await;
        (task, frames[before].json().unwrap())
    }
}

/// `d.presence` of an IDENTIFY frame without twilight's `created_at: null`.
fn presence_of(frame: &RecordedFrame) -> Value {
    let mut presence = frame.json().unwrap()["d"]["presence"].clone();
    if let Some(activities) = presence["activities"].as_array_mut() {
        for activity in activities {
            let removed = activity.as_object_mut().unwrap().remove("created_at");
            assert!(removed.as_ref().is_none_or(Value::is_null), "{removed:?}");
        }
    }
    presence
}

fn member(id: u64) -> Value {
    json!({
        "deaf": false,
        "mute": false,
        "flags": 0,
        "joined_at": "2026-10-20T12:00:00.000000+00:00",
        "roles": [],
        "nick": null,
        "communication_disabled_until": null,
        "user": { "id": id.to_string(), "username": "u", "discriminator": "0", "avatar": null },
    })
}

fn chunk(guild: u64, nonce: &str, index: u32, count: u32, members: &[Value]) -> Value {
    json!({
        "guild_id": guild.to_string(),
        "members": members,
        "chunk_index": index,
        "chunk_count": count,
        "not_found": [],
        "nonce": nonce,
    })
}

fn nonce(frame: &Value) -> String {
    frame["d"]["nonce"].as_str().unwrap().to_owned()
}

/// A guild owned by `shard` of `total`.
fn guild_on(shard: u64, total: u64) -> Id<GuildMarker> {
    Id::new(((shard + total * 5) << 22) | 7)
}

/// P1, L3: every IDENTIFY carries the presence and `large_threshold`.
#[tokio::test(flavor = "multi_thread")]
async fn identify_carries_presence_and_large_threshold() {
    let running = Running::start(2, 0..2, |b| {
        b.presence(Presence::watching("servers"))
            .large_threshold(LargeThreshold::new::<100>());
    })
    .await;
    let identifies = running.ops(2);
    assert_eq!(identifies.len(), 2);
    for frame in &identifies {
        assert_eq!(
            presence_of(frame),
            json!({
                "status": "online",
                "afk": false,
                "since": null,
                "activities": [{ "type": 3, "name": "servers" }],
            })
        );
        assert_eq!(frame.json().unwrap()["d"]["large_threshold"], json!(100));
    }
    running.stop().await;
}

/// P2, L3 default: no presence, `large_threshold` 50.
#[tokio::test(flavor = "multi_thread")]
async fn identify_defaults() {
    let running = Running::start(1, 0..1, |_| {}).await;
    let identify = running.ops(2)[0].json().unwrap();
    assert!(identify["d"]["presence"].is_null(), "{identify}");
    assert_eq!(identify["d"]["large_threshold"], json!(50));
    running.stop().await;
}

/// P7, P8, P9: op 3 on every shard, then on one; out of range sends
/// nothing.
#[tokio::test(flavor = "multi_thread")]
async fn runtime_presence_on_all_shards_or_one() {
    let running = Running::start(4, 2..4, |_| {}).await;
    let presence = running.gateway.presence();
    presence.set(Presence::custom("☕ brewing")).unwrap();
    let frames = running.wait_frames(2, |f| f.op() == Some(3)).await;
    let mut shards: Vec<_> = frames
        .iter()
        .map(|f| running.shard_of(f.connection).unwrap())
        .collect();
    shards.sort_unstable();
    assert_eq!(shards, [2, 3]);
    for frame in &frames {
        let d = &frame.json().unwrap()["d"];
        assert_eq!(d["activities"][0]["state"], json!("☕ brewing"));
        assert_eq!(d["activities"][0]["type"], json!(4));
    }

    presence.set_shard(3, Presence::dnd()).unwrap();
    let frames = running.wait_frames(3, |f| f.op() == Some(3)).await;
    let last = &frames[2];
    assert_eq!(running.shard_of(last.connection), Some(3));
    assert_eq!(last.json().unwrap()["d"]["status"], json!("dnd"));
    assert_eq!(last.json().unwrap()["d"]["activities"], json!([]));

    for shard in [1, 4, u32::MAX] {
        assert_eq!(
            presence.set_shard(shard, Presence::idle()),
            Err(GatewayError::ShardNotInRange { shard, range: 2..4 })
        );
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(running.ops(3).len(), 3);
    let gateway = running.stop().await;
    // P10: after `run_until` returned.
    assert_eq!(
        gateway.presence().set(Presence::online()),
        Err(GatewayError::NotRunning)
    );
    assert_eq!(
        gateway
            .request_guild_members(MemberRequest::users(Id::new(1), [Id::new(2)]))
            .await,
        Err(GatewayError::NotRunning)
    );
}

/// P12, P11: a resume sends no extra op 3; a new session's READY restores
/// the runtime presence.
#[tokio::test(flavor = "multi_thread")]
async fn runtime_presence_survives_a_reidentify() {
    let running = Running::start(1, 0..1, |b| {
        b.presence(Presence::online());
    })
    .await;
    running
        .gateway
        .presence()
        .set(Presence::listening("rain"))
        .unwrap();
    running.wait_frames(1, |f| f.op() == Some(3)).await;

    // P12: a resume.
    running.gw.close(4000, "reconnect").unwrap();
    running.wait_frames(1, |f| f.op() == Some(6)).await;
    running.wait_sessions(1).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(running.ops(3).len(), 1, "no op 3 after RESUMED");

    // P11: the session is invalidated; the shard identifies again.
    assert_eq!(running.gw.send_raw(r#"{"op":9,"d":false}"#), 1);
    running.wait_frames(2, |f| f.op() == Some(2)).await;
    let frames = running.wait_frames(2, |f| f.op() == Some(3)).await;
    let identify = &running.ops(2)[1];
    let resent = &frames[1];
    assert!(resent.received_at >= identify.received_at);
    let d = &resent.json().unwrap()["d"];
    assert_eq!(d["activities"][0]["name"], json!("rain"));
    assert_eq!(d["activities"][0]["type"], json!(2));
    // The new IDENTIFY still carries the initial presence.
    assert_eq!(presence_of(identify)["activities"], json!([]));
    running.stop().await;
}

/// M1, M5, M17: users and prefix requests are answered by chunks; a
/// `MemberChunk` handler sees the chunk too.
#[tokio::test(flavor = "multi_thread")]
async fn member_requests_are_answered_by_chunks() {
    let running = Running::start(1, 0..1, |b| {
        b.module(Module::new("chunks").add::<OnChunk>());
    })
    .await;
    let guild = Id::new(5);
    let (task, frame) = running
        .request(MemberRequest::users(guild, [Id::new(7)]))
        .await;
    let n = nonce(&frame);
    assert!(n.len() < 32);
    assert_eq!(
        frame["d"],
        json!({ "guild_id": "5", "user_ids": ["7"], "nonce": n })
    );
    assert_eq!(
        running
            .gw
            .dispatch("GUILD_MEMBERS_CHUNK", chunk(5, &n, 0, 1, &[member(7)])),
        1
    );
    let members = tokio::time::timeout(WAIT, task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(members.members.len(), 1);
    assert_eq!(members.members[0].user.id, Id::new(7));
    let deadline = Instant::now() + WAIT;
    while running.probe.lines() != [format!("chunk:{n}")] {
        assert!(Instant::now() < deadline, "{:?}", running.probe.lines());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let (task, frame) = running
        .request(MemberRequest::prefix(guild, "ab", 10).unwrap())
        .await;
    assert_eq!(frame["d"]["query"], json!("ab"));
    assert_eq!(frame["d"]["limit"], json!(10));
    let n = nonce(&frame);
    running
        .gw
        .dispatch("GUILD_MEMBERS_CHUNK", chunk(5, &n, 1, 2, &[member(9)]));
    running
        .gw
        .dispatch("GUILD_MEMBERS_CHUNK", chunk(5, &n, 0, 2, &[member(8)]));
    let members = tokio::time::timeout(WAIT, task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let ids: Vec<_> = members.members.iter().map(|m| m.user.id.get()).collect();
    assert_eq!(ids, [8, 9]);
    running.stop().await;
}

/// M11: `RATE_LIMITED` for the request's nonce reaches the caller (GW-07).
#[tokio::test(flavor = "multi_thread")]
async fn gw_07_rate_limited_reaches_the_caller() {
    let running = Running::start(1, 0..1, |_| {}).await;
    let guild = Id::new(5);
    let (task, frame) = running
        .request(MemberRequest::users(guild, [Id::new(7)]))
        .await;
    let n = nonce(&frame);
    running.gw.dispatch(
        "RATE_LIMITED",
        json!({ "opcode": 8, "retry_after": 2.5, "meta": { "guild_id": "5", "nonce": n } }),
    );
    assert_eq!(
        tokio::time::timeout(WAIT, task).await.unwrap().unwrap(),
        Err(GatewayError::RateLimited {
            guild,
            retry_after: Duration::from_millis(2500)
        })
    );
    running.stop().await;
}

/// M12: answers with a foreign nonce reach no waiter (and no handler is
/// registered, so no sink either).
#[tokio::test(flavor = "multi_thread")]
async fn foreign_nonces_are_not_delivered() {
    let running = Running::start(1, 0..1, |_| {}).await;
    let guild = Id::new(5);
    let (task, frame) = running
        .request(MemberRequest::users(guild, [Id::new(7)]))
        .await;
    running.gw.dispatch(
        "RATE_LIMITED",
        json!({ "opcode": 8, "retry_after": 1.0, "meta": { "guild_id": "5", "nonce": "foreign" } }),
    );
    running.gw.dispatch(
        "GUILD_MEMBERS_CHUNK",
        chunk(5, "foreign", 0, 1, &[member(1)]),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!task.is_finished());
    running.gw.dispatch(
        "GUILD_MEMBERS_CHUNK",
        chunk(5, &nonce(&frame), 0, 1, &[member(7)]),
    );
    let members = tokio::time::timeout(WAIT, task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(members.members.len(), 1);
    assert_eq!(members.members[0].user.id, Id::new(7));
    running.stop().await;
}

/// M15: a guild owned by a shard outside the range sends nothing; an
/// in-range guild's op 8 goes to its owning shard only.
#[tokio::test(flavor = "multi_thread")]
async fn member_requests_go_to_the_owning_shard() {
    let running = Running::start(4, 0..2, |_| {}).await;
    let outside = guild_on(3, 4);
    assert_eq!(
        running
            .gateway
            .request_guild_members(MemberRequest::users(outside, [Id::new(2)]))
            .await,
        Err(GatewayError::GuildNotInRange {
            guild: outside,
            shard: 3,
            range: 0..2
        })
    );
    assert!(running.ops(8).is_empty());

    let inside = guild_on(1, 4);
    assert_eq!(running.gateway.shard_for_guild(inside), Ok(1));
    let (task, _) = running
        .request(MemberRequest::users(inside, [Id::new(2)]))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let requests = running.ops(8);
    assert_eq!(requests.len(), 1);
    assert_eq!(running.shard_of(requests[0].connection), Some(1));
    task.abort();
    running.stop().await;
}
