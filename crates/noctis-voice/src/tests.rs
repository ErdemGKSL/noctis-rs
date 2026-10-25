//! The queue end to end (Story 17.2): an App with voice on `FakeGateway` and
//! `FakeDiscordHttp`, a pending join whose queue plays on a `FakePlayer`,
//! and a Module whose hooks record the track events.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_core::lifecycle::HookEvent;
use noctis_core::{App, HandlerResult, HookCtx, LifecycleHook, Module, Runtime, Token};
use noctis_testkit::fake::{FakeDiscordHttp, FakeGateway, GatewayBot, SessionStartLimit};
use serde_json::json;
use tokio::sync::oneshot;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::call::VoiceShared;
use crate::queue::QueueCell;
use crate::queue::fake::Played;
use crate::{
    AppBuilderVoiceExt as _, CallHandle, EndReason, QueueEmpty, Track, TrackEnd, TrackError,
    TrackInfo, TrackStart, Voice, VoiceConfig, VoiceError, VoiceExt as _,
};

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// The process-wide metrics recorder of this test binary.
fn snapshotter() -> &'static Snapshotter {
    static SNAPSHOTTER: OnceLock<Snapshotter> = OnceLock::new();
    SNAPSHOTTER.get_or_init(|| {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        recorder
            .install()
            .unwrap_or_else(|e| panic!("the only recorder of this binary: {e}"));
        snapshotter
    })
}

/// `noctis_voice_tracks_total{app, result}` so far. A snapshot swaps the
/// counters to zero, so every snapshot is added into one running total.
fn tracks(app: &str, result: &str) -> u64 {
    static TOTALS: Mutex<Vec<(String, String, u64)>> = Mutex::new(Vec::new());
    let mut totals = TOTALS.lock().unwrap_or_else(PoisonError::into_inner);
    for (key, _, _, value) in snapshotter().snapshot().into_vec() {
        let key = key.key();
        let DebugValue::Counter(n) = value else {
            continue;
        };
        if key.name() != "noctis_voice_tracks_total" {
            continue;
        }
        let label = |name: &str| {
            key.labels()
                .find(|l| l.key() == name)
                .map(|l| l.value().to_owned())
                .unwrap_or_default()
        };
        let (app, result) = (label("app"), label("result"));
        match totals
            .iter_mut()
            .find(|(a, r, _)| *a == app && *r == result)
        {
            Some((_, _, total)) => *total += n,
            None => totals.push((app, result, n)),
        }
    }
    totals
        .iter()
        .find(|(a, r, _)| a == app && r == result)
        .map_or(0, |(_, _, total)| *total)
}

