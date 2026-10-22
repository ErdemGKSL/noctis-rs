// NOTE: the bot code in `src/` is still written against the planned Noctis API
// (architecture spine); this file uses the real `noctis::testkit` API (Story 8.4).

//! End-to-end flows without Discord (FR-51): invoke a command, click by Route or label,
//! assert on the rendered message, its response type and its Limit cost. Snapshots live
//! in `tests/snapshots/` (insta); review changes with `cargo insta review`.
//!
//! `TestApp` runs the bot's own `configure` on local Discord fakes; every run is one
//! simulated interaction through the real pipeline. It needs a multi-thread runtime.
//! The memory check lives in `tests/memory.rs` (its own binary, dhat allocator).

use noctis::Permissions;
use noctis::prelude::*;
use noctis::testkit::{ResponseKind, TestApp, TestGuild, TestUser};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use noctis_bot_template::configure;
use noctis_bot_template::modules::profile::EditBio;
use noctis_bot_template::modules::setup::{PickLogChannel, PickModRole};

const GUILD: Id<GuildMarker> = Id::new(100);
const ADMIN: Id<UserMarker> = Id::new(200);
const MEMBER: Id<UserMarker> = Id::new(201);
const LOGS: Id<ChannelMarker> = Id::new(300);
const MODS: Id<RoleMarker> = Id::new(400);

