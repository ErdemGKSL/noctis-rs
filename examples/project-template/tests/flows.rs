//! End-to-end flows without Discord (FR-51): invoke a command, click by Route or label,
//! assert on the rendered message, its response type and its Limit cost. Snapshots live
//! in `tests/snapshots/` (insta); review changes with `cargo insta review`.
//!
//! `TestApp` runs the bot's own `configure` on local Discord fakes; every run is one
//! simulated interaction through the real pipeline. It needs a multi-thread runtime.
//! The memory check lives in `tests/memory.rs` (its own binary, dhat allocator).

use noctis::ports::memory::GuildAccess;
use noctis::prelude::*;
use noctis::{BootFlags, Permissions};
use std::sync::{Arc, Mutex};

use noctis::testkit::fake::{FakeResponse, Method, RecordedRequest, StatusCode};
use noctis::testkit::{Response, ResponseKind, TestApp, TestError, TestGuild, TestUser};
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

    let step1 = app
        .command("setup start")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
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
    // A Components V2 message always has components (its text is one); the done state
    // has nothing left to click: no action row, so no button or select.
    assert!(done.components().iter().all(|c| c.component_type() != 1));
    assert!(done.buttons().is_empty());
    Ok(())
}

/// Runs `/setup start` to its Finish click for `ADMIN`.
async fn finish_setup(app: &TestApp) -> anyhow::Result<Response> {
    let step1 = app
        .command("setup start")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    let picked = app
        .select(step1.component::<PickLogChannel>()?)
        .channels([LOGS])
        .by(ADMIN)
        .run()
        .await?;
    let step2 = app.click(picked.button("Next")?).by(ADMIN).run().await?;
    let step2 = app
        .select(step2.component::<PickModRole>()?)
        .roles([MODS])
        .by(ADMIN)
        .run()
        .await?;
    let step3 = app.click(step2.button("Next")?).by(ADMIN).run().await?;
    Ok(app.click(step3.button("Finish")?).by(ADMIN).run().await?)
}

