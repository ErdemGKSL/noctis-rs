//! The `voice` feature (Story 17.1): `.voice(..)` adds `GUILD_VOICE_STATES`, and
//! `/join` and `/leave` are mounted and answer privately. `TestApp` runs on the fake
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