/// What the Module's hooks saw, in order.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Seen {
    fn push(&self, line: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    fn get(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn title(track: &TrackInfo) -> &str {
    track.title().unwrap_or("?")
}

/// A hook of `E` that records `describe(event)` with its Module.
fn hook<E: HookEvent>(seen: &Arc<Seen>, describe: fn(&E) -> String) -> LifecycleHook {
    let seen = Arc::clone(seen);
    LifecycleHook::__new::<E, _, _>("music::Recorder", move |ctx: HookCtx, event: E| {
        let seen = Arc::clone(&seen);
        async move {
            assert_eq!(ctx.module(), "music");
            seen.push(describe(&event));
            HandlerResult::Ok(())
        }
    })
}

/// The Module recording the four track events.
fn music(seen: &Arc<Seen>) -> Module {
    Module::new("music")
        .__add(hook::<TrackStart>(seen, |e| {
            format!("start {}", title(&e.track))
        }))
        .__add(hook::<TrackEnd>(seen, |e| {
            let reason = match e.reason {
                EndReason::Finished => "finished",
                EndReason::Skipped => "skipped",
                EndReason::Stopped => "stopped",
            };
            format!("end {} {reason}", title(&e.track))
        }))
        .__add(hook::<TrackError>(seen, |e| {
            format!("error {} {}", title(&e.track), e.error)
        }))
        .__add(hook::<QueueEmpty>(seen, |e| format!("empty {}", e.guild)))
}

/// A guild owned by shard 1 of 2.
fn guild() -> Id<GuildMarker> {
    Id::new((1 << 22) | 7)
}

async fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A running App with voice on the fakes, its queues on `FakePlayer`s.
struct Running {
    gw: Arc<FakeGateway>,
    _http: FakeDiscordHttp,
    voice: Voice,
    shared: noctis_core::State<VoiceShared>,
    played: Played,
    seen: Arc<Seen>,
    stop: Option<oneshot::Sender<()>>,
    run: tokio::task::JoinHandle<Result<(), noctis_core::Error>>,
}

impl Running {
    async fn start(app: &str, config: VoiceConfig) -> Self {
        Self::start_with(app, config, |_| {}).await
    }

    /// As [`start`](Self::start), with `customize` applied to the builder.
    async fn start_with(
        app: &str,
        config: VoiceConfig,
        customize: impl FnOnce(&mut noctis_core::AppBuilder),
    ) -> Self {
        snapshotter();
        let http = FakeDiscordHttp::start_with(GatewayBot {
            shards: 2,
            session_start_limit: SessionStartLimit {
                max_concurrency: 16,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        })
        .await
        .unwrap_or_else(|e| panic!("{e}"));
        let gw = Arc::new(FakeGateway::start().await.unwrap_or_else(|e| panic!("{e}")));
        let seen = Arc::new(Seen::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .app_id(app)
            .token(Token::new("fake-token").unwrap_or_else(|e| panic!("{e}")))
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .shard_total(2)
            .module(music(&seen))
            .voice(config);
        customize(&mut builder);
        let built = builder.build().unwrap_or_else(|e| panic!("{e}"));
        let shared = built
            .state::<VoiceShared>()
            .unwrap_or_else(|| panic!("voice installed"));
        let played = Played::default();
        *shared
            .fake_player
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(played.clone());
        let voice = built.voice();
        let gateway = built.gateway();
        let (stop, stopped) = oneshot::channel();
        let run = tokio::spawn(built.run_until(async {
            let _ = stopped.await;
        }));
        until("two sessions and READY", || {
            gw.dispatch("TYPING_START", json!({})) >= 2 && gateway.current_user_id().is_some()
        })
        .await;
        Self {
            gw,
            _http: http,
            voice,
            shared,
            played,
            seen,
            stop: Some(stop),
            run,
        }
    }

    /// A pending join of `guild()` and its handle.
    async fn pending_call(&self) -> CallHandle {
        self.pending_call_in(guild()).await
    }

    /// A pending join of `guild` and its handle.
    async fn pending_call_in(&self, guild: Id<GuildMarker>) -> CallHandle {
        let _join = tokio::spawn(self.voice.join(guild, Id::new(10)));
        until("the pending call", || self.voice.call(guild).is_some()).await;
        self.voice
            .call(guild)
            .unwrap_or_else(|| panic!("a pending call"))
    }

    fn queue(&self, call: &CallHandle) -> Arc<QueueCell> {
        self.shared
            .queue(call.guild(), call.generation)
            .unwrap_or_else(|| panic!("an open queue"))
    }

    /// The token the player was last asked to play.
    fn token(&self) -> u64 {
        self.played.last_token().unwrap_or_default()
    }

    async fn seen(&self, n: usize) -> Vec<String> {
        until(&format!("{n} track events"), || self.seen.get().len() >= n).await;
        self.seen.get()
    }

    /// The op 4 frames' `d.channel_id`.
    fn op4_channels(&self) -> Vec<serde_json::Value> {
        self.gw
            .frames()
            .into_iter()
            .filter(|f| f.op() == Some(4))
            .filter_map(|f| f.json().map(|v| v["d"]["channel_id"].clone()))
            .collect()
    }

    async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(WAIT, &mut self.run).await;
    }
}

fn track(name: &str) -> Track {
    Track::file(format!("{name}.wav"))
        .unwrap_or_else(|e| panic!("{e}"))
        .title(name)
}

/// A stale handle: every mutator is `Interrupted`, the getters read nothing.
async fn assert_stale(call: &CallHandle) {
    let interrupted = VoiceError::Interrupted { guild: guild() };
    assert_eq!(call.enqueue(track("x")).await, Err(interrupted.clone()));
    assert_eq!(call.skip(), Err(interrupted.clone()));
    assert_eq!(call.pause(), Err(interrupted.clone()));
    assert_eq!(call.resume(), Err(interrupted.clone()));
    assert_eq!(call.stop(), Err(interrupted.clone()));
    assert_eq!(call.clear(), Err(interrupted.clone()));
    assert_eq!(call.set_volume(1.0), Err(interrupted));
    assert_eq!(call.now_playing(), None);
    assert!(!call.is_paused());
    assert!(call.list().is_empty());
    assert_eq!(call.queue_len(), 0);
    assert!((call.volume() - 1.0).abs() < f32::EPSILON);
}

/// End, error and end, then skip, stop and a leave with a queue: the hooks
/// see exactly the queue's events, the counter counts each current track
/// once, and a stale handle is refused.
#[tokio::test(flavor = "multi_thread")]
async fn track_events_reach_hooks() {
    const APP: &str = "voice-queue";
    let running = Running::start(
        APP,
        VoiceConfig::default().join_timeout(Duration::from_mins(1)),
    )
    .await;
    let call = running.pending_call().await;
    let queue = running.queue(&call);

    let a = call
        .enqueue(track("a"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let b = call
        .enqueue(track("b"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let c = call
        .enqueue(track("c"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(a.id() != b.id() && b.id() != c.id());
    assert_eq!(call.now_playing(), Some(a.clone()));
    assert_eq!(call.list(), [b.clone(), c.clone()]);
    assert_eq!(call.queue_len(), 3);

    queue.on_playable(running.token());
    queue.on_end(running.token());
    queue.on_playable(running.token());
    let b_token = running.token();
    queue.on_error(b_token, "decode failed".into());
    queue.on_end(b_token); // the driver's End after Error: ignored
    queue.on_playable(running.token());
    queue.on_end(running.token());
    let empty = format!("empty {}", guild());
    assert_eq!(
        running.seen(7).await,
        [
            "start a",
            "end a finished",
            "start b",
            "error b decode failed",
            "start c",
            "end c finished",
            empty.as_str(),
        ]
    );
    assert_eq!(running.played.titles(), ["a", "b", "c"]);
    until("the counters", || {
        tracks(APP, "finished") == 2 && tracks(APP, "error") == 1
    })
    .await;

    // Skip, then stop.
    for name in ["d", "e", "f"] {
        call.enqueue(track(name))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(call.pause().unwrap_or_else(|e| panic!("{e}")));
    assert!(call.is_paused());
    call.set_volume(0.5).unwrap_or_else(|e| panic!("{e}"));
    assert!((call.volume() - 0.5).abs() < f32::EPSILON);
    assert_eq!(
        call.skip()
            .unwrap_or_else(|e| panic!("{e}"))
            .map(|t| t.title().map(str::to_owned)),
        Some(Some("d".to_owned()))
    );
    assert!(!call.is_paused());
    assert_eq!(call.stop(), Ok(2));
    assert_eq!(
        running.seen(10).await[7..],
        ["end d skipped", "end e stopped", empty.as_str()]
    );

    // A leave with a queue: no event, the current track counted stopped.
    call.enqueue(track("g"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    call.enqueue(track("h"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    running
        .voice
        .leave(guild())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    until("the counters", || {
        tracks(APP, "skipped") == 1 && tracks(APP, "stopped") == 2
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(running.seen.get().len(), 10, "{:?}", running.seen.get());

    assert_stale(&call).await;
    running.stop().await;
}

/// The queue bound and the volume check, through the handle; track ids are
/// unique across the App's guilds.
#[tokio::test(flavor = "multi_thread")]
async fn queue_full_and_bad_volume() {
    let running = Running::start(
        "voice-bound",
        VoiceConfig::default()
            .join_timeout(Duration::from_mins(1))
            .max_queue(2),
    )
    .await;
    let call = running.pending_call().await;
    call.enqueue(track("a"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    call.enqueue(track("b"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        call.enqueue(track("c")).await,
        Err(VoiceError::QueueFull {
            guild: guild(),
            max: 2
        })
    );
    assert_eq!(call.queue_len(), 2);
    assert!(matches!(
        call.set_volume(f32::NAN),
        Err(VoiceError::InvalidVolume { .. })
    ));
    assert_eq!(call.clear(), Ok(1));
    // Shard 0's guild 7: a second queue; ids stay unique per App.
    let other = running.pending_call_in(Id::new(7)).await;
    let mut ids = vec![
        call.now_playing().map(|t| t.id()),
        other.enqueue(track("x")).await.ok().map(|t| t.id()),
        call.enqueue(track("y")).await.ok().map(|t| t.id()),
        other.enqueue(track("z")).await.ok().map(|t| t.id()),
    ];
    assert!(ids.iter().all(Option::is_some), "{ids:?}");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4);
    running.stop().await;
}

/// Idle leave: a connected call idle for `idle_leave` sends op 4 leave and
/// is removed; a pending one never arms the timer.
#[tokio::test(flavor = "multi_thread")]
async fn idle_leave_leaves() {
    let running = Running::start(
        "voice-idle",
        VoiceConfig::default()
            .join_timeout(Duration::from_mins(1))
            .idle_leave(Some(Duration::from_secs(1))),
    )
    .await;
    let call = running.pending_call().await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(running.voice.active_calls(), 1, "a pending call stays");
    running.shared.connected(call.guild(), call.generation);
    until("the idle leave", || running.voice.active_calls() == 0).await;
    until("op 4 leave", || {
        running.op4_channels().last() == Some(&serde_json::Value::Null)
    })
    .await;
    assert_eq!(call.channel(), None);
    running.stop().await;
}

/// Activity after the idle timer's sleep but before its removal keeps the
/// call: the removal re-checks idleness.
#[tokio::test(flavor = "multi_thread")]
async fn idle_leave_rechecks_idleness() {
    let running = Running::start(
        "voice-idle-race",
        VoiceConfig::default().join_timeout(Duration::from_mins(1)),
    )
    .await;
    let call = running.pending_call().await;
    running.shared.connected(call.guild(), call.generation);
    call.enqueue(track("a"))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    // What the elapsed timer runs, after the enqueue landed.
    running
        .shared
        .idle_leave(call.guild(), call.generation)
        .await;
    assert_eq!(running.voice.active_calls(), 1, "a playing call stays");
    assert!(call.pause().unwrap_or_else(|e| panic!("{e}")));
    running
        .shared
        .idle_leave(call.guild(), call.generation)
        .await;
    assert_eq!(running.voice.active_calls(), 0, "an idle call leaves");
    running.stop().await;
}

/// App shutdown with a queue: the current track is stopped on the player
/// and counted `stopped`, and no track event fires.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_stops_the_current_track() {
    const APP: &str = "voice-shutdown";
    let running = Running::start(
        APP,
        VoiceConfig::default().join_timeout(Duration::from_mins(1)),
    )
    .await;
    let call = running.pending_call().await;
    for name in ["a", "b"] {
        call.enqueue(track(name))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let token = running.token();
    let (played, seen) = (running.played.clone(), Arc::clone(&running.seen));
    let stopped = tracks(APP, "stopped");
    running.stop().await;
    assert_eq!(played.all().last(), Some(&crate::queue::Cmd::Stop(token)));
    until("the stopped counter", || {
        tracks(APP, "stopped") == stopped + 1
    })
    .await;
    assert!(seen.get().is_empty(), "{:?}", seen.get());
}

/// The driver-event mapping: `Playable`, `End` and `Error` reach the hooks
/// as `TrackStart`, `TrackEnd{Finished}` and `TrackError`; a stale token
/// and a dropped queue are ignored.
#[tokio::test(flavor = "multi_thread")]
async fn driver_reports_map_to_events() {
    use songbird::events::{EventContext, TrackEvent};

    use crate::player::Report;

    let running = Running::start(
        "voice-report",
        VoiceConfig::default().join_timeout(Duration::from_mins(1)),
    )
    .await;
    let call = running.pending_call().await;
    let queue = running.queue(&call);
    for name in ["a", "b"] {
        call.enqueue(track(name))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let report = |token, event| {
        Report {
            queue: Arc::downgrade(&queue),
            token,
            event,
        }
        .report(&EventContext::Track(&[]));
    };
    let a = running.token();
    report(a, TrackEvent::Playable);
    report(a, TrackEvent::End);
    let b = running.token();
    report(a, TrackEvent::Error); // stale: ignored
    Report {
        queue: std::sync::Weak::new(),
        token: b,
        event: TrackEvent::End,
    }
    .report(&EventContext::Track(&[])); // dropped queue: ignored
    report(b, TrackEvent::Error);
    let empty = format!("empty {}", guild());
    assert_eq!(
        running.seen(4).await,
        [
            "start a",
            "end a finished",
            "error b the track failed",
            empty.as_str(),
        ]
    );
    running.stop().await;
}

/// The bot's own `VOICE_STATE_UPDATE` in `guild()`'s `channel`.
fn bot_voice_state(running: &Running, channel: u64) {
    running.gw.dispatch(
        "VOICE_STATE_UPDATE",
        json!({
            "guild_id": guild().to_string(),
            "channel_id": channel.to_string(),
            "user_id": "1",
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

/// A guild channel JSON: voice with `bitrate`, or text without.
fn channel_json(guild: Id<GuildMarker>, id: u64, bitrate: Option<u32>) -> serde_json::Value {
    let mut channel = json!({
        "id": id.to_string(),
        "guild_id": guild.to_string(),
        "type": if bitrate.is_some() { 2 } else { 0 },
        "name": format!("c{id}"),
        "position": 0,
        "permission_overwrites": [],
    });
    if let Some(bitrate) = bitrate {
        channel["bitrate"] = json!(bitrate);
        channel["user_limit"] = json!(0);
    }
    channel
}

/// Caches `guild()` with V (10, 96 kbps), V2 (11, 32 kbps) and a text
/// channel (12); channel 13 is never cached.
fn seed_cache(running: &Running) {
    let g = guild();
    running.gw.dispatch(
        "GUILD_CREATE",
        json!({
            "id": g.to_string(), "name": "G", "owner_id": "1", "preferred_locale": "en-US",
            "afk_timeout": 300, "default_message_notifications": 0,
            "explicit_content_filter": 0, "features": [], "mfa_level": 0, "nsfw_level": 0,
            "premium_progress_bar_enabled": false, "premium_tier": 0,
            "system_channel_flags": 0, "verification_level": 0, "emojis": [], "stickers": [],
            "channels": [
                channel_json(g, 10, Some(96_000)),
                channel_json(g, 11, Some(32_000)),
                channel_json(g, 12, None),
            ],
            "roles": [], "threads": [], "members": [], "voice_states": [], "presences": [],
            "stage_instances": [], "guild_scheduled_events": [],
        }),
    );
}

/// A `CHANNEL_UPDATE` of `channel` in `guild` with `bitrate`.
fn channel_update(running: &Running, guild: Id<GuildMarker>, channel: u64, bitrate: u32) {
    running.gw.dispatch(
        "CHANNEL_UPDATE",
        channel_json(guild, channel, Some(bitrate)),
    );
}

async fn bitrate_is(running: &Running, want: u32) {
    until(&format!("bitrate {want}"), || {
        running.shared.bitrate(guild()) == Some(want)
    })
    .await;
}

/// The encoder follows the call's channel bitrate: the bot's own voice
/// state (from the cache), `CHANNEL_UPDATE` of that channel, a move; other
/// channels, other guilds and others' voice states change nothing.
#[tokio::test(flavor = "multi_thread")]
async fn bitrate_follows_the_channel() {
    let running = Running::start(
        "voice-bitrate",
        VoiceConfig::default().join_timeout(Duration::from_mins(1)),
    )
    .await;
    seed_cache(&running);
    let _call = running.pending_call().await;
    assert_eq!(running.shared.bitrate(guild()), Some(64_000), "unknown yet");
    // Another member's voice state in V: ignored.
    running.gw.dispatch(
        "VOICE_STATE_UPDATE",
        json!({
            "guild_id": guild().to_string(), "channel_id": "11", "user_id": "99",
            "session_id": "s", "deaf": false, "mute": false, "self_deaf": false,
            "self_mute": false, "self_video": false, "suppress": false,
            "request_to_speak_timestamp": null
        }),
    );
    bot_voice_state(&running, 10);
    bitrate_is(&running, 96_000).await;
    channel_update(&running, guild(), 10, 128_000);
    bitrate_is(&running, 128_000).await;
    // Another channel of G, and a channel of a guild with no call.
    channel_update(&running, guild(), 12, 8_000);
    channel_update(&running, Id::new(7), 70, 8_000);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(running.shared.bitrate(guild()), Some(128_000));
    assert_eq!(running.shared.bitrate(Id::new(7)), None);
    // A move to V2 (cached), then to an uncached channel.
    bot_voice_state(&running, 11);
    bitrate_is(&running, 32_000).await;
    bot_voice_state(&running, 13);
    bitrate_is(&running, 64_000).await;
    running.stop().await;
}

/// `max_bitrate` caps the channel's bitrate.
#[tokio::test(flavor = "multi_thread")]
async fn max_bitrate_caps_the_channel() {
    let running = Running::start(
        "voice-bitrate-cap",
        VoiceConfig::default()
            .join_timeout(Duration::from_mins(1))
            .max_bitrate(Some(64_000)),
    )
    .await;
    seed_cache(&running);
    let _call = running.pending_call().await;
    bot_voice_state(&running, 11);
    bitrate_is(&running, 32_000).await;
    bot_voice_state(&running, 10);
    bitrate_is(&running, 64_000).await;
    running.stop().await;
}

/// Without a Discord cache the channel's bitrate is unknown (64 kbps)
/// until a `CHANNEL_UPDATE` names it.
#[tokio::test(flavor = "multi_thread")]
async fn bitrate_without_a_cache() {
    let running = Running::start_with(
        "voice-bitrate-nocache",
        VoiceConfig::default().join_timeout(Duration::from_mins(1)),
        |builder| {
            builder.discord_cache(noctis_core::DiscordCacheConfig::none());
        },
    )
    .await;
    seed_cache(&running);
    let call = running.pending_call().await;
    bot_voice_state(&running, 11);
    until("the move", || call.channel() == Some(Id::new(11))).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(running.shared.bitrate(guild()), Some(64_000));
    channel_update(&running, guild(), 11, 128_000);
    bitrate_is(&running, 128_000).await;
    running.stop().await;
}
