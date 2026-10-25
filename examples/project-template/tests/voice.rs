//! The `voice` feature (Stories 17.1, 17.2): `.voice(..)` adds `GUILD_VOICE_STATES`;
//! `/join`, `/leave`, `/play`, `/skip` and `/stop` are mounted and answer privately. `TestApp` runs on the fake
//! gateway, so `/leave` sends op 4 with no channel and succeeds.
#![cfg(feature = "voice")]

use noctis::prelude::*;
use noctis::testkit::{TestApp, TestGuild, TestUser};
use noctis::twilight::model::gateway::Intents;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{GuildMarker, UserMarker};
use noctis_bot_template::configure;

const GUILD: Id<GuildMarker> = Id::new(100);
const USER: Id<UserMarker> = Id::new(200);

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
    let skip = app.command("skip").by(USER).in_guild(GUILD).run().await?;
    let stop = app.command("stop").by(USER).in_guild(GUILD).run().await?;
    for reply in [play, skip, stop] {
        assert!(reply.is_ephemeral());
        assert_eq!(reply.text(), "Use /join first.");
    }
    Ok(())
}
