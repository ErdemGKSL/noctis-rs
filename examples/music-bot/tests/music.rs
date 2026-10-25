//! The music bot end to end through `noctis::testkit` with the recording driver: every
//! run is a simulated interaction through the real pipeline; joins send op 4 through
//! the gateway fake and resolve on its answers; queues play on a `VoiceRecorder`. No
//! UDP, no `yt-dlp`, no network.
//!
//! Fixture: guild `G` with text channel `C` and voice channels `V` and `V2`; user `A`
//! is in `V` (`GET /guilds/10/voice-states/40`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::time::{Duration, Instant};

use noctis::publish::PublishScope;
use noctis::testkit::fake::{
    FakeDiscordHttp, FakeResponse, GatewayBot, Method, RecordedRequest, StatusCode,
};
use noctis::testkit::voice::{RecordedCommand, RecordedEvent, VoiceRecorder};
use noctis::testkit::{Response, ResponseKind, TestApp, TestError, TestGuild, TestUser};
use noctis::twilight::model::channel::ChannelType;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use noctis::ui::ButtonStyle;
use noctis::voice::{AppBuilderVoiceExt as _, EndReason, TrackSource, VoiceConfig};
use noctis::{App, Runtime, Token};
use serde_json::{Value, json};

const G: Id<GuildMarker> = Id::new(10);
const C: Id<ChannelMarker> = Id::new(20);
const V: Id<ChannelMarker> = Id::new(50);
const V2: Id<ChannelMarker> = Id::new(51);
const A: Id<UserMarker> = Id::new(40);

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// The music bot with the recording driver (the last `.voice(..)` wins), the fixture
/// seeded, and the gateway fake answering op 4 when `answer` is set.
async fn start(answer: bool) -> (TestApp, VoiceRecorder) {
    let config = VoiceConfig::recording();
    let recorder = config.recorder().expect("a recording config");
    let mut builder = App::builder();
    music_bot::configure(&mut builder).voice(config);
    let app = TestApp::build(&mut builder)
        .await
        .unwrap()
        .answer_voice(answer)
        .guild(
            TestGuild::new(G, "Guild")
                .text_channel(C, "music")
                .channel(V, "Lounge", ChannelType::GuildVoice)
                .channel(V2, "Stage", ChannelType::GuildVoice),
        )
        .user(TestUser::new(A).name("ana"));
    in_voice(&app, V);
    (app, recorder)
}

/// Registers `A`'s voice state in `channel` (the last registration wins).
fn in_voice(app: &TestApp, channel: Id<ChannelMarker>) {
    let state = json!({
        "guild_id": G.to_string(),
        "channel_id": channel.to_string(),
        "user_id": A.to_string(),
        "session_id": "user-session",
        "deaf": false, "mute": false, "self_deaf": false, "self_mute": false,
        "self_video": false, "suppress": false, "request_to_speak_timestamp": null
    });
    app.http_fake().route(
        Method::GET,
        format!("/guilds/{G}/voice-states/{A}"),
        FakeResponse::json(StatusCode::OK, &state),
    );
}

async fn play(app: &TestApp, query: &str) -> Result<Response, TestError> {
    app.command("play")
        .option("query", query)
        .by(A)
        .in_guild(G)
        .in_channel(C)
        .run()
        .await
}

async fn command(app: &TestApp, name: &str) -> Result<Response, TestError> {
    app.command(name)
        .by(A)
        .in_guild(G)
        .in_channel(C)
        .run()
        .await
}

/// The `channel_id` of every op 4 the App sent.
fn op4_channels(app: &TestApp) -> Vec<Value> {
    app.voice_state_updates()
        .iter()
        .map(|d| d["channel_id"].clone())
        .collect()
}

/// An ephemeral one-line reply.
fn private(reply: &Response, text: &str) {
    assert!(reply.is_ephemeral(), "{}", reply.text());
    assert_eq!(reply.text(), text);
}

/// The titles of the recorded `Play` commands.
fn played(recorder: &VoiceRecorder) -> Vec<String> {
    recorder
        .commands(G)
        .iter()
        .filter_map(|c| match c {
            RecordedCommand::Play { track, .. } => track.title().map(str::to_owned),
            _ => None,
        })
        .collect()
}

