//! The four recipes end to end through `noctis::testkit`: every run is a simulated
//! interaction through the real pipeline, answered on local Discord fakes.
//!
//! Fixture: guild `G` with text channel `C` and role `R`; users `A` and `B`. Rows
//! refer to the Story 8.5 I/O matrix.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::collections::BTreeSet;

use noctis::App;
use noctis::testkit::{
    Response, ResponseKind, TestApp, TestError, TestGuild, TestMessage, TestUser,
};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use noctis::ui::ButtonStyle;
use ui_recipes::configure;
use ui_recipes::settings::{ConfirmDelete, PickSystem, SetRoles, ToggleSystem};
use ui_recipes::steps::{PickChannel, PickRole};
use ui_recipes::store::{Data, SetupConfig, SetupStore, TicketStore, TicketSystem};

const G: Id<GuildMarker> = Id::new(10);
const C: Id<ChannelMarker> = Id::new(20);
const R: Id<RoleMarker> = Id::new(30);
const A: Id<UserMarker> = Id::new(40);
const B: Id<UserMarker> = Id::new(41);

/// The recipes bot on `data`, with the fixture seeded.
async fn start(data: Data) -> TestApp {
    TestApp::build(configure(&mut App::builder(), data))
        .await
        .unwrap()
        .guild(
            TestGuild::new(G, "Guild")
                .text_channel(C, "logs")
                .role(R, "Mods"),
        )
        .user(TestUser::new(A).name("ana"))
        .user(TestUser::new(B).name("ben"))
}

/// Component types of the final message, in pre-order.
fn types(response: &Response) -> Vec<u8> {
    response
        .components()
        .iter()
        .map(noctis::testkit::TestComponent::component_type)
        .collect()
}

/// The final message's masked JSON, compact.
fn json(response: &Response) -> String {
    response.message_json().to_string()
}

// --- Tabs ----------------------------------------------------------------------

/// T1, T2.
#[tokio::test(flavor = "multi_thread")]
async fn tabs_disable_the_active_tab() -> Result<(), TestError> {
    let app = start(Data::sample()).await;
    let first = app.command("dashboard").run().await?;
    assert_eq!(first.kind(), ResponseKind::Reply);
    assert!(!first.is_ephemeral());
    let active = |r: &Response, tab: &str| -> Result<(), TestError> {
        for label in ["Overview", "Activity", "About"] {
            let button = r.button(label)?;
            let on = label == tab;
            assert_eq!(button.is_disabled(), on, "{label} on {tab}");
            let style = if on {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            };
            assert_eq!(button.style(), Some(style), "{label} on {tab}");
        }
        Ok(())
    };
    active(&first, "Overview")?;
    let activity = app.click(first.button("Activity")?).run().await?;
    assert_eq!(activity.kind(), ResponseKind::Update);
    active(&activity, "Activity")?;
    let about = app.click(activity.button("About")?).run().await?;
    assert_eq!(about.kind(), ResponseKind::Update);
    active(&about, "About")?;
    let bodies: BTreeSet<String> = [&first, &activity, &about].map(Response::text).into();
    assert_eq!(bodies.len(), 3);
    app.stop().await
}

// --- Pagination ----------------------------------------------------------------

/// P3: Next until disabled shows every item once, and no page exceeds a Limit.
#[tokio::test(flavor = "multi_thread")]
async fn pagination_walks_every_item_once() -> Result<(), TestError> {
    let app = start(Data::sample()).await;
    let mut page = app.command("items").run().await?;
    assert!(page.button("Previous")?.is_disabled());
    let mut seen = Vec::new();
    loop {
        let cost = page.cost();
        assert!(cost.components <= 40 && cost.text_chars <= 4000, "{cost:?}");
        seen.extend(page.text().lines().filter_map(|line| {
            let (n, rest) = line.split_once(". ")?;
            rest.starts_with("Item ").then(|| n.parse::<usize>().ok())?
        }));
        let next = page.button("Next")?;
        if next.is_disabled() {
            break;
        }
        page = app.click(next).run().await?;
        assert_eq!(page.kind(), ResponseKind::Update);
        assert!(!page.button("Previous")?.is_disabled());
    }
    assert_eq!(seen, (1..=300).collect::<Vec<_>>());
    app.stop().await
}

// --- Step flow -----------------------------------------------------------------

fn labels(response: &Response) -> Vec<String> {
    response
        .buttons()
        .iter()
        .filter_map(|b| b.label().map(str::to_owned))
        .collect()
}

