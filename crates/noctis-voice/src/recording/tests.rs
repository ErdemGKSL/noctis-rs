//! The recording driver end to end (Story 17.4): an App with
//! `VoiceConfig::recording()` on `FakeGateway` (`answer_voice` on unless a
//! row says otherwise) and `FakeDiscordHttp`. Joins resolve on the fake's
//! op 4 answers; queues play on the recorder.

use std::time::{Duration, Instant};

use noctis_core::App;
use noctis_testkit::fake::FakeGatewayConfig;
use serde_json::{Value, json};
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use super::{RecordedCommand, RecordedEvent, VoiceRecorder};
use crate::tests::{Running, WAIT, guild, track, until};
use crate::{
    AppBuilderVoiceExt as _, CallHandle, EndReason, QueueEmpty, Track, TrackEnd, TrackError,
    TrackInfo, TrackStart, VoiceConfig, VoiceError, VoiceExt as _,
};

/// A second guild, owned by shard 0 of 2.
fn guild2() -> Id<GuildMarker> {
    Id::new((2 << 22) | 7)
}

/// A recording config and its recorder.
fn recording(config: VoiceConfig) -> (VoiceConfig, VoiceRecorder) {
    let recorder = config
        .recorder()
        .unwrap_or_else(|| panic!("a recording config"));
    (config, recorder)
}

/// A running App on a recording config with a 15 s join timeout.
async fn start(app: &str) -> (Running, VoiceRecorder) {
    let (config, recorder) =
        recording(VoiceConfig::recording().join_timeout(Duration::from_secs(15)));
    (Running::start(app, config).await, recorder)
}

/// Joins `guild`'s channel `channel` (auto-answered).
async fn join(running: &Running, guild: Id<GuildMarker>, channel: u64) -> CallHandle {
    tokio::time::timeout(WAIT, running.voice.join(guild, Id::new(channel)))
        .await
        .unwrap_or_else(|_| panic!("the join resolves within the wait"))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Switches the gateway fake's `answer_voice`.
fn answer(running: &Running, on: bool) {
    running.gw.set_config(FakeGatewayConfig {
        answer_voice: on,
        ..running.gw.config()
    });
}

/// The bot's own `VOICE_STATE_UPDATE` in `guild`'s `channel` (or none).
fn bot_state(running: &Running, guild: Id<GuildMarker>, channel: Option<u64>) {
    running.gw.dispatch(
        "VOICE_STATE_UPDATE",
        json!({
            "guild_id": guild.to_string(),
            "channel_id": channel.map(|c| c.to_string()),
            "user_id": "1",
            "session_id": "manual-session",
            "deaf": false, "mute": false, "self_deaf": true, "self_mute": false,
            "self_video": false, "suppress": false, "request_to_speak_timestamp": null
        }),
    );
}

/// The `VOICE_SERVER_UPDATE` of `guild` with `endpoint`.
fn server(running: &Running, guild: Id<GuildMarker>, endpoint: Option<&str>) {
    running.gw.dispatch(
        "VOICE_SERVER_UPDATE",
        json!({ "guild_id": guild.to_string(), "token": "t", "endpoint": endpoint }),
    );
}

fn enqueue_now(call: &CallHandle, track: Track) -> TrackInfo {
    futures_util::FutureExt::now_or_never(call.enqueue(track))
        .unwrap_or_else(|| panic!("enqueue is ready at once"))
        .unwrap_or_else(|e| panic!("{e}"))
}

fn play(info: &TrackInfo, volume: f32) -> RecordedCommand {
    RecordedCommand::Play {
        track: info.clone(),
        volume,
    }
}

fn start_event(guild: Id<GuildMarker>, track: &TrackInfo) -> RecordedEvent {
    RecordedEvent::Start(TrackStart {
        guild,
        track: track.clone(),
    })
}

fn end_event(guild: Id<GuildMarker>, track: &TrackInfo, reason: EndReason) -> RecordedEvent {
    RecordedEvent::End(TrackEnd {
        guild,
        track: track.clone(),
        reason,
    })
}

fn empty_event(guild: Id<GuildMarker>) -> RecordedEvent {
    RecordedEvent::Empty(QueueEmpty { guild })
}

/// Join auto-answered, the same channel again, a move, a leave: one op 4
/// each (none for the same channel), the commands kept across the move.
#[tokio::test(flavor = "multi_thread")]
async fn join_same_channel_move_and_leave() {
    let (running, recorder) = start("voice-rec-join").await;
    let g = guild();
    let call = join(&running, g, 10).await;
    assert_eq!(call.channel(), Some(Id::new(10)));
    assert_eq!(running.voice.active_calls(), 1);
    assert_eq!(running.op4_channels(), [json!("10")]);

    let again = join(&running, g, 10).await;
    assert_eq!(again.generation, call.generation);
    assert_eq!(running.op4_channels(), [json!("10")], "no new op 4");

    let a = enqueue_now(&call, track("a"));
    let moved = join(&running, g, 11).await;
    assert_eq!(moved.generation, call.generation);
    assert_eq!(moved.channel(), Some(Id::new(11)));
    assert_eq!(running.op4_channels(), [json!("10"), json!("11")]);
    assert_eq!(
        recorder.commands(g),
        [play(&a, 1.0)],
        "kept across the move"
    );

    moved.leave().await.unwrap_or_else(|e| panic!("{e}"));
    assert!(running.voice.call(g).is_none());
    until("the op 4 leave", || {
        running.op4_channels() == [json!("10"), json!("11"), Value::Null]
    })
    .await;
    assert_eq!(recorder.commands(g), [play(&a, 1.0), RecordedCommand::Stop]);
    assert!(recorder.events(g).is_empty(), "a leave fires no event");
    running.stop().await;
}

/// Manual answers complete a pending join in either order; a `null`
/// endpoint is ignored until a real one arrives.
#[tokio::test(flavor = "multi_thread")]
async fn join_on_manual_answers() {
    let (running, _recorder) = start("voice-rec-manual").await;
    answer(&running, false);
    for (g, state_first) in [(guild(), true), (guild2(), false)] {
        let pending = tokio::spawn(running.voice.join(g, Id::new(10)));
        until("the op 4", || {
            running.gw.frames().iter().any(|f| {
                f.op() == Some(4)
                    && f.json()
                        .is_some_and(|v| v["d"]["guild_id"] == json!(g.to_string()))
            })
        })
        .await;
        if state_first {
            bot_state(&running, g, Some(10));
            server(&running, g, None);
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(!pending.is_finished(), "a null endpoint is ignored");
            server(&running, g, Some("voice.example.invalid:443"));
        } else {
            server(&running, g, Some("voice.example.invalid:443"));
            bot_state(&running, g, Some(10));
        }
        let call = tokio::time::timeout(WAIT, pending)
            .await
            .unwrap_or_else(|_| panic!("resolves within the wait"))
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(call.channel(), Some(Id::new(10)));
    }
    assert_eq!(running.voice.active_calls(), 2);
    running.stop().await;
}

/// No answer: `Timeout` after the join timeout, the call removed and op 4
/// leave sent.
#[tokio::test(flavor = "multi_thread")]
async fn join_without_an_answer_times_out() {
    let (config, _recorder) =
        recording(VoiceConfig::recording().join_timeout(Duration::from_secs(1)));
    let running = Running::start("voice-rec-timeout", config).await;
    answer(&running, false);
    let error = running.voice.join(guild(), Id::new(10)).await.err();
    assert_eq!(
        error,
        Some(VoiceError::Timeout {
            guild: guild(),
            after: Duration::from_secs(1)
        })
    );
    assert_eq!(running.voice.active_calls(), 0);
    until("the op 4 leave", || {
        running.op4_channels() == [json!("10"), Value::Null]
    })
    .await;
    running.stop().await;
}

/// Before `run` a recording join is still the gateway's `NotRunning`.
#[tokio::test]
async fn join_before_run_is_refused() {
    let mut builder = App::builder();
    builder.voice(VoiceConfig::recording());
    let app = builder.build().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        app.voice().join(guild(), Id::new(10)).await.err(),
        Some(VoiceError::Gateway(
            noctis_gateway::GatewayError::NotRunning
        ))
    );
    assert_eq!(app.voice().active_calls(), 0);
}