/// `/play` joins A's channel and answers with the public player.
#[tokio::test(flavor = "multi_thread")]
async fn play_joins_and_shows_the_player() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    let reply = play(&app, "never gonna").await?;
    assert!(!reply.is_ephemeral());
    assert_eq!(reply.kind(), ResponseKind::Reply);
    let text = reply.text();
    assert!(text.contains("## Now playing"), "{text}");
    assert!(text.contains("**never gonna**"), "{text}");
    assert!(text.contains("-# Requested by <@40>"), "{text}");
    for label in ["Pause", "Skip", "Stop"] {
        reply.button(label)?;
    }
    assert_eq!(op4_channels(&app), [json!("50")]);
    let commands = recorder.commands(G);
    let [RecordedCommand::Play { track, volume }] = commands.as_slice() else {
        panic!("{commands:?}");
    };
    assert_eq!(track.title(), Some("never gonna"));
    assert_eq!(track.requested_by(), Some(A));
    assert!(matches!(track.source(), TrackSource::Command(p) if *p == *"yt-dlp"));
    assert!((volume - 1.0).abs() < f32::EPSILON);
    app.stop().await
}

/// No voice state (Discord's 404), a blank query, a call in another channel.
#[tokio::test(flavor = "multi_thread")]
async fn play_refusals() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    app.http_fake().route(
        Method::GET,
        format!("/guilds/{G}/voice-states/{A}"),
        FakeResponse::json(
            StatusCode::NOT_FOUND,
            &json!({ "message": "Unknown Voice State", "code": 10065 }),
        ),
    );
    private(
        &play(&app, "never gonna").await?,
        "Join a voice channel first.",
    );
    private(&play(&app, "   ").await?, "Type a song name or a link.");
    assert!(app.voice_state_updates().is_empty());
    assert!(recorder.commands(G).is_empty());

    in_voice(&app, V2);
    play(&app, "first").await?;
    in_voice(&app, V);
    private(
        &play(&app, "second").await?,
        "I'm already playing in <#51>.",
    );
    assert_eq!(op4_channels(&app), [json!("51")]);
    assert_eq!(played(&recorder), ["first"]);
    app.stop().await
}

/// A second `/play` queues behind the first in the same call.
#[tokio::test(flavor = "multi_thread")]
async fn play_twice_queues() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    play(&app, "never gonna").await?;
    let second = play(&app, "lofi beats").await?;
    let text = second.text();
    assert!(text.contains("**never gonna**"), "{text}");
    assert!(text.contains("1. lofi beats"), "{text}");
    assert!(text.contains("-# Page 1 of 1 · 1 queued"), "{text}");
    assert_eq!(op4_channels(&app), [json!("50")]);
    assert_eq!(played(&recorder), ["never gonna"]);
    app.stop().await
}

/// The control commands: one private line each, the recorder's commands, op 4 leave.
#[tokio::test(flavor = "multi_thread")]
async fn control_commands() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    for name in ["pause", "resume", "skip", "stop", "leave"] {
        private(&command(&app, name).await?, "I'm not in a voice channel.");
    }
    private(
        &app.command("volume")
            .option("percent", 50_i64)
            .by(A)
            .in_guild(G)
            .run()
            .await?,
        "I'm not in a voice channel.",
    );
    for query in ["never gonna", "lofi beats", "third"] {
        play(&app, query).await?;
    }
    private(&command(&app, "pause").await?, "Paused.");
    private(&command(&app, "pause").await?, "Already paused.");
    private(&command(&app, "resume").await?, "Resumed.");
    private(&command(&app, "resume").await?, "Already playing.");
    private(&command(&app, "skip").await?, "Skipped never gonna.");
    let volume = |percent: i64| {
        app.command("volume")
            .option("percent", percent)
            .by(A)
            .in_guild(G)
            .run()
    };
    private(&volume(50).await?, "Volume set to 50%.");
    private(&volume(0).await?, "Volume set to 0%.");
    private(&volume(200).await?, "Volume set to 200%.");
    private(&command(&app, "stop").await?, "Stopped; 2 tracks removed.");
    private(&command(&app, "skip").await?, "Nothing is playing.");
    private(&command(&app, "pause").await?, "Nothing is playing.");
    private(&command(&app, "leave").await?, "Left the voice channel.");
    // `/leave` with nothing current stops nothing.
    let commands = recorder.commands(G);
    assert_eq!(commands.len(), 9, "{commands:?}");
    assert!(
        matches!(&commands[0], RecordedCommand::Play { track, .. } if track.title() == Some("never gonna"))
    );
    assert_eq!(
        commands[1..4],
        [
            RecordedCommand::Pause,
            RecordedCommand::Resume,
            RecordedCommand::Stop
        ]
    );
    assert!(
        matches!(&commands[4], RecordedCommand::Play { track, .. } if track.title() == Some("lofi beats"))
    );
    assert_eq!(
        commands[5..],
        [
            RecordedCommand::SetVolume(0.5),
            RecordedCommand::SetVolume(0.0),
            RecordedCommand::SetVolume(2.0),
            RecordedCommand::Stop,
        ]
    );
    assert_eq!(op4_channels(&app), [json!("50"), Value::Null]);
    app.stop().await
}

