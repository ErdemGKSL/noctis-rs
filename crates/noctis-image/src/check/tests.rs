//! TW-01 runtime half: warn-once, deny, the cap, truncation, literal skip
//! and theme tokens.

use super::*;
use crate::{Classes, div, span, text};

fn tree() -> Node {
    div(Classes::dynamic("flx"))
        .child(span(Classes::dynamic("p-4")))
        .child(text("x", Classes::dynamic("flx")))
}

#[test]
fn tw_01_warn_once_per_distinct_string() {
    let check = ClassCheck::new(ClassCheckMode::Warn);
    assert_eq!(check.check(&tree(), &Theme::empty()), Ok(1));
    assert_eq!(check.check(&tree(), &Theme::empty()), Ok(0));
    let other = div(Classes::dynamic("bad"));
    assert_eq!(check.check(&other, &Theme::empty()), Ok(1));
}

#[test]
fn tw_01_deny_returns_the_first_invalid_string() {
    let check = ClassCheck::new(ClassCheckMode::Deny);
    let tree = div(Classes::dynamic("p-4"))
        .child(span(Classes::dynamic("flx m-2")))
        .child(span(Classes::dynamic("nope")));
    let err = check.check(&tree, &Theme::empty()).unwrap_err();
    let classes: Vec<&str> = err
        .classes()
        .iter()
        .map(crate::tw::UnknownClass::class)
        .collect();
    assert_eq!(classes, ["flx"]);
    assert_eq!(
        check
            .check(&self::tree(), &Theme::empty())
            .map_err(|e| e.classes().len()),
        Err(1)
    );
    assert_eq!(
        check.check(&div(Classes::dynamic("flex")), &Theme::empty()),
        Ok(0)
    );
}

#[test]
fn tw_01_pre_order_walk() {
    let check = ClassCheck::new(ClassCheckMode::Deny);
    let tree = div(Classes::default())
        .child(div(Classes::default()).child(span(Classes::dynamic("first"))))
        .child(span(Classes::dynamic("second")));
    let err = check.check(&tree, &Theme::empty()).unwrap_err();
    assert_eq!(err.classes()[0].class(), "first");
}

#[test]
fn tw_01_literals_are_trusted() {
    let tree = div(Classes::__literal("flx")).child(span(Classes::__literal("bad")));
    for mode in [ClassCheckMode::Warn, ClassCheckMode::Deny] {
        assert_eq!(ClassCheck::new(mode).check(&tree, &Theme::empty()), Ok(0));
    }
    // A join with a dynamic part is unchecked as a whole.
    let joined = div(Classes::__literal("flx").and(Classes::dynamic("p-4")));
    assert!(
        ClassCheck::new(ClassCheckMode::Deny)
            .check(&joined, &Theme::empty())
            .is_err()
    );
}

#[test]
fn tw_01_cap_of_remembered_strings() {
    let check = ClassCheck::new(ClassCheckMode::Warn);
    let tree = div(Classes::default())
        .children((0..=MAX_REMEMBERED).map(|i| span(Classes::dynamic(format!("bad-{i}")))));
    assert_eq!(check.check(&tree, &Theme::empty()), Ok(MAX_REMEMBERED));
    let state = check.warned.lock().unwrap();
    assert_eq!(state.seen.len(), MAX_REMEMBERED);
    assert!(state.full);
    drop(state);
    let later = div(Classes::dynamic("another-bad"));
    assert_eq!(check.check(&later, &Theme::empty()), Ok(0));
    assert_eq!(check.warned.lock().unwrap().seen.len(), MAX_REMEMBERED);
}

#[test]
fn tw_01_long_strings_are_cut_for_the_log() {
    let long = "x".repeat(10_000);
    let excerpt = log_excerpt(&long);
    assert_eq!(excerpt.len(), MAX_LOGGED_BYTES + '…'.len_utf8());
    assert!(excerpt.ends_with('…'));
    assert_eq!(log_excerpt("short"), Cow::Borrowed("short"));
    let exact = "y".repeat(MAX_LOGGED_BYTES);
    assert_eq!(log_excerpt(&exact), Cow::Borrowed(exact.as_str()));
    // A cut never splits a char: 511 ASCII bytes then a 2-byte char.
    let wide = format!("{}é{}", "a".repeat(511), "b".repeat(100));
    let excerpt = log_excerpt(&wide);
    assert_eq!(excerpt, format!("{}…", "a".repeat(511)));
    let check = ClassCheck::new(ClassCheckMode::Warn);
    assert_eq!(
        check.check(&div(Classes::dynamic(long)), &Theme::empty()),
        Ok(1)
    );
}

#[test]
fn tw_01_theme_tokens() {
    let theme = Theme::parse("@theme { --color-brand: #5865f2; }").unwrap();
    let tree = div(Classes::dynamic("bg-brand"));
    let check = ClassCheck::new(ClassCheckMode::Warn);
    assert_eq!(check.check(&tree, &theme), Ok(0));
    assert_eq!(check.check(&tree, &Theme::empty()), Ok(1));
}

#[test]
fn poisoned_lock_is_recovered() {
    let check = std::sync::Arc::new(ClassCheck::new(ClassCheckMode::Warn));
    let clone = std::sync::Arc::clone(&check);
    let _ = std::thread::spawn(move || {
        let _guard = clone.warned.lock().unwrap();
        panic!("poison the lock");
    })
    .join();
    assert!(check.warned.is_poisoned());
    assert_eq!(check.check(&tree(), &Theme::empty()), Ok(1));
}
