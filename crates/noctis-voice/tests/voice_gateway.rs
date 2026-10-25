//! Voice connections end to end (Story 17.1): an App with `.voice(..)`
//! against `FakeDiscordHttp` and `FakeGateway` (shards 0..2 of 4, READY user
//! 1). Every op 4 reaches the guild's owning shard (VC-01) and the call map
//! keeps one call per guild (VC-02). A successful join needs a real voice
//! server and is not tested here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis::gateway::{GatewayError, GatewayHandle, Presence};
use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker};
use noctis::voice::{CallHandle, Voice, VoiceConfig, VoiceError};
use noctis::{Runtime, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeGatewayConfig, FrameKind, GatewayBot, IdentifyReply,
    RecordedFrame, SessionStartLimit,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);
/// The shard total; this process runs shards 0..2.
const TOTAL: u64 = 4;
/// The bot user id the fake's READY carries.
const BOT: u64 = 1;

/// The process-wide metrics recorder; each test uses its own `app` label.
fn snapshotter() -> &'static Snapshotter {
    static SNAPSHOTTER: OnceLock<Snapshotter> = OnceLock::new();
    SNAPSHOTTER.get_or_init(|| {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        recorder
            .install()
            .expect("the only recorder of this binary");
        snapshotter
    })
}

/// `noctis_voice_calls{app}`.
fn calls_gauge(app: &str) -> Option<f64> {
    snapshotter()
        .snapshot()
        .into_vec()
        .into_iter()
        .find_map(|(key, _, _, value)| {
            let key = key.key();
            let matches = key.name() == "noctis_voice_calls"
                && key.labels().any(|l| l.key() == "app" && l.value() == app);
            match value {
                DebugValue::Gauge(v) if matches => Some(v.into_inner()),
                _ => None,
            }
        })
}

/// A guild owned by `shard` of [`TOTAL`].
fn guild_on(shard: u64, n: u64) -> Id<GuildMarker> {
    Id::new(((shard + TOTAL * n) << 22) | 7)
}

fn channel(n: u64) -> Id<ChannelMarker> {
    Id::new(n)
}

/// Starts `FakeDiscordHttp` (4 shards) and a `FakeGateway` with `config`.
async fn start_fakes(config: FakeGatewayConfig) -> (FakeDiscordHttp, Arc<FakeGateway>) {
    snapshotter();
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 4,
        session_start_limit: SessionStartLimit {
            max_concurrency: 16,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    let gw = Arc::new(FakeGateway::start_with(config).await.unwrap());
    (http, gw)
}

/// An App builder with voice against both fakes (shards 0..2 of 4).
fn voice_builder(
    app: &str,
    http: &FakeDiscordHttp,
    gw: &FakeGateway,
    config: VoiceConfig,
    grace: Duration,
) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .app_id(app)
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .gateway_url(gw.url())
        .shard_total(4)
        .shard_range(0..2)
        .shutdown_grace(grace)
        .voice(config);
    builder
}

/// Polls `done` until it holds, for at most [`WAIT`].
async fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A running App with voice against both fakes.
struct Running {
    gw: Arc<FakeGateway>,
    _http: FakeDiscordHttp,
    gateway: GatewayHandle,
    voice: Voice,
    app: &'static str,
    stop: Option<oneshot::Sender<()>>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(app: &'static str, config: VoiceConfig, grace: Duration) -> Self {
        let (http, gw) = start_fakes(FakeGatewayConfig::default()).await;
        let app_built = voice_builder(app, &http, &gw, config, grace)
            .build()
            .unwrap();
        let gateway = app_built.gateway();
        let voice = app_built.voice();
        let (stop, stopped) = oneshot::channel();
        let run = tokio::spawn(app_built.run_until(async {
            let _ = stopped.await;
        }));
        let running = Self {
            gw,
            _http: http,
            gateway,
            voice,
            app,
            stop: Some(stop),
            run,
        };
        running
            .until("two sessions and the bot user id", || {
                running.gw.dispatch("TYPING_START", json!({})) >= 2
                    && running.gateway.current_user_id() == Some(Id::new(BOT))
            })
            .await;
        running
    }

