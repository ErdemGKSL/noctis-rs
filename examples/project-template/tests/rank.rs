//! The rank feature's messages: `/leaderboard` through the real pipeline, and the View
//! the `AwardXp` event handler posts, tested as the pure function it is (an event has no
//! `TestApp` run; the handler only loads, renders and calls `render_level_up`).

use noctis::App;
use noctis::rules::Locale;
use noctis::testkit::{ResponseKind, TestApp, TestGuild, TestUser, fake_emoji_id, render_json};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{GuildMarker, UserMarker};
use noctis::ui::Attachment;
use noctis_bot_template::configure;
use noctis_bot_template::modules::rank::view::render_level_up;
use noctis_bot_template::strings::Strings;

const GUILD: Id<GuildMarker> = Id::new(100);
const MEMBER: Id<UserMarker> = Id::new(201);

/// Nobody has XP yet: the designed empty state, under a heading with the drawn
/// `level_up` emoji (`#[emoji(render)]`; `TestApp` resolves it to a fake id).
#[tokio::test(flavor = "multi_thread")]
async fn leaderboard_starts_empty() -> anyhow::Result<()> {
    let app = TestApp::build(configure(&mut App::builder()))
        .await?
        .guild(TestGuild::new(GUILD, "Test Guild"))
        .user(TestUser::new(MEMBER));
    let board = app
        .command("leaderboard")
        .by(MEMBER)
        .in_guild(GUILD)
        .run()
        .await?;
    assert_eq!(board.kind(), ResponseKind::Reply);
    let heading = format!("## <:level_up:{}> Leaderboard", fake_emoji_id("level_up"));
    assert!(board.text().contains(&heading), "{}", board.text());
    assert!(board.text().contains("Nobody has earned XP yet"));
    app.stop().await?;
    Ok(())
}

/// The level-up message: one line and the card in a Media Gallery with alt text. Built
/// on an App whose emoji table never listed, so it also shows the degraded path: the
/// missing emoji writes nothing and the line still reads.
#[tokio::test(flavor = "multi_thread")]
async fn level_up_view_matches_snapshot() -> anyhow::Result<()> {
    let app = configure(&mut App::builder()).build()?;
    let s = Strings::of(Locale::EnUs);
    let card = Attachment::new(vec![0_u8; 16], "webp")?;
    let msg = render_level_up("De_niz", 8, card, &s, app.emojis());
    insta::assert_json_snapshot!("rank_level_up_view", render_json(&msg)?);
    Ok(())
}
