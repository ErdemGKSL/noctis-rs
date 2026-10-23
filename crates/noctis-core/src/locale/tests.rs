//! Story 11.3 rows 1-17 and 26: the built-in resolution, the raw-code
//! parse, the locale cell and the guild record.

use noctis_rules::Locale::{self, De, EnGb, EnUs, Es419, EsEs, PtBr, Tr};

use super::*;
use crate::test_support::{
    Capture, guild_create, guild_create_unavailable, guild_delete, guild_update,
};

/// The resolution of raw codes over `available` and `default`.
fn run(available: &[Locale], default: Locale, user: &str, guild: Option<&str>) -> Locales {
    resolve(
        LocaleSet::of(available.iter().copied()),
        default,
        parse(Some(user)),
        parse(guild),
    )
}

fn pair(user: Locale, guild: Locale) -> Locales {
    Locales { user, guild }
}

#[test]
fn index_is_the_all_position() {
    for (i, locale) in Locale::ALL.into_iter().enumerate() {
        assert_eq!(usize::from(index(locale)), i);
        assert_eq!(at(u16::from(index(locale))), locale);
    }
}

/// Rows 1 and 2: a code without a file takes its sibling's, before the
/// default.
#[test]
fn sibling_before_default() {
    assert_eq!(run(&[EnUs], EnUs, "en-GB", None), pair(EnUs, EnUs));
    assert_eq!(run(&[EnUs, Tr], Tr, "en-GB", None), pair(EnUs, EnUs));
}

/// Row 3: a code's own file wins over its sibling's.
#[test]
fn own_file_wins() {
    assert_eq!(run(&[EnGb, EnUs], EnUs, "en-GB", None).user, EnGb);
}

/// Rows 4 and 5: without a sibling the chain moves to the guild, then the
/// default.
#[test]
fn no_sibling_moves_on() {
    assert_eq!(run(&[EnUs, Tr], Tr, "pt-BR", Some("de")), pair(Tr, Tr));
    assert_eq!(run(&[EnUs, Tr], Tr, "pt-BR", Some("tr")), pair(Tr, Tr));
}

/// Row 6: a tr user in an en-US guild.
#[test]
fn tr_user_in_en_us_guild() {
    assert_eq!(run(&[EnUs, Tr], EnUs, "tr", Some("en-US")), pair(Tr, EnUs));
}

/// Row 7: the es siblings.
#[test]
fn es_sibling() {
    assert_eq!(run(&[EsEs], EnUs, "es-419", None).user, EsEs);
    assert_eq!(run(&[Es419], EnUs, "es-ES", None).user, Es419);
}

/// Row 8: both of the user's steps miss, so the guild step decides.
#[test]
fn user_steps_miss() {
    assert_eq!(run(&[Tr], Tr, "en-GB", Some("tr")), pair(Tr, Tr));
}

/// Row 9: a DM's Guild Locale falls back to the user's.
#[test]
fn dm_guild_falls_back_to_user() {
    assert_eq!(run(&[EnUs, Tr], EnUs, "tr", None), pair(Tr, Tr));
}

/// Row 10: nothing known is the default.
#[test]
fn nothing_known_is_default() {
    let set = LocaleSet::of([EnUs, Tr]);
    assert_eq!(resolve(set, Tr, None, None), Locales::both(Tr));
}

/// Row 11: an unknown code is skipped silently.
#[test]
fn unknown_code_skipped() {
    let capture = Capture::default();
    let resolved = tracing::subscriber::with_default(capture.subscriber(), || {
        run(&[De, EnUs], EnUs, "xx-YY", Some("de"))
    });
    assert_eq!(resolved.user, De);
    assert_eq!(capture.text(), "");
}

/// Rows 12 and 13: wrong case, bare, empty, garbage and huge codes are
/// absent.
#[test]
fn malformed_codes_are_absent() {
    let huge = "x".repeat(10 * 1024);
    for code in [
        "en-us", "en", "", "en_US", "türkçe", " tr", "tr ", "tr\u{0}", &huge,
    ] {
        assert_eq!(parse(Some(code)), None, "{code:?}");
        assert_eq!(
            run(&[EnUs, Tr], EnUs, code, Some("tr")).user,
            Tr,
            "{code:?}"
        );
    }
    assert_eq!(parse(None), None);
}

/// Row 14: no catalogs means `{en-US}` and `en-US`.
#[test]
fn no_catalogs() {
    let table = AppLocales::new(&[], None);
    assert_eq!(table.default_locale(), EnUs);
    assert_eq!(table.available(), LocaleSet::of([EnUs]));
    assert_eq!(
        table.builtin_raw(Some("tr"), Some("de")),
        Locales::both(EnUs)
    );
}

/// Row 15: with every file, each code resolves to itself.
#[test]
fn all_files_resolve_to_themselves() {
    for locale in Locale::ALL {
        assert_eq!(
            run(&Locale::ALL, EnUs, locale.as_str(), None).user,
            locale,
            "{locale}"
        );
    }
}

/// Row 16: with only en-US, every code resolves to en-US.
#[test]
fn only_en_us() {
    for locale in Locale::ALL {
        assert_eq!(run(&[EnUs], EnUs, locale.as_str(), None).user, EnUs);
    }
}

/// Row 17: a default without en-US.
#[test]
fn default_without_en_us() {
    assert_eq!(run(&[Tr], Tr, "en-US", None).user, Tr);
}