    async fn default(app: &'static str) -> Self {
        Self::start(app, VoiceConfig::default(), Duration::from_secs(10)).await
    }

    /// Polls `done` until it holds, for at most [`WAIT`].
    async fn until(&self, what: &str, done: impl Fn() -> bool) {
        until(what, done).await;
    }

    /// Stops the App and waits for `run_until` to return.
    async fn stop(mut self) -> Self {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(WAIT, &mut self.run)
            .await
            .expect("run_until returned")
            .unwrap()
            .unwrap();
        self
    }

    /// The connection that identified as `shard`.
    fn connection_of(&self, shard: u64) -> u64 {
        self.gw
            .frames()
            .into_iter()
            .filter(|f| f.op() == Some(2))
            .find(|f| f.json().unwrap()["d"]["shard"][0].as_u64() == Some(shard))
            .map(|f| f.connection)
            .expect("the shard identified")
    }

    /// The op 4 frames, as `(connection, d)`.
    fn op4(&self) -> Vec<(u64, Value)> {
        self.gw
            .frames()
            .into_iter()
            .filter(|f| f.op() == Some(4))
            .map(|f| (f.connection, f.json().unwrap()["d"].clone()))
            .collect()
    }

    /// The op 4 frames for `guild`, as `d.channel_id`.
    fn op4_channels(&self, guild: Id<GuildMarker>) -> Vec<Value> {
        self.op4()
            .into_iter()
            .filter(|(_, d)| d["guild_id"] == json!(guild.to_string()))
            .map(|(_, d)| d["channel_id"].clone())
            .collect()
    }

    async fn wait_op4(&self, guild: Id<GuildMarker>, n: usize) -> Vec<Value> {
        self.until(&format!("{n} op 4 frames for {guild}"), || {
            self.op4_channels(guild).len() >= n
        })
        .await;
        self.op4_channels(guild)
    }

    /// Starts a join in the background.
    fn join(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
    ) -> JoinHandle<Result<CallHandle, VoiceError>> {
        tokio::spawn(self.voice.join(guild, channel))
    }

    /// Dispatches a `VOICE_STATE_UPDATE` of `user` in `guild`.
    fn voice_state(&self, guild: Id<GuildMarker>, user: u64, channel: Option<u64>) {
        let channel = channel.map(|c| c.to_string());
        self.gw.dispatch(
            "VOICE_STATE_UPDATE",
            json!({
                "guild_id": guild.to_string(),
                "channel_id": channel,
                "user_id": user.to_string(),
                "session_id": "voice-session",
                "deaf": false,
                "mute": false,
                "self_deaf": true,
                "self_mute": false,
                "self_video": false,
                "suppress": false,
                "request_to_speak_timestamp": null
            }),
        );
    }

    fn gauge(&self) -> Option<f64> {
        calls_gauge(self.app)
    }
}

async fn finished<T>(task: JoinHandle<T>) -> T {
    finished_within(WAIT, task).await
}

async fn finished_within<T>(bound: Duration, task: JoinHandle<T>) -> T {
    tokio::time::timeout(bound, task)
        .await
        .expect("the task finished")
        .unwrap()
}