/// `/volume` declares 0..=200 (Discord refuses other values before the bot sees
/// them): the published command carries the bounds.
#[tokio::test(flavor = "multi_thread")]
async fn volume_bounds_are_declared() -> Result<(), Box<dyn std::error::Error>> {
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await?;
    http.route(
        Method::GET,
        "/applications/@me",
        FakeResponse::json(
            StatusCode::OK,
            &json!({"id": "7", "integration_types_config": {"0": {}}}),
        ),
    )
    .route(
        Method::GET,
        "/applications/7/commands",
        FakeResponse::json(StatusCode::OK, &json!([])),
    )
    .route_with(Method::PUT, "/applications/7/commands", |request| {
        FakeResponse::json(StatusCode::OK, &request.json().unwrap_or(Value::Null))
    });
    let mut builder = App::builder();
    music_bot::configure(&mut builder)
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token")?)
        .http_proxy(http.url());
    builder.build()?.publish(PublishScope::Global).await?;
    let body = http
        .requests()
        .iter()
        .find(|r| r.method == Method::PUT)
        .and_then(RecordedRequest::json)
        .expect("the PUT of the global commands");
    let volume = body
        .as_array()
        .and_then(|commands| commands.iter().find(|c| c["name"] == "volume"))
        .expect("/volume");
    let percent = &volume["options"][0];
    assert_eq!(percent["name"], "percent");
    assert_eq!(
        (&percent["min_value"], &percent["max_value"]),
        (&json!(0), &json!(200))
    );
    Ok(())
}

/// Pause, Resume, Skip and Stop on the player update it in place.
#[tokio::test(flavor = "multi_thread")]
async fn player_buttons_update_in_place() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    play(&app, "a").await?;
    let player = play(&app, "b").await?;

    let paused = app.click(player.button("Pause")?).by(A).run().await?;
    assert_eq!(paused.kind(), ResponseKind::Update);
    let resume = paused.button("Resume")?;
    assert_eq!(resume.style(), Some(ButtonStyle::Primary));
    assert!(paused.text().contains("-# Paused · Volume 100%"));
    let resumed = app.click(resume).by(A).run().await?;
    assert_eq!(resumed.kind(), ResponseKind::Update);
    resumed.button("Pause")?;

    let skipped = app.click(resumed.button("Skip")?).by(A).run().await?;
    assert_eq!(skipped.kind(), ResponseKind::Update);
    assert!(skipped.text().contains("**b**"), "{}", skipped.text());

    let stopped = app.click(skipped.button("Stop")?).by(A).run().await?;
    assert_eq!(stopped.kind(), ResponseKind::Update);
    assert!(stopped.text().contains("Nothing is playing."));
    assert!(stopped.buttons().is_empty());

    let commands = recorder.commands(G);
    assert_eq!(
        commands[1..3],
        [RecordedCommand::Pause, RecordedCommand::Resume]
    );
    let events = recorder.events(G);
    let reasons: Vec<EndReason> = events
        .iter()
        .filter_map(|e| match e {
            RecordedEvent::End(end) => Some(end.reason),
            _ => None,
        })
        .collect();
    assert_eq!(reasons, [EndReason::Skipped, EndReason::Stopped]);
    assert!(matches!(events.last(), Some(RecordedEvent::Empty(_))));
    app.stop().await
}