/// Events: the Guild step, then the default, for both locales.
#[test]
fn events_resolve_guild_then_default() {
    let set = LocaleSet::of([EnUs, Tr]);
    assert_eq!(resolve(set, EnUs, None, Some(Tr)), Locales::both(Tr));
    assert_eq!(resolve(set, EnUs, None, Some(De)), Locales::both(EnUs));
    assert_eq!(resolve(set, EnUs, None, Some(EnGb)), Locales::both(EnUs));
}

/// The cell keeps both locales in one atomic; updates read the current
/// pair.
#[test]
fn cell_round_trips() {
    let cell = LocaleCell::new(pair(Tr, De));
    assert_eq!(cell.get(), pair(Tr, De));
    cell.update(|l| Locales { guild: PtBr, ..l });
    assert_eq!(cell.get(), pair(Tr, PtBr));
    cell.update(|_| Locales::both(EnGb));
    assert_eq!(cell.get(), Locales::both(EnGb));
    assert_eq!(format!("{cell:?}"), "Locales { user: EnGb, guild: EnGb }");
}

/// Rows 22-24: the record follows `GUILD_CREATE`, `GUILD_UPDATE` and
/// `GUILD_DELETE`.
#[test]
fn record_follows_guild_events() {
    let table = AppLocales::new(&[], None);
    let nine = Some(Id::new(9));
    table.observe(&guild_create(9, "tr"));
    assert_eq!(table.guild(nine), Some(Tr));
    table.observe(&guild_update(9, "de"));
    assert_eq!(table.guild(nine), Some(De));
    table.observe(&guild_delete(9, Some(true)));
    assert_eq!(table.guild(nine), Some(De));
    table.observe(&guild_delete(9, None));
    assert_eq!(table.guild(nine), None);
    table.observe(&guild_create(9, "tr"));
    table.observe(&guild_delete(9, Some(false)));
    assert_eq!(table.guild(nine), None);
    table.observe(&guild_create_unavailable(9));
    assert_eq!(table.guild(nine), None);
    table.observe(&guild_create(9, "en-us"));
    assert_eq!(table.guild(nine), None);
    table.observe(&guild_create(9, "tr"));
    table.observe(&guild_update(9, "en-us"));
    assert_eq!(table.guild(nine), None);
    assert_eq!(table.guild(None), None);
}

/// Row 25: an interaction refreshes an existing record only.
#[test]
fn interactions_refresh_existing_records_only() {
    let table = AppLocales::new(&[], None);
    table.observe(&guild_create(9, "tr"));
    table.refresh(Some(Id::new(9)), Some(De));
    assert_eq!(table.guild(Some(Id::new(9))), Some(De));
    table.refresh(Some(Id::new(9)), None);
    assert_eq!(table.guild(Some(Id::new(9))), Some(De));
    table.refresh(Some(Id::new(10)), Some(De));
    assert_eq!(table.guild(Some(Id::new(10))), None);
    assert_eq!(table.recorded(), 1);
}

/// Row 26: the record is bounded by guild membership.
#[test]
fn record_is_bounded() {
    let table = AppLocales::new(&[], None);
    for id in 1..=10_000 {
        table.observe(&guild_create(id, "tr"));
    }
    assert_eq!(table.recorded(), 10_000);
    for id in 1..=10_000 {
        table.observe(&guild_delete(id, None));
    }
    assert_eq!(table.recorded(), 0);
    for id in 1..=10_000 {
        table.refresh(Some(Id::new(id)), Some(Tr));
    }
    assert_eq!(table.recorded(), 0);
}

fn stage4(user: Option<Locale>, guild: Option<Locale>) -> Stage4 {
    Stage4 {
        invocation: Invocation::Command { path: "where" },
        guild_id: Some(Id::new(7)),
        user_id: Some(Id::new(1)),
        discord_user: user,
        discord_guild: guild,
    }
}

/// Rows 29-31: the resolver sees the request and its value is used
/// verbatim.
#[test]
fn resolver_sees_request_and_wins() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let custom = move |request: &LocaleRequest| {
        log.lock().unwrap().push(*request);
        Locales {
            guild: PtBr,
            ..request.resolved
        }
    };
    let table = AppLocales::new(&[], Some(Arc::new(custom)));
    let locales = table.resolve_run(stage4(parse(Some("xx")), Some(Tr)), "h");
    assert_eq!(locales, pair(EnUs, PtBr));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].discord_user, None);
    assert_eq!(seen[0].discord_guild, Some(Tr));
    assert_eq!(seen[0].resolved, Locales::both(EnUs));
    assert_eq!(seen[0].invocation, Invocation::Command { path: "where" });
}

/// Row 34: a panicking resolver keeps the built-in resolution, with one
/// `error` naming the handler.
#[test]
fn resolver_panic_keeps_builtin() {
    let panics = |_: &LocaleRequest| -> Locales { panic!("boom") };
    let table = AppLocales::new(&[], Some(Arc::new(panics)));
    let capture = Capture::default();
    let locales = tracing::subscriber::with_default(capture.subscriber(), || {
        table.resolve_run(stage4(Some(Tr), None), "bot::Where")
    });
    assert_eq!(locales, Locales::both(EnUs));
    let lines = capture.lines_with(&["ERROR", "noctis::i18n", "handler=\"bot::Where\""]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
}