/// VC-01: the join frame goes to the owning shard only; a pending join is a
/// call. A move updates it, foreign states and a `null` endpoint are
/// ignored, and a disconnect removes it.
#[tokio::test(flavor = "multi_thread")]
async fn vc_01_join_pending_moved_and_disconnected() {
    let running = Running::default("vc-join").await;
    let guild = guild_on(1, 3);
    let join = running.join(guild, channel(10));
    let frames = running.wait_op4(guild, 1).await;
    assert_eq!(frames, [json!("10")]);
    let (connection, d) = running.op4().pop().unwrap();
    assert_eq!(connection, running.connection_of(1));
    assert_eq!(
        d,
        json!({
            "guild_id": guild.to_string(),
            "channel_id": "10",
            "self_mute": false,
            "self_deaf": true
        })
    );
    assert_eq!(running.voice.active_calls(), 1);
    let call = running.voice.call(guild).unwrap();
    assert_eq!(call.guild(), guild);
    assert_eq!(call.channel(), Some(channel(10)));
    running
        .until("the gauge", || running.gauge() == Some(1.0))
        .await;

    // Moved.
    running.voice_state(guild, BOT, Some(11));
    running
        .until("the move", || call.channel() == Some(channel(11)))
        .await;
    // Ignored: another user's state, a guild without a call, a null endpoint.
    running.voice_state(guild, 2, None);
    running.voice_state(guild_on(0, 5), BOT, None);
    running.gw.dispatch(
        "VOICE_SERVER_UPDATE",
        json!({ "guild_id": guild.to_string(), "token": "t", "endpoint": null }),
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(call.channel(), Some(channel(11)));
    assert!(!join.is_finished());

    // Disconnected.
    running.voice_state(guild, BOT, None);
    assert_eq!(
        finished(join).await.err(),
        Some(VoiceError::Interrupted { guild })
    );
    assert_eq!(running.voice.active_calls(), 0);
    assert_eq!(call.channel(), None);
    running
        .until("the gauge", || running.gauge() == Some(0.0))
        .await;
    assert_eq!(
        running.op4_channels(guild).len(),
        1,
        "no op 4 on a disconnect"
    );
    running.stop().await;
}

/// VC-01: a guild outside the Shard Range and a refused op 4 create no
/// call; the original `GatewayError` is returned.
#[tokio::test(flavor = "multi_thread")]
async fn vc_01_refused_joins_and_leaves() {
    let running = Running::default("vc-refused").await;
    let outside = guild_on(3, 1);
    assert_eq!(
        finished(running.join(outside, channel(10))).await.err(),
        Some(VoiceError::Gateway(GatewayError::GuildNotInRange {
            guild: outside,
            shard: 3,
            range: 0..2
        }))
    );
    assert_eq!(running.voice.active_calls(), 0);

    // A call on shard 0, then shard 0's budget runs out.
    let guild = guild_on(0, 1);
    let pending = running.join(guild, channel(20));
    running.wait_op4(guild, 1).await;
    let presence = running.gateway.presence();
    while presence.set_shard(0, Presence::online()).is_ok() {}
    let other = guild_on(0, 2);
    let refused = finished(running.join(other, channel(21))).await;
    assert!(
        matches!(
            refused,
            Err(VoiceError::Gateway(GatewayError::SendLimit {
                shard: 0,
                ..
            }))
        ),
        "{refused:?}"
    );
    assert!(running.voice.call(other).is_none());
    // A leave with a call: refused, the call is removed anyway.
    assert!(matches!(
        running.voice.leave(guild).await,
        Err(VoiceError::Gateway(GatewayError::SendLimit {
            shard: 0,
            ..
        }))
    ));
    assert_eq!(running.voice.active_calls(), 0);
    assert_eq!(
        finished(pending).await.err(),
        Some(VoiceError::Interrupted { guild })
    );
    // A leave without a call still sends op 4 (shard 1 has budget).
    let free = guild_on(1, 1);
    running.voice.leave(free).await.unwrap();
    assert_eq!(running.wait_op4(free, 1).await, [Value::Null]);
    assert_eq!(running.op4_channels(other), Vec::<Value>::new());
    running.stop().await;
}

/// VC-02: a second join replaces the first (which is `Interrupted`); a
/// leave removes the call and makes its handles stale.
#[tokio::test(flavor = "multi_thread")]
async fn vc_02_second_join_moves_and_stale_handles() {
    let running = Running::default("vc-two").await;
    let guild = guild_on(0, 3);
    let first = running.join(guild, channel(30));
    running.wait_op4(guild, 1).await;
    let handle = running.voice.call(guild).unwrap();
    let second = running.join(guild, channel(31));
    assert_eq!(
        finished(first).await.err(),
        Some(VoiceError::Interrupted { guild })
    );
    let frames = running.wait_op4(guild, 2).await;
    assert_eq!(frames.first(), Some(&json!("30")));
    assert_eq!(frames.last(), Some(&json!("31")));
    assert_eq!(running.voice.active_calls(), 1);
    assert_eq!(handle.channel(), Some(channel(31)), "the same call");
    assert!(!second.is_finished());

    running.voice.leave(guild).await.unwrap();
    assert_eq!(running.wait_op4(guild, 3).await.last(), Some(&Value::Null));
    assert_eq!(
        finished(second).await.err(),
        Some(VoiceError::Interrupted { guild })
    );
    assert_eq!(running.voice.active_calls(), 0);
    assert_eq!(handle.channel(), None);
    let sent = running.op4().len();
    assert_eq!(handle.leave().await, Err(VoiceError::Interrupted { guild }));
    assert_eq!(running.op4().len(), sent, "a stale handle sends nothing");

    // A new call of the guild does not revive the old handle.
    let third = running.join(guild, channel(32));
    running.wait_op4(guild, 4).await;
    assert_eq!(handle.channel(), None);
    assert_eq!(
        running.voice.call(guild).unwrap().channel(),
        Some(channel(32))
    );
    running
        .gw
        .dispatch("GUILD_DELETE", json!({ "id": guild.to_string() }));
    assert_eq!(
        finished(third).await.err(),
        Some(VoiceError::Interrupted { guild })
    );
    assert_eq!(running.voice.active_calls(), 0);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        running.op4_channels(guild).len(),
        4,
        "no op 4 on GUILD_DELETE"
    );
    running.stop().await;
}