/// Finish posts a static log line through the bot's own webhook in the log channel
/// (FR-65): created once, stored, reused on the next Finish.
#[tokio::test(flavor = "multi_thread")]
async fn setup_finish_logs_through_its_webhook() -> anyhow::Result<()> {
    let app = app().await?;
    let log: Arc<Mutex<Vec<RecordedRequest>>> = Arc::default();
    let record = |answer: serde_json::Value| {
        let log = Arc::clone(&log);
        move |request: &RecordedRequest| {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            FakeResponse::json(StatusCode::OK, &answer)
        }
    };
    app.http_fake()
        .route_with(
            Method::POST,
            format!("/channels/{LOGS}/webhooks"),
            record(serde_json::json!({"id": "900", "token": "log-token", "channel_id": LOGS.to_string()})),
        )
        .route_with(
            Method::POST,
            "/webhooks/900/log-token",
            record(serde_json::json!({"id": "71", "channel_id": LOGS.to_string(), "attachments": []})),
        );

    for _ in 0..2 {
        assert!(finish_setup(&app).await?.text().contains("Setup complete"));
    }

    let requests = log
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let creates = requests
        .iter()
        .filter(|r| r.path.ends_with("/webhooks"))
        .count();
    let sends: Vec<_> = requests
        .iter()
        .filter(|r| r.path.ends_with("/log-token"))
        .collect();
    assert_eq!((creates, sends.len()), (1, 2), "created once, then reused");
    for send in sends {
        assert_eq!(
            send.query.as_deref(),
            Some("wait=true&with_components=true")
        );
        let body = send
            .json()
            .ok_or_else(|| anyhow::anyhow!("webhook body is not JSON"))?;
        assert_eq!(body["flags"], 32768);
        assert!(body.get("content").is_none());
        assert_eq!(body["components"][0]["type"], 17);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_cancel_leaves_no_components() -> anyhow::Result<()> {
    let app = app().await?;
    let step1 = app
        .command("setup start")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    let cancelled = app.click(step1.button("Cancel")?).by(ADMIN).run().await?;
    assert_eq!(cancelled.kind(), ResponseKind::Update);
    assert!(cancelled.buttons().is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_rejects_other_users() -> anyhow::Result<()> {
    let app = app().await?;
    let step1 = app
        .command("setup start")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    // The setup panel is ephemeral: Discord shows it, and so its buttons, only to the
    // admin who ran the command. The `owner` guard is shown on a public reply below.
    let err = app
        .click(step1.button("Cancel")?)
        .by(MEMBER)
        .run()
        .await
        .err();
    assert!(
        matches!(err, Some(TestError::NotVisible { user: 201 })),
        "{err:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_edit_rejects_other_users() -> anyhow::Result<()> {
    let app = app().await?;
    // MEMBER's own `/profile` is a public reply with their "Edit bio" button.
    let profile = app
        .command("profile")
        .by(MEMBER)
        .in_guild(GUILD)
        .run()
        .await?;
    assert!(!profile.is_ephemeral());
    // `owner = user` is a Route Guard: anyone else gets an ephemeral notice and the
    // handler never runs (no modal opens).
    let denied = app
        .click(profile.component::<EditBio>()?)
        .by(ADMIN)
        .run()
        .await?;
    assert!(denied.is_ephemeral());
    assert!(denied.is_guard_notice());
    assert_ne!(denied.kind(), ResponseKind::Modal);
    app.stop().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dashboard_roles_tab_pages_within_limits() -> anyhow::Result<()> {
    let app = app().await?;
    let overview = app
        .command("dashboard")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    assert!(overview.button("Overview")?.is_disabled());

    // The Roles tab reads `ctx.cache().roles(..)`: every `TestGuild` seed is fed to the
    // Discord cache as a GUILD_CREATE before the first run.
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
    let auto = app
        .autocomplete("dashboard", "module", "pro")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    assert_eq!(auto.kind(), ResponseKind::Autocomplete);
    let suggestions = auto.suggestions();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].0, "profile");
    assert!(suggestions[0].1.starts_with("Profiles and XP"));

    // A picked module opens the Modules tab; an unknown value falls back to Overview.
    let modules = app
        .command("dashboard")
        .option("module", "profile")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    assert!(modules.button("Modules")?.is_disabled());
    let forged = app
        .command("dashboard")
        .option("module", "nope")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
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
    let options = inputs[1]["component"]["options"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!options.is_empty(), "{blank}");
    assert!(
        options.iter().all(|o| o.get("default").is_none()),
        "{blank}"
    );

    let form = opened
        .modal()?
        .text("bio", "Hello there")
        .radio("pronouns", "they");
    let saved = app.submit(form).run().await?;
    assert_eq!(saved.kind(), ResponseKind::Reply);

    let edit = app
        .click(saved.component::<EditBio>()?)
        .by(MEMBER)
        .run()
        .await?;
    assert_eq!(edit.kind(), ResponseKind::Modal);
    let filled = modal_data!(edit)?;
    let inputs = &filled["components"];
    assert_eq!(inputs[0]["component"]["value"], "Hello there", "{filled}");
    let options = inputs[1]["component"]["options"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let defaults: Vec<_> = options
        .iter()
        .filter(|o| o["default"] == true)
        .map(|o| o["value"].clone())
        .collect();
    assert_eq!(defaults, ["they"], "{filled}");
    app.stop().await?;
    Ok(())
}

/// Boot Flags: `profile/badges` carries `badges`, so `none` drops `/badge` and the
/// default (`all`) keeps it.
#[tokio::test(flavor = "multi_thread")]
async fn badges_follow_the_boot_flags() -> anyhow::Result<()> {
    let mut builder = App::builder();
    configure(&mut builder).boot_flags(BootFlags::none());
    let pruned = TestApp::build(&mut builder)
        .await?
        .guild(TestGuild::new(GUILD, "Test Guild"))
        .user(TestUser::new(MEMBER));
    let missing = pruned
        .command("badge")
        .option("name", "Helper")
        .by(MEMBER)
        .in_guild(GUILD)
        .run()
        .await;
    assert!(
        matches!(missing, Err(TestError::UnknownCommand { .. })),
        "{missing:?}"
    );
    pruned.stop().await?;

    let app = app().await?;
    let badge = app
        .command("badge")
        .option("name", "Helper")
        .by(MEMBER)
        .in_guild(GUILD)
        .run()
        .await?;
    assert_eq!(badge.kind(), ResponseKind::Reply);
    app.stop().await?;
    Ok(())
}

/// Multiple Apps: a white-label App `acme` built from the same `configure` serves only
/// its customer's guild and has no `/setup` (restricted to the main App with
/// `apps = [..]`); the main App skips that guild with a deny list and answers there with
/// the ephemeral `noctis.module.disabled` notice.
#[tokio::test(flavor = "multi_thread")]
async fn white_label_app_serves_only_its_guild() -> anyhow::Result<()> {
    let guild = || TestGuild::new(GUILD, "Test Guild");
    let mut builder = App::builder();
    configure(&mut builder)
        .app_id("acme")
        .guild_access(GuildAccess::allow([GUILD]));
    let acme = TestApp::build(&mut builder)
        .await?
        .guild(guild())
        .user(TestUser::new(ADMIN));
    let setup = acme
        .command("setup start")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await;
    assert!(
        matches!(setup, Err(TestError::UnknownCommand { .. })),
        "{setup:?}"
    );
    let profile = acme
        .command("profile")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    assert_eq!(profile.kind(), ResponseKind::Reply);
    assert!(
        !profile.text().contains("disabled here"),
        "{}",
        profile.text()
    );
    acme.stop().await?;

    let mut builder = App::builder();
    configure(&mut builder).guild_access(GuildAccess::deny([GUILD]));
    let main = TestApp::build(&mut builder)
        .await?
        .guild(guild())
        .user(TestUser::new(ADMIN));
    let skipped = main
        .command("profile")
        .by(ADMIN)
        .in_guild(GUILD)
        .run()
        .await?;
    assert_eq!(skipped.kind(), ResponseKind::Reply);
    assert!(skipped.is_ephemeral());
    assert_eq!(skipped.text(), "This feature is disabled here.");
    main.stop().await?;
    Ok(())
}