/// Enqueue, start (once), finish with a next track, fail, finish the last:
/// the commands and events in order, the hooks see the same events.
#[tokio::test(flavor = "multi_thread")]
async fn reports_drive_the_queue() {
    let (running, recorder) = start("voice-rec-reports").await;
    let g = guild();
    assert_eq!(recorder.start(g), None, "no call");
    let call = join(&running, g, 10).await;
    assert_eq!(recorder.start(g), None, "an empty queue");

    let a = enqueue_now(&call, track("a"));
    assert_eq!(recorder.commands(g), [play(&a, 1.0)]);
    assert!(recorder.events(g).is_empty(), "no TrackStart before start");
    assert_eq!(call.now_playing(), Some(a.clone()));

    assert_eq!(recorder.start(g), Some(a.clone()));
    assert_eq!(recorder.start(g), Some(a.clone()), "a second start");
    assert_eq!(recorder.events(g), [start_event(g, &a)]);

    let b = enqueue_now(&call, track("b"));
    let c = enqueue_now(&call, track("c"));
    assert_eq!(recorder.finish(g), Some(a.clone()));
    assert_eq!(
        recorder.commands(g),
        [play(&a, 1.0), play(&b, 1.0)],
        "a finished track needs no Stop"
    );
    assert_eq!(recorder.fail(g, "boom"), Some(b.clone()));
    assert_eq!(recorder.finish(g), Some(c.clone()));
    assert_eq!(recorder.finish(g), None, "nothing current");
    assert_eq!(recorder.fail(g, "late"), None);
    let error = RecordedEvent::Error(TrackError {
        guild: g,
        track: b.clone(),
        error: "boom".to_owned(),
    });
    assert_eq!(
        recorder.events(g),
        [
            start_event(g, &a),
            end_event(g, &a, EndReason::Finished),
            error,
            end_event(g, &c, EndReason::Finished),
            empty_event(g),
        ]
    );
    assert_eq!(
        recorder.commands(g),
        [
            play(&a, 1.0),
            play(&b, 1.0),
            RecordedCommand::Stop,
            play(&c, 1.0)
        ]
    );
    let empty = format!("empty {g}");
    assert_eq!(
        running.seen(5).await,
        [
            "start a",
            "end a finished",
            "error b boom",
            "end c finished",
            empty.as_str()
        ]
    );
    running.stop().await;
}

