//! Pixel snapshot of the profile card (FR-51). The first run writes
//! `tests/snapshots/profile_card.png` (commit it); later runs compare with a small
//! tolerance, and a mismatch writes `tests/snapshots/profile_card.new.png`.

use noctis::App;
use noctis::image::RemoteImage;
use noctis::testkit::{TestApp, assert_render_snapshot};
use noctis_bot_template::{configure, modules::profile::ProfileCard};

#[tokio::test(flavor = "multi_thread")]
async fn profile_card_matches_snapshot() -> anyhow::Result<()> {
    // The same builder as production: `configure` installs the ImagePlugin (through
    // `.theme(..)`), and TestApp builds on `Runtime::for_test()` with the Discord fakes,
    // so rendering uses the default font and a placeholder avatar with no network.
    let app = TestApp::build(configure(&mut App::builder())).await?;
    let card = ProfileCard {
        name: "Deniz".into(),
        subtitle: "they/them".into(),
        level_label: "Level 7".into(),
        xp_label: "42 / 100 XP".into(),
        progress: 0.42,
        avatar: RemoteImage::placeholder(),
    };
    assert_render_snapshot!(app.renderer(), "profile_card", card, tolerance = 0.01);
    app.stop().await?;
    Ok(())
}

/// The rank feature's three Templates and the drawn `level_up` emoji, each a pixel
/// snapshot. The level-up card is animated (play once, then a hold): a snapshot draws
/// its still, `Frame::STILL`, which is the end state its last frame rests on.
#[tokio::test(flavor = "multi_thread")]
async fn rank_cards_match_snapshots() -> anyhow::Result<()> {
    use noctis::image::Template as _;
    use noctis::image::{AnimatedFormat, Plays};
    use noctis_bot_template::emoji::LevelUpGlyph;
    use noctis_bot_template::modules::rank::view::{
        ActivityChart, BoardCard, BoardRow, LevelUpCard,
    };

    let app = TestApp::build(configure(&mut App::builder())).await?;

    // The animation as declared: WebP, played once, resting 4 s on the last frame.
    let spec = LevelUpCard::ANIMATION.ok_or_else(|| anyhow::anyhow!("not animated"))?;
    assert_eq!(spec.plays, Plays::ONCE);
    assert_eq!(spec.hold_end.as_millis(), 4000);
    assert_eq!(spec.format, AnimatedFormat::DEFAULT);
    let card = LevelUpCard {
        headline: "LEVEL UP".into(),
        name: "Deniz".into(),
        level_label: "Level 8".into(),
        avatar: RemoteImage::placeholder(),
    };
    assert_render_snapshot!(app.renderer(), "rank_level_up", card, tolerance = 0.01);

    let chart = ActivityChart {
        title: "Messages per day".into(),
        today_label: "Today: 31".into(),
        days: vec![12, 18, 9, 22, 30, 27, 14, 8, 19, 25, 33, 41, 36, 31],
    };
    assert_render_snapshot!(app.renderer(), "rank_activity", chart, tolerance = 0.01);

    let rows = [
        ("Deniz", 942_u16),
        ("Ada", 811),
        ("Mert", 640),
        ("Bo", 377),
        ("Sami", 120),
    ]
    .into_iter()
    .zip(1..)
    .map(|((name, xp), rank)| BoardRow {
        rank,
        name: name.into(),
        score_label: format!("Level {} · {xp} XP", xp / 100),
        share: f32::from(xp) / 942.0,
        avatar: RemoteImage::placeholder(),
    })
    .collect();
    let board = BoardCard {
        title: "Leaderboard".into(),
        rows,
    };
    assert_render_snapshot!(app.renderer(), "rank_board", board, tolerance = 0.01);

    // The `#[emoji(render)]` art, as Publish renders and uploads it.
    let glyph = LevelUpGlyph { tint: 0x58_65F2 };
    assert_render_snapshot!(app.renderer(), "emoji_level_up", glyph, tolerance = 0.01);
    app.stop().await?;
    Ok(())
}
