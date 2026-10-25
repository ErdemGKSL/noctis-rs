//! The `voice` feature (Stories 17.1, 17.2, 17.4): `.voice(..)` adds `GUILD_VOICE_STATES`;
//! `/join`, `/leave`, `/play`, `/skip` and `/stop` are mounted and answer privately. `TestApp` runs on the fake
//! gateway, so `/leave` sends op 4 with no channel and succeeds. With `VoiceConfig::recording()` and
//! `TestApp::answer_voice(true)` a join resolves without UDP and the queue plays on a `VoiceRecorder`.
#![cfg(feature = "voice")]

use noctis::prelude::*;
use noctis::testkit::fake::{FakeResponse, Method, StatusCode};
use noctis::testkit::voice::RecordedCommand;
use noctis::testkit::{TestApp, TestGuild, TestUser};
use noctis::twilight::model::channel::ChannelType;
use noctis::twilight::model::gateway::Intents;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use noctis::voice::VoiceConfig;
use noctis_bot_template::configure;
use serde_json::json;

const GUILD: Id<GuildMarker> = Id::new(100);
const USER: Id<UserMarker> = Id::new(200);
const LOUNGE: Id<ChannelMarker> = Id::new(300);

#[tokio::test(flavor = "multi_thread")]
async fn voice_adds_the_intent() -> anyhow::Result<()> {
    let app = configure(&mut App::builder()).build()?;
    assert!(app.intents().contains(Intents::GUILD_VOICE_STATES));
    assert!(app.modules().ids().any(|id| id == "voice"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn leave_answers_privately() -> anyhow::Result<()> {
    let app = TestApp::build(configure(&mut App::builder()))
        .await?
        .guild(TestGuild::new(GUILD, "Test Guild"))
        .user(TestUser::new(USER));
    let left = app.command("leave").by(USER).in_guild(GUILD).run().await?;
    assert!(left.is_ephemeral());
    assert_eq!(left.text(), "Left the voice channel.");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_commands_exist() -> anyhow::Result<()> {
    let app = TestApp::build(configure(&mut App::builder()))
        .await?
        .guild(TestGuild::new(GUILD, "Test Guild"))
        .user(TestUser::new(USER));
    // Without a call each queue command asks to join first, privately.
    let play = app
        .command("play")
        .option("url", "https://cdn.example.com/a.ogg")
        .by(USER)
        .in_guild(GUILD)
        .run()
        .await?;
    // `/play` looks the author up first; the fake answers Discord's 404 (in no channel).
    assert!(play.is_ephemeral());
    assert_eq!(play.text(), "Join a voice channel first.");
    assert!(app.voice_state_updates().is_empty());
    let skip = app.command("skip").by(USER).in_guild(GUILD).run().await?;
    let stop = app.command("stop").by(USER).in_guild(GUILD).run().await?;
    for reply in [skip, stop] {
        assert!(reply.is_ephemeral());
        assert_eq!(reply.text(), "Use /join first.");
    }
    Ok(())
}

/// `/play` without a call joins the author's channel: one op 4 through the gateway fake
/// (answered by it), and the recording driver gets the track. No UDP, no media fetch.
#[tokio::test(flavor = "multi_thread")]
async fn play_joins_the_authors_channel() -> anyhow::Result<()> {
    let config = VoiceConfig::recording();
    let recorder = config.recorder().expect("a recording config");
    let mut builder = App::builder();
    configure(&mut builder).voice(config); // the last `.voice(..)` wins
    let app = TestApp::build(&mut builder)
        .await?
        .answer_voice(true)
        .guild(TestGuild::new(GUILD, "Test Guild").channel(
            LOUNGE,
            "Lounge",
            ChannelType::GuildVoice,
        ))
        .user(TestUser::new(USER));
    let state = json!({
        "guild_id": GUILD.to_string(), "channel_id": LOUNGE.to_string(),
        "user_id": USER.to_string(), "session_id": "s", "deaf": false, "mute": false,
        "self_deaf": false, "self_mute": false, "self_video": false, "suppress": false,
        "request_to_speak_timestamp": null
    });
    app.http_fake().route(
        Method::GET,
        format!("/guilds/{GUILD}/voice-states/{USER}"),
        FakeResponse::json(StatusCode::OK, &state),
    );
    let reply = app
        .command("play")
        .option("url", "https://cdn.example.com/a.ogg")
        .by(USER)
        .in_guild(GUILD)
        .run()
        .await?;
    assert_eq!(
        reply.text(),
        "Queued https://cdn.example.com/a.ogg (#1 in the queue)."
    );
    let channels: Vec<_> = app
        .voice_state_updates()
        .iter()
        .map(|d| d["channel_id"].clone())
        .collect();
    assert_eq!(channels, [json!(LOUNGE.to_string())]);
    let commands = recorder.commands(GUILD);
    assert!(
        matches!(commands.as_slice(), [RecordedCommand::Play { .. }]),
        "{commands:?}"
    );
    app.stop().await?;
    Ok(())
}
