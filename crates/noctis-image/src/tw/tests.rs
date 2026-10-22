//! One test per row of the Story 10.1 I/O matrix (the xtask rows live in
//! `xtask/src/tw_grammar/tests.rs`), plus the rename and hint invariants.

use super::generated::{EXACT_RENAMES, HINTS, PREFIX_RENAMES};
use super::*;

/// A theme defining exactly the listed tokens.
struct Tokens(&'static [(ThemeNamespace, &'static str)]);

impl ThemeTokens for Tokens {
    fn contains(&self, ns: ThemeNamespace, token: &str) -> bool {
        self.0.iter().any(|&(n, t)| n == ns && t == token)
    }
}

fn utility(class: &str) -> Option<Utility<'_>> {
    classify(class).map(|c| c.utility)
}

fn assert_builtin_ok(classes: &[&str]) {
    for class in classes {
        assert_eq!(utility(class), Some(Utility::Builtin), "{class}");
        assert_eq!(validate(class, &NoTheme), Ok(()), "{class}");
    }
}

fn assert_unknown(classes: &[&str]) {
    for class in classes {
        assert_eq!(classify(class), None, "{class}");
        assert!(validate(class, &NoTheme).is_err(), "{class}");
    }
}

fn theme(class: &str) -> (&'static [ThemeNamespace], &str) {
    match utility(class) {
        Some(Utility::Theme { namespaces, token }) => (namespaces, token),
        other => panic!("{class}: expected a theme utility, got {other:?}"),
    }
}

fn only_suggestion(classes: &str) -> Option<Suggestion> {
    let err = validate(classes, &NoTheme).expect_err(classes);
    assert_eq!(err.classes().len(), 1, "{err}");
    err.classes()[0].suggestion().cloned()
}

#[test]
fn fixed_utility() {
    assert_builtin_ok(&["flex", "hidden", "shadow-2xs"]);
}

#[test]
fn spacing() {
    assert_builtin_ok(&["p-4", "m-0", "w-2.5", "gap-x-96"]);
}

#[test]
fn keyword_and_fraction() {
    assert_builtin_ok(&["w-full", "w-1/2", "items-center"]);
}

#[test]
fn palette_color_and_opacity_bounds() {
    assert_builtin_ok(&["bg-red-500", "bg-red-500/0", "bg-red-500/100"]);
}

#[test]
fn opacity_above_100_is_unknown() {
    assert_unknown(&["bg-red-500/101"]);
}

#[test]
fn v4_gradient() {
    assert_builtin_ok(&["bg-linear-to-r", "from-red-500", "via-50%"]);
}

#[test]
fn v3_gradient_suggests_the_v4_name() {
    assert!(matches!(
        utility("bg-gradient-to-r"),
        Some(Utility::Theme { .. })
    ));
    assert_eq!(
        only_suggestion("bg-gradient-to-r"),
        Some(Suggestion::Rename("bg-linear-to-r".to_owned()))
    );
}

#[test]
fn v3_gradient_rename_keeps_variant_and_important() {
    assert!(matches!(
        utility("sm:!bg-gradient-to-r"),
        Some(Utility::Theme { .. })
    ));
    assert_eq!(
        only_suggestion("sm:!bg-gradient-to-r"),
        Some(Suggestion::Rename("sm:!bg-linear-to-r".to_owned()))
    );
    assert_eq!(
        suggest("bg-gradient-to-tl!"),
        Some(Suggestion::Rename("bg-linear-to-tl!".to_owned()))
    );
}

#[test]
fn v3_exact_rename() {
    assert_eq!(classify("overflow-ellipsis"), None);
    assert_eq!(
        only_suggestion("overflow-ellipsis"),
        Some(Suggestion::Rename("text-ellipsis".to_owned()))
    );
}

#[test]
fn v3_hints() {
    assert!(matches!(
        utility("bg-opacity-50"),
        Some(Utility::Theme { .. })
    ));
    assert_eq!(classify("space-x-4"), None);
    assert!(
        matches!(only_suggestion("bg-opacity-50"), Some(Suggestion::Hint(h)) if h.contains("opacity modifier"))
    );
    assert!(
        matches!(only_suggestion("space-x-4"), Some(Suggestion::Hint(h)) if h.contains("gap-x-*"))
    );
}