/// No answer within `join_timeout`: op 4 leave, the call is removed.
#[tokio::test(flavor = "multi_thread")]
async fn join_timeout_leaves() {
    let config = VoiceConfig::default().join_timeout(Duration::from_secs(1));
    let running = Running::start("vc-timeout", config, Duration::from_secs(10)).await;
    let guild = guild_on(1, 4);
    let started = Instant::now();
    let result = finished(running.join(guild, channel(40))).await;
    assert_eq!(
        result.err(),
        Some(VoiceError::Timeout {
            guild,
            after: Duration::from_secs(1)
        })
    );
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(running.wait_op4(guild, 2).await, [json!("40"), Value::Null]);
    assert_eq!(running.voice.active_calls(), 0);
    running
        .until("the gauge", || running.gauge() == Some(0.0))
        .await;
    running.stop().await;
}

/// Both voice events reach the call, so the driver tries the endpoint and
/// fails: op 4 leave, the call is removed.
#[tokio::test(flavor = "multi_thread")]
async fn join_driver_error_leaves() {
    let config = VoiceConfig::default().join_timeout(Duration::from_secs(30));
    let running = Running::start("vc-driver", config, Duration::from_secs(10)).await;
    let guild = guild_on(0, 6);
    let join = running.join(guild, channel(60));
    running.wait_op4(guild, 1).await;
    running.voice_state(guild, BOT, Some(60));
    running.gw.dispatch(
        "VOICE_SERVER_UPDATE",
        json!({
            "guild_id": guild.to_string(),
            "token": "secret-voice-token",
            "endpoint": "127.0.0.1:9"
        }),
    );
    // Songbird retries the failed connect 5 times with a jittered 0.25 s to
    // 10 s backoff (about 8 s to 17 s in all), so the error can come after
    // `WAIT`; the 30 s join timeout bounds it.
    let result = finished_within(Duration::from_secs(35), join).await;
    let Err(VoiceError::Driver {
        guild: failed,
        reason,
    }) = result
    else {
        panic!("expected a driver error: {result:?}");
    };
    assert_eq!(failed, guild);
    assert!(!reason.contains("secret-voice-token"), "{reason}");
    assert_eq!(running.wait_op4(guild, 2).await, [json!("60"), Value::Null]);
    assert_eq!(running.voice.active_calls(), 0);
    running.stop().await;
}

