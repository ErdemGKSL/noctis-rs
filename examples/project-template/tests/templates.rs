// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.
// The template crate builds from Story 14.2, when this snapshot joins the gate.

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