/// 30 queued: `/queue`, Next, Previous; a click after `/leave` shows the empty state.
#[tokio::test(flavor = "multi_thread")]
async fn queue_pages_and_stale_player() -> Result<(), TestError> {
    let (app, _recorder) = start(true).await;
    for n in 0..=30 {
        play(&app, &format!("track {n}")).await?;
    }
    let first = command(&app, "queue").await?;
    assert!(!first.is_ephemeral());
    assert!(first.text().contains("1. track 1\n"), "{}", first.text());
    assert!(first.button("Previous")?.is_disabled());
    let second = app.click(first.button("Next")?).by(A).run().await?;
    assert_eq!(second.kind(), ResponseKind::Update);
    assert!(second.text().contains("-# Page 2 of"), "{}", second.text());
    assert!(!second.button("Previous")?.is_disabled());
    let back = app.click(second.button("Previous")?).by(A).run().await?;
    assert!(back.text().contains("-# Page 1 of"));
    assert!(back.button("Previous")?.is_disabled());

    private(&command(&app, "leave").await?, "Left the voice channel.");
    let stale = app.click(back.button("Next")?).by(A).run().await?;
    assert_eq!(stale.kind(), ResponseKind::Update);
    assert!(stale.text().contains("Nothing is playing."));
    app.stop().await
}

/// The driver reports through the recorder: start, finish (the next track plays),
/// fail (the queue empties); the events in order.
#[tokio::test(flavor = "multi_thread")]
async fn recorder_reports_track_events() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    play(&app, "a").await?;
    play(&app, "b").await?;
    assert_eq!(
        recorder
            .start(G)
            .and_then(|t| t.title().map(str::to_owned))
            .as_deref(),
        Some("a")
    );
    assert!(recorder.finish(G).is_some());
    assert_eq!(played(&recorder), ["a", "b"]);
    assert!(recorder.fail(G, "boom").is_some());
    let events: Vec<String> = recorder
        .events(G)
        .iter()
        .map(|e| match e {
            RecordedEvent::Start(s) => format!("start {}", s.track.title().unwrap_or("?")),
            RecordedEvent::End(end) => {
                format!(
                    "end {} {}",
                    end.track.title().unwrap_or("?"),
                    end.reason.as_str()
                )
            }
            RecordedEvent::Error(error) => {
                format!(
                    "error {} {}",
                    error.track.title().unwrap_or("?"),
                    error.error
                )
            }
            RecordedEvent::Empty(_) => "empty".to_owned(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        events,
        ["start a", "end a finished", "error b boom", "empty"]
    );
    app.stop().await
}

/// A 512-char query: the track's title is its first 256 chars.
#[tokio::test(flavor = "multi_thread")]
async fn long_query_title_is_cut() -> Result<(), TestError> {
    let (app, recorder) = start(true).await;
    let query = "q".repeat(512);
    play(&app, &query).await?;
    assert_eq!(played(&recorder), ["q".repeat(256)]);
    app.stop().await
}

/// Without the auto-answer the test answers op 4 itself, concurrently with `/play`.
#[tokio::test(flavor = "multi_thread")]
async fn manual_voice_answers() -> Result<(), TestError> {
    let (app, recorder) = start(false).await;
    let injector = async {
        let deadline = Instant::now() + WAIT;
        while app.voice_state_updates().is_empty() {
            assert!(Instant::now() < deadline, "no op 4 within the wait");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        app.dispatch_bot_voice_state(G, Some(V));
        app.dispatch_voice_server(G);
    };
    let (reply, ()) = tokio::join!(play(&app, "never gonna"), injector);
    let reply = reply?;
    assert!(reply.text().contains("**never gonna**"), "{}", reply.text());
    assert_eq!(op4_channels(&app), [json!("50")]);
    assert_eq!(played(&recorder), ["never gonna"]);
    app.stop().await
}
