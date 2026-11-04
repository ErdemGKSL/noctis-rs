//! Story 18.1 (FR-81, AD-31): registrations discovered through their
//! attribute's `module = ".."`, from separate files, mounted by
//! `AppBuilder::discover()`.

#![cfg(feature = "testkit")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

mod commands;
mod events;
mod modules;
mod ui;

use noctis::prelude::*;
use noctis::testkit::{ResponseKind, TestApp};
use noctis::{BootFlags, BuildViolation};

/// `/shop` (file `commands.rs`) and its Button (file `ui.rs`) are listed
/// nowhere; the `shop` Module is created by discovery.
#[tokio::test(flavor = "multi_thread")]
async fn discovered_command_and_button_dispatch() {
    let mut builder = App::builder();
    builder.discover();
    let app = TestApp::build(&mut builder).await.unwrap();
    let shop = app.command("shop").run().await.unwrap();
    assert_eq!(shop.kind(), ResponseKind::Reply);
    assert!(shop.text().contains("Item 1"), "{}", shop.text());
    let next = app
        .click(shop.component::<ui::Next>().unwrap())
        .run()
        .await
        .unwrap();
    assert_eq!(next.kind(), ResponseKind::Update);
    assert!(next.text().contains("Item 2"), "{}", next.text());
    app.stop().await.unwrap();
}

/// Missing Modules are created under their parent path; a mounted Module
/// with a discovered id receives the registrations.
#[tokio::test]
async fn discovered_modules_are_created_or_merged() {
    let app = App::builder()
        .module(Module::new("xp"))
        .discover()
        .build()
        .unwrap();
    let mut ids: Vec<_> = app.modules().ids().collect();
    ids.sort_unstable();
    assert_eq!(ids, ["beta", "beta/tools", "shop", "shop/admin", "xp"]);
}

/// Without `discover()` nothing discovered is mounted.
#[tokio::test]
async fn nothing_is_mounted_without_discover() {
    let app = App::builder().build().unwrap();
    assert_eq!(app.modules().ids().count(), 0);
}

/// A discovered registration keeps its Boot Flags: `/shopadmin reset`
/// (`flags = ["admin"]`) is pruned, and its Module `shop/admin` stays.
#[tokio::test(flavor = "multi_thread")]
async fn discovered_flags_prune() {
    let mut builder = App::builder();
    builder.boot_flags(BootFlags::none()).discover();
    let app = TestApp::build(&mut builder).await.unwrap();
    assert!(app.command("shopadmin reset").run().await.is_err());
    // A declared Module's flags prune what is discovered into it.
    assert!(app.command("betastats").run().await.is_err());
    assert!(app.command("shop").run().await.is_ok());
    app.stop().await.unwrap();
}

/// A struct both added and discovered is a build error.
#[tokio::test]
async fn added_and_discovered_is_a_duplicate() {
    let err = App::builder()
        .module(Module::new("xp").add::<events::AwardXp>())
        .discover()
        .build()
        .unwrap_err();
    assert!(
        err.violations().iter().any(|v| matches!(
            v,
            BuildViolation::DuplicateRegistration { module: "xp", type_name }
                if type_name.ends_with("events::AwardXp")
        )),
        "{err}"
    );
}

/// Story 18.2: a declared Module's discovered Command runs while its flag is
/// selected.
#[tokio::test(flavor = "multi_thread")]
async fn declared_modules_hold_their_discovered_registrations() {
    let mut builder = App::builder();
    builder.discover();
    let app = TestApp::build(&mut builder).await.unwrap();
    let stats = app.command("betastats").run().await.unwrap();
    assert!(stats.text().contains("Item 9"), "{}", stats.text());
    app.stop().await.unwrap();
}

/// A declared id also mounted by hand is mounted twice.
#[tokio::test]
async fn declared_and_mounted_is_a_duplicate_module() {
    let err = App::builder()
        .module(Module::new("beta"))
        .discover()
        .build()
        .unwrap_err();
    assert!(
        err.violations()
            .contains(&BuildViolation::DuplicateModule { id: "beta" }),
        "{err}"
    );
}