async fn app() -> anyhow::Result<TestApp> {
    let mut guild = TestGuild::new(GUILD, "Test Guild")
        .text_channel(LOGS, "logs")
        .role(MODS, "Mods");
    // Enough roles for several pages on the Roles tab.
    for n in 0..120 {
        guild = guild.role(Id::new(1_000 + n), format!("role-{n}"));
    }
    Ok(TestApp::build(configure(&mut App::builder()))
        .await?
        .guild(guild)
        .user(TestUser::new(ADMIN).permissions(Permissions::MANAGE_GUILD))
        .user(TestUser::new(MEMBER)))
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_flow_validates_then_finishes() -> anyhow::Result<()> {
    let app = app().await?;

    let step1 = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
    assert_eq!(step1.kind(), ResponseKind::Reply);
    assert!(step1.is_ephemeral());
    assert!(step1.text().contains("Step 1 of 3: Log channel"));
    // Next stays disabled until the step's value is set (per-step validation).
    assert!(step1.button("Next")?.is_disabled());
    insta::assert_json_snapshot!("setup_step1", step1.message_json());

    let picked = app
        .select(step1.component::<PickLogChannel>()?)
        .channels([LOGS])
        .by(ADMIN)
        .run()
        .await?;
    assert_eq!(picked.kind(), ResponseKind::Update);

    let step2 = app.click(picked.button("Next")?).by(ADMIN).run().await?;
    let step2 = app
        .select(step2.component::<PickModRole>()?)
        .roles([MODS])
        .by(ADMIN)
        .run()
        .await?;
    let step3 = app.click(step2.button("Next")?).by(ADMIN).run().await?;
    assert!(step3.text().contains("Step 3 of 3: Confirm"));

    let done = app.click(step3.button("Finish")?).by(ADMIN).run().await?;
    assert!(done.text().contains("Setup complete"));
    assert!(done.components().is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_cancel_leaves_no_components() -> anyhow::Result<()> {
    let app = app().await?;
    let step1 = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
    let cancelled = app.click(step1.button("Cancel")?).by(ADMIN).run().await?;
    assert_eq!(cancelled.kind(), ResponseKind::Update);
    assert!(cancelled.buttons().is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_rejects_other_users() -> anyhow::Result<()> {
    let app = app().await?;
    let step1 = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
    // The `owner` Route Guard answers with an ephemeral notice; the handler never runs.
    let denied = app.click(step1.button("Cancel")?).by(MEMBER).run().await?;
    assert!(denied.is_ephemeral());
    assert!(denied.is_guard_notice());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "planned, Story 12.5: ctx.cache() seeding"]
async fn dashboard_roles_tab_pages_within_limits() -> anyhow::Result<()> {
    let app = app().await?;
    let overview = app.command("dashboard").by(ADMIN).in_guild(GUILD).run().await?;
    assert!(overview.button("Overview")?.is_disabled());

    // [planned, Story 12.5] The Roles tab reads `ctx.cache().roles(..)`; seeding the
    // Discord cache from `TestGuild` arrives with that story, so this flow runs once
    // `ctx.cache()` is seedable.
    let roles = app.click(overview.button("Roles")?).by(ADMIN).run().await?;
    // The tab row stays on every page, the active tab is disabled, prev is disabled on
    // the first page, and no page breaks a Limit (UD-5, UD-6, FR-62).
    assert!(roles.button("Roles")?.is_disabled());
    assert!(roles.button("Previous")?.is_disabled());
    assert!(roles.cost().components <= 40);
    assert!(roles.cost().text_chars <= 4000);

    let mut page = app.click(roles.button("Next")?).by(ADMIN).run().await?;
    assert!(page.button("Roles")?.is_disabled());
    while !page.button("Next")?.is_disabled() {
        assert!(page.cost().components <= 40);
        page = app.click(page.button("Next")?).by(ADMIN).run().await?;
    }
    assert!(!page.button("Previous")?.is_disabled());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dashboard_module_autocomplete_suggests_stored_modules() -> anyhow::Result<()> {
    let app = app().await?;
    // The focused text arrives raw and partial; the method filters, Noctis sends type 8.
    let auto = app.autocomplete("dashboard", "module", "pro").by(ADMIN).in_guild(GUILD).run().await?;
    assert_eq!(auto.kind(), ResponseKind::Autocomplete);
    let suggestions = auto.suggestions();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].0, "profile");
    assert!(suggestions[0].1.starts_with("Profiles and XP"));

    // A picked module opens the Modules tab; an unknown value falls back to Overview.
    let modules = app.command("dashboard").option("module", "profile").by(ADMIN).in_guild(GUILD).run().await?;
    assert!(modules.button("Modules")?.is_disabled());
    let forged = app.command("dashboard").option("module", "nope").by(ADMIN).in_guild(GUILD).run().await?;
    assert!(forged.button("Overview")?.is_disabled());
    Ok(())
}

/// The `data` of a run's type-9 (modal) callback.
macro_rules! modal_data {
    ($response:expr) => {
        $response
            .http_calls()
            .iter()
            .filter_map(|c| c.json())
            .find(|j| j["type"] == 9)
            .map(|j| j["data"].clone())
            .ok_or_else(|| anyhow::anyhow!("no modal callback"))
    };
}

/// `/bio` opens a blank bio modal; after a save, "Edit bio" reopens it pre-filled with
/// the stored bio and pronouns (FR-63).
#[tokio::test(flavor = "multi_thread")]
async fn edit_bio_opens_pre_filled() -> anyhow::Result<()> {
    let app = app().await?;

    let opened = app.command("bio").by(MEMBER).in_guild(GUILD).run().await?;
    assert_eq!(opened.kind(), ResponseKind::Modal);
    let blank = modal_data!(opened)?;
    let inputs = &blank["components"];
    assert!(inputs[0]["component"].get("value").is_none(), "{blank}");
    let options = inputs[1]["component"]["options"].as_array().cloned().unwrap_or_default();
    assert!(!options.is_empty(), "{blank}");
    assert!(options.iter().all(|o| o.get("default").is_none()), "{blank}");

    let form = opened.modal()?.text("bio", "Hello there").radio("pronouns", "they");
    let saved = app.submit(form).run().await?;
    assert_eq!(saved.kind(), ResponseKind::Reply);

    let edit = app.click(saved.component::<EditBio>()?).by(MEMBER).run().await?;
    assert_eq!(edit.kind(), ResponseKind::Modal);
    let filled = modal_data!(edit)?;
    let inputs = &filled["components"];
    assert_eq!(inputs[0]["component"]["value"], "Hello there", "{filled}");
    let options = inputs[1]["component"]["options"].as_array().cloned().unwrap_or_default();
    let defaults: Vec<_> =
        options.iter().filter(|o| o["default"] == true).map(|o| o["value"].clone()).collect();
    assert_eq!(defaults, ["they"], "{filled}");
    app.stop().await?;
    Ok(())
}