/// The `CallHandle` operations as commands and events; nothing after
/// `stop()`; guilds never mix; an unknown guild reads empty.
#[tokio::test(flavor = "multi_thread")]
async fn handle_operations_are_recorded() {
    let (running, recorder) = start("voice-rec-ops").await;
    let (g, g2) = (guild(), guild2());
    let call = join(&running, g, 10).await;
    let other = join(&running, g2, 20).await;
    let a = enqueue_now(&call, track("a"));
    let b = enqueue_now(&call, track("b"));
    let x = enqueue_now(&other, track("x"));
    assert!(call.pause().unwrap_or_default());
    assert!(call.resume().unwrap_or_default());
    call.set_volume(0.5).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(call.skip().ok().flatten(), Some(a.clone()));
    assert_eq!(call.stop().ok(), Some(1));
    assert_eq!(recorder.start(g), None, "nothing after stop()");
    assert_eq!(
        recorder.commands(g),
        [
            play(&a, 1.0),
            RecordedCommand::Pause,
            RecordedCommand::Resume,
            RecordedCommand::SetVolume(0.5),
            RecordedCommand::Stop,
            play(&b, 0.5),
            RecordedCommand::Stop,
        ]
    );
    assert_eq!(
        recorder.events(g),
        [
            end_event(g, &a, EndReason::Skipped),
            end_event(g, &b, EndReason::Stopped),
            empty_event(g),
        ]
    );
    assert_eq!(recorder.commands(g2), [play(&x, 1.0)]);
    assert!(recorder.events(g2).is_empty());
    let unknown = Id::new(3 << 22);
    assert!(recorder.commands(unknown).is_empty());
    assert!(recorder.events(unknown).is_empty());
    assert!(format!("{recorder:?}").contains("guilds: 2"));
    running.stop().await;
}

/// A unicode title and a 300-char title are recorded as the queue holds
/// them (the long one cut to 256 chars).
#[tokio::test(flavor = "multi_thread")]
async fn titles_are_recorded_as_queued() {
    let (running, recorder) = start("voice-rec-titles").await;
    let g = guild();
    let call = join(&running, g, 10).await;
    let unicode = enqueue_now(&call, track("x").title("Gülümse 🎵"));
    let long = enqueue_now(&call, track("y").title("é".repeat(300)));
    assert_eq!(long.title().map(|t| t.chars().count()), Some(256));
    assert_eq!(recorder.finish(g), Some(unicode.clone()));
    let commands = recorder.commands(g);
    let titles: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            RecordedCommand::Play { track, .. } => track.title().map(str::to_owned),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["Gülümse 🎵".to_owned(), "é".repeat(256)]);
    running.stop().await;
}

/// Shutdown with a live call: the auto-answer confirms the leave, so the
/// run ends without waiting out the 5 s leave budget.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_leave_is_confirmed() {
    let (running, recorder) = start("voice-rec-shutdown").await;
    let g = guild();
    let call = join(&running, g, 10).await;
    let a = enqueue_now(&call, track("a"));
    let gw = std::sync::Arc::clone(&running.gw);
    let began = Instant::now();
    running.stop().await;
    assert!(
        began.elapsed() < Duration::from_secs(4),
        "{:?}",
        began.elapsed()
    );
    let channels: Vec<Value> = gw
        .frames()
        .iter()
        .filter(|f| f.op() == Some(4))
        .filter_map(|f| f.json().map(|v| v["d"]["channel_id"].clone()))
        .collect();
    assert_eq!(channels, [json!("10"), Value::Null]);
    assert_eq!(recorder.commands(g), [play(&a, 1.0), RecordedCommand::Stop]);
}

/// Each recording App gets its own Songbird `Scheduler` (`VoiceShared::driver_config`):
/// a second App on a fresh runtime still joins and plays after the first App's runtime
/// was dropped (a shared scheduler would be bound to the dead runtime).
#[test]
fn recording_apps_survive_a_dropped_runtime() {
    for app in ["voice-rec-rt-1", "voice-rec-rt-2"] {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|e| panic!("{e}"));
        runtime.block_on(async {
            let (running, recorder) = start(app).await;
            let g = guild();
            let call = join(&running, g, 10).await;
            let a = enqueue_now(&call, track("a"));
            assert_eq!(recorder.commands(g), [play(&a, 1.0)]);
            running.stop().await;
        });
        drop(runtime);
    }
}
