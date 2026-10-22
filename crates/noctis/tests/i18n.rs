//! Story 11.1: `noctis::i18n!` end to end on the YAML fixtures of
//! `tests/fixtures/i18n/` (Matrix 1, 39, 40, 45-47, 51, 53-55, 58, 59).

#![cfg(feature = "i18n")]

use noctis::rules::{Locale, MessageKey, Translate};

/// `basic/`: en-US, tr, en-GB, es-ES (lacking `a`) and es-419.
#[expect(
    deprecated,
    reason = "es-ES lacks `a`: the sibling fallback under test"
)]
mod basic {
    noctis::i18n!(dir = "tests/fixtures/i18n/basic");
}

/// `partial/`: tr lacks `a` and `b` (LOC-04).
#[expect(deprecated, reason = "LOC-04 fallback under test")]
mod partial {
    noctis::i18n!(dir = "tests/fixtures/i18n/partial");
}

use basic::{Strings, catalog, keys};

/// Matrix 1.
#[test]
fn happy_path() {
    assert_eq!(Strings::of(Locale::Tr).common.refresh(), "Yenile");
    assert_eq!(Strings::of(Locale::EnUs).common.refresh(), "Refresh");
    assert_eq!(keys::COMMON_REFRESH.as_str(), "common.refresh");
    assert_eq!(catalog().get(Locale::Tr, "common.refresh"), Some("Yenile"));
}

/// Matrix 39, 40, 45-47 and the keyword accessors of row 29.
#[test]
fn accessors_format_their_arguments() {
    let en = Strings::of(Locale::EnUs);
    assert_eq!(en.greet("Ada", 3), "Hi Ada, Ada! You have 3");
    assert_eq!(
        Strings::of(Locale::Tr).greet("Ada", 3),
        "Selam Ada, Ada! 3 tane var"
    );
    assert_eq!(en.p("a", "b"), "b of a");
    assert_eq!(en.braces(7), "{7}");
    assert_eq!(en.unicode("ü"), "çğ😀 ü");
    assert_eq!(en.empty(), "");
    assert_eq!(en.r#type(), "Kind");
    assert_eq!(en.profile.r#match("x"), "Match x");
    assert_eq!(keys::PROFILE_MATCH.as_str(), "profile.match");
    let plain: &'static str = en.common.refresh();
    let formatted: String = en.greet("a", 1);
    assert_eq!(
        (plain, formatted.as_str()),
        ("Refresh", "Hi a, a! You have 1")
    );
}

/// Matrix 53-55.
#[test]
fn rows_follow_siblings_then_the_default() {
    assert_eq!(Strings::of(Locale::EnGb).a(), "gb-a");
    assert_eq!(Strings::of(Locale::Es419).a(), "latam-a");
    assert_eq!(Strings::of(Locale::EsEs).a(), "latam-a");
    assert_eq!(Strings::of(Locale::EsEs).common.refresh(), "Actualizar");
    assert_eq!(Strings::of(Locale::PtBr).a(), "us-a");
    for locale in Locale::ALL {
        let s = Strings::of(locale);
        assert!(!s.common.refresh().is_empty(), "{locale}");
    }
}

/// Matrix 51: the LOC-04 fallback, through the accessors and `Translate`.
#[test]
fn partial_locales_fall_back() {
    let tr = partial::Strings::of(Locale::Tr);
    assert_eq!((tr.a(), tr.b(), tr.c()), ("A", "B", "Ç"));
    let catalog = partial::catalog();
    assert_eq!(catalog.get(Locale::Tr, "a"), None);
    assert_eq!(
        catalog
            .translate(Locale::Tr, partial::keys::A, &[])
            .as_deref(),
        Some("A")
    );
}

/// Matrix 58, 59, 34, 36.
#[test]
fn catalog_lookups() {
    let c = catalog();
    assert_eq!(c.origin(), "noctis/tests/fixtures/i18n/basic");
    assert_eq!(c.default_locale(), Locale::EnUs);
    assert_eq!(
        c.locales().collect::<Vec<_>>(),
        [
            Locale::EnGb,
            Locale::EnUs,
            Locale::EsEs,
            Locale::Es419,
            Locale::Tr
        ]
    );
    assert_eq!(c.get(Locale::De, "common.refresh"), None);
    let keys: Vec<&str> = c.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(keys, sorted);
    assert!(keys.contains(&"cmd.setup.description") && keys.contains(&"noctis.expired"));
    assert_eq!(
        c.get(Locale::EnUs, "cmd.setup.description"),
        Some("Set {x} up")
    );
    assert_eq!(c.placeholders("cmd.setup.description"), None);
    assert_eq!(c.placeholders("greet"), Some(&["name", "n"][..]));
    assert_eq!(c.placeholders("p"), Some(&["0", "1"][..]));
    assert_eq!(c.placeholders("noctis.expired"), Some(&[][..]));
    let n: &dyn std::fmt::Display = &5;
    let name: &dyn std::fmt::Display = &"Ada";
    assert_eq!(
        c.translate(
            Locale::Ja,
            MessageKey::new("greet"),
            &[("name", name), ("n", n)]
        )
        .as_deref(),
        Some("Hi Ada, Ada! You have 5")
    );
    assert_eq!(c.translate(Locale::Tr, MessageKey::new("nope"), &[]), None);
}
