// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Pixel snapshot of the profile card (FR-51). The first run writes
//! `tests/snapshots/profile_card.png`; later runs compare with a small tolerance.

use noctis::image::RemoteImage;
use noctis::testkit::{assert_render_snapshot, test_renderer};
use noctis_bot_template::modules::profile::ProfileCard;

#[tokio::test]
async fn profile_card_matches_snapshot() -> anyhow::Result<()> {
    let renderer = test_renderer()?; // default font, fixed assets, no network
    let card = ProfileCard {
        name: "Deniz".into(),
        subtitle: "they/them".into(),
        level_label: "Level 7".into(),
        xp_label: "42 / 100 XP".into(),
        progress: 0.42,
        avatar: RemoteImage::placeholder(),
    };
    assert_render_snapshot!(renderer, "profile_card", card, tolerance = 0.01);
    Ok(())
}