/// S1-S6.
#[tokio::test(flavor = "multi_thread")]
async fn step_flow_validates_each_step() -> Result<(), TestError> {
    let setup = SetupStore::default();
    let app = start(Data {
        setup: setup.clone(),
        ..Data::sample()
    })
    .await;
    let step1 = app.command("setup").by(A).in_guild(G).run().await?;
    assert_eq!(step1.kind(), ResponseKind::Reply);
    assert!(step1.is_ephemeral());
    assert!(step1.text().starts_with("## Step 1 of 3: Log channel"));
    assert_eq!(labels(&step1), ["Cancel", "Next"]);

    // S2: Next without a channel stays on step 1 with the notice.
    let nagged = app.click(step1.button("Next")?).run().await?;
    assert_eq!(nagged.kind(), ResponseKind::Update);
    assert!(nagged.text().contains("Step 1 of 3"));
    assert!(nagged.text().contains("Pick a log channel first."));

    // S3: the pick stays on step 1 as the select's default, then Next moves on.
    let picked = app
        .select(nagged.component::<PickChannel>()?)
        .channels([C])
        .run()
        .await?;
    assert!(picked.text().contains("Step 1 of 3"));
    assert!(!picked.text().contains("first."));
    assert!(
        json(&picked).contains(r#"[{"id":"20","type":"channel"}]"#),
        "{}",
        json(&picked)
    );
    let step2 = app.click(picked.button("Next")?).run().await?;
    assert!(step2.text().starts_with("## Step 2 of 3: Moderator role"));
    assert_eq!(labels(&step2), ["Back", "Cancel", "Next"]);

    // S4: Back keeps the channel.
    let back = app.click(step2.button("Back")?).run().await?;
    assert!(back.text().contains("Step 1 of 3"));
    assert!(json(&back).contains(r#"[{"id":"20","type":"channel"}]"#));
    let step2 = app.click(back.button("Next")?).run().await?;

    // S5: Next on step 2 without a role.
    let nagged = app.click(step2.button("Next")?).run().await?;
    assert!(nagged.text().contains("Step 2 of 3"));
    assert!(nagged.text().contains("Pick a moderator role first."));

    // S6: pick the role, confirm, finish.
    let picked = app
        .select(nagged.component::<PickRole>()?)
        .roles([R])
        .run()
        .await?;
    let step3 = app.click(picked.button("Next")?).run().await?;
    assert!(step3.text().starts_with("## Step 3 of 3: Confirm"));
    assert!(step3.text().contains("<#20>") && step3.text().contains("<@&30>"));
    assert_eq!(labels(&step3), ["Back", "Cancel", "Finish"]);
    let done = app.click(step3.button("Finish")?).run().await?;
    assert_eq!(done.kind(), ResponseKind::Update);
    assert_eq!(types(&done), [10]);
    assert_eq!(
        done.text(),
        "Setup complete: logs in <#20>, moderators <@&30>."
    );
    assert_eq!(
        setup.get(G),
        Some(SetupConfig {
            log_channel: C,
            mod_role: R
        })
    );
    app.stop().await
}

/// S7: Cancel on every step leaves one Text Display.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_leaves_nothing_to_click() -> Result<(), TestError> {
    let app = start(Data::sample()).await;
    for steps in 0..3 {
        let mut screen = app.command("setup").by(A).in_guild(G).run().await?;
        if steps >= 1 {
            screen = app
                .select(screen.component::<PickChannel>()?)
                .channels([C])
                .run()
                .await?;
            screen = app.click(screen.button("Next")?).run().await?;
        }
        if steps >= 2 {
            screen = app
                .select(screen.component::<PickRole>()?)
                .roles([R])
                .run()
                .await?;
            screen = app.click(screen.button("Next")?).run().await?;
        }
        let cancelled = app.click(screen.button("Cancel")?).run().await?;
        assert_eq!(cancelled.kind(), ResponseKind::Update);
        assert_eq!(types(&cancelled), [10], "after {steps} steps");
        assert_eq!(cancelled.text(), "Setup cancelled.");
    }
    app.stop().await
}

/// S8: another user cannot see, so cannot click, an ephemeral step.
#[tokio::test(flavor = "multi_thread")]
async fn ephemeral_step_is_private() -> Result<(), TestError> {
    let app = start(Data::sample()).await;
    let step1 = app.command("setup").by(A).in_guild(G).run().await?;
    let err = app
        .click(step1.button("Next")?)
        .by(B)
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::NotVisible { user: 41 }), "{err}");
    app.stop().await
}