#[test]
fn unknown_prefix_has_no_suggestion() {
    for class in ["ring-2", "divide-y", "z-10", "flx"] {
        assert_eq!(classify(class), None, "{class}");
        assert_eq!(only_suggestion(class), None, "{class}");
    }
}

#[test]
fn theme_token() {
    assert_eq!(theme("bg-brand"), (&[ThemeNamespace::Color][..], "brand"));
    assert_eq!(
        theme("text-brand/50"),
        (&[ThemeNamespace::Color][..], "brand")
    );
    let brand = Tokens(&[(ThemeNamespace::Color, "brand")]);
    for class in ["bg-brand", "text-brand/50"] {
        assert!(validate(class, &NoTheme).is_err(), "{class}");
        assert_eq!(validate(class, &brand), Ok(()), "{class}");
    }
}

#[test]
fn theme_token_in_several_namespaces() {
    assert_eq!(
        theme("text-title"),
        (&[ThemeNamespace::Text, ThemeNamespace::Color][..], "title")
    );
    for ns in [ThemeNamespace::Text, ThemeNamespace::Color] {
        let tokens: &'static [(ThemeNamespace, &str)] = match ns {
            ThemeNamespace::Text => &[(ThemeNamespace::Text, "title")],
            _ => &[(ThemeNamespace::Color, "title")],
        };
        assert_eq!(validate("text-title", &Tokens(tokens)), Ok(()), "{ns:?}");
    }
    assert!(validate("text-title", &Tokens(&[(ThemeNamespace::Spacing, "title")])).is_err());
}

#[test]
fn opacity_on_a_non_color_or_out_of_range() {
    assert_unknown(&["m-brand/50", "bg-brand/150"]);
}

#[test]
fn negative() {
    assert_builtin_ok(&["-m-4", "-translate-x-1/2", "-col-start-2"]);
}

#[test]
fn negative_rejected() {
    assert_unknown(&["-m-auto", "-bg-red-500", "-flex"]);
}

#[test]
fn negative_theme_token() {
    assert_unknown(&["-bg-brand"]);
}

#[test]
fn breakpoints() {
    assert_builtin_ok(&["sm:flex", "2xl:p-4"]);
    assert_eq!(
        classify("sm:flex").and_then(|c| c.variant),
        Some(Variant::Builtin("sm"))
    );
    assert_unknown(&["SM:flex"]);
}

#[test]
fn custom_variant() {
    let class = classify("hover:flex").expect("hover:flex");
    assert_eq!(class.variant, Some(Variant::Custom("hover")));
    assert_eq!(class.utility, Utility::Builtin);
    assert!(validate("hover:flex", &NoTheme).is_err());
    let hover = Tokens(&[(ThemeNamespace::Breakpoint, "hover")]);
    assert_eq!(validate("hover:flex", &hover), Ok(()));
}

#[test]
fn stacked_or_malformed_variant() {
    assert_unknown(&["sm:md:flex", "!sm:flex", ":flex"]);
}

#[test]
fn important() {
    for class in ["!p-4", "p-4!", "!p-4!"] {
        let parsed = classify(class).expect(class);
        assert!(parsed.important, "{class}");
        assert_eq!(parsed.utility, Utility::Builtin, "{class}");
    }
    assert!(!classify("p-4").expect("p-4").important);
    assert_unknown(&["!!p-4"]);
}

#[test]
fn arbitrary() {
    assert_builtin_ok(&["w-[10px]", "bg-[#ff0000]", "rotate-[45deg]"]);
}

#[test]
fn arbitrary_rejected() {
    assert_unknown(&["w-[]", "w-[calc(1px+1px)]", "bg-[url(a.png)]"]);
}

#[test]
fn numeric_oddities() {
    assert_unknown(&["w-inf", "w-NaN", "w-1e3", "w-FULL", "w-0.1"]);
}

#[test]
fn unicode() {
    assert_unknown(&["bg-rød", "w-４", "ｆlex"]);
}

#[test]
fn size_bounds() {
    let token_64 = format!("a{}", "b".repeat(63));
    let token_65 = format!("a{}", "b".repeat(64));
    assert!(matches!(
        utility(&format!("bg-{token_64}")),
        Some(Utility::Theme { .. })
    ));
    assert_eq!(classify(&format!("bg-{token_65}")), None);
    assert!(classify(&format!("{token_64}:flex")).is_some());
    assert_eq!(classify(&format!("{token_65}:flex")), None);

    // `font-[<ident>]` has no length cap of its own, so it reaches the class cap.
    let class = |len: usize| format!("font-[A{}]", "a".repeat(len - 8));
    let (at_limit, over_limit) = (class(256), class(257));
    assert_eq!((at_limit.len(), over_limit.len()), (256, 257));
    assert_eq!(utility(&at_limit), Some(Utility::Builtin));
    assert_eq!(classify(&over_limit), None);
}