/// Answers every op 4 leave with the bot's own `VOICE_STATE_UPDATE` without
/// a channel, as Discord does.
fn confirm_leaves(gw: Arc<FakeGateway>, guilds: Vec<Id<GuildMarker>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut pending = guilds;
        let deadline = Instant::now() + WAIT;
        while !pending.is_empty() && Instant::now() < deadline {
            let frames: Vec<RecordedFrame> = gw.frames();
            pending.retain(|guild| {
                let left = frames.iter().filter(|f| f.op() == Some(4)).any(|f| {
                    let d = f.json().unwrap()["d"].clone();
                    d["guild_id"] == json!(guild.to_string()) && d["channel_id"].is_null()
                });
                if left {
                    gw.dispatch(
                        "VOICE_STATE_UPDATE",
                        json!({
                            "guild_id": guild.to_string(),
                            "channel_id": null,
                            "user_id": BOT.to_string(),
                            "session_id": "voice-session",
                            "deaf": false,
                            "mute": false,
                            "self_deaf": true,
                            "self_mute": false,
                            "self_video": false,
                            "suppress": false,
                            "request_to_speak_timestamp": null
                        }),
                    );
                }
                !left
            });
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
}

/// The index of the first close frame on `connection`.
fn close_index(frames: &[RecordedFrame], connection: u64) -> usize {
    frames
        .iter()
        .position(|f| f.connection == connection && matches!(f.kind, FrameKind::Close { .. }))
        .expect("the shard closed")
}

/// Shutdown leaves every channel before the shards close; the map ends
/// empty and the gauge at 0.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_leaves_before_close() {
    let running = Running::default("vc-shutdown").await;
    let guilds = [guild_on(0, 7), guild_on(1, 7)];
    let joins: Vec<_> = guilds
        .iter()
        .map(|guild| running.join(*guild, channel(70)))
        .collect();
    for guild in guilds {
        running.wait_op4(guild, 1).await;
    }
    running
        .until("the gauge", || running.gauge() == Some(2.0))
        .await;
    let responder = confirm_leaves(Arc::clone(&running.gw), guilds.to_vec());
    let started = Instant::now();
    let running = running.stop().await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "confirmed leaves end the task early"
    );
    finished(responder).await;
    let frames = running.gw.frames();
    for (shard, guild) in [(0, guilds[0]), (1, guilds[1])] {
        let connection = running.connection_of(shard);
        let leave = frames
            .iter()
            .position(|f| {
                f.connection == connection
                    && f.op() == Some(4)
                    && f.json().unwrap()["d"]["guild_id"] == json!(guild.to_string())
                    && f.json().unwrap()["d"]["channel_id"].is_null()
            })
            .expect("op 4 leave at shutdown");
        assert!(leave < close_index(&frames, connection), "shard {shard}");
    }
    assert_eq!(running.voice.active_calls(), 0);
    assert_eq!(running.gauge(), Some(0.0));
    for join in joins {
        assert!(matches!(
            finished(join).await,
            Err(VoiceError::Interrupted { .. })
        ));
    }
}

/// Without confirmations the task is cut at `min(shutdown_grace, 5 s)` and
/// the shutdown goes on.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_without_confirmation_is_cut() {
    let running = Running::start(
        "vc-shutdown-cut",
        VoiceConfig::default(),
        Duration::from_secs(1),
    )
    .await;
    let guild = guild_on(1, 8);
    let _join = running.join(guild, channel(80));
    running.wait_op4(guild, 1).await;
    let started = Instant::now();
    let running = running.stop().await;
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(running.op4_channels(guild), [json!("80"), Value::Null]);
    assert_eq!(running.voice.active_calls(), 0);
}

/// VC-02: a refused move of a pending call drops the call: the pending join
/// is `Interrupted`, nothing stays in the map.
#[tokio::test(flavor = "multi_thread")]
async fn vc_02_refused_move_drops_the_call() {
    let running = Running::default("vc-refused-move").await;
    let guild = guild_on(0, 9);
    let pending = running.join(guild, channel(90));
    running.wait_op4(guild, 1).await;
    let presence = running.gateway.presence();
    while presence.set_shard(0, Presence::online()).is_ok() {}
    let refused = finished(running.join(guild, channel(91))).await;
    assert!(
        matches!(
            refused,
            Err(VoiceError::Gateway(GatewayError::SendLimit {
                shard: 0,
                ..
            }))
        ),
        "{refused:?}"
    );
    assert_eq!(running.voice.active_calls(), 0);
    assert!(running.voice.call(guild).is_none());
    assert_eq!(
        finished(pending).await.err(),
        Some(VoiceError::Interrupted { guild })
    );
    running
        .until("the gauge", || running.gauge() == Some(0.0))
        .await;
    running.stop().await;
}