// --- Settings screen -----------------------------------------------------------

/// The fixture with `tickets` as the ticket store.
async fn settings(tickets: &TicketStore) -> TestApp {
    start(Data {
        tickets: tickets.clone(),
        ..Data::sample()
    })
    .await
}

/// E1-E4.
#[tokio::test(flavor = "multi_thread")]
async fn settings_add_and_switch() -> Result<(), TestError> {
    let tickets = TicketStore::default();
    let app = settings(&tickets).await;
    let empty = app.command("tickets").by(A).in_guild(G).run().await?;
    assert!(empty.is_ephemeral());
    assert!(empty.text().contains("No ticket systems yet."));
    assert_eq!(labels(&empty), ["Add"]);
    assert_eq!(empty.button("Add")?.style(), Some(ButtonStyle::Primary));

    let one = app.click(empty.button("Add")?).run().await?;
    assert_eq!(one.kind(), ResponseKind::Update);
    assert!(one.text().contains("**Ticket system 1**"));
    assert_eq!(labels(&one), ["Rename", "Disable", "Delete", "Add"]);
    assert!(one.component::<SetRoles>().is_ok());
    assert!(
        one.component::<PickSystem>().is_err(),
        "no switcher for one system"
    );
    assert_eq!(one.button("Add")?.style(), Some(ButtonStyle::Secondary));

    let two = app.click(one.button("Add")?).run().await?;
    let picker = two.component::<PickSystem>()?;
    assert_eq!(picker.options(), ["1", "2"]);
    assert!(json(&two).contains(r#"{"default":true,"label":"Ticket system 2","value":"2"}"#));
    assert!(two.text().contains("**Ticket system 2**"));

    let switched = app.select(picker).values(["1"]).run().await?;
    assert!(switched.text().contains("**Ticket system 1**"));
    assert!(json(&switched).contains(r#"{"default":true,"label":"Ticket system 1","value":"1"}"#));
    app.stop().await
}

/// E5.
#[tokio::test(flavor = "multi_thread")]
async fn settings_toggle() -> Result<(), TestError> {
    let tickets = TicketStore::default();
    let app = settings(&tickets).await;
    let empty = app.command("tickets").by(A).in_guild(G).run().await?;
    let on = app.click(empty.button("Add")?).run().await?;
    assert!(on.text().contains("**Enabled** · on"));
    assert_eq!(on.button("Disable")?.style(), Some(ButtonStyle::Danger));
    let off = app.click(on.button("Disable")?).run().await?;
    assert!(off.text().contains("**Enabled** · off"));
    assert_eq!(off.button("Enable")?.style(), Some(ButtonStyle::Success));
    assert!(!tickets.systems(G)[0].enabled);
    let again = app.click(off.button("Enable")?).run().await?;
    assert!(again.text().contains("**Enabled** · on"));
    app.stop().await
}

/// E6-E8: button → modal → re-render of the same message.
#[tokio::test(flavor = "multi_thread")]
async fn settings_rename_through_a_modal() -> Result<(), TestError> {
    let tickets = TicketStore::default();
    let app = settings(&tickets).await;
    let empty = app.command("tickets").by(A).in_guild(G).run().await?;
    let screen = app.click(empty.button("Add")?).run().await?;
    let opened = app.click(screen.button("Rename")?).run().await?;
    assert_eq!(opened.kind(), ResponseKind::Modal);
    let saved = app
        .submit(opened.modal()?.text("name", "Destek 🎫"))
        .run()
        .await?;
    assert_eq!(saved.kind(), ResponseKind::Update);
    assert_eq!(
        saved.message().map(TestMessage::id),
        screen.message().map(TestMessage::id)
    );
    assert!(saved.text().contains("**Destek 🎫**"));

    let opened = app.click(saved.button("Rename")?).run().await?;
    let bold = app
        .submit(opened.modal()?.text("name", "*bold*"))
        .run()
        .await?;
    assert!(bold.text().contains(r"**\*bold\***"), "{}", bold.text());

    // E7: a blank name is refused with a notice; the store is unchanged.
    let opened = app.click(bold.button("Rename")?).run().await?;
    let blank = app
        .submit(opened.modal()?.text("name", "   "))
        .run()
        .await?;
    assert_eq!(blank.kind(), ResponseKind::Reply);
    assert!(blank.is_ephemeral());
    assert_eq!(blank.text(), "The name can't be empty.");
    assert_eq!(tickets.systems(G)[0].name, "*bold*");

    // E8: the client refuses a name over max_length.
    let opened = app.click(bold.button("Rename")?).run().await?;
    let err = app
        .submit(opened.modal()?.text("name", "x".repeat(41)))
        .run()
        .await
        .unwrap_err();
    assert!(matches!(err, TestError::InvalidInput { .. }), "{err}");
    app.stop().await
}

/// E9: the role select saves the pick and is pre-filled with it.
#[tokio::test(flavor = "multi_thread")]
async fn settings_roles_are_pre_filled() -> Result<(), TestError> {
    let tickets = TicketStore::default();
    let app = settings(&tickets).await;
    let empty = app.command("tickets").by(A).in_guild(G).run().await?;
    let screen = app.click(empty.button("Add")?).run().await?;
    let set = app
        .select(screen.component::<SetRoles>()?)
        .roles([R])
        .run()
        .await?;
    assert_eq!(tickets.systems(G)[0].roles, [R]);
    assert!(
        json(&set).contains(r#""default_values":[{"id":"30","type":"role"}]"#),
        "{}",
        json(&set)
    );
    let cleared = app
        .select(set.component::<SetRoles>()?)
        .roles([])
        .run()
        .await?;
    assert!(tickets.systems(G)[0].roles.is_empty());
    assert!(!json(&cleared).contains("default_values"));
    app.stop().await
}

/// E10, E11: delete asks first; Confirm is one-shot.
#[tokio::test(flavor = "multi_thread")]
async fn settings_delete_confirms_once() -> Result<(), TestError> {
    let tickets = TicketStore::default();
    let app = settings(&tickets).await;
    let empty = app.command("tickets").by(A).in_guild(G).run().await?;
    let screen = app.click(empty.button("Add")?).run().await?;

    let ask = app.click(screen.button("Delete")?).run().await?;
    assert_eq!(
        ask.text(),
        "Delete **Ticket system 1**? This can't be undone."
    );
    assert_eq!(ask.button("Confirm")?.style(), Some(ButtonStyle::Danger));
    let kept = app.click(ask.button("Cancel")?).run().await?;
    assert!(kept.text().contains("**Ticket system 1**"));
    assert_eq!(tickets.systems(G).len(), 1);

    let ask = app.click(kept.button("Delete")?).run().await?;
    let confirm = ask.component::<ConfirmDelete>()?;
    let gone = app.click(confirm.clone()).run().await?;
    assert!(tickets.systems(G).is_empty());
    assert!(gone.text().contains("No ticket systems yet."));
    let again = app.click(confirm).run().await?;
    assert!(again.is_expired_notice(), "{}", again.text());
    app.stop().await
}

/// E12: a stale toggle of a deleted system.
#[tokio::test(flavor = "multi_thread")]
async fn settings_stale_toggle() -> Result<(), TestError> {
    let tickets = TicketStore::default();
    let app = settings(&tickets).await;
    let empty = app.command("tickets").by(A).in_guild(G).run().await?;
    let screen = app.click(empty.button("Add")?).run().await?;
    assert!(tickets.delete(G, 1));
    let stale = app.click(screen.component::<ToggleSystem>()?).run().await?;
    assert_eq!(stale.text(), "That ticket system no longer exists.");
    app.stop().await
}

/// E13: the 25th system removes the Add section; a stale Add gets the limit notice.
#[tokio::test(flavor = "multi_thread")]
async fn settings_stop_at_25() -> Result<(), TestError> {
    let systems = (1..=24)
        .map(|id| TicketSystem {
            id,
            name: format!("Ticket system {id}"),
            enabled: true,
            roles: Vec::new(),
        })
        .collect();
    let tickets = TicketStore::with(G, systems);
    let app = settings(&tickets).await;
    let screen = app.command("tickets").by(A).in_guild(G).run().await?;
    let full = app.click(screen.button("Add")?).run().await?;
    assert_eq!(tickets.systems(G).len(), 25);
    assert!(full.button("Add").is_err());
    assert!(full.text().contains("**Ticket system 25**"));
    let stale = app.click(screen.button("Add")?).run().await?;
    assert_eq!(stale.text(), "You can have at most 25 ticket systems.");
    app.stop().await
}