#[test]
fn whitespace() {
    assert_eq!(validate("", &NoTheme), Ok(()));
    assert_eq!(validate("  \t\n", &NoTheme), Ok(()));
    let err = validate("flex\u{3000}p-4 bad", &NoTheme).expect_err("bad");
    assert_eq!(
        err.classes()
            .iter()
            .map(UnknownClass::class)
            .collect::<Vec<_>>(),
        ["bad"]
    );
    assert_eq!(validate("flex\u{3000}p-4", &NoTheme), Ok(()));
}

#[test]
fn duplicates_are_reported_once_in_first_seen_order() {
    let err = validate("bad bad flex zz bad", &NoTheme).expect_err("bad");
    assert_eq!(
        err.classes()
            .iter()
            .map(UnknownClass::class)
            .collect::<Vec<_>>(),
        ["bad", "zz"]
    );
}

#[test]
fn display_reads_like_the_acceptance_criterion() {
    let err = validate("p-4 bg-gradient-to-r", &NoTheme).expect_err("v3 class");
    assert_eq!(
        err.to_string(),
        "unknown Tailwind class `bg-gradient-to-r`; use `bg-linear-to-r` (Tailwind v4)"
    );
    let err = validate("flx space-y-2", &NoTheme).expect_err("two classes");
    assert_eq!(
        err.to_string(),
        "unknown Tailwind class `flx`\nunknown Tailwind class `space-y-2`; use \
         `gap-x-*`/`gap-y-*` on a flex or grid parent"
    );
}

#[test]
fn every_rename_target_is_builtin_and_no_source_is() {
    for (from, to, suffixes) in PREFIX_RENAMES {
        for suffix in *suffixes {
            let (source, target) = (format!("{from}{suffix}"), format!("{to}{suffix}"));
            assert_eq!(utility(&target), Some(Utility::Builtin), "{target}");
            assert_ne!(utility(&source), Some(Utility::Builtin), "{source}");
            assert_eq!(suggest(&source), Some(Suggestion::Rename(target)));
        }
    }
    for (from, to) in EXACT_RENAMES {
        assert_eq!(utility(to), Some(Utility::Builtin), "{to}");
        assert_ne!(utility(from), Some(Utility::Builtin), "{from}");
    }
}

#[test]
fn no_hint_source_is_builtin() {
    for (prefix, _) in HINTS {
        for suffix in ["0", "4", "50", "100", "px", "reverse"] {
            let class = format!("{prefix}{suffix}");
            assert_ne!(utility(&class), Some(Utility::Builtin), "{class}");
            assert!(
                matches!(suggest(&class), Some(Suggestion::Hint(_))),
                "{class}"
            );
        }
    }
}

#[test]
fn css_prefixes() {
    assert_eq!(ThemeNamespace::Color.css_prefix(), "--color-");
    assert_eq!(ThemeNamespace::Breakpoint.css_prefix(), "--breakpoint-");
}

#[test]
fn generated_tables_are_sorted_for_binary_search() {
    assert!(FIXED.is_sorted());
    assert!(PREFIXES.is_sorted_by_key(|(k, _)| *k));
    assert!(VAR_TARGETS.is_sorted_by_key(|(k, _)| *k));
}

#[test]
fn samples_are_nonempty_and_unique() {
    let samples = __crosscheck::samples();
    let unique: BTreeSet<&String> = samples.iter().collect();
    assert_eq!(unique.len(), samples.len());
    assert!(samples.iter().any(|s| s == "brand:flex"));
}

#[test]
fn var_target_prefix_stops_the_search() {
    // takumi reads `--radius-5` and stops; the grammar must not retry `rounded`.
    assert_eq!(classify("rounded-t-5"), None);
}

#[test]
fn dotted_custom_variant_is_unknown() {
    assert_eq!(classify("a.b:flex"), None);
}

#[test]
fn negative_class_gets_a_hint() {
    assert!(matches!(suggest("-space-x-4"), Some(Suggestion::Hint(_))));
}