/// `self_deaf(false)` reaches the join frame and a leave without a call.
#[tokio::test(flavor = "multi_thread")]
async fn self_deaf_false_reaches_op4() {
    let config = VoiceConfig::default().self_deaf(false);
    let running = Running::start("vc-undeaf", config, Duration::from_secs(10)).await;
    let guild = guild_on(1, 10);
    let _join = running.join(guild, channel(100));
    running.wait_op4(guild, 1).await;
    let free = guild_on(0, 10);
    running.voice.leave(free).await.unwrap();
    running.wait_op4(free, 1).await;
    for target in [guild, free] {
        let (_, d) = running
            .op4()
            .into_iter()
            .find(|(_, d)| d["guild_id"] == json!(target.to_string()))
            .unwrap();
        assert_eq!(d["self_deaf"], json!(false), "{d}");
    }
    running.stop().await;
}

/// The shards run but no READY arrived: `NotReady`, no call.
#[tokio::test(flavor = "multi_thread")]
async fn join_before_ready_is_not_ready() {
    let (http, gw) = start_fakes(FakeGatewayConfig {
        identify: IdentifyReply::Ignore,
        ..FakeGatewayConfig::default()
    })
    .await;
    let app = voice_builder(
        "vc-not-ready",
        &http,
        &gw,
        VoiceConfig::default(),
        Duration::from_secs(1),
    )
    .build()
    .unwrap();
    let voice = app.voice();
    let (stop, stopped) = oneshot::channel::<()>();
    let run = tokio::spawn(app.run_until(async {
        let _ = stopped.await;
    }));
    until("an IDENTIFY", || {
        gw.frames().iter().any(|f| f.op() == Some(2))
    })
    .await;
    assert_eq!(
        voice.join(guild_on(0, 11), channel(110)).await.err(),
        Some(VoiceError::NotReady)
    );
    assert_eq!(voice.active_calls(), 0);
    let _ = stop.send(());
    finished(run).await.unwrap();
}

/// Where the `AppStarted` hook below puts `ctx.voice()`.
#[derive(Default)]
struct VoiceSlot(std::sync::Mutex<Option<Voice>>);

/// Takes the App's `Voice` from a context (which binds no label).
#[derive(Hook)]
struct GrabVoice;

impl GrabVoice {
    #[handler]
    async fn handle(self, ctx: HookCtx, _ev: AppStarted) -> HandlerResult<()> {
        let slot = ctx.state::<VoiceSlot>().expect("registered");
        *slot.0.lock().unwrap() = Some(ctx.voice());
        Ok(())
    }
}

/// `AppStarted` alone binds the gauge's `app` label: `app.voice()` is never
/// called.
#[tokio::test(flavor = "multi_thread")]
async fn app_started_binds_the_gauge() {
    let (http, gw) = start_fakes(FakeGatewayConfig::default()).await;
    let mut builder = voice_builder(
        "vc-started",
        &http,
        &gw,
        VoiceConfig::default(),
        Duration::from_secs(1),
    );
    builder
        .state(VoiceSlot::default())
        .module(Module::new("grab").add::<GrabVoice>());
    let app = builder.build().unwrap();
    let gateway = app.gateway();
    let slot = app.state::<VoiceSlot>().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let run = tokio::spawn(app.run_until(async {
        let _ = stopped.await;
    }));
    until("the bot user id and the hook", || {
        gw.dispatch("TYPING_START", json!({})) >= 2
            && gateway.current_user_id() == Some(Id::new(BOT))
            && slot.0.lock().unwrap().is_some()
    })
    .await;
    let voice = slot.0.lock().unwrap().clone().unwrap();
    let guild = guild_on(0, 12);
    let join = tokio::spawn(voice.join(guild, channel(120)));
    until("the pending call", || voice.active_calls() == 1).await;
    until("the gauge", || calls_gauge("vc-started") == Some(1.0)).await;
    let _ = stop.send(());
    finished(run).await.unwrap();
    assert!(finished(join).await.is_err());
}
