use crate::events::{EventCtx, EventHandler, MessageCreate};
use crate::{BuildViolation, HandlerResult, Module, Register};

use super::{Discovered, mount};

async fn noop(_: EventCtx, _: MessageCreate) -> HandlerResult<()> {
    Ok(())
}

macro_rules! handler {
    ($ty:ident, $name:literal) => {
        struct $ty;

        impl Register for $ty {
            const __NAME: &'static str = $name;

            fn __register(module: Module) -> Module {
                module.__add(EventHandler::__new($name, false, false, noop))
            }
        }
    };
}

handler!(A, "bot::A");
handler!(B, "bot::B");
handler!(C, "bot::C");

fn entry<T: Register>(module: &'static str) -> Discovered {
    Discovered::__new(module, T::__NAME, T::__register)
}

fn names(module: &Module) -> Vec<&'static str> {
    module.handlers().iter().map(EventHandler::name).collect()
}

/// The ids of a tree, pre-order.
fn ids(module: &Module, out: &mut Vec<&'static str>) {
    out.push(module.id());
    for child in module.children() {
        ids(child, out);
    }
}

#[test]
fn merges_into_a_mounted_module_after_its_own() {
    let mut trees = vec![Module::new("xp").add::<A>()];
    let b = entry::<B>("xp");
    let mut violations = Vec::new();
    mount(&mut trees, &[&b], &mut violations);
    assert!(violations.is_empty());
    assert_eq!(trees.len(), 1);
    assert_eq!(names(&trees[0]), ["bot::A", "bot::B"]);
}

#[test]
fn merges_into_a_nested_mounted_module() {
    let mut trees = vec![Module::new("shop").module(Module::new("shop/admin"))];
    let a = entry::<A>("shop/admin");
    let mut violations = Vec::new();
    mount(&mut trees, &[&a], &mut violations);
    assert_eq!(trees.len(), 1);
    assert_eq!(names(&trees[0].children()[0]), ["bot::A"]);
}

#[test]
fn creates_missing_modules_under_their_parent_path() {
    let mut trees = vec![Module::new("other")];
    let (a, b) = (entry::<A>("eco/shop/admin"), entry::<B>("eco"));
    let mut violations = Vec::new();
    mount(&mut trees, &[&a, &b], &mut violations);
    assert_eq!(trees.len(), 2);
    let mut out = Vec::new();
    ids(&trees[1], &mut out);
    assert_eq!(out, ["eco", "eco/shop", "eco/shop/admin"]);
    assert_eq!(names(&trees[1]), ["bot::B"]);
    assert_eq!(names(&trees[1].children()[0].children()[0]), ["bot::A"]);
}

#[test]
fn added_and_discovered_is_reported_once_and_not_merged() {
    let mut trees = vec![Module::new("a").module(Module::new("a/b").add::<C>())];
    let c = entry::<C>("xp");
    let mut violations = Vec::new();
    mount(&mut trees, &[&c], &mut violations);
    assert_eq!(
        violations,
        [BuildViolation::DuplicateRegistration {
            type_name: "bot::C",
            module: "xp",
        }]
    );
    assert_eq!(trees.len(), 1, "no Module is created for a duplicate");
}

#[test]
fn linked_entries_are_sorted_by_module_then_name() {
    let sorted = super::linked();
    assert!(
        sorted
            .windows(2)
            .all(|w| (w[0].module, w[0].name) <= (w[1].module, w[1].name))
    );
}

#[test]
fn duplicate_registration_text() {
    let v = BuildViolation::DuplicateRegistration {
        type_name: "bot::cmds::Ping",
        module: "util",
    };
    assert!(
        v.to_string().ends_with("or the `add::<Ping>()` call"),
        "{v}"
    );
}

#[test]
fn declared_modules_mount_under_their_parent_path() {
    let mut trees = vec![Module::new("shop")];
    let declared = vec![
        Module::new("eco"),
        Module::new("eco/bank").add::<A>(),
        Module::new("shop/admin"),
        Module::new("tools/misc"),
    ];
    super::mount_modules(&mut trees, declared);
    let mut out = Vec::new();
    for tree in &trees {
        ids(tree, &mut out);
    }
    assert_eq!(
        out,
        [
            "shop",
            "shop/admin",
            "eco",
            "eco/bank",
            "tools",
            "tools/misc"
        ]
    );
    // A registration discovered for a declared Module merges into it.
    let b = entry::<B>("eco/bank");
    let mut violations = Vec::new();
    mount(&mut trees, &[&b], &mut violations);
    assert!(violations.is_empty());
    assert_eq!(names(&trees[1].children()[0]), ["bot::A", "bot::B"]);
}

#[test]
fn a_declared_module_add_counts_as_added() {
    let mut trees = Vec::new();
    super::mount_modules(&mut trees, vec![Module::new("eco").add::<C>()]);
    let c = entry::<C>("eco");
    let mut violations = Vec::new();
    mount(&mut trees, &[&c], &mut violations);
    assert_eq!(violations.len(), 1);
}
