// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! End-to-end flows without Discord (FR-51): invoke a command, click by Route or label,
//! assert on the View, its response type and its Limit cost. Snapshots live in
//! `tests/snapshots/` (insta); review changes with `cargo insta review`.

use noctis::id::Id;
use noctis::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use noctis::prelude::*;
use noctis::testkit::{ResponseKind, TestApp, TestGuild, TestUser};
use noctis_bot_template::configure;
use noctis_bot_template::modules::setup::{PickLogChannel, PickModRole};

const GUILD: Id<GuildMarker> = Id::new(100);
const ADMIN: Id<UserMarker> = Id::new(200);
const MEMBER: Id<UserMarker> = Id::new(201);
const LOGS: Id<ChannelMarker> = Id::new(300);
const MODS: Id<RoleMarker> = Id::new(400);

fn app() -> anyhow::Result<TestApp> {
    Ok(TestApp::build(configure(&mut App::builder()))?
        .guild(
            TestGuild::new(GUILD, "Test Guild")
                .text_channel(LOGS, "logs")
                .role(MODS, "Mods")
                .roles_named("role", 120), // enough to force the Roles tab to paginate
        )
        .user(TestUser::new(ADMIN).permissions(Permissions::MANAGE_GUILD))
        .user(TestUser::new(MEMBER)))
}

#[tokio::test]
async fn setup_wizard_validates_then_finishes() -> anyhow::Result<()> {
    let app = app()?;

    let step1 = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
    assert_eq!(step1.kind(), ResponseKind::Reply);
    assert!(step1.is_ephemeral());
    assert!(step1.text().contains("Step 1 of 3: Log channel"));
    insta::assert_json_snapshot!("setup_step1", step1.view_json());

    // Next without a channel stays on Step 1 and shows the validation message (FR-27).
    let blocked = app.click(step1.button("Next")).by(ADMIN).run().await?;
    assert!(blocked.text().contains("Pick a log channel"));

    let picked = app
        .select(blocked.component::<PickLogChannel>())
        .channels([LOGS])
        .by(ADMIN)
        .run()
        .await?;
    assert_eq!(picked.kind(), ResponseKind::Update);

    let step2 = app.click(picked.button("Next")).by(ADMIN).run().await?;
    let step2 = app
        .select(step2.component::<PickModRole>())
        .roles([MODS])
        .by(ADMIN)
        .run()
        .await?;
    let step3 = app.click(step2.button("Next")).by(ADMIN).run().await?;
    assert!(step3.text().contains("Step 3 of 3: Confirm"));

    let done = app.click(step3.button("Finish")).by(ADMIN).run().await?;
    assert!(done.text().contains("Setup complete"));
    assert!(done.components().is_empty());
    Ok(())
}

#[tokio::test]
async fn wizard_rejects_other_users() -> anyhow::Result<()> {
    let app = app()?;
    let step1 = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
    // The `owner` Route Guard answers with an ephemeral notice; the handler never runs.
    let denied = app.click(step1.button("Next")).by(MEMBER).run().await?;
    assert!(denied.is_ephemeral());
    assert!(denied.is_guard_notice());
    Ok(())
}

#[tokio::test]
async fn dashboard_roles_tab_paginates_within_limits() -> anyhow::Result<()> {
    let app = app()?;
    let overview = app.command("dashboard").by(ADMIN).in_guild(GUILD).run().await?;
    let roles = app.click(overview.button("Roles")).by(ADMIN).run().await?;

    // The Navbar stays on every page, the active tab is disabled, and no page breaks a
    // Limit (FR-26, FR-62).
    assert!(roles.button("Roles").is_disabled());
    assert!(roles.cost().components <= 40);
    assert!(roles.cost().text_chars <= 4000);
    let page2 = app.click(roles.button_by_emoji("▶")).by(ADMIN).run().await?;
    assert!(page2.button("Roles").is_disabled());
    Ok(())
}

#[tokio::test]
async fn handlers_hold_no_per_message_state() -> anyhow::Result<()> {
    // NFR-2 / UD-7: framework memory stays flat however many dashboards are opened.
    let app = app()?;
    noctis::testkit::assert_memory_flat(&app, 1_000, |app| async move {
        app.command("dashboard").by(ADMIN).in_guild(GUILD).run().await.map(drop)
    })
    .await?;
    Ok(())
}
