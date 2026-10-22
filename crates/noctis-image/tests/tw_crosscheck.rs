//! Cross-checks the strict class grammar against takumi's own parser.
//!
//! The oracle parses each class alone with takumi-core's
//! `TailwindValues::from_str` and reads its `Debug` form: no value is Empty, a
//! `VarUtility` whose fallbacks are all `None` only reads a variable
//! (`VarOnly`), anything else is Builtin. The grammar must agree on every
//! sample and corpus class (Builtin <-> Builtin, Theme <-> `VarOnly`,
//! Unknown <-> Empty), on the variant kind and on `!important`; a corpus line
//! marked `narrowed=` must be Unknown to the grammar while takumi accepts it.
//! Every sample also renders once, and must not panic.
#![cfg(feature = "render")]
#![allow(clippy::expect_used, reason = "tests panic with the failing case")]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;

use noctis_image::tw::{self, Class, Utility, Variant};
use takumi::prelude::{Fonts, Node, RenderOptions, Viewport};
use takumi_core::style::TailwindValues;

const CORPUS: &str = include_str!("tw_corpus.txt");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Builtin,
    Theme,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VariantKind {
    None,
    Builtin,
    Custom,
}

/// What takumi makes of one class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Oracle {
    verdict: Verdict,
    variant: VariantKind,
    important: bool,
}

fn oracle(class: &str) -> Oracle {
    let values = TailwindValues::from_str(class).expect("TailwindValues::from_str is infallible");
    let debug = format!("{values:?}");
    let verdict = if debug.contains("inner: []") {
        Verdict::Unknown
    } else if debug.contains("VarUtility(")
        && debug.contains("fallback: None")
        && !debug.contains("fallback: Some(")
    {
        Verdict::Theme
    } else {
        Verdict::Builtin
    };
    let variant = if debug.contains("breakpoint: Some(Breakpoint { width: None") {
        VariantKind::Custom
    } else if debug.contains("breakpoint: Some(") {
        VariantKind::Builtin
    } else {
        VariantKind::None
    };
    Oracle {
        verdict,
        variant,
        important: debug.contains("important: true"),
    }
}

fn grammar(class: &str) -> Option<Class<'_>> {
    tw::classify(class)
}

fn verdict_of(class: Option<&Class<'_>>) -> Verdict {
    match class.map(|c| c.utility) {
        None => Verdict::Unknown,
        Some(Utility::Builtin) => Verdict::Builtin,
        Some(_) => Verdict::Theme,
    }
}

/// The mismatch between grammar and oracle for `class`, if any.
fn compare(class: &str) -> Option<String> {
    let ours = grammar(class);
    let theirs = oracle(class);
    let verdict = verdict_of(ours.as_ref());
    if verdict != theirs.verdict {
        return Some(format!(
            "`{class}`: grammar {verdict:?}, takumi {:?}",
            theirs.verdict
        ));
    }
    let ours = ours?;
    let variant = match ours.variant {
        None => VariantKind::None,
        Some(Variant::Builtin(_)) => VariantKind::Builtin,
        Some(_) => VariantKind::Custom,
    };
    if variant != theirs.variant {
        return Some(format!(
            "`{class}`: grammar variant {variant:?}, takumi {:?}",
            theirs.variant
        ));
    }
    (ours.important != theirs.important).then(|| {
        format!(
            "`{class}`: grammar important={}, takumi important={}",
            ours.important, theirs.important
        )
    })
}

/// Checks every corpus line; returns one message per failure.
fn check_corpus(corpus: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for (number, line) in corpus.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(class), Some(expected)) = (parts.next(), parts.next()) else {
            failures.push(format!("line {}: malformed: {line}", number + 1));
            continue;
        };
        let expected = match expected {
            "builtin" => Verdict::Builtin,
            "theme" => Verdict::Theme,
            "unknown" => Verdict::Unknown,
            other => {
                failures.push(format!(
                    "line {}: `{class}`: unknown verdict `{other}`",
                    number + 1
                ));
                continue;
            }
        };
        let narrowed = parts.next().is_some_and(|p| p.starts_with("narrowed="));
        let ours = verdict_of(grammar(class).as_ref());
        if ours != expected {
            failures.push(format!(
                "`{class}`: grammar {ours:?}, corpus expects {expected:?}"
            ));
        }
        if narrowed {
            let theirs = oracle(class).verdict;
            if ours != Verdict::Unknown || theirs == Verdict::Unknown {
                failures.push(format!(
                    "`{class}`: marked narrowed but grammar {ours:?}, takumi {theirs:?}"
                ));
            }
        } else if let Some(mismatch) = compare(class) {
            failures.push(mismatch);
        }
    }
    failures
}

#[test]
fn oracle_markers_are_pinned() {
    let builtin = oracle("bg-red-500");
    assert_eq!(builtin.verdict, Verdict::Builtin);
    assert_eq!(oracle("bg-brand").verdict, Verdict::Theme);
    assert_eq!(oracle("foo:flex").variant, VariantKind::Custom);
    assert_eq!(oracle("sm:flex").variant, VariantKind::Builtin);
    assert!(oracle("!flex").important);
    assert!(!builtin.important);
    assert_eq!(builtin.variant, VariantKind::None);
    assert_eq!(oracle("flx").verdict, Verdict::Unknown);
}

#[test]
fn samples_agree_with_takumi() {
    let samples = tw::__crosscheck::samples();
    assert!(samples.len() > 500, "only {} samples", samples.len());
    let failures: Vec<String> = samples.iter().filter_map(|class| compare(class)).collect();
    assert!(
        failures.is_empty(),
        "{} of {} samples disagree with takumi:\n{}",
        failures.len(),
        samples.len(),
        failures.join("\n")
    );
}

#[test]
fn corpus_agrees_with_takumi() {
    let failures = check_corpus(CORPUS);
    assert!(
        failures.is_empty(),
        "{} corpus lines fail:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn a_wrong_corpus_line_fails_naming_the_class() {
    let failures = check_corpus("flex unknown\n");
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("`flex`"), "{failures:?}");
}

#[test]
fn every_sample_renders_without_panicking() {
    let fonts = Fonts::default();
    let mut panics = Vec::new();
    for class in tw::__crosscheck::samples() {
        let Ok(values) = TailwindValues::from_str(&class) else {
            continue;
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            let options = RenderOptions::builder()
                .viewport(Viewport::new((64, 64)))
                .fonts(&fonts)
                .node(Node::container([]).with_tw(values))
                .build();
            // A render error is allowed; only a panic fails.
            let _ = takumi::render(options);
        }));
        if result.is_err() {
            panics.push(class);
        }
    }
    assert!(
        panics.is_empty(),
        "rendering panicked for: {}",
        panics.join(", ")
    );
}
