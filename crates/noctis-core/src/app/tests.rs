use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use noctis_ports::{
    BoxFuture, PortError, RenderError, RenderJob, RenderOutput, TemplateDescriptor,
};

use super::*;
use crate::ports::PortSlot;
use crate::{AppIdError, HttpConfigError, Resource, Shared};

const SECRET: &str = "SECRET123";

fn token() -> Token {
    Token::new(SECRET).unwrap()
}

/// A lookup answering only `NOCTIS_TOKEN` (every other variable is unset).
fn lookup(value: Option<OsString>) -> impl Fn(&str) -> Option<OsString> {
    lookup_var("NOCTIS_TOKEN", value)
}

/// A lookup answering only `name` (every other variable is unset).
fn lookup_var(name: &'static str, value: Option<OsString>) -> impl Fn(&str) -> Option<OsString> {
    move |var| {
        assert!(
            matches!(
                var,
                "NOCTIS_TOKEN"
                    | "NOCTIS_HTTP_PROXY"
                    | "NOCTIS_GATEWAY_URL"
                    | "NOCTIS_SHARD_TOTAL"
                    | "NOCTIS_SHARD_RANGE"
                    | "NOCTIS_SHUTDOWN_GRACE"
                    | "NOCTIS_PUBLISHER"
                    | "NOCTIS_DEV_GUILD"
                    | "NOCTIS_FEATURES"
                    | "NOCTIS_PAYLOAD_KEY"
                    | "NOCTIS_PAYLOAD_KEY_PREVIOUS"
                    | "NOCTIS_APP_ID"
            ),
            "unexpected variable {var}"
        );
        if var == name { value.clone() } else { None }
    }
}

// ---- from_env (Matrix 8) ----

#[test]
fn from_env_token_rules() {
    for raw in [None, Some(""), Some("  \n")] {
        let b = AppBuilder::from_lookup(lookup(raw.map(OsString::from))).unwrap();
        assert!(b.token.is_none(), "{raw:?}");
    }
    let b = AppBuilder::from_lookup(lookup(Some("  tok \n".into()))).unwrap();
    assert_eq!(b.token.as_ref().unwrap().expose(), "tok");

    let err = AppBuilder::from_lookup(lookup(Some("a b".into()))).unwrap_err();
    assert_eq!(
        err,
        EnvError::InvalidToken {
            var: "NOCTIS_TOKEN",
            source: crate::TokenError::InvalidChar { index: 1 },
        }
    );
    assert!(!err.to_string().contains("a b"));
    assert!(!format!("{err:?}").contains("a b"));
}

#[cfg(unix)]
#[test]
fn from_env_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err =
        AppBuilder::from_lookup(lookup(Some(OsString::from_vec(vec![b'a', 0xff])))).unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_TOKEN"
        }
    );
    let err: Error = err.into();
    assert!(matches!(err, Error::Env(EnvError::NotUnicode { .. })));
}

// ---- build (Matrix 9-19) ----

/// A `CacheStore` counting `get` calls and storing nothing.
struct CountingCache(Arc<AtomicUsize>);

impl CacheStore for CountingCache {
    fn get<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(None) })
    }
    fn put<'a>(
        &'a self,
        _: &'a str,
        _: Bytes,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async { Ok(()) })
    }
    fn put_if_absent<'a>(
        &'a self,
        _: &'a str,
        _: Bytes,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        Box::pin(async { Ok(false) })
    }
    fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async { Ok(()) })
    }
    fn incr_with_ttl<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        Box::pin(async { Ok(0) })
    }
}

#[derive(Debug, PartialEq)]
struct Meta(&'static str);

#[tokio::test]
async fn build_happy() {
    let gets = Arc::new(AtomicUsize::new(0));
    let app = App::builder()
        .token(token())
        .state(Meta("noctis"))
        .cache_store(CountingCache(Arc::clone(&gets)))
        .build()
        .unwrap();
    assert!(Arc::ptr_eq(app.http(), app.http()));
    assert_eq!(app.http().token(), Some(&*format!("Bot {SECRET}")));
    let prefixed = App::builder()
        .token(Token::new("Bot abc").unwrap())
        .build()
        .unwrap();
    assert_eq!(prefixed.http().token(), Some("Bot abc"));
    assert_eq!(app.http_timeout(), Duration::from_secs(10));
    assert_eq!(app.intents(), Intents::GUILDS);
    assert_eq!(*app.state::<Meta>().unwrap(), Meta("noctis"));
    assert!(app.state::<u64>().is_none());
    assert!(!app.ports().is_default(PortSlot::CacheStore));
    assert_eq!(app.ports().cache_store.get("k").await.unwrap(), None);
    assert_eq!(gets.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn build_without_token_and_two_apps() {
    let a = App::builder().build().unwrap();
    let b = App::builder().build().unwrap();
    assert!(a.token.is_none());
    assert!(a.http().token().is_none());
    assert!(!Arc::ptr_eq(a.http(), b.http()));
}

#[tokio::test]
async fn defaults() {
    let app = App::builder().build().unwrap();
    for slot in PortSlot::ALL {
        assert!(app.ports().is_default(*slot), "{slot:?}");
    }
    assert_eq!(app.ports().default_slots().len(), 11);
    assert!(!app.has_renderer());
    let store = &app.ports().cache_store;
    store
        .put("k", Bytes::from_static(b"v"), None)
        .await
        .unwrap();
    assert_eq!(
        store.get("k").await.unwrap(),
        Some(Bytes::from_static(b"v"))
    );
}

#[tokio::test]
async fn custom_port_last_call_wins() {
    let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let app = App::builder()
        .cache_store(CountingCache(Arc::clone(&a)))
        .cache_store(CountingCache(Arc::clone(&b)))
        .build()
        .unwrap();
    app.ports().cache_store.get("k").await.unwrap();
    assert_eq!(b.load(Ordering::SeqCst), 1);
    assert_eq!(a.load(Ordering::SeqCst), 0);
    assert!(!app.ports().is_default(PortSlot::CacheStore));
    assert!(app.ports().is_default(PortSlot::RefStore));
}

#[tokio::test]
async fn timeout_bounds() {
    for (timeout, ok) in [
        (Duration::ZERO, false),
        (Duration::from_micros(999), false),
        (Duration::from_millis(1), true),
        (Duration::from_mins(5), true),
        (Duration::from_mins(5) + Duration::from_nanos(1), false),
        (Duration::MAX, false),
    ] {
        let result = App::builder().http_timeout(timeout).build();
        if ok {
            assert_eq!(result.unwrap().http_timeout(), timeout);
        } else {
            assert_eq!(
                result.unwrap_err().violations(),
                [BuildViolation::InvalidHttpTimeout { actual: timeout }]
            );
        }
    }
}

#[tokio::test]
async fn intents_union_with_guilds_floor() {
    let app = App::builder()
        .intents(Intents::GUILD_MESSAGES)
        .intents(Intents::MESSAGE_CONTENT)
        .build()
        .unwrap();
    assert_eq!(
        app.intents(),
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT
    );
    let app = App::builder().intents(Intents::empty()).build().unwrap();
    assert_eq!(app.intents(), Intents::GUILDS);
}

#[tokio::test]
async fn duplicate_state() {
    let mut builder = App::builder();
    builder.state(1_u32).state(2_u32);
    assert_eq!(*builder.states.get::<u32>().unwrap(), 1);
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicateState { type_name: "u32" }]
    );
}

static SHOP: noctis_i18n::Catalog = noctis_i18n::Catalog::__new(
    "shop/locales",
    noctis_rules::Locale::EnUs,
    &[(noctis_rules::Locale::EnUs, &[("common.ok", "OK")])],
    &[("common.ok", &[])],
);

/// Story 11.1 Matrix 61: a key two catalogs define is a violation naming
/// it and both origins.
#[tokio::test]
async fn duplicate_locale_key() {
    static OTHER: noctis_i18n::Catalog = noctis_i18n::Catalog::__new(
        "other/locales",
        noctis_rules::Locale::EnUs,
        &[(noctis_rules::Locale::Tr, &[("common.ok", "Tamam")])],
        &[],
    );
    let err = App::builder()
        .locales(SHOP)
        .locales(OTHER)
        .state(1_u8)
        .state(2_u8)
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::DuplicateState { type_name: "u8" },
            BuildViolation::DuplicateLocaleKey {
                key: "common.ok",
                first: "shop/locales",
                second: "other/locales",
                first_owner: LocaleOwner::App,
                second_owner: LocaleOwner::App,
            },
        ]
    );
    assert_eq!(
        err.violations()[1].to_string(),
        "locale key `common.ok` is defined by two catalogs: `shop/locales` (AppBuilder::locales) \
         and `other/locales` (AppBuilder::locales)"
    );
}

/// Story 11.1 Matrix 62, 63: one catalog twice is deduplicated; without
/// catalogs the strings are the built-in ones.
#[tokio::test]
async fn locales_are_idempotent() {
    use noctis_rules::{Locale, MessageKey, Translate};
    let app = App::builder().locales(SHOP).locales(SHOP).build().unwrap();
    let ok = app
        .shared()
        .strings
        .translate(Locale::Tr, MessageKey::new("common.ok"), &[]);
    assert_eq!(ok.as_deref(), Some("OK"));
    let app = App::builder().build().unwrap();
    for key in noctis_i18n::keys::ALL {
        assert_eq!(
            app.shared().strings.translate(Locale::Tr, key, &[]),
            noctis_i18n::Builtin.translate(Locale::Tr, key, &[])
        );
    }
}

mod module_locales {
    //! Story 11.2: Module-scoped locale catalogs.

    use noctis_i18n::Catalog;
    use noctis_rules::{Locale, MessageKey, Translate};

    use super::*;

    /// `S`: `shop.title` (en-US "Shop", tr "Dükkan").
    static S: Catalog = Catalog::__new(
        "bot/mod_shop",
        Locale::EnUs,
        &[
            (Locale::EnUs, &[("shop.title", "Shop")]),
            (Locale::Tr, &[("shop.title", "Dükkan")]),
        ],
        &[("shop.title", &[])],
    );
    /// `A`: `admin.done`.
    static A: Catalog = Catalog::__new(
        "bot/mod_admin",
        Locale::EnUs,
        &[(Locale::EnUs, &[("admin.done", "Done")])],
        &[("admin.done", &[])],
    );
    /// `C`: `shop.title` again.
    static C: Catalog = Catalog::__new(
        "bot/mod_clash",
        Locale::EnUs,
        &[(Locale::EnUs, &[("shop.title", "Store")])],
        &[("shop.title", &[])],
    );
    /// `P`: the App-level `common.ok`.
    static P: Catalog = Catalog::__new(
        "bot/app",
        Locale::EnUs,
        &[(Locale::EnUs, &[("common.ok", "OK")])],
        &[("common.ok", &[])],
    );
    /// A third catalog with `shop.title`.
    static X: Catalog = Catalog::__new(
        "bot/mod_extra",
        Locale::EnUs,
        &[(Locale::EnUs, &[("shop.title", "Market")])],
        &[("shop.title", &[])],
    );

    fn text(app: &App, locale: Locale, key: &'static str) -> Option<String> {
        app.shared()
            .strings
            .translate(locale, MessageKey::new(key), &[])
    }

    fn clash(
        key: &'static str,
        first: &'static str,
        second: &'static str,
        first_owner: LocaleOwner,
        second_owner: LocaleOwner,
    ) -> BuildViolation {
        BuildViolation::DuplicateLocaleKey {
            key,
            first,
            second,
            first_owner,
            second_owner,
        }
    }

    /// How many catalogs survive the origin dedup of the merge.
    fn merged_catalogs(app: &[Catalog], roots: &[Module]) -> usize {
        let catalogs: Vec<Catalog> = locale_sources(app, roots)
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        noctis_i18n::AppStrings::new(&catalogs)
            .unwrap()
            .catalogs()
            .len()
    }

    fn build(builder: &mut AppBuilder) -> Result<App, BuildError> {
        builder.build()
    }

    /// Story 11.2 Matrix 1
    #[tokio::test]
    async fn two_modules_with_disjoint_keys() {
        let app = build(
            App::builder()
                .module(Module::new("shop").locales(S))
                .module(Module::new("admin").locales(A)),
        )
        .unwrap();
        assert_eq!(
            text(&app, Locale::Tr, "shop.title").as_deref(),
            Some("Dükkan")
        );
        assert_eq!(
            text(&app, Locale::EnUs, "shop.title").as_deref(),
            Some("Shop")
        );
        assert_eq!(
            text(&app, Locale::Tr, "admin.done").as_deref(),
            Some("Done")
        );
    }

    /// Story 11.2 Matrix 2
    #[tokio::test]
    async fn app_and_module_catalogs_together() {
        let app = build(
            App::builder()
                .locales(P)
                .module(Module::new("shop").locales(S)),
        )
        .unwrap();
        assert_eq!(text(&app, Locale::EnUs, "common.ok").as_deref(), Some("OK"));
        assert_eq!(
            text(&app, Locale::EnUs, "shop.title").as_deref(),
            Some("Shop")
        );
    }

    /// Story 11.2 Matrix 3
    #[tokio::test]
    async fn module_vs_module_clash() {
        let err = build(
            App::builder()
                .module(Module::new("shop").locales(S))
                .module(Module::new("admin").locales(C)),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [clash(
                "shop.title",
                "bot/mod_shop",
                "bot/mod_clash",
                LocaleOwner::Module("shop"),
                LocaleOwner::Module("admin"),
            )]
        );
        assert_eq!(
            err.violations()[0].to_string(),
            "locale key `shop.title` is defined by two catalogs: `bot/mod_shop` (Module `shop`) \
             and `bot/mod_clash` (Module `admin`)"
        );
    }

    /// Story 11.2 Matrix 4
    #[tokio::test]
    async fn app_catalogs_come_first_whatever_the_call_order() {
        let err = build(
            App::builder()
                .module(Module::new("shop").locales(C))
                .locales(S),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [clash(
                "shop.title",
                "bot/mod_shop",
                "bot/mod_clash",
                LocaleOwner::App,
                LocaleOwner::Module("shop"),
            )]
        );
        assert!(
            err.violations()[0].to_string().contains(
                "`bot/mod_shop` (AppBuilder::locales) and `bot/mod_clash` (Module `shop`)"
            )
        );
    }

    /// Story 11.2 Matrix 5
    #[tokio::test]
    async fn three_way_clash() {
        let err = build(
            App::builder()
                .module(Module::new("shop").locales(S))
                .module(Module::new("admin").locales(C))
                .module(Module::new("extra").locales(X)),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [
                clash(
                    "shop.title",
                    "bot/mod_shop",
                    "bot/mod_clash",
                    LocaleOwner::Module("shop"),
                    LocaleOwner::Module("admin"),
                ),
                clash(
                    "shop.title",
                    "bot/mod_shop",
                    "bot/mod_extra",
                    LocaleOwner::Module("shop"),
                    LocaleOwner::Module("extra"),
                ),
            ]
        );
    }

    /// Story 11.2 Matrix 6
    #[tokio::test]
    async fn several_clashing_keys_in_key_order() {
        static ONE: Catalog = Catalog::__new(
            "bot/one",
            Locale::EnUs,
            &[(Locale::EnUs, &[("a.x", "x"), ("a.y", "y")])],
            &[("a.x", &[]), ("a.y", &[])],
        );
        static TWO: Catalog = Catalog::__new(
            "bot/two",
            Locale::EnUs,
            &[(Locale::EnUs, &[("a.x", "x"), ("a.y", "y")])],
            &[("a.x", &[]), ("a.y", &[])],
        );
        let err = build(
            App::builder()
                .module(Module::new("one").locales(ONE))
                .module(Module::new("two").locales(TWO)),
        )
        .unwrap_err();
        let one = LocaleOwner::Module("one");
        let two = LocaleOwner::Module("two");
        assert_eq!(
            err.violations(),
            [
                clash("a.x", "bot/one", "bot/two", one, two),
                clash("a.y", "bot/one", "bot/two", one, two),
            ]
        );
    }

    /// Story 11.2 Matrix 7
    #[tokio::test]
    async fn same_catalog_in_two_modules_is_deduplicated() {
        let app = build(
            App::builder()
                .module(Module::new("shop").locales(S))
                .module(Module::new("admin").locales(S)),
        )
        .unwrap();
        assert_eq!(
            text(&app, Locale::Tr, "shop.title").as_deref(),
            Some("Dükkan")
        );
        let roots = [
            Module::new("shop").locales(S),
            Module::new("admin").locales(S),
        ];
        assert_eq!(merged_catalogs(&[], &roots), 1);
        assert_eq!(
            locale_sources(&[], &roots)[0].1,
            LocaleOwner::Module("shop")
        );
    }

    /// Story 11.2 Matrix 8
    #[tokio::test]
    async fn same_catalog_in_app_and_module_is_deduplicated() {
        let app = build(
            App::builder()
                .locales(S)
                .module(Module::new("shop").locales(S)),
        )
        .unwrap();
        assert_eq!(
            text(&app, Locale::Tr, "shop.title").as_deref(),
            Some("Dükkan")
        );
    }

    /// Story 11.2 Matrix 9
    #[tokio::test]
    async fn one_catalog_twice_in_one_module_is_deduplicated() {
        let shop = Module::new("shop").locales(S).locales(S);
        assert_eq!(merged_catalogs(&[], std::slice::from_ref(&shop)), 1);
        let app = build(App::builder().module(shop)).unwrap();
        assert_eq!(
            text(&app, Locale::Tr, "shop.title").as_deref(),
            Some("Dükkan")
        );
    }

    /// Story 11.2 Matrix 10
    #[tokio::test]
    async fn two_clashing_catalogs_in_one_module() {
        let err =
            build(App::builder().module(Module::new("shop").locales(S).locales(C))).unwrap_err();
        let shop = LocaleOwner::Module("shop");
        assert_eq!(
            err.violations(),
            [clash(
                "shop.title",
                "bot/mod_shop",
                "bot/mod_clash",
                shop,
                shop
            )]
        );
    }

    /// Story 11.2 Matrix 11
    #[tokio::test]
    async fn nested_child_module_contributes() {
        let app = build(
            App::builder().module(Module::new("ops").module(Module::new("ops/audit").locales(A))),
        )
        .unwrap();
        assert_eq!(
            text(&app, Locale::EnUs, "admin.done").as_deref(),
            Some("Done")
        );
    }

    /// Story 11.2 Matrix 12
    #[tokio::test]
    async fn child_clashes_with_parent() {
        let err = build(
            App::builder().module(
                Module::new("ops")
                    .module(Module::new("ops/audit").locales(C))
                    .locales(S),
            ),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [clash(
                "shop.title",
                "bot/mod_shop",
                "bot/mod_clash",
                LocaleOwner::Module("ops"),
                LocaleOwner::Module("ops/audit"),
            )]
        );
    }

    /// Story 11.2 Matrix 13
    #[tokio::test]
    async fn modules_merge_in_pre_order() {
        let err = build(
            App::builder()
                .module(Module::new("r1").module(Module::new("r1/c1").locales(C)))
                .module(Module::new("r2").locales(S)),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [clash(
                "shop.title",
                "bot/mod_clash",
                "bot/mod_shop",
                LocaleOwner::Module("r1/c1"),
                LocaleOwner::Module("r2"),
            )]
        );
        let roots = [
            Module::new("r1").locales(P).module(
                Module::new("r1/c1")
                    .locales(C)
                    .module(Module::new("r1/c1/g1").locales(A)),
            ),
            Module::new("r2").locales(S),
        ];
        let owners: Vec<LocaleOwner> = locale_sources(&[X], &roots)
            .into_iter()
            .map(|(_, o)| o)
            .collect();
        assert_eq!(
            owners,
            [
                LocaleOwner::App,
                LocaleOwner::Module("r1"),
                LocaleOwner::Module("r1/c1"),
                LocaleOwner::Module("r1/c1/g1"),
                LocaleOwner::Module("r2"),
            ]
        );
    }

    /// Story 11.2 Matrix 14
    #[tokio::test]
    async fn strings_only_module() {
        let app = build(App::builder().module(Module::new("strings").locales(S))).unwrap();
        assert_eq!(
            text(&app, Locale::EnUs, "shop.title").as_deref(),
            Some("Shop")
        );
    }

    /// Story 11.2 Matrix 16
    #[tokio::test]
    async fn module_catalog_keeps_its_own_default_locale() {
        static TR_ONLY: Catalog = Catalog::__new(
            "bot/mod_tr",
            Locale::Tr,
            &[(Locale::Tr, &[("tr.only", "Merhaba")])],
            &[("tr.only", &[])],
        );
        let app = build(
            App::builder()
                .locales(P)
                .module(Module::new("tr").locales(TR_ONLY)),
        )
        .unwrap();
        assert_eq!(
            text(&app, Locale::De, "tr.only").as_deref(),
            Some("Merhaba")
        );
    }

    static EXPIRED_TR: Catalog = Catalog::__new(
        "bot/mod_expired",
        Locale::EnUs,
        &[(Locale::Tr, &[("noctis.expired", "Süresi doldu")])],
        &[],
    );

    /// Story 11.2 Matrix 17
    #[tokio::test]
    async fn module_noctis_override_applies_app_wide() {
        let app = build(
            App::builder()
                .module(Module::new("other"))
                .module(Module::new("overrides").locales(EXPIRED_TR)),
        )
        .unwrap();
        let strings = &app.shared().strings;
        assert_eq!(
            strings
                .translate(Locale::Tr, noctis_i18n::keys::EXPIRED, &[])
                .as_deref(),
            Some("Süresi doldu")
        );
        assert_eq!(
            strings.translate(Locale::De, noctis_i18n::keys::EXPIRED, &[]),
            noctis_i18n::Builtin.translate(Locale::De, noctis_i18n::keys::EXPIRED, &[])
        );
    }

    /// Story 11.2 Matrix 18
    #[tokio::test]
    async fn two_modules_overriding_one_noctis_key_clash() {
        static EXPIRED_TR2: Catalog = Catalog::__new(
            "bot/mod_expired2",
            Locale::EnUs,
            &[(Locale::Tr, &[("noctis.expired", "Süresi doldu")])],
            &[],
        );
        let err = build(
            App::builder()
                .module(Module::new("a").locales(EXPIRED_TR))
                .module(Module::new("b").locales(EXPIRED_TR2)),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [clash(
                "noctis.expired",
                "bot/mod_expired",
                "bot/mod_expired2",
                LocaleOwner::Module("a"),
                LocaleOwner::Module("b"),
            )]
        );
    }

    /// Story 11.2 Matrix 19
    #[tokio::test]
    async fn empty_catalog_has_no_effect() {
        static EMPTY: Catalog = Catalog::__new("bot/empty", Locale::EnUs, &[], &[]);
        let app = build(
            App::builder()
                .module(Module::new("empty").locales(EMPTY))
                .module(Module::new("shop").locales(S)),
        )
        .unwrap();
        assert_eq!(
            text(&app, Locale::EnUs, "shop.title").as_deref(),
            Some("Shop")
        );
    }

    /// Story 11.2 Matrix 20
    #[tokio::test]
    async fn no_catalogs_anywhere_is_builtin() {
        let app =
            build(App::builder().module(Module::new("a").module(Module::new("a/b")))).unwrap();
        for key in noctis_i18n::keys::ALL {
            assert_eq!(
                app.shared().strings.translate(Locale::Tr, key, &[]),
                noctis_i18n::Builtin.translate(Locale::Tr, key, &[])
            );
        }
    }

    /// Story 11.2 Matrix 21
    #[tokio::test]
    async fn unicode_module_id_in_the_message() {
        let err = build(
            App::builder()
                .module(Module::new("çay/ölç").locales(S))
                .module(Module::new("x").locales(C)),
        )
        .unwrap_err();
        // The id itself is invalid (FR-44), and the locale clash still names it.
        let [invalid, clash] = err.violations() else {
            panic!("{:?}", err.violations());
        };
        assert!(matches!(
            invalid,
            BuildViolation::InvalidModuleId {
                id: "çay/ölç", ..
            }
        ));
        assert!(clash.to_string().contains("(Module `çay/ölç`)"));
    }

    /// Story 11.2 Matrix 22
    #[tokio::test]
    async fn many_modules_each_with_a_catalog() {
        let mut builder = App::builder();
        let mut keys = Vec::new();
        for i in 0..64 {
            let key: &'static str = Box::leak(format!("m{i}.key").into_boxed_str());
            let origin: &'static str = Box::leak(format!("bot/m{i}").into_boxed_str());
            let value: &'static str = Box::leak(format!("value {i}").into_boxed_str());
            let entries: &'static [(&'static str, &'static str)] =
                Box::leak(Box::new([(key, value)]));
            let files: &'static [(Locale, &'static [(&'static str, &'static str)])] =
                Box::leak(Box::new([(Locale::EnUs, entries)]));
            let placeholders: &'static [(&'static str, &'static [&'static str])] =
                Box::leak(Box::new([(key, &[] as &'static [&'static str])]));
            let id: &'static str = Box::leak(format!("m{i}").into_boxed_str());
            builder.module(Module::new(id).locales(Catalog::__new(
                origin,
                Locale::EnUs,
                files,
                placeholders,
            )));
            keys.push((key, value));
        }
        let app = builder.build().unwrap();
        for (key, value) in keys {
            assert_eq!(text(&app, Locale::Tr, key).as_deref(), Some(value));
        }
    }

    /// Story 11.2 Matrix 23
    #[test]
    fn debug_lists_catalog_origins() {
        let debug = format!("{:?}", Module::new("m").locales(S));
        assert!(debug.contains(r#"locales: ["bot/mod_shop"]"#), "{debug}");
    }

    /// Story 11.2 Matrix 24
    #[test]
    fn locale_owner_display() {
        assert_eq!(LocaleOwner::App.to_string(), "AppBuilder::locales");
        assert_eq!(LocaleOwner::Module("x").to_string(), "Module `x`");
    }

    /// Story 11.2 Matrix 25
    #[tokio::test]
    async fn clash_reported_with_other_violations() {
        let err = build(
            App::builder()
                .module(Module::new("shop").locales(S))
                .module(Module::new("admin").locales(C))
                .state(1_u16)
                .state(2_u16),
        )
        .unwrap_err();
        assert_eq!(
            err.violations(),
            [
                BuildViolation::DuplicateState { type_name: "u16" },
                clash(
                    "shop.title",
                    "bot/mod_shop",
                    "bot/mod_clash",
                    LocaleOwner::Module("shop"),
                    LocaleOwner::Module("admin"),
                ),
            ]
        );
    }
}

mod locale_resolution {
    //! Story 11.3 rows 14, 17-19 and 35: the App's available locales,
    //! default and resolver.

    use noctis_i18n::Catalog;
    use noctis_rules::Locale;

    use super::*;
    use crate::locale::{LocaleSet, Stage4};
    use crate::{Invocation, LocaleRequest, Locales};

    /// An App catalog: en-US default, en-US and tr files.
    static APP: Catalog = Catalog::__new(
        "bot/app_loc",
        Locale::EnUs,
        &[(Locale::EnUs, &[]), (Locale::Tr, &[])],
        &[],
    );
    /// A Module catalog: de default, de file.
    static DE: Catalog = Catalog::__new("bot/mod_de", Locale::De, &[(Locale::De, &[])], &[]);
    /// A tr-only catalog with a tr default.
    static TR: Catalog = Catalog::__new("bot/tr_only", Locale::Tr, &[(Locale::Tr, &[])], &[]);

    fn table(builder: &mut AppBuilder) -> App {
        builder.build().unwrap()
    }

    fn request(user: Option<Locale>) -> Stage4 {
        Stage4 {
            invocation: Invocation::Command { path: "where" },
            guild_id: None,
            user_id: Some(Id::new(1)),
            discord_user: user,
            discord_guild: None,
        }
    }

    /// Row 14: no catalogs is `{en-US}` and `en-US`.
    #[tokio::test]
    async fn no_catalogs() {
        let app = table(&mut App::builder());
        let locales = &app.shared().locales;
        assert_eq!(locales.available(), LocaleSet::of([Locale::EnUs]));
        assert_eq!(locales.default_locale(), Locale::EnUs);
        assert_eq!(
            locales.builtin_raw(Some("tr"), Some("de")),
            Locales::both(Locale::EnUs)
        );
    }

    /// Row 17: a default without en-US.
    #[tokio::test]
    async fn default_without_en_us() {
        let app = table(App::builder().locales(TR));
        let locales = &app.shared().locales;
        assert_eq!(locales.default_locale(), Locale::Tr);
        assert_eq!(locales.builtin_raw(Some("en-US"), None).user, Locale::Tr);
    }

    /// Row 18: availability is the union of the merged catalogs; the
    /// default is the first catalog's (App catalogs first).
    #[tokio::test]
    async fn merged_availability() {
        let app = table(
            App::builder()
                .module(Module::new("de").locales(DE))
                .locales(APP),
        );
        let locales = &app.shared().locales;
        assert_eq!(
            locales.available(),
            LocaleSet::of([Locale::EnUs, Locale::Tr, Locale::De])
        );
        assert_eq!(locales.default_locale(), Locale::EnUs);
        assert_eq!(locales.builtin_raw(Some("de"), None).user, Locale::De);
    }

    /// Row 19: a Module-only catalog sets the default.
    #[tokio::test]
    async fn module_only_catalog() {
        let app = table(App::builder().module(Module::new("de").locales(DE)));
        assert_eq!(app.shared().locales.default_locale(), Locale::De);
    }

    /// Row 35: the last `locale_resolver` call wins.
    #[tokio::test]
    async fn last_resolver_wins() {
        let app = table(
            App::builder()
                .locale_resolver(|_: &LocaleRequest| Locales::both(Locale::Fr))
                .locale_resolver(|_: &LocaleRequest| Locales::both(Locale::Ja)),
        );
        assert_eq!(
            app.shared()
                .locales
                .resolve_run(request(Some(Locale::Tr)), "h"),
            Locales::both(Locale::Ja)
        );
        let debug = format!(
            "{:?}",
            App::builder().locale_resolver(|r: &LocaleRequest| r.resolved)
        );
        assert!(debug.contains("locale_resolver: true"), "{debug}");
    }
}

#[test]
fn build_outside_runtime() {
    let err = App::builder().build().unwrap_err();
    assert_eq!(err.violations(), [BuildViolation::NoTokioRuntime]);
}

struct Reporter;

impl Plugin for Reporter {
    fn install(&self, app: &mut AppBuilder) {
        app.report(BuildViolation::plugin::<Self>("needs config"));
    }
}

#[test]
fn every_violation_at_once() {
    let err = App::builder()
        .state(1_u8)
        .state(2_u8)
        .http_timeout(Duration::ZERO)
        .plugin(Reporter)
        .plugin(Reporter)
        .build()
        .unwrap_err();
    let reporter = std::any::type_name::<Reporter>();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::DuplicateState { type_name: "u8" },
            BuildViolation::DuplicatePlugin {
                type_name: reporter
            },
            BuildViolation::Plugin {
                plugin: reporter,
                message: "needs config".into()
            },
            BuildViolation::InvalidHttpTimeout {
                actual: Duration::ZERO
            },
            BuildViolation::NoTokioRuntime,
        ]
    );
    let text = err.to_string();
    assert!(text.starts_with("5 App-build violation(s)\n"));
    assert_eq!(text.matches("\n  - ").count(), 5);
    assert_eq!(err.clone().into_violations().len(), 5);
}

#[tokio::test]
async fn build_resets_builder() {
    let runtime = Runtime::for_test();
    let mut builder = App::builder();
    builder
        .runtime(&runtime)
        .token(token())
        .state(1_u8)
        .cache_store(CountingCache(Arc::default()));
    let first = builder.build().unwrap();
    assert!(first.token.is_some());
    let second = builder.build().unwrap();
    assert!(second.token.is_none());
    assert!(second.state::<u8>().is_none());
    assert!(second.ports().is_default(PortSlot::CacheStore));
    assert_eq!(second.runtime(), &runtime);
}

#[tokio::test]
async fn runtime_attach() {
    let app = App::builder().build().unwrap();
    assert_eq!(app.runtime(), Runtime::global());
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
    assert!(app.runtime().is_test());
    assert_ne!(app.runtime(), Runtime::global());
}

#[tokio::test]
async fn redaction() {
    let mut builder = App::builder();
    builder.token(token()).state(1_u8);
    let builder_debug = format!("{builder:?}");
    assert!(builder_debug.contains("Token(<redacted>)"));
    let app = builder.build().unwrap();
    let mut failing = App::builder();
    failing.token(token()).http_timeout(Duration::ZERO);
    let build_err = failing.build().unwrap_err();
    let env_err = AppBuilder::from_lookup(lookup(Some(format!("{SECRET} x").into()))).unwrap_err();
    for text in [
        builder_debug,
        format!("{app:?}"),
        format!("{app:#?}"),
        format!("{build_err:?}"),
        build_err.to_string(),
        format!("{env_err:?}"),
        env_err.to_string(),
    ] {
        assert!(!text.contains(SECRET), "{text}");
    }
}

// ---- plugins (Matrix 25-28) ----

struct Pool;

impl Resource for Pool {
    type Config = u8;

    fn init(_: &u8) -> Self {
        Self
    }
}

struct PoolPlugin(u8);

impl Plugin for PoolPlugin {
    fn install(&self, app: &mut AppBuilder) {
        if let Err(conflict) = app.current_runtime().resource_or_init::<Pool>(self.0) {
            app.report(conflict.into());
        }
    }
}

#[tokio::test]
async fn resource_conflict_via_plugin() {
    let runtime = Runtime::for_test();
    App::builder()
        .runtime(&runtime)
        .plugin(PoolPlugin(1))
        .build()
        .unwrap();
    let err = App::builder()
        .runtime(&runtime)
        .plugin(PoolPlugin(2))
        .build()
        .unwrap_err();
    assert!(matches!(
        err.violations(),
        [BuildViolation::ResourceConflict(c)] if c.frozen == "1" && c.requested == "2"
    ));
    // Separate test Runtimes do not interfere.
    for config in [1, 2] {
        App::builder()
            .runtime(&Runtime::for_test())
            .plugin(PoolPlugin(config))
            .build()
            .unwrap();
    }
}

struct FakeRenderer;

impl Renderer for FakeRenderer {
    fn render(&self, _: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        Box::pin(async { Err(RenderError::failed("fake")) })
    }

    fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

struct RendererPlugin(Arc<AtomicUsize>);

impl Plugin for RendererPlugin {
    fn install(&self, app: &mut AppBuilder) {
        self.0.fetch_add(1, Ordering::SeqCst);
        app.renderer(Arc::new(FakeRenderer));
    }
}

#[tokio::test]
async fn plugin_installs_renderer_once() {
    let installs = Arc::new(AtomicUsize::new(0));
    let app = App::builder()
        .plugin(RendererPlugin(Arc::clone(&installs)))
        .build()
        .unwrap();
    assert!(app.has_renderer());
    assert_eq!(installs.load(Ordering::SeqCst), 1);
}

/// A Template for the register tests.
#[derive(serde::Serialize, serde::Deserialize)]
struct Tpl;

impl noctis_ports::Template for Tpl {
    const ID: &'static str = "tpl";
    const VERSION: u32 = 3;
    const SIZE: noctis_ports::Size = noctis_ports::Size::new(1, 1);
    const FORMAT: noctis_ports::Format = noctis_ports::Format::Png;
    const ANIMATION: Option<noctis_ports::AnimationSpec> = None;

    fn view() -> noctis_ports::ErasedView {
        noctis_ports::ErasedView::new(())
    }
}

/// A Renderer recording what `register` received, or rejecting it.
struct Registering {
    seen: Mutex<Vec<(&'static str, u32)>>,
    reject: bool,
}

impl Renderer for Registering {
    fn render(&self, _: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        Box::pin(async { Err(RenderError::QueueFull) })
    }

    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        self.seen
            .lock()
            .unwrap()
            .extend(templates.iter().map(|t| (t.id, t.version)));
        if self.reject {
            Err(RenderError::failed(std::io::Error::other("no fonts")))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn build_registers_templates_with_the_renderer() {
    let renderer = Arc::new(Registering {
        seen: Mutex::new(Vec::new()),
        reject: false,
    });
    let app = App::builder()
        .renderer(Arc::clone(&renderer) as Arc<dyn Renderer>)
        .module(Module::new("m").template::<Tpl>())
        .build()
        .unwrap();
    assert!(app.has_renderer());
    assert!(app.renderer().is_some());
    assert_eq!(*renderer.seen.lock().unwrap(), [("tpl", 3)]);
}

/// A Renderer recording the ports `bind` received.
#[derive(Default)]
struct Binding {
    bound: Mutex<Vec<Arc<dyn AssetStore>>>,
}

impl Renderer for Binding {
    fn render(&self, _: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        Box::pin(async { Err(RenderError::QueueFull) })
    }

    fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }

    fn bind(&self, ports: RendererPorts) {
        self.bound.lock().unwrap().push(ports.asset_store);
    }
}

#[tokio::test]
async fn img_03_build_binds_the_namespaced_asset_store() {
    use noctis_ports::memory::MemoryAssetStore;

    let renderer = Arc::new(Binding::default());
    let backing = Arc::new(MemoryAssetStore::new());
    App::builder()
        .app_id("a")
        .asset_store(Arc::clone(&backing))
        .renderer(Arc::clone(&renderer) as Arc<dyn Renderer>)
        .build()
        .unwrap();
    let bound = renderer.bound.lock().unwrap().clone();
    assert_eq!(bound.len(), 1, "bind runs once per build");
    bound[0]
        .put("img:v1:k", Bytes::from_static(b"png"), None)
        .await
        .unwrap();
    assert_eq!(
        backing.get("a:asset:img:v1:k").await.unwrap(),
        Some(Bytes::from_static(b"png"))
    );
    assert_eq!(
        bound[0].get("img:v1:k").await.unwrap().as_deref(),
        Some(&b"png"[..])
    );
}

#[tokio::test]
async fn a_rejected_register_is_template_rejected() {
    let renderer = Arc::new(Registering {
        seen: Mutex::new(Vec::new()),
        reject: true,
    });
    let err = App::builder()
        .renderer(renderer)
        .module(Module::new("m").template::<Tpl>())
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::TemplateRejected {
            message: "render failed: no fonts".to_owned()
        }]
    );
    assert!(
        err.to_string()
            .contains("the Renderer cannot render the registered Templates")
    );
}

#[tokio::test]
async fn templates_without_a_renderer_warn_once() {
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let app = App::builder()
        .module(Module::new("m").template::<Tpl>())
        .build()
        .unwrap();
    assert!(!app.has_renderer());
    let lines = capture.lines_with(&["noctis::render", "no Renderer is installed"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(lines[0].contains("templates=1"), "{}", lines[0]);
    // No Templates, no warning.
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    App::builder().build().unwrap();
    assert!(capture.lines_with(&["noctis::render"]).is_empty());
}

type Log = Arc<Mutex<Vec<&'static str>>>;

struct P1(Log);
struct P2(Log);
struct P3(Log);

impl Plugin for P1 {
    fn install(&self, app: &mut AppBuilder) {
        self.0.lock().unwrap().push("P1");
        app.plugin(P3(Arc::clone(&self.0)));
    }
}

impl Plugin for P2 {
    fn install(&self, _: &mut AppBuilder) {
        self.0.lock().unwrap().push("P2");
    }
}

impl Plugin for P3 {
    fn install(&self, _: &mut AppBuilder) {
        self.0.lock().unwrap().push("P3");
    }
}

#[tokio::test]
async fn plugin_order_and_nesting() {
    let log = Log::default();
    App::builder()
        .plugin(P1(Arc::clone(&log)))
        .plugin(P2(Arc::clone(&log)))
        .build()
        .unwrap();
    assert_eq!(*log.lock().unwrap(), ["P1", "P2", "P3"]);
}

/// A 3-level Module tree (Story 13.1 T1).
fn ops_tree() -> Module {
    Module::new("ops").module(Module::new("ops/audit").module(Module::new("ops/audit/log")))
}

/// Story 13.1 T13: one tree on two Apps builds on both (ids are unique per
/// App).
#[tokio::test]
async fn module_tree_on_two_apps() {
    let tree = ops_tree();
    App::builder().module(tree.clone()).build().unwrap();
    App::builder().module(tree).build().unwrap();
}

struct MountsOps;

impl Plugin for MountsOps {
    fn install(&self, app: &mut AppBuilder) {
        app.module(ops_tree());
    }
}

/// Story 13.1 T14: a plugin-mounted root that repeats a mounted id is a
/// `DuplicateModule`.
#[tokio::test]
async fn plugin_mounted_duplicate_module() {
    let err = App::builder()
        .module(Module::new("ops"))
        .plugin(MountsOps)
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicateModule { id: "ops" }]
    );
}

struct Value(u8, Arc<AtomicUsize>);

impl Plugin for Value {
    fn install(&self, _: &mut AppBuilder) {
        self.1.store(usize::from(self.0), Ordering::SeqCst);
    }
}

#[tokio::test]
async fn duplicate_plugin_and_plugin_mut() {
    let seen = Arc::new(AtomicUsize::new(0));
    let mut builder = App::builder();
    builder
        .plugin(Value(1, Arc::clone(&seen)))
        .plugin(Value(2, Arc::clone(&seen)));
    assert!(builder.plugin_mut::<P2>().is_none());
    let value = builder.plugin_mut::<Value>().unwrap();
    assert_eq!(value.0, 1);
    value.0 = 9;
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::DuplicatePlugin {
            type_name: std::any::type_name::<Value>()
        }]
    );
    assert_eq!(seen.load(Ordering::SeqCst), 9);
}

// ---- run (Matrix 29-31) ----

#[test]
fn run_outside_runtime() {
    let tokio_rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let app = {
        let _guard = tokio_rt.enter();
        App::builder().token(token()).build().unwrap()
    };
    let mut run = std::pin::pin!(app.run_with_args(std::iter::empty::<&str>()));
    let poll = run.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(matches!(
        poll,
        Poll::Ready(Err(Error::Start(StartError::NoRuntime)))
    ));
}

#[tokio::test]
async fn run_current_thread() {
    let app = App::builder().token(token()).build().unwrap();
    assert!(matches!(
        app.run_with_args(std::iter::empty::<&str>()).await,
        Err(Error::Start(StartError::NotMultiThread))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_multi_thread() {
    // Past the token check, `run` reads GET /gateway/bot: nothing listens on
    // port 1, so it fails there without touching the network.
    let app = App::builder()
        .token(token())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    assert!(matches!(
        app.run_with_args(std::iter::empty::<&str>()).await,
        Err(Error::Start(StartError::GatewayInfo { status: None }))
    ));
    let app = App::builder().build().unwrap();
    assert!(matches!(
        app.run_with_args(std::iter::empty::<&str>()).await,
        Err(Error::Start(StartError::MissingToken))
    ));
}

// ---- http_proxy (Story 5.2 Matrix 9-11) ----

/// Every client goes through the edge, but only `.http_proxy` drops
/// twilight's local rate limiter.
#[tokio::test]
async fn ratelimiter_kept_without_http_proxy() {
    let app = App::builder().build().unwrap();
    assert!(app.http().ratelimiter().is_some());
    let app = App::builder().http_proxy("p:1").build().unwrap();
    assert!(app.http().ratelimiter().is_none());
}

#[tokio::test]
async fn http_proxy_wiring() {
    let app = App::builder().http_proxy("http://p:1").build().unwrap();
    assert_eq!(app.http_proxy(), Some("p:1"));
    assert!(format!("{app:?}").contains("http_proxy: Some(\"p:1\")"));

    let app = App::builder()
        .http_proxy("a:1")
        .http_proxy("b:2")
        .build()
        .unwrap();
    assert_eq!(app.http_proxy(), Some("b:2"));

    let app = App::builder().build().unwrap();
    assert_eq!(app.http_proxy(), None);

    let err = App::builder()
        .http_proxy("https://x")
        .http_timeout(Duration::ZERO)
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpTimeout {
                actual: Duration::ZERO
            },
            BuildViolation::InvalidHttpProxy {
                reason: crate::HttpProxyError::UnsupportedScheme
            },
        ]
    );
}

#[tokio::test]
async fn http_proxy_redaction() {
    let mut builder = App::builder();
    builder.http_proxy("user:SECRET@h");
    let debug = format!("{builder:?}");
    assert!(debug.contains("http_proxy: true"), "{debug}");
    assert!(!debug.contains("SECRET"), "{debug}");
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidHttpProxy {
            reason: crate::HttpProxyError::NotAnAuthority
        }]
    );
    assert!(err.to_string().contains("invalid HTTP proxy: "));
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains("SECRET"), "{text}");
    }
}

#[tokio::test]
async fn from_env_http_proxy() {
    for raw in [None, Some(""), Some("  \n")] {
        let b = AppBuilder::from_lookup(lookup_var("NOCTIS_HTTP_PROXY", raw.map(OsString::from)))
            .unwrap();
        assert!(b.http_proxy.is_none(), "{raw:?}");
    }
    let app = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_HTTP_PROXY",
        Some(" http://p:1 \n".into()),
    ))
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(app.http_proxy(), Some("p:1"));

    let err = AppBuilder::from_lookup(lookup_var("NOCTIS_HTTP_PROXY", Some("https://x".into())))
        .unwrap()
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidHttpProxy {
            reason: crate::HttpProxyError::UnsupportedScheme
        }]
    );
}

#[cfg(unix)]
#[test]
fn from_env_http_proxy_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_HTTP_PROXY",
        Some(OsString::from_vec(vec![b'a', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_HTTP_PROXY"
        }
    );
}

#[test]
fn from_env_reads_token_first() {
    let seen = Mutex::new(Vec::new());
    let _ = AppBuilder::from_lookup(|var| {
        seen.lock().unwrap().push(var.to_owned());
        None
    })
    .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "NOCTIS_TOKEN",
            "NOCTIS_HTTP_PROXY",
            "NOCTIS_GATEWAY_URL",
            "NOCTIS_SHARD_TOTAL",
            "NOCTIS_SHARD_RANGE",
            "NOCTIS_SHUTDOWN_GRACE",
            "NOCTIS_PUBLISHER",
            "NOCTIS_DEV_GUILD",
            "NOCTIS_FEATURES",
            "NOCTIS_PAYLOAD_KEY",
            "NOCTIS_PAYLOAD_KEY_PREVIOUS",
            "NOCTIS_APP_ID",
        ]
    );
}

// ---- publisher and dev guild (Story 6.9 Matrix 31) ----

#[test]
fn from_env_publisher() {
    let read = |raw: &str| {
        AppBuilder::from_lookup(lookup_var("NOCTIS_PUBLISHER", Some(raw.into())))
            .map(|b| b.publisher)
    };
    for (raw, expected) in [
        ("true", true),
        ("TRUE", true),
        ("1", true),
        ("false", false),
        ("0", false),
        (" true ", true),
        ("", false),
    ] {
        assert_eq!(read(raw), Ok(expected), "{raw:?}");
    }
    for raw in ["yes", "2", "nope", "11"] {
        let err = read(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidPublisher {
                var: "NOCTIS_PUBLISHER"
            },
            "{raw:?}"
        );
        assert!(!err.to_string().contains(raw), "{err}");
    }
}

#[test]
fn from_env_dev_guild() {
    let read = |raw: &str| {
        AppBuilder::from_lookup(lookup_var("NOCTIS_DEV_GUILD", Some(raw.into())))
            .map(|b| b.dev_guild)
    };
    assert_eq!(read("42"), Ok(Some(Id::new(42))));
    assert_eq!(read(" 007 "), Ok(Some(Id::new(7))));
    assert_eq!(read(""), Ok(None));
    for raw in ["0", "-1", "4x", "123456789012345678901"] {
        let err = read(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidDevGuild {
                var: "NOCTIS_DEV_GUILD"
            },
            "{raw:?}"
        );
        assert!(!err.to_string().contains(raw), "{err}");
    }
}

#[tokio::test]
async fn publisher_and_dev_guild_setters() {
    let mut builder = App::builder();
    builder
        .publisher(true)
        .dev_guild(Id::new(1))
        .dev_guild(Id::new(9));
    assert!(format!("{builder:?}").contains("publisher: true"));
    let app = builder.build().unwrap();
    assert!(app.is_publisher());
    assert_eq!(app.dev_guild(), Some(Id::new(9)));
    let app = App::builder().build().unwrap();
    assert!(!app.is_publisher());
    assert_eq!(app.dev_guild(), None);
}

/// Story 12.3 S7: default `true`, the setter, the last call wins.
#[tokio::test]
async fn resume_sessions_setter() {
    assert!(App::builder().build().unwrap().resume_sessions());
    let mut builder = App::builder();
    builder.resume_sessions(false);
    assert!(format!("{builder:?}").contains("resume_sessions: false"));
    let app = builder.build().unwrap();
    assert!(!app.resume_sessions());
    assert!(format!("{app:?}").contains("resume_sessions: false"));
    let app = App::builder()
        .resume_sessions(false)
        .resume_sessions(true)
        .build()
        .unwrap();
    assert!(app.resume_sessions());
}

// ---- gateway_url (Story 5.3 Matrix 10-12) ----

#[tokio::test]
async fn gateway_url_wiring() {
    let app = App::builder().gateway_url("WS://p:1/").build().unwrap();
    assert_eq!(app.gateway_url(), Some("ws://p:1"));
    assert!(format!("{app:?}").contains("gateway_url: Some(\"ws://p:1\")"));

    let app = App::builder()
        .gateway_url("ws://a:1")
        .gateway_url("ws://b:2")
        .build()
        .unwrap();
    assert_eq!(app.gateway_url(), Some("ws://b:2"));

    let app = App::builder().build().unwrap();
    assert_eq!(app.gateway_url(), None);

    let err = App::builder()
        .gateway_url("http://x")
        .http_proxy("https://x")
        .http_timeout(Duration::ZERO)
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpTimeout {
                actual: Duration::ZERO
            },
            BuildViolation::InvalidHttpProxy {
                reason: crate::HttpProxyError::UnsupportedScheme
            },
            BuildViolation::InvalidGatewayUrl {
                reason: crate::GatewayUrlError::UnsupportedScheme
            },
        ]
    );
}

#[tokio::test]
async fn gateway_url_redaction() {
    let mut builder = App::builder();
    builder.gateway_url("wss://u:SECRET@h");
    let debug = format!("{builder:?}");
    assert!(debug.contains("gateway_url: true"), "{debug}");
    assert!(!debug.contains("SECRET"), "{debug}");
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidGatewayUrl {
            reason: crate::GatewayUrlError::NotAnAuthority
        }]
    );
    assert!(err.to_string().contains("invalid gateway URL: "));
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains("SECRET"), "{text}");
    }
}

#[tokio::test]
async fn from_env_gateway_url() {
    for raw in [None, Some(""), Some(" \t")] {
        let b = AppBuilder::from_lookup(lookup_var("NOCTIS_GATEWAY_URL", raw.map(OsString::from)))
            .unwrap();
        assert!(b.gateway_url.is_none(), "{raw:?}");
    }
    let app = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_GATEWAY_URL",
        Some(" ws://p:1 \n".into()),
    ))
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(app.gateway_url(), Some("ws://p:1"));

    let err = AppBuilder::from_lookup(lookup_var("NOCTIS_GATEWAY_URL", Some("http://x".into())))
        .unwrap()
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidGatewayUrl {
            reason: crate::GatewayUrlError::UnsupportedScheme
        }]
    );
}

#[cfg(unix)]
#[test]
fn from_env_gateway_url_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_GATEWAY_URL",
        Some(OsString::from_vec(vec![b'a', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_GATEWAY_URL"
        }
    );
}

// ---- shard_total (Story 5.4 Matrix 8, 9, 18) ----

#[tokio::test]
async fn shard_total_builder() {
    assert_eq!(
        App::builder().shard_total(5).build().unwrap().shard_total(),
        Some(5)
    );
    assert_eq!(App::builder().build().unwrap().shard_total(), None);
    let err = App::builder().shard_total(0).build().unwrap_err();
    assert_eq!(err.violations(), [BuildViolation::InvalidShardTotal]);
    assert_eq!(
        err.violations()[0].to_string(),
        "shard total must be at least 1: omit shard_total to use Discord's recommendation"
    );
    assert_eq!(
        App::builder()
            .shard_total(0)
            .shard_total(5)
            .build()
            .unwrap()
            .shard_total(),
        Some(5)
    );
    assert_eq!(
        App::builder()
            .shard_total(u32::MAX)
            .build()
            .unwrap()
            .shard_total(),
        Some(u32::MAX)
    );
    let err = App::builder()
        .shard_total(0)
        .http_timeout(Duration::ZERO)
        .gateway_url("http://x")
        .build()
        .unwrap_err();
    assert!(matches!(
        err.violations(),
        [
            BuildViolation::InvalidHttpTimeout { .. },
            BuildViolation::InvalidGatewayUrl { .. },
            BuildViolation::InvalidShardTotal,
        ]
    ));
}

#[tokio::test]
async fn shard_total_debug() {
    let mut builder = App::builder();
    builder.shard_total(7);
    assert!(format!("{builder:?}").contains("shard_total: Some(7)"));
    let app = builder.build().unwrap();
    assert!(format!("{app:?}").contains("shard_total: Some(7)"));
}

#[test]
fn from_env_shard_total() {
    let read = |raw: &str| {
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_TOTAL", Some(raw.into())))
            .map(|b| b.shard_total)
    };
    assert_eq!(
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_TOTAL", None))
            .unwrap()
            .shard_total,
        None
    );
    for (raw, expected) in [
        ("", None),
        (" \t", None),
        (" 12 \n", Some(12)),
        ("007", Some(7)),
        ("4294967295", Some(u32::MAX)),
        ("0", Some(0)),
    ] {
        assert_eq!(read(raw), Ok(expected), "{raw:?}");
    }
    for raw in [
        "4294967296",
        "-1",
        "+5",
        "1_000",
        "1e3",
        "abc",
        "\u{ff11}\u{ff12}",
        "12345678901",
    ] {
        let err = read(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidShardTotal {
                var: "NOCTIS_SHARD_TOTAL"
            },
            "{raw:?}"
        );
        assert_eq!(
            err.to_string(),
            "environment variable `NOCTIS_SHARD_TOTAL` is not a shard total: set a whole number \
             of shards such as 16"
        );
    }
}

#[tokio::test]
async fn from_env_shard_total_zero_fails_at_build() {
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_TOTAL", Some("0".into()))).unwrap();
    let err = builder.build().unwrap_err();
    assert_eq!(err.violations(), [BuildViolation::InvalidShardTotal]);
}

#[cfg(unix)]
#[test]
fn from_env_shard_total_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_SHARD_TOTAL",
        Some(OsString::from_vec(vec![b'1', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_SHARD_TOTAL"
        }
    );
}

#[test]
fn startup_close_codes_map_to_start_errors() {
    assert_eq!(start_error(3, Some(4004)), StartError::InvalidToken);
    assert_eq!(
        start_error(3, Some(4013)),
        StartError::InvalidIntents { shard: 3 }
    );
    assert_eq!(
        start_error(3, Some(4014)),
        StartError::DisallowedIntents { shard: 3 }
    );
    for code in [4010, 4011, 4012] {
        assert_eq!(
            start_error(3, Some(code)),
            StartError::FatalClose { shard: 3, code }
        );
    }
    assert_eq!(start_error(3, None), StartError::ShardEnded { shard: 3 });
}

/// Story 12.2: a shard that resumed a stored session counts as ready, so
/// startup completes on RESUMED without a READY.
#[tokio::test]
async fn startup_completes_on_resumed() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    for event in [
        ShardEvent::Resumed { shard: 1 },
        ShardEvent::Resumed { shard: 1 },
        ShardEvent::Ready { shard: 0 },
    ] {
        tx.send(event).unwrap();
    }
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
        Ok(())
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Resumed { shard: 0 }).unwrap();
    assert_eq!(
        await_startup(&mut rx, &status(1), &Lifecycle::new()).await,
        Ok(())
    );
}

#[tokio::test]
async fn startup_waits_for_every_ready_and_maps_closed_channel() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    for event in [
        ShardEvent::Ready { shard: 1 },
        ShardEvent::Ready { shard: 1 },
        ShardEvent::Disconnected {
            shard: 0,
            close_code: Some(4000),
        },
        ShardEvent::Resumed { shard: 9 },
        ShardEvent::Ready { shard: 9 },
        ShardEvent::Ready { shard: 0 },
    ] {
        tx.send(event).unwrap();
    }
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
        Ok(())
    );

    tx.send(ShardEvent::Ready { shard: 0 }).unwrap();
    drop(tx);
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
        Err(StartError::GatewayStopped)
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Ended {
        shard: 1,
        close_code: Some(4014),
    })
    .unwrap();
    assert_eq!(
        await_startup(&mut rx, &status(2), &Lifecycle::new()).await,
        Err(StartError::DisallowedIntents { shard: 1 })
    );
}

#[test]
fn start_error_messages_name_cause_and_fix() {
    let errors = [
        StartError::InvalidToken,
        StartError::GatewayInfo { status: Some(500) },
        StartError::GatewayInfo { status: None },
        StartError::InvalidGatewayInfo,
        StartError::ShardTotalTooSmall {
            total: 4,
            minimum: 8,
        },
        StartError::SessionStartLimit {
            required: 20,
            remaining: 19,
            reset_after: Duration::from_secs(5),
        },
        StartError::InvalidIntents { shard: 0 },
        StartError::DisallowedIntents { shard: 0 },
        StartError::FatalClose {
            shard: 0,
            code: 4011,
        },
        StartError::ShardEnded { shard: 0 },
        StartError::GatewayStopped,
    ];
    for error in errors {
        let text = error.to_string();
        assert!(text.contains(':'), "{text}");
    }
    assert!(
        StartError::InvalidToken
            .to_string()
            .starts_with("Discord rejected the bot token (HTTP 401 or close 4004)")
    );
    assert!(
        StartError::GatewayInfo { status: Some(500) }
            .to_string()
            .contains("HTTP 500")
    );
    assert!(
        StartError::DisallowedIntents { shard: 0 }
            .to_string()
            .contains(
                "enable the privileged intents in the Developer Portal or stop requesting them"
            )
    );
}

/// A `ShardStatus` of App `"default"` over `total` shards.
fn status(total: u32) -> ShardStatus {
    ranged_status(0..total, total)
}

/// A `ShardStatus` of App `"default"` over the shards `shards` of `total`.
fn ranged_status(shards: Range<u32>, total: u32) -> ShardStatus {
    ShardStatus {
        app: Arc::from("default"),
        shards,
        total,
    }
}

// ---- shutdown_grace (Story 5.8 Matrix 1-8) ----

#[tokio::test]
async fn shutdown_grace_builder() {
    let grace = |value: Duration| App::builder().shutdown_grace(value).build();
    assert_eq!(
        App::builder().build().unwrap().shutdown_grace(),
        Duration::from_secs(10),
        "Matrix 1"
    );
    for ok in [
        Duration::ZERO,
        Duration::from_mins(15),
        Duration::from_nanos(1),
    ] {
        assert_eq!(grace(ok).unwrap().shutdown_grace(), ok, "Matrix 2: {ok:?}");
    }
    for bad in [
        Duration::from_mins(15) + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        let err = grace(bad).unwrap_err();
        assert_eq!(
            err.violations(),
            [BuildViolation::InvalidShutdownGrace { actual: bad }],
            "Matrix 3"
        );
    }
    assert_eq!(
        BuildViolation::InvalidShutdownGrace {
            actual: Duration::from_secs(901)
        }
        .to_string(),
        "shutdown grace 901s is out of range: use a value in 0 s..=900 s"
    );
    let err = App::builder()
        .http_timeout(Duration::ZERO)
        .handler_concurrency(0)
        .shutdown_grace(Duration::MAX)
        .build()
        .unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [
                BuildViolation::InvalidHttpTimeout { .. },
                BuildViolation::InvalidHandlerConcurrency { .. },
                BuildViolation::InvalidShutdownGrace { .. },
            ]
        ),
        "Matrix 3 order: {err:?}"
    );

    let mut builder = App::builder();
    builder
        .shutdown_grace(Duration::from_secs(1))
        .shutdown_grace(Duration::from_secs(2));
    assert!(format!("{builder:?}").contains("shutdown_grace: 2s"));
    let app = builder.build().unwrap();
    assert_eq!(app.shutdown_grace(), Duration::from_secs(2), "Matrix 4");
    assert!(format!("{app:?}").contains("shutdown_grace: 2s"));
    assert_eq!(
        builder.build().unwrap().shutdown_grace(),
        Duration::from_secs(10),
        "Matrix 4 reset"
    );
}

#[tokio::test]
async fn from_env_shutdown_grace() {
    let read = |raw: &str| {
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", Some(raw.into())))
            .map(|b| b.shutdown_grace)
    };
    for (raw, secs) in [("30", 30), (" 0 ", 0), ("0900", 900), ("9999", 9999)] {
        assert_eq!(
            read(raw),
            Ok(Duration::from_secs(secs)),
            "Matrix 5: {raw:?}"
        );
    }
    let err = AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", Some("9999".into())))
        .unwrap()
        .build()
        .unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidShutdownGrace {
            actual: Duration::from_secs(9999)
        }]
    );
    assert_eq!(
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", None))
            .unwrap()
            .shutdown_grace,
        Duration::from_secs(10),
        "Matrix 6: missing"
    );
    for raw in ["", "   "] {
        assert_eq!(read(raw), Ok(Duration::from_secs(10)), "Matrix 6: {raw:?}");
    }
    for raw in [
        "10000",
        "1.5",
        "-1",
        "+1",
        "10s",
        "1e3",
        "1_0",
        "\u{ff13}\u{ff10}",
        "\u{663}",
    ] {
        let err = read(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidShutdownGrace {
                var: "NOCTIS_SHUTDOWN_GRACE"
            },
            "Matrix 7: {raw:?}"
        );
        let text = err.to_string();
        assert!(text.contains("NOCTIS_SHUTDOWN_GRACE"), "{text}");
        assert!(!text.contains(raw), "{text}");
    }
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHUTDOWN_GRACE", Some("30".into()))).unwrap();
    builder.shutdown_grace(Duration::from_secs(5));
    assert_eq!(
        builder.build().unwrap().shutdown_grace(),
        Duration::from_secs(5),
        "Matrix 8 override"
    );
}

#[cfg(unix)]
#[test]
fn from_env_shutdown_grace_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_SHUTDOWN_GRACE",
        Some(OsString::from_vec(vec![b'1', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_SHUTDOWN_GRACE"
        },
        "Matrix 8"
    );
}

// ---- run, signals and errors (Story 5.8 Matrix 40) ----

#[test]
fn run_future_is_send() {
    fn assert_send<T: Send>(_: &T) {}
    let tokio_rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let _guard = tokio_rt.enter();
    let app = App::builder().build().unwrap();
    let run = app.run_with_args(std::iter::empty::<&str>());
    assert_send(&run);
    drop(run);
    let app = App::builder().build().unwrap();
    let run = app.run_until(std::future::pending());
    assert_send(&run);
}

/// Matrix 40: a runtime without an IO driver cannot install the signal
/// handlers; `run` returns a typed error instead of panicking.
#[test]
fn run_without_io_driver() {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let outcome = tokio_rt.block_on(async {
        let app = App::builder().token(token()).build().unwrap();
        app.run_with_args(std::iter::empty::<&str>()).await
    });
    assert!(
        matches!(outcome, Err(Error::Start(StartError::SignalHandler))),
        "{outcome:?}"
    );
    assert_eq!(
        StartError::SignalHandler.to_string(),
        "the Ctrl-C/SIGTERM handler could not be installed: build the tokio runtime with \
         enable_all() (as #[tokio::main] does)"
    );
}

#[test]
fn run_error_message() {
    let error = Error::from(RunError::AllShardsEnded);
    assert!(matches!(error, Error::Run(RunError::AllShardsEnded)));
    assert_eq!(
        error.to_string(),
        "every shard ended without a shutdown request (see the \"shard closed fatally\" logs): \
         fix the cause, then restart"
    );
}

/// An already-resolved shutdown ends `run_until` with `Ok(())` before any
/// request is sent, logging `shutdown started` and `shutdown complete`.
#[tokio::test(flavor = "multi_thread")]
async fn run_until_already_requested() {
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let app = App::builder()
        .token(token())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(15), app.run_until(async {})).await;
    assert!(matches!(outcome, Ok(Ok(()))), "{outcome:?}");
    let text = capture.text();
    let started = text
        .find("shutdown started")
        .unwrap_or_else(|| panic!("shutdown started missing: {text}"));
    let complete = text
        .find("shutdown complete")
        .unwrap_or_else(|| panic!("shutdown complete missing: {text}"));
    assert!(started < complete, "{text}");
    assert!(
        text.contains("reason=\"requested\"") || text.contains("reason=requested"),
        "{text}"
    );
}

// ---- supervise and lifecycle (Story 5.8 Matrix 28-34) ----

/// Drives `supervise` over `events` (sender dropped by the caller or not)
/// with a shutdown that never resolves, recording metrics and logs.
struct Supervised {
    capture: crate::test_support::Capture,
    snapshot: Vec<(
        metrics_util::CompositeKey,
        Option<metrics::Unit>,
        Option<metrics::SharedString>,
        metrics_util::debugging::DebugValue,
    )>,
    lifecycle: Vec<LifecycleEvent>,
    stop: Option<Stop>,
}

/// Feeds `events` to `supervise` (total 2) and stops after they are handled.
async fn supervised(events: &[ShardEvent], close: bool) -> Supervised {
    use metrics_util::debugging::DebuggingRecorder;

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let lifecycle = Lifecycle::new();
    let mut rx_lifecycle = lifecycle.subscribe();
    let status = status(2);
    status.init();
    let (tx, mut rx) = mpsc::unbounded_channel();
    for event in events {
        tx.send(*event).unwrap();
    }
    if close {
        drop(tx);
    } else {
        std::mem::forget(tx);
    }
    // `supervise` handles every queued event before a yield lets the
    // shutdown future (a 50 ms sleep) win.
    let stop = tokio::time::timeout(
        Duration::from_secs(15),
        supervise(
            &mut rx,
            &status,
            &lifecycle,
            None,
            tokio::time::sleep(Duration::from_millis(50)),
        ),
    )
    .await
    .ok();
    let snapshot = snapshotter.snapshot().into_vec();
    std::mem::forget(status);
    let mut seen = Vec::new();
    while let Ok(event) = rx_lifecycle.try_recv() {
        seen.push(event);
    }
    Supervised {
        capture,
        snapshot,
        lifecycle: seen,
        stop,
    }
}

impl Supervised {
    fn gauge(&self, shard: &str) -> Option<f64> {
        self.snapshot.iter().find_map(|(key, .., value)| {
            let matches = key.key().name() == "noctis_shard_status"
                && key
                    .key()
                    .labels()
                    .any(|l| l.key() == "shard" && l.value() == shard);
            match value {
                metrics_util::debugging::DebugValue::Gauge(v) if matches => Some(v.0),
                _ => None,
            }
        })
    }
}

/// Matrix 28: a fatal close after startup is loud and the App keeps
/// running.
#[tokio::test]
async fn fatal_close_after_startup() {
    let run = supervised(
        &[
            ShardEvent::Ready { shard: 0 },
            ShardEvent::Ready { shard: 1 },
            ShardEvent::Ended {
                shard: 1,
                close_code: Some(4014),
            },
        ],
        false,
    )
    .await;
    assert_eq!(run.stop, Some(Stop::Requested), "supervise keeps running");
    let lines = run
        .capture
        .lines_with(&["shard closed fatally", "shard=1", "close_code=4014"]);
    assert_eq!(lines.len(), 1, "{}", run.capture.text());
    assert!(lines[0].contains("ERROR"), "{}", lines[0]);
    assert!(
        lines[0].contains("4014 (disallowed intents)"),
        "{}",
        lines[0]
    );
    assert!(!run.capture.text().contains("shard ended\n"));
    assert_eq!(run.gauge("1"), Some(0.0));
    assert_eq!(run.gauge("0"), Some(1.0));
    assert_eq!(
        run.lifecycle
            .iter()
            .filter(|e| matches!(e, LifecycleEvent::ShardFatal { .. }))
            .collect::<Vec<_>>(),
        [&LifecycleEvent::ShardFatal {
            shard: 1,
            close_code: Some(4014)
        }]
    );
}

/// Matrix 29: each fatal cause is logged with its typed error text.
#[tokio::test]
async fn fatal_close_causes() {
    for code in [None, Some(4004), Some(4010), Some(4012)] {
        let run = supervised(
            &[ShardEvent::Ended {
                shard: 0,
                close_code: code,
            }],
            false,
        )
        .await;
        let expected = start_error(0, code).to_string();
        let lines = run.capture.lines_with(&["shard closed fatally", &expected]);
        assert_eq!(lines.len(), 1, "{code:?}: {}", run.capture.text());
        assert_eq!(
            run.lifecycle,
            [LifecycleEvent::ShardFatal {
                shard: 0,
                close_code: code
            }]
        );
    }
    assert_eq!(start_error(0, None), StartError::ShardEnded { shard: 0 });
    assert_eq!(start_error(0, Some(4004)), StartError::InvalidToken);
}

/// Matrix 30: every shard ended (or the channel closed) stops supervise.
#[tokio::test]
async fn all_shards_ended() {
    let ended = |shard| ShardEvent::Ended {
        shard,
        close_code: Some(4004),
    };
    let run = supervised(&[ended(0), ended(1)], false).await;
    assert_eq!(run.stop, Some(Stop::AllEnded));
    let run = supervised(&[ended(0), ended(0)], false).await;
    assert_eq!(
        run.stop,
        Some(Stop::Requested),
        "one shard twice is not all"
    );
    let run = supervised(&[ended(0)], true).await;
    assert_eq!(run.stop, Some(Stop::AllEnded), "closed channel");
}

/// Matrix 31: an out-of-range shard is logged and broadcast; no gauge.
#[tokio::test]
async fn out_of_range_fatal() {
    let run = supervised(
        &[ShardEvent::Ended {
            shard: 9,
            close_code: Some(4014),
        }],
        false,
    )
    .await;
    assert_eq!(
        run.capture
            .lines_with(&["shard closed fatally", "shard=9"])
            .len(),
        1
    );
    assert_eq!(
        run.lifecycle,
        [LifecycleEvent::ShardFatal {
            shard: 9,
            close_code: Some(4014)
        }]
    );
    assert_eq!(run.gauge("9"), None);
    assert_eq!(run.stop, Some(Stop::Requested));
}

/// Matrix 32: the lifecycle mapping; `Closed` broadcasts nothing.
#[tokio::test]
async fn lifecycle_mapping() {
    let run = supervised(
        &[
            ShardEvent::Ready { shard: 0 },
            ShardEvent::Resumed { shard: 0 },
            ShardEvent::Disconnected {
                shard: 0,
                close_code: Some(4000),
            },
            ShardEvent::Ready { shard: 1 },
            ShardEvent::Closed { shard: 1 },
        ],
        false,
    )
    .await;
    assert_eq!(
        run.lifecycle,
        [
            LifecycleEvent::ShardReady { shard: 0 },
            LifecycleEvent::ShardResumed { shard: 0 },
            LifecycleEvent::ShardDisconnected {
                shard: 0,
                close_code: Some(4000)
            },
            LifecycleEvent::ShardReady { shard: 1 },
        ]
    );
    assert_eq!(run.gauge("1"), Some(0.0));
    assert!(!run.capture.text().contains("fatally"));
    assert_eq!(
        run.capture
            .lines_with(&["DEBUG", "shard closed", "shard=1"])
            .len(),
        1
    );
}

/// Matrix 33: no receivers or a lagging one never block or fail.
#[tokio::test]
async fn lifecycle_without_receivers_and_lag() {
    let lifecycle = Lifecycle::new();
    lifecycle.send(LifecycleEvent::ShutdownStarted);
    let mut rx = lifecycle.subscribe();
    for shard in 0..300 {
        lifecycle.shard(&ShardEvent::Ready { shard });
    }
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(44))
    ));
    assert_eq!(rx.try_recv(), Ok(LifecycleEvent::ShardReady { shard: 44 }));
}

/// `await_startup` broadcasts shard events, and a fatal close during
/// startup is a `ShardFatal` too.
#[tokio::test]
async fn startup_broadcasts_lifecycle() {
    let lifecycle = Lifecycle::new();
    let mut seen = lifecycle.subscribe();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Ready { shard: 0 }).unwrap();
    tx.send(ShardEvent::Ended {
        shard: 1,
        close_code: Some(4014),
    })
    .unwrap();
    assert_eq!(
        await_startup(&mut rx, &status(2), &lifecycle).await,
        Err(StartError::DisallowedIntents { shard: 1 })
    );
    assert_eq!(seen.try_recv(), Ok(LifecycleEvent::ShardReady { shard: 0 }));
    assert_eq!(
        seen.try_recv(),
        Ok(LifecycleEvent::ShardFatal {
            shard: 1,
            close_code: Some(4014)
        })
    );
}

/// Matrix 34: the App's lifecycle receiver sees exactly one
/// `ShutdownStarted` per shutdown (the sequence itself is covered end to end
/// in `noctis-testkit/tests/shutdown.rs`).
#[tokio::test]
async fn app_lifecycle_channel() {
    let app = App::builder().build().unwrap();
    let mut rx = app.subscribe_lifecycle();
    app.lifecycle.send(LifecycleEvent::ShutdownStarted);
    assert_eq!(rx.try_recv(), Ok(LifecycleEvent::ShutdownStarted));
    assert!(rx.try_recv().is_err());
}

/// Matrix 34: one `ShutdownStarted`, broadcast before any shard reports
/// `Closed`; the sequence logs in order.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_sequence_order() {
    use std::num::NonZeroU16;

    use noctis_gateway::EventTypeFlags;

    use crate::worker::Shutdown;

    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .shutdown_grace(Duration::from_millis(100))
        .build()
        .unwrap();
    let mut lifecycle = app.subscribe_lifecycle();
    let controls = Shutdown::default();
    let (sender, receiver) = noctis_gateway::lanes(app.lane_config());
    let mut drain = AbortOnDrop(tokio::spawn(drain(receiver, app.workers(controls.clone()))));
    // Nothing listens on port 1: the shards never identify, so they stop
    // at once without a close frame.
    let options = GatewayOptions {
        app: Arc::from("default"),
        token: SECRET.to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some("ws://127.0.0.1:1".to_owned()),
        event_types: EventTypeFlags::INTERACTION_CREATE,
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::MIN)),
        session_store: Arc::new(noctis_ports::memory::MemorySessionStore::new()),
        resume: true,
        presence: None,
        large_threshold: LargeThreshold::default(),
        shards: 0..2,
        total: NonZeroU32::new(2).unwrap(),
        max_concurrency: NonZeroU16::MIN,
        close_timeout: SHARD_CLOSE_TIMEOUT,
    };
    let (gateway, mut events) = Gateway::start(options, Arc::new(sender)).unwrap();
    let status = status(2);
    let watch = Watch {
        events: &mut events,
        status: &status,
        lifecycle: &app.lifecycle,
    };
    tokio::time::timeout(
        Duration::from_secs(15),
        app.shut_down(
            Stop::Requested,
            "requested",
            gateway,
            watch,
            &controls,
            &mut drain,
        ),
    )
    .await
    .unwrap();
    let mut seen = Vec::new();
    while let Ok(event) = lifecycle.try_recv() {
        seen.push(event);
    }
    let started: Vec<_> = seen
        .iter()
        .filter(|e| **e == LifecycleEvent::ShutdownStarted)
        .collect();
    assert_eq!(started.len(), 1, "{seen:?}");
    let text = capture.text();
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing: {text}"))
    };
    assert!(
        text.contains("reason=\"requested\"") || text.contains("reason=requested"),
        "{text}"
    );
    assert!(at("shutdown started") < at("shard closed"), "{text}");
    assert!(at("shard closed") < at("shutdown complete"), "{text}");
    assert_eq!(text.matches("shard closed").count(), 2, "{text}");
    assert!(!text.contains("handlers still running"), "{text}");
    assert!(drain.0.is_finished());
    assert!(controls.tracker.is_closed() && controls.tracker.is_empty());
}

// ---- HttpConfig and the edge (Story 5.9 Matrix 1-5, 7, 24) ----

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Matrix 1: the default config.
#[tokio::test]
async fn http_config_default() {
    let app = App::builder().build().unwrap();
    let config = app.http_config();
    assert_eq!(config, HttpConfig::default());
    assert_eq!(config.ping_interval, Duration::from_secs(20));
    assert_eq!(config.ping_timeout, Duration::from_secs(5));
    assert_eq!(config.hedge_after, ms(1000));
    assert_eq!(config.connect_timeout, ms(1500));
    assert_eq!(config.dns_ttl_floor, Duration::from_secs(30));
    assert_eq!(config.dns_ttl_cap, Duration::from_mins(5));
    assert!(!config.upstream_h2c);
    assert!(format!("{app:?}").contains("http_config"));
    assert!(format!("{:?}", App::builder()).contains("http_config"));
}

/// Matrix 2: both ends of every range build.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_bounds_ok() {
    let low = {
        let mut c = HttpConfig::default();
        c.ping_interval = ms(50);
        c.ping_timeout = ms(50);
        c.hedge_after = ms(300);
        c.connect_timeout = ms(100);
        c.dns_ttl_floor = Duration::from_secs(1);
        c.dns_ttl_cap = Duration::from_secs(1);
        c
    };
    let high = {
        let mut c = HttpConfig::default();
        c.ping_interval = Duration::from_mins(5);
        c.ping_timeout = Duration::from_mins(1);
        c.hedge_after = ms(2800);
        c.connect_timeout = Duration::from_secs(30);
        c.dns_ttl_floor = Duration::from_hours(24);
        c.dns_ttl_cap = Duration::from_hours(24);
        c
    };
    for config in [low, high] {
        let app = App::builder().http(config).build().unwrap();
        assert_eq!(app.http_config(), config);
    }
}

/// Matrix 3: one violation per bad field, in field order, among the
/// others, with exact messages.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_bounds_bad() {
    let ns = Duration::from_nanos(1);
    for bad in [
        ms(49),
        Duration::ZERO,
        Duration::from_mins(5) + ns,
        Duration::MAX,
    ] {
        let mut config = HttpConfig::default();
        config.ping_interval = bad;
        let err = App::builder().http(config).build().unwrap_err();
        assert_eq!(
            err.violations(),
            [BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::PingInterval(bad)
            }]
        );
    }
    let mut config = HttpConfig::default();
    config.ping_interval = ms(49);
    config.ping_timeout = Duration::from_mins(1) + ns;
    config.hedge_after = ms(299);
    config.connect_timeout = ms(99);
    config.dns_ttl_floor = ms(999);
    config.dns_ttl_cap = Duration::from_hours(24) + ns;
    let err = App::builder()
        .http_timeout(Duration::ZERO)
        .http(config)
        .shutdown_grace(Duration::MAX)
        .build()
        .unwrap_err();
    let messages: Vec<String> = err.violations().iter().map(ToString::to_string).collect();
    assert_eq!(
        messages,
        [
            "HTTP timeout 0ns is out of range: use a value in 1 ms..=300 s".to_owned(),
            format!(
                "shutdown grace {:?} is out of range: use a value in 0 s..=900 s",
                Duration::MAX
            ),
            "invalid HTTP config: HttpConfig.ping_interval 49ms is out of range: use 50 ms..=300 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.ping_timeout 60.000000001s is out of range: use 50 ms..=60 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.hedge_after 299ms is out of range: use 300 ms..=2800 ms"
                .to_owned(),
            "invalid HTTP config: HttpConfig.connect_timeout 99ms is out of range: use 100 ms..=30 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.dns_ttl_floor 999ms is out of range: use 1 s..=86400 s"
                .to_owned(),
            "invalid HTTP config: HttpConfig.dns_ttl_cap 86400.000000001s is out of range: use 1 s..=86400 s"
                .to_owned(),
        ]
    );
    let mut config = HttpConfig::default();
    config.hedge_after = ms(2800) + ns;
    config.connect_timeout = Duration::from_secs(30) + ns;
    config.ping_timeout = ms(49);
    let err = App::builder().http(config).build().unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::PingTimeout(ms(49))
            },
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::HedgeAfter(ms(2800) + ns)
            },
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::ConnectTimeout(Duration::from_secs(30) + ns)
            },
        ]
    );
}

/// Matrix 4: the TTL order rule.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_ttl_order() {
    let mut config = HttpConfig::default();
    config.dns_ttl_floor = Duration::from_mins(1);
    config.dns_ttl_cap = Duration::from_secs(30);
    let err = App::builder().http(config).build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidHttpConfig {
            reason: HttpConfigError::DnsTtlOrder {
                floor: Duration::from_mins(1),
                cap: Duration::from_secs(30)
            }
        }]
    );
    assert_eq!(
        err.violations()[0].to_string(),
        "invalid HTTP config: HttpConfig.dns_ttl_floor 60s is above dns_ttl_cap 30s"
    );
    config.dns_ttl_floor = Duration::ZERO;
    config.dns_ttl_cap = Duration::ZERO;
    let err = App::builder().http(config).build().unwrap_err();
    assert_eq!(
        err.violations(),
        [
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::DnsTtlFloor(Duration::ZERO)
            },
            BuildViolation::InvalidHttpConfig {
                reason: HttpConfigError::DnsTtlCap(Duration::ZERO)
            },
        ]
    );
}

/// Matrix 5: the last call wins and `build()` resets it.
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
#[tokio::test]
async fn http_config_last_wins_and_reset() {
    let mut a = HttpConfig::default();
    a.hedge_after = ms(500);
    let mut b = HttpConfig::default();
    b.hedge_after = ms(700);
    let mut builder = App::builder();
    let app = builder.http(a).http(b).build().unwrap();
    assert_eq!(app.http_config(), b);
    let again = builder.build().unwrap();
    assert_eq!(again.http_config(), HttpConfig::default());
}

/// The edge-bind violation's message.
#[test]
fn http_edge_violation_message() {
    let v = BuildViolation::HttpEdge {
        kind: std::io::ErrorKind::AddrInUse,
    };
    assert_eq!(
        v.to_string(),
        "the Discord HTTP edge could not bind 127.0.0.1:0 (AddrInUse)"
    );
}

/// Matrix 7: a runtime without an IO driver still builds; the edge logs
/// that it could not start and nothing panics out.
#[test]
fn build_without_io_driver() {
    let capture = crate::test_support::Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let tokio_rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tokio_rt.block_on(async {
        let app = App::builder().token(token()).build().unwrap();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        drop(app);
    });
    assert!(
        capture.text().contains(
            "the Discord HTTP edge could not start: build the tokio runtime with enable_all()"
        ),
        "{}",
        capture.text()
    );
}

/// Matrix 24: only a disconnect without a close frame asks the keeper for a
/// redial.
#[tokio::test]
async fn shard_network_loss_requests_redial() {
    let app = App::builder().build().unwrap();
    let keeper = app.edge().keeper();
    let lifecycle = Lifecycle::new();
    let status = status(2);
    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(ShardEvent::Disconnected {
        shard: 0,
        close_code: None,
    })
    .unwrap();
    tx.send(ShardEvent::Disconnected {
        shard: 0,
        close_code: Some(4000),
    })
    .unwrap();
    let stop = supervise(
        &mut rx,
        &status,
        &lifecycle,
        Some(&keeper),
        tokio::time::sleep(Duration::from_millis(50)),
    )
    .await;
    assert_eq!(stop, Stop::Requested);
    assert_eq!(keeper.redial_requests(), 1);
}

/// The 2.9 s deadline error's message.
#[test]
fn ack_deadline_missed_message() {
    let error = crate::AckDeadlineMissed {
        elapsed: Duration::from_millis(2950),
    };
    assert_eq!(
        error.to_string(),
        "interaction not acknowledged in time: 2.95s after receipt, past the 2.9 s ack deadline \
         (Discord allows 3 s; INT-01): defer slow work"
    );
}

mod commands;

// ---- ack_deadline (Story 6.6 Matrix 14 and 15) ----

#[tokio::test]
async fn ack_deadline_builder() {
    assert_eq!(
        App::builder().build().unwrap().ack_deadline(),
        Duration::from_secs(2),
        "Matrix 15"
    );
    let deadline = |value: Duration| App::builder().ack_deadline(value).build();
    for ok in [Duration::ZERO, Duration::from_millis(2500)] {
        assert_eq!(
            deadline(ok).unwrap().ack_deadline(),
            ok,
            "Matrix 14: {ok:?}"
        );
    }
    let bad = Duration::from_millis(2501);
    assert_eq!(
        deadline(bad).unwrap_err().violations(),
        [BuildViolation::InvalidAckDeadline { actual: bad }],
        "Matrix 14"
    );
    assert_eq!(
        BuildViolation::InvalidAckDeadline { actual: bad }.to_string(),
        "ack deadline 2.501s is out of range: use a value in 0 ms..=2500 ms"
    );
    let mut builder = App::builder();
    builder
        .ack_deadline(Duration::from_millis(100))
        .ack_deadline(Duration::from_millis(200));
    assert!(format!("{builder:?}").contains("ack_deadline: 200ms"));
    let app = builder.build().unwrap();
    assert_eq!(app.ack_deadline(), Duration::from_millis(200));
    assert!(format!("{app:?}").contains("ack_deadline: 200ms"));
}

// ---- ref_ttl (Story 7.5 Matrix 37) ----

#[tokio::test]
async fn ref_ttl_builder() {
    assert_eq!(
        App::builder().build().unwrap().ref_ttl(),
        Duration::from_hours(24)
    );
    let day = Duration::from_hours(24);
    for ok in [Duration::from_secs(1), day * 365] {
        assert_eq!(
            App::builder().ref_ttl(ok).build().unwrap().ref_ttl(),
            ok,
            "{ok:?}"
        );
    }
    for bad in [
        Duration::ZERO,
        Duration::from_millis(999),
        day * 365 + Duration::from_secs(1),
    ] {
        // Listed with every other violation.
        let deadline = Duration::from_secs(3);
        assert_eq!(
            App::builder()
                .ref_ttl(bad)
                .ack_deadline(deadline)
                .build()
                .unwrap_err()
                .violations(),
            [
                BuildViolation::InvalidAckDeadline { actual: deadline },
                BuildViolation::InvalidRefTtl { actual: bad },
            ],
            "{bad:?}"
        );
    }
    assert_eq!(
        BuildViolation::InvalidRefTtl {
            actual: Duration::ZERO
        }
        .to_string(),
        "ref TTL 0ns is out of range: use a value in 1 s..=365 d"
    );
    let mut builder = App::builder();
    builder.ref_ttl(day).ref_ttl(Duration::from_mins(1));
    assert!(format!("{builder:?}").contains("ref_ttl: 60s"));
    assert_eq!(builder.build().unwrap().ref_ttl(), Duration::from_mins(1));
}

/// Matrix 15: a CLI dry run writes the report, then a newline, to its
/// output.
#[tokio::test(flavor = "multi_thread")]
async fn cli_dry_run_writes_the_report() {
    use crate::edge::test_support::TestServer;
    use crate::test_support::command;

    let server = TestServer::start_with(
        false,
        Arc::new(|seen| {
            let body: &'static [u8] = if seen.path.ends_with("/applications/@me") {
                br#"{"id": "7", "integration_types_config": {"0": {}}}"#
            } else {
                b"[]"
            };
            (hyper::StatusCode::OK, Bytes::from_static(body))
        }),
    )
    .await;
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(token())
        .http_proxy(server.authority())
        .module(Module::new("m").__add(command("m::ping", &["ping"], "Pong")));
    let app = builder.build().unwrap();

    let mut out = Vec::new();
    app.run_mode_to(CliMode::Publish, true, &mut out)
        .await
        .unwrap();
    let report = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text, format!("{report}\n"));
    assert!(text.contains("\"name\": \"ping\""), "{text}");
    assert!(text.contains("+ create chat_input `ping`"), "{text}");
    assert!(text.contains("creates: 1 (REG-05"), "{text}");
    assert!(server.seen().iter().all(|s| s.method != hyper::Method::PUT));
}

/// Story 11.4 Matrix 1, 6: a dry run's target JSON carries the
/// localizations.
#[tokio::test(flavor = "multi_thread")]
async fn loc_dry_run_reports_localizations() {
    use crate::edge::test_support::TestServer;
    use crate::test_support::command;
    use noctis_rules::Locale;

    static BOT: noctis_i18n::Catalog = noctis_i18n::Catalog::__new(
        "bot/locales",
        Locale::EnUs,
        &[
            (Locale::EnUs, &[("cmd.ping.description", "Pong!")]),
            (Locale::Tr, &[("cmdword.ping", "ping-tr")]),
        ],
        &[],
    );
    let server = TestServer::start_with(
        false,
        Arc::new(|seen| {
            let body: &'static [u8] = if seen.path.ends_with("/applications/@me") {
                br#"{"id": "7", "integration_types_config": {"0": {}}}"#
            } else {
                b"[]"
            };
            (hyper::StatusCode::OK, Bytes::from_static(body))
        }),
    )
    .await;
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(token())
        .http_proxy(server.authority())
        .locales(BOT)
        .module(Module::new("m").__add(command("m::ping", &["ping"], "Pong!")));
    let app = builder.build().unwrap();
    let report = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(report.targets()[0].json()).unwrap();
    assert_eq!(
        json[0]["name_localizations"],
        serde_json::json!({"tr": "ping-tr"})
    );
    assert_eq!(
        json[0]["description_localizations"],
        serde_json::json!({"en-GB": "Pong!", "en-US": "Pong!"})
    );
    assert!(server.seen().iter().all(|s| s.method != hyper::Method::PUT));
}

// ---- payload keys (Story 7.2 Matrix 13-16) ----

/// Matrix 15: `NOCTIS_PAYLOAD_KEY` and `NOCTIS_PAYLOAD_KEY_PREVIOUS` parse
/// 64 hex digits (trimmed; empty is unset); a bad value names the variable,
/// never the value.
#[test]
fn from_env_payload_keys() {
    use noctis_codec::KeyError;

    let hex = "ab".repeat(32);
    for var in ["NOCTIS_PAYLOAD_KEY", "NOCTIS_PAYLOAD_KEY_PREVIOUS"] {
        let read = |raw: &str| {
            AppBuilder::from_lookup(lookup_var(var, Some(raw.into()))).map(|b| {
                if var == "NOCTIS_PAYLOAD_KEY" {
                    b.payload_key.is_some()
                } else {
                    b.previous_payload_key.is_some()
                }
            })
        };
        assert_eq!(read(&hex), Ok(true));
        assert_eq!(read(&hex.to_uppercase()), Ok(true));
        assert_eq!(read(&format!("  {hex}  ")), Ok(true));
        assert_eq!(read(""), Ok(false));
        let g10 = format!("{}g{}", &hex[..10], &hex[11..]);
        let cases = [
            ("0".repeat(63), KeyError::Length { len: 63 }),
            ("0".repeat(65), KeyError::Length { len: 65 }),
            (
                format!("0x{}", "0".repeat(62)),
                KeyError::InvalidChar { index: 1 },
            ),
            (g10, KeyError::InvalidChar { index: 10 }),
        ];
        for (raw, source) in cases {
            let err = read(&raw).unwrap_err();
            assert_eq!(err, EnvError::InvalidPayloadKey { var, source }, "{raw:?}");
            let text = format!("{err} {err:?} {}", std::error::Error::source(&err).unwrap());
            assert!(text.contains(var), "{text}");
            assert!(!text.contains(&raw), "{text}");
            assert!(!text.contains("000"), "{text}");
        }
    }
}

/// Matrix 13 and 14: a mounted `signed` Route needs a key; a previous key
/// needs a current one.
#[tokio::test]
async fn cid_06_payload_key_build_violations() {
    use crate::test_support::{signed_button, test_route};

    test_route!(
        TSignedBio,
        "sig.bio",
        noctis_codec::RouteShape::INLINE.signed(),
        0
    );

    let mut builder = App::builder();
    builder.module(Module::new("shop").__add(signed_button("m::Sig")).__add(
        crate::ModalHandler::__new::<TSignedBio, _, _>(
            "m::SigBio",
            "sig.bio",
            |_: TSignedBio, _: crate::ModalSubmitCtx| async { Ok(crate::Responded::new()) },
        ),
    ));
    let err = builder.build().unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [
                BuildViolation::MissingPayloadKey { route: "sig" },
                BuildViolation::MissingPayloadKey { route: "sig.bio" },
            ]
        ),
        "{err}"
    );
    assert_eq!(
        err.violations()[0].to_string(),
        "[CID-06] Route `sig` is `signed`, but no payload key is configured: set \
         NOCTIS_PAYLOAD_KEY (64 hex digits) or call `.payload_key(..)`"
    );

    let key = || PayloadKey::from_bytes([0xab; 32]);
    let mut builder = App::builder();
    builder.previous_payload_key(key());
    let err = builder.build().unwrap_err();
    assert!(
        matches!(err.violations(), [BuildViolation::PreviousKeyWithoutKey]),
        "{err}"
    );
    assert!(
        err.violations()[0]
            .to_string()
            .starts_with("[CID-06] a previous payload key")
    );

    let mut builder = App::builder();
    builder
        .module(Module::new("shop").__add(signed_button("m::Sig")))
        .payload_key(key())
        .previous_payload_key(key());
    let app = builder.build().unwrap();
    assert!(app.shared().payload_keys.is_some());
}

/// Matrix 16: no `Debug` of the builder or the App shows the key.
#[tokio::test]
async fn payload_key_is_never_printed() {
    let key = PayloadKey::from_bytes([0xab; 32]);
    let mut builder = App::builder();
    builder.payload_key(key.clone()).previous_payload_key(key);
    let debug = format!("{builder:?} {builder:#?}");
    assert!(debug.contains("payload_key: true"), "{debug}");
    assert!(!debug.contains("abab") && !debug.contains("171"), "{debug}");
    let app = builder.build().unwrap();
    let debug = format!("{app:?} {app:#?}");
    assert!(debug.contains("payload_key: true"), "{debug}");
    assert!(!debug.contains("abab") && !debug.contains("171"), "{debug}");
}

// ---- App ids and namespaced ports (Story 7.4) ----

/// A real-format token of application `1234567890123456789`.
const ID_TOKEN: &str = "MTIzNDU2Nzg5MDEyMzQ1Njc4OQ.x.y";

/// Builds `builder` and returns the App plus the `noctis::app` warnings.
fn build_logged(builder: &mut AppBuilder) -> (Result<App, BuildError>, Vec<String>) {
    let capture = crate::test_support::Capture::default();
    let result = {
        let _logs = tracing::subscriber::set_default(capture.subscriber());
        builder.build()
    };
    (result, capture.lines_with(&["WARN", "noctis::app"]))
}

#[tokio::test]
async fn app_id_default_from_token() {
    for raw in [
        ID_TOKEN.to_owned(),
        "MTIzNDU2Nzg5MDEyMzQ1Njc4OQ==.x.y".to_owned(),
        format!("Bot {ID_TOKEN}"),
    ] {
        let (app, warnings) = build_logged(App::builder().token(Token::new(raw.clone()).unwrap()));
        let app = app.unwrap();
        assert_eq!(app.app_id().as_str(), "9do1sj396nf9", "{raw}");
        assert_eq!(app.app_label(), "9do1sj396nf9");
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}

#[tokio::test]
async fn app_id_undecodable_token_warns() {
    for raw in [
        "fake-token",
        "abc.def",
        "YWJj.x.y",
        "MA.x.y",
        "MTExMTExMTExMTExMTExMTExMTEx.x.y",
        "MTg0NDY3NDQwNzM3MDk1NTE2MTY.x.y",
    ] {
        let (app, warnings) = build_logged(App::builder().token(Token::new(raw).unwrap()));
        assert_eq!(app.unwrap().app_id().as_str(), "default", "{raw}");
        assert_eq!(warnings.len(), 1, "{raw}: {warnings:?}");
        assert!(
            warnings[0].contains(
                "could not derive the App id from the bot token; call AppBuilder::app_id"
            )
        );
        assert!(!warnings[0].contains(raw), "{raw}");
    }
}

#[tokio::test]
async fn app_id_without_token() {
    let (app, warnings) = build_logged(&mut App::builder());
    let app = app.unwrap();
    assert_eq!(app.app_id().as_str(), "default");
    assert_eq!(app.app_label(), "default");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(format!("{app:?}").contains(r#"app_id: AppId("default")"#));
}

#[tokio::test]
async fn app_id_explicit_wins() {
    let (app, warnings) = build_logged(
        App::builder()
            .token(Token::new(ID_TOKEN).unwrap())
            .app_id("my-bot"),
    );
    assert_eq!(app.unwrap().app_id().as_str(), "my-bot");
    assert!(warnings.is_empty());
    let mut builder = App::builder();
    builder.app_id("a").app_id("b");
    assert!(format!("{builder:?}").contains(r#"app_id: Some("b")"#));
    assert_eq!(builder.build().unwrap().app_id().as_str(), "b");
}

#[tokio::test]
async fn app_id_invalid_is_a_violation() {
    for (raw, reason) in [
        ("Bad", AppIdError::InvalidChar { index: 0, ch: 'B' }),
        ("", AppIdError::Empty),
    ] {
        let err = App::builder()
            .app_id(raw)
            .ack_deadline(Duration::from_secs(3))
            .build()
            .unwrap_err();
        let violations = err.violations();
        assert!(
            violations.contains(&BuildViolation::InvalidAppId {
                reason: reason.clone()
            }),
            "{violations:?}"
        );
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, BuildViolation::InvalidAckDeadline { .. })),
            "{violations:?}"
        );
    }
    let message = BuildViolation::InvalidAppId {
        reason: AppIdError::InvalidChar { index: 0, ch: 'B' },
    }
    .to_string();
    assert!(
        message.starts_with("invalid App id: App id has `B` at index 0"),
        "{message}"
    );
}

#[tokio::test]
async fn app_id_from_env() {
    let builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_APP_ID", Some(" prod-1 ".into()))).unwrap();
    assert_eq!(builder.app_id.as_deref(), Some("prod-1"));
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_APP_ID", Some("".into()))).unwrap();
    assert!(builder.app_id.is_none());
    assert_eq!(builder.build().unwrap().app_id().as_str(), "default");
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_APP_ID", Some("PROD".into()))).unwrap();
    let err = builder.build().unwrap_err();
    assert_eq!(
        err.violations(),
        [BuildViolation::InvalidAppId {
            reason: AppIdError::InvalidChar { index: 0, ch: 'P' }
        }]
    );
    let mut builder =
        AppBuilder::from_lookup(lookup_var("NOCTIS_APP_ID", Some("PROD".into()))).unwrap();
    builder.app_id("x");
    assert_eq!(builder.build().unwrap().app_id().as_str(), "x");
}

#[cfg(unix)]
#[test]
fn from_env_app_id_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = AppBuilder::from_lookup(lookup_var(
        "NOCTIS_APP_ID",
        Some(OsString::from_vec(vec![b'a', 0xff])),
    ))
    .unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_APP_ID"
        }
    );
}

#[tokio::test]
async fn ports_are_namespaced() {
    let app = App::builder().app_id("a").build().unwrap();
    let ports = app.ports();
    assert_eq!(ports.ref_store.prefix(), "a:ref:");
    assert_eq!(ports.session_store.prefix(), "a:session:");
    assert_eq!(ports.module_state_store.prefix(), "a:module_state:");
    assert_eq!(ports.rate_limit_store.prefix(), "a:rate_limit:");
    assert_eq!(ports.cache_store.prefix(), "a:cache:");
    assert_eq!(ports.render_cache.prefix(), "a:render_cache:");
    assert_eq!(ports.asset_store.prefix(), "a:asset:");
}

#[tokio::test]
async fn arc_store_across_apps() {
    use noctis_ports::memory::MemoryCacheStore;

    let store = Arc::new(MemoryCacheStore::new());
    let a = App::builder()
        .app_id("a")
        .cache_store(Arc::clone(&store))
        .build()
        .unwrap();
    let erased: Arc<dyn CacheStore> = store.clone();
    let b = App::builder()
        .app_id("b")
        .cache_store(erased)
        .build()
        .unwrap();
    a.shared()
        .kv
        .put("k", Bytes::from_static(b"1"), None)
        .await
        .unwrap();
    b.shared()
        .kv
        .put("k", Bytes::from_static(b"2"), None)
        .await
        .unwrap();
    assert_eq!(
        store.get("a:cache:k").await.unwrap(),
        Some(Bytes::from_static(b"1"))
    );
    assert_eq!(
        store.get("b:cache:k").await.unwrap(),
        Some(Bytes::from_static(b"2"))
    );
    assert!(!a.ports().is_default(PortSlot::CacheStore));
}

#[tokio::test]
async fn shared_store_across_apps() {
    use noctis_ports::memory::MemoryCacheStore;

    let store = Arc::new(MemoryCacheStore::new());
    let a = App::builder()
        .app_id("a")
        .cache_store(Shared::new(Arc::clone(&store)))
        .build()
        .unwrap();
    let b = App::builder()
        .app_id("b")
        .cache_store(Shared::new(Arc::clone(&store)))
        .build()
        .unwrap();
    let c = App::builder()
        .app_id("c")
        .cache_store(Arc::clone(&store))
        .build()
        .unwrap();
    assert_eq!(a.ports().cache_store.prefix(), "_shared:cache:");
    a.shared()
        .kv
        .put("k", Bytes::from_static(b"1"), None)
        .await
        .unwrap();
    assert_eq!(
        b.shared().kv.get("k").await.unwrap(),
        Some(Bytes::from_static(b"1"))
    );
    assert_eq!(c.shared().kv.get("k").await.unwrap(), None);
    assert_eq!(
        store.get("_shared:cache:k").await.unwrap(),
        Some(Bytes::from_static(b"1"))
    );
}

#[test]
fn app_label_is_the_metric_label() {
    use metrics_util::debugging::DebuggingRecorder;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let app = runtime.block_on(async { App::builder().app_id("bot-a").build().unwrap() });
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    {
        let _metrics = metrics::set_default_local_recorder(&recorder);
        ShardStatus {
            app: Arc::clone(&app.shared().app),
            shards: 0..1,
            total: 1,
        }
        .init();
    }
    let labels: Vec<String> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .flat_map(|(key, ..)| {
            key.key()
                .labels()
                .filter(|l| l.key() == "app")
                .map(|l| l.value().to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(!labels.is_empty());
    assert!(labels.iter().all(|l| l == "bot-a"), "{labels:?}");
}

// ---- shard_range (Story 12.1 E1-E9, B1-B11, G3-G5) ----

/// `NOCTIS_SHARD_RANGE` read through `from_lookup`.
fn read_range(raw: impl Into<OsString>) -> Result<Option<Range<u32>>, EnvError> {
    AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_RANGE", Some(raw.into())))
        .map(|b| b.shard_range)
}

#[test]
fn from_env_shard_range_parses() {
    assert_eq!(
        AppBuilder::from_lookup(lookup_var("NOCTIS_SHARD_RANGE", None))
            .unwrap()
            .shard_range,
        None,
        "E3 absent"
    );
    #[expect(
        clippy::reversed_empty_ranges,
        reason = "E8: an empty range parses and fails at build()"
    )]
    let reversed = 9..3;
    for (raw, expected) in [
        ("32..64", Some(32..64)),                                 // E1
        ("  0007..0010\n", Some(7..10)),                          // E2
        ("", None),                                               // E3
        ("   ", None),                                            // E3
        ("4294967294..4294967295", Some(u32::MAX - 1..u32::MAX)), // E4
        ("5..5", Some(5..5)),                                     // E8
        ("9..3", Some(reversed)),                                 // E8
        ("0000000000..0000000001", Some(0..1)),
    ] {
        assert_eq!(read_range(raw), Ok(expected), "{raw:?}");
    }
}

#[test]
fn from_env_shard_range_rejects() {
    for raw in [
        "0..4294967296",  // E5
        "00000000001..2", // E5: 11 digits
        "32..=63",        // E6
        "32-63",
        "32..",
        "..64",
        "..",
        "32",
        "32...64",
        "32..64..96",
        "32 .. 64",
        "+1..2",
        "-1..2",
        "0x1..2",
        "1_0..20",
        "32..64/128",
        "\u{663}..\u{664}",   // E7: Arabic-Indic digits
        "\u{ff13}..\u{ff14}", // E7: full-width digits
        "SECRET..x",          // E9
    ] {
        let err = read_range(raw).unwrap_err();
        assert_eq!(
            err,
            EnvError::InvalidShardRange {
                var: "NOCTIS_SHARD_RANGE"
            },
            "{raw:?}"
        );
        let text = err.to_string();
        assert_eq!(
            text,
            "environment variable `NOCTIS_SHARD_RANGE` is not a shard range: set start..end, \
             e.g. 0..16"
        );
        assert!(!text.contains("SECRET"));
    }
}

#[cfg(unix)]
#[test]
fn from_env_shard_range_not_unicode() {
    use std::os::unix::ffi::OsStringExt;
    let err = read_range(OsString::from_vec(vec![b'0', b'.', b'.', 0xff])).unwrap_err();
    assert_eq!(
        err,
        EnvError::NotUnicode {
            var: "NOCTIS_SHARD_RANGE"
        }
    );
}

#[tokio::test]
async fn from_env_shard_range_reaches_the_app() {
    let mut builder = AppBuilder::from_lookup(|var| match var {
        "NOCTIS_SHARD_TOTAL" => Some("128".into()),
        "NOCTIS_SHARD_RANGE" => Some("32..64".into()),
        _ => None,
    })
    .unwrap();
    let app = builder.build().unwrap();
    assert_eq!(app.shard_range(), Some(32..64));
    assert_eq!(app.shard_total(), Some(128));
}

/// The violations `build()` records for `total` and `range`.
fn range_violations(total: Option<u32>, range: Range<u32>) -> Vec<BuildViolation> {
    let mut builder = App::builder();
    if let Some(total) = total {
        builder.shard_total(total);
    }
    builder.shard_range(range);
    match builder.build() {
        Ok(_) => Vec::new(),
        Err(err) => err.violations().to_vec(),
    }
}

#[tokio::test]
async fn shard_range_builder_accepts() {
    let app = App::builder()
        .shard_total(128)
        .shard_range(32..64)
        .build()
        .unwrap();
    assert_eq!(app.shard_range(), Some(32..64), "B1");
    assert_eq!(app.shard_total(), Some(128), "B1");
    for (total, range) in [
        (4, 2..4),               // B4
        (4, 0..4),               // B4
        (1, 0..1),               // B6
        (128, 127..128),         // B6
        (u32::MAX, 0..u32::MAX), // B7
    ] {
        let app = App::builder()
            .shard_total(total)
            .shard_range(range.clone())
            .build()
            .unwrap();
        assert_eq!(app.shard_range(), Some(range));
    }
    let app = App::builder()
        .shard_total(4)
        .shard_range(0..2)
        .shard_range(2..4)
        .build()
        .unwrap();
    assert_eq!(app.shard_range(), Some(2..4), "B10 last call wins");
    assert_eq!(App::builder().build().unwrap().shard_range(), None, "B11");
}

#[tokio::test]
async fn shard_range_builder_rejects() {
    #[expect(clippy::reversed_empty_ranges, reason = "B2: the input under test")]
    let reversed = 9..3;
    for (start, end, range) in [(0, 0, 0..0), (5, 5, 5..5), (9, 3, reversed)] {
        assert_eq!(
            range_violations(Some(16), range),
            [BuildViolation::EmptyShardRange { start, end }],
            "B2"
        );
    }
    assert_eq!(
        range_violations(None, 0..4),
        [BuildViolation::ShardRangeWithoutTotal { start: 0, end: 4 }],
        "B3"
    );
    for (start, end) in [(2, 5), (4, 5), (u32::MAX - 1, u32::MAX)] {
        assert_eq!(
            range_violations(Some(4), start..end),
            [BuildViolation::ShardRangeOutOfBounds {
                start,
                end,
                total: 4
            }],
            "B5"
        );
    }
    assert_eq!(
        range_violations(Some(0), 0..1),
        [BuildViolation::InvalidShardTotal],
        "B8"
    );
    assert_eq!(
        range_violations(None, 3..3),
        [BuildViolation::EmptyShardRange { start: 3, end: 3 }],
        "B9"
    );
}

#[test]
fn shard_range_violation_messages() {
    assert_eq!(
        BuildViolation::EmptyShardRange { start: 5, end: 5 }.to_string(),
        "shard range 5..5 is empty: use start..end with start below end, e.g. 0..16"
    );
    assert_eq!(
        BuildViolation::ShardRangeWithoutTotal { start: 0, end: 4 }.to_string(),
        "shard range 0..4 needs a fixed shard total: set shard_total (NOCTIS_SHARD_TOTAL) to the \
         same value on every process"
    );
    assert_eq!(
        BuildViolation::ShardRangeOutOfBounds {
            start: 2,
            end: 5,
            total: 4
        }
        .to_string(),
        "shard range 2..5 exceeds the shard total 4: the end must be at most 4"
    );
}

#[tokio::test]
async fn shard_range_debug() {
    let mut builder = App::builder();
    builder.shard_total(4).shard_range(2..4);
    assert!(format!("{builder:?}").contains("shard_range: Some(2..4)"));
    let app = builder.build().unwrap();
    assert!(format!("{app:?}").contains("shard_range: Some(2..4)"));
}

/// `ShardEvent::Ready` for each of `shards`, on a channel kept open.
fn readies(shards: &[u32]) -> mpsc::UnboundedReceiver<ShardEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    for &shard in shards {
        tx.send(ShardEvent::Ready { shard }).unwrap();
    }
    std::mem::forget(tx);
    rx
}

/// `ShardEvent::Ended` for each of `shards`, on a channel kept open.
fn endings(shards: &[u32]) -> mpsc::UnboundedReceiver<ShardEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    for &shard in shards {
        tx.send(ShardEvent::Ended {
            shard,
            close_code: Some(4004),
        })
        .unwrap();
    }
    std::mem::forget(tx);
    rx
}

/// G3: startup is done once every shard of the range is READY; shards
/// outside it count for nothing.
#[tokio::test]
async fn startup_waits_only_for_the_shard_range() {
    let status = ranged_status(2..4, 4);
    let lifecycle = Lifecycle::new();
    let waited = tokio::time::timeout(
        Duration::from_millis(100),
        await_startup(&mut readies(&[0, 1, 4, 2, 2]), &status, &lifecycle),
    )
    .await;
    assert!(waited.is_err(), "shard 3 is not ready yet");
    assert_eq!(
        await_startup(&mut readies(&[0, 1, 4, 2, 3]), &status, &lifecycle).await,
        Ok(())
    );
}

/// G5: once both shards of the range ended, `supervise` stops with
/// `AllEnded`; an end outside the range, or a repeated one, counts for
/// nothing.
#[tokio::test]
async fn supervise_ends_when_the_range_ended() {
    let status = ranged_status(2..4, 4);
    let lifecycle = Lifecycle::new();
    let pending = tokio::time::timeout(
        Duration::from_millis(100),
        supervise(
            &mut endings(&[0, 1, 2, 2]),
            &status,
            &lifecycle,
            None,
            std::future::pending::<()>(),
        ),
    )
    .await;
    assert!(pending.is_err(), "shard 3 is still running");
    let stop = supervise(
        &mut endings(&[0, 2, 3]),
        &status,
        &lifecycle,
        None,
        std::future::pending::<()>(),
    )
    .await;
    assert_eq!(stop, Stop::AllEnded);
}

/// G4: `noctis_shard_status{shard}` exists only for the range's shards.
#[test]
fn shard_gauges_cover_only_the_range() {
    use metrics_util::debugging::DebuggingRecorder;

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        let status = ranged_status(2..4, 4);
        status.init();
        status.update(&ShardEvent::Ready { shard: 0 });
        status.update(&ShardEvent::Ready { shard: 3 });
        std::mem::forget(status);
    });
    let mut shards: Vec<String> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.key().name() == "noctis_shard_status")
        .flat_map(|(key, ..)| {
            key.key()
                .labels()
                .filter(|l| l.key() == "shard")
                .map(|l| l.value().to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    shards.sort();
    assert_eq!(shards, ["2", "3"]);
}

// ---- runtime locale overlays (Story 11.5) ----

mod overlays {
    use std::sync::Mutex;

    use noctis_i18n::{Catalog, OverlaySlot, OverlayViolation};
    use noctis_ports::{BoxFuture, LocaleSource, PortError};
    use noctis_rules::Locale;
    use tokio::sync::Notify;

    use super::*;

    type Answer =
        Box<dyn Fn(&str) -> Option<Result<Vec<(String, String)>, PortError>> + Send + Sync>;

    /// A `LocaleSource` recording its calls; `answer` returns `None` to
    /// never resolve.
    struct Stub {
        calls: Arc<Mutex<Vec<String>>>,
        called: Arc<Notify>,
        answer: Answer,
    }

    impl LocaleSource for Stub {
        fn load<'a>(
            &'a self,
            locale: &'a str,
        ) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>> {
            self.calls.lock().unwrap().push(locale.to_owned());
            self.called.notify_one();
            let answer = (self.answer)(locale);
            Box::pin(async move {
                match answer {
                    Some(result) => result,
                    None => std::future::pending().await,
                }
            })
        }
    }

    fn stub(
        answer: impl Fn(&str) -> Option<Result<Vec<(String, String)>, PortError>>
        + Send
        + Sync
        + 'static,
    ) -> (Stub, Arc<Mutex<Vec<String>>>, Arc<Notify>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let called = Arc::new(Notify::new());
        let stub = Stub {
            calls: Arc::clone(&calls),
            called: Arc::clone(&called),
            answer: Box::new(answer),
        };
        (stub, calls, called)
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    const FILES: &[(Locale, &[(&str, &str)])] = &[
        (
            Locale::EnUs,
            &[("greet.hello", "Hello"), ("cmd.greet.description", "Greet")],
        ),
        (Locale::Tr, &[("greet.hello", "Merhaba")]),
        (Locale::Uk, &[]),
    ];
    const PLACEHOLDERS: &[(&str, &[&str])] = &[("greet.hello", &[])];
    const CONTENT: &[&str] = &["greet.hello"];

    /// Defines a catalog with its own process-wide slot.
    macro_rules! catalog {
        ($name:ident) => {
            static $name: Catalog = {
                static SLOT: OverlaySlot = OverlaySlot::new();
                Catalog::__new("bot/locales", Locale::EnUs, FILES, PLACEHOLDERS)
                    .__with_overlay(&SLOT, CONTENT)
            };
        };
    }

    fn app(catalog: Option<Catalog>, source: Option<Stub>) -> App {
        let mut builder = App::builder();
        builder.runtime(&Runtime::for_test()).token(token());
        if let Some(catalog) = catalog {
            builder.locales(catalog);
        }
        if let Some(source) = source {
            builder.locale_source(source);
        }
        builder.build().unwrap()
    }

    /// Story 11.5 Matrix 6
    #[tokio::test(flavor = "multi_thread")]
    async fn one_load_per_available_locale_in_order() {
        static PLAIN: Catalog = Catalog::__new("plain/locales", Locale::EnUs, FILES, PLACEHOLDERS);
        let (source, calls, _) = stub(|_| Some(Ok(Vec::new())));
        let app = app(Some(PLAIN), Some(source));
        app.load_locale_overlays().await.unwrap();
        assert_eq!(*calls.lock().unwrap(), ["en-US", "tr", "uk"]);
    }

    /// Story 11.5 Matrix 7
    #[tokio::test(flavor = "multi_thread")]
    async fn no_catalogs_never_load() {
        let (source, calls, _) = stub(|_| Some(Ok(pairs(&[("cmd.x", "y")]))));
        let app = app(None, Some(source));
        app.load_locale_overlays().await.unwrap();
        assert!(calls.lock().unwrap().is_empty());
    }

    /// Story 11.5 Matrix 8
    #[tokio::test(flavor = "multi_thread")]
    async fn default_port_touches_nothing() {
        catalog!(C);
        let capture = crate::test_support::Capture::default();
        let _logs = tracing::subscriber::set_default(capture.subscriber());
        let app = app(Some(C), None);
        app.load_locale_overlays().await.unwrap();
        assert_eq!(C.__overlay_row(Locale::Tr), None);
        assert!(!capture.text().contains("locale overlays installed"));
    }

    /// Story 11.5: a valid load installs and logs once.
    #[tokio::test(flavor = "multi_thread")]
    async fn valid_overlays_install_and_log() {
        catalog!(C);
        let capture = crate::test_support::Capture::default();
        let _logs = tracing::subscriber::set_default(capture.subscriber());
        let (source, _, _) = stub(|code| {
            Some(Ok(if code == "tr" {
                pairs(&[("greet.hello", "Selam")])
            } else {
                Vec::new()
            }))
        });
        let app = app(Some(C), Some(source));
        app.load_locale_overlays().await.unwrap();
        assert_eq!(
            app.shared()
                .strings
                .translate(
                    Locale::Tr,
                    noctis_rules::MessageKey::new("greet.hello"),
                    &[]
                )
                .as_deref(),
            Some("Selam")
        );
        let text = capture.text();
        assert_eq!(
            text.matches("locale overlays installed").count(),
            1,
            "{text}"
        );
        assert!(text.contains("values=1"), "{text}");
        assert!(text.contains("catalogs=1"), "{text}");
    }

    /// Story 11.5 Matrix 23
    #[tokio::test(flavor = "multi_thread")]
    async fn load_failure_stops_loading() {
        catalog!(C);
        let (source, calls, _) = stub(|code| {
            Some(if code == "tr" {
                Err(PortError::unavailable("offline"))
            } else {
                Ok(pairs(&[("greet.hello", "Hi")]))
            })
        });
        let app = app(Some(C), Some(source));
        let err = app.load_locale_overlays().await.unwrap_err();
        assert!(
            matches!(
                err,
                LocaleOverlayError::Load {
                    locale: Locale::Tr,
                    ..
                }
            ),
            "{err:?}"
        );
        let source = std::error::Error::source(&err).expect("the PortError");
        assert!(source.downcast_ref::<PortError>().is_some());
        assert_eq!(*calls.lock().unwrap(), ["en-US", "tr"]);
        assert_eq!(C.__overlay_row(Locale::EnUs), None);
    }

    /// Story 11.5 Matrix 27
    #[tokio::test(flavor = "multi_thread")]
    async fn shutdown_during_load_returns_ok() {
        catalog!(C);
        let (source, _, called) = stub(|_| None);
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(token())
            .http_proxy("127.0.0.1:1")
            .locales(C)
            .locale_source(source);
        let app = builder.build().unwrap();
        let outcome = tokio::time::timeout(
            Duration::from_secs(15),
            app.run_until(async move { called.notified().await }),
        )
        .await;
        assert!(matches!(outcome, Ok(Ok(()))), "{outcome:?}");
    }

    fn server_answer() -> crate::edge::test_support::Answer {
        Arc::new(|seen| {
            let body: &'static [u8] = if seen.path.ends_with("/applications/@me") {
                br#"{"id": "7", "integration_types_config": {"0": {}}}"#
            } else {
                b"[]"
            };
            (hyper::StatusCode::OK, Bytes::from_static(body))
        })
    }

    /// Story 11.5 Matrix 30
    #[tokio::test(flavor = "multi_thread")]
    async fn cli_publish_never_loads() {
        use crate::edge::test_support::TestServer;

        catalog!(C);
        let server = TestServer::start_with(false, server_answer()).await;
        let (source, calls, _) = stub(|_| Some(Ok(pairs(&[("cmd.greet.description", "x")]))));
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(token())
            .http_proxy(server.authority())
            .locales(C)
            .locale_source(source);
        let app = builder.build().unwrap();
        let mut out = Vec::new();
        app.run_mode_to(CliMode::Publish, true, &mut out)
            .await
            .unwrap();
        assert!(calls.lock().unwrap().is_empty());
    }

    /// Story 11.5 Matrix 31
    #[tokio::test(flavor = "multi_thread")]
    async fn invalid_overlay_fails_before_any_discord_request() {
        use crate::edge::test_support::TestServer;

        catalog!(C);
        let server = TestServer::start_with(false, server_answer()).await;
        let (source, _, _) = stub(|code| {
            Some(Ok(if code == "tr" {
                pairs(&[("cmd.greet.description", "Selamla")])
            } else {
                Vec::new()
            }))
        });
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(token())
            .http_proxy(server.authority())
            .publisher(true)
            .locales(C)
            .locale_source(source);
        let app = builder.build().unwrap();
        let outcome = tokio::time::timeout(
            Duration::from_secs(15),
            app.run_until(std::future::pending()),
        )
        .await
        .expect("startup fails fast");
        let Err(Error::LocaleOverlay(LocaleOverlayError::Invalid(violations))) = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            violations,
            [OverlayViolation::ReservedKey {
                locale: Locale::Tr,
                key: "cmd.greet.description".into(),
            }]
        );
        assert!(server.seen().is_empty(), "{:?}", server.seen().len());
        assert_eq!(C.__overlay_row(Locale::Tr), None);
    }
}

/// Story 12.4 R16 (RL-02): a Shard Range smaller than the total without a
/// proxy is a Cluster at risk of the global limit.
#[test]
fn rl_02_cluster_without_proxy() {
    for (range, total) in [(0..2, 4), (2..4, 4), (0..1, 2)] {
        assert!(
            cluster_without_proxy(Some(&range), Some(total), None),
            "{range:?}/{total}"
        );
    }
}

/// Story 12.4 R17: the whole range, no Cluster or a proxy is not.
#[test]
fn cluster_with_proxy_or_whole_range() {
    assert!(!cluster_without_proxy(Some(&(0..4)), Some(4), None));
    assert!(!cluster_without_proxy(None, Some(4), None));
    assert!(!cluster_without_proxy(None, None, None));
    assert!(!cluster_without_proxy(Some(&(0..2)), Some(4), Some("p:1")));
}

/// Story 12.4 E13 (RL-02): exactly one warning naming the risk and the fix.
#[test]
fn rl_02_warning_text() {
    let capture = crate::test_support::Capture::default();
    tracing::subscriber::with_default(capture.subscriber(), || {
        warn_cluster_without_proxy(Some(&(0..2)), Some(4));
    });
    let lines = capture.lines_with(&["WARN", "noctis::app", "RL-02"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    let line = &lines[0];
    for needle in [
        "50 requests/s",
        "NOCTIS_HTTP_PROXY",
        "AppBuilder::http_proxy",
        "start=0",
        "end=2",
        "total=4",
    ] {
        assert!(line.contains(needle), "{needle}: {line}");
    }
    assert_eq!(capture.text().lines().count(), 1);
}

/// Story 12.4 (RL-02): an App running a Shard Range without a proxy warns
/// exactly once; the same App with a proxy does not.
#[tokio::test]
async fn rl_02_app_warns_without_proxy() {
    for (proxy, expected) in [(None, 1), (Some("127.0.0.1:1"), 0)] {
        let mut builder = App::builder();
        builder.token(token()).shard_total(4).shard_range(0..2);
        if let Some(proxy) = proxy {
            builder.http_proxy(proxy);
        }
        let app = builder.build().unwrap();
        let capture = crate::test_support::Capture::default();
        tracing::subscriber::with_default(capture.subscriber(), || {
            app.warn_cluster_without_proxy();
        });
        let lines = capture.lines_with(&["WARN", "noctis::app", "RL-02"]);
        assert_eq!(lines.len(), expected, "{proxy:?}: {}", capture.text());
    }
}

/// Story 12.4 E11: the default request timeout is 10 s.
#[tokio::test]
async fn default_http_timeout_is_ten_seconds() {
    let app = App::builder().build().unwrap();
    assert_eq!(app.http_timeout(), Duration::from_secs(10));
}

// ---- Story 13.2: Boot Flags at the CLI surface ----

mod boot_flags {
    use super::*;
    use crate::BootFlagError;
    use crate::edge::test_support::TestServer;
    use crate::test_support::command;

    fn only(flags: &[&str]) -> BootFlags {
        BootFlags::only(flags).expect("valid flags")
    }

    fn features(value: Option<&str>) -> impl Fn(&str) -> Option<OsString> {
        lookup_var("NOCTIS_FEATURES", value.map(OsString::from))
    }

    fn selection(value: Option<&str>, args: &[&str]) -> Result<BootFlags, Error> {
        let builder = AppBuilder::from_sources(features(value), args.iter().copied())?;
        Ok(builder.boot_flags.clone())
    }

    /// E1-E5: sources and precedence.
    #[test]
    fn env_and_argv() {
        assert_eq!(
            selection(Some(" a,b "), &["bot"]).unwrap(),
            only(&["a", "b"])
        );
        assert_eq!(selection(Some(""), &["bot"]).unwrap(), BootFlags::all());
        assert_eq!(selection(None, &["bot"]).unwrap(), BootFlags::all());
        assert_eq!(
            selection(Some("none"), &["bot"]).unwrap(),
            BootFlags::none()
        );
        let err = selection(Some("a,,b"), &["bot"]).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Env(EnvError::InvalidBootFlags {
                    var: "NOCTIS_FEATURES",
                    reason: BootFlagError::Empty
                })
            ),
            "{err:?}"
        );
        assert_eq!(
            selection(Some("a"), &["bot", "--features", "b"]).unwrap(),
            only(&["b"])
        );
        assert_eq!(
            selection(None, &["bot", "--mine", "--features=all"]).unwrap(),
            BootFlags::all()
        );
        let err = selection(None, &["bot", "--features", "A"]).unwrap_err();
        assert!(
            matches!(err, Error::Cli(CliError::InvalidFeatures { .. })),
            "{err:?}"
        );
        // E5: a setter afterwards wins.
        let mut builder =
            AppBuilder::from_sources(features(Some("a")), ["bot", "--features", "b"]).unwrap();
        builder.boot_flags(BootFlags::none());
        assert_eq!(builder.boot_flags, BootFlags::none());
    }

    /// A remote holding `bravo` (pruned by `{a}`), answering every
    /// `GET .../commands` with it.
    async fn server() -> TestServer {
        TestServer::start_with(
            false,
            Arc::new(|seen| {
                let body: &'static [u8] = if seen.path.ends_with("/applications/@me") {
                    br#"{"id": "7", "integration_types_config": {"0": {}}}"#
                } else {
                    br#"[{"id": "9", "application_id": "7", "version": "1", "type": 1, "name": "bravo", "description": "Bravo", "options": []}]"#
                };
                (hyper::StatusCode::OK, Bytes::from_static(body))
            }),
        )
        .await
    }

    fn modules(builder: &mut AppBuilder, server: &TestServer) {
        builder
            .runtime(&Runtime::for_test())
            .token(token())
            .http_proxy(server.authority())
            .module(Module::new("m").__add(command("m::ping", &["ping"], "Pong")))
            .module(Module::new("alpha").flags(["a"]).__add(command(
                "alpha::alpha",
                &["alpha"],
                "Alpha",
            )))
            .module(Module::new("bravo").flags(["b"]).__add(command(
                "bravo::bravo",
                &["bravo"],
                "Bravo",
            )));
    }

    /// R3: `from_env`-style build with `--publish --dry-run --features a`.
    #[tokio::test(flavor = "multi_thread")]
    async fn dry_run_reflects_the_selection() {
        let server = server().await;
        let args = ["bot", "--publish", "--dry-run", "--features", "a"];
        let mut builder = AppBuilder::from_sources(features(None), args).unwrap();
        modules(&mut builder, &server);
        let app = builder.build().unwrap();
        assert_eq!(app.boot_flags(), &only(&["a"]));

        let mut out = Vec::new();
        app.run_mode_to(CliMode::Publish, true, &mut out)
            .await
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"name\": \"ping\""), "{text}");
        assert!(text.contains("\"name\": \"alpha\""), "{text}");
        assert!(!text.contains("\"name\": \"bravo\""), "{text}");
        assert!(text.contains("- delete chat_input `bravo`"), "{text}");
        assert!(server.seen().iter().all(|s| s.method != hyper::Method::PUT));

        // R1: an equal `--features` proceeds.
        let args = NoctisArgs::split(args).unwrap().0;
        app.run_with_args(args).await.unwrap();
        assert!(server.seen().iter().all(|s| s.method != hyper::Method::PUT));
    }

    /// R2: a differing `--features` fails before any request.
    #[tokio::test(flavor = "multi_thread")]
    async fn mismatch_fails_first() {
        let server = server().await;
        let mut builder = App::builder();
        modules(&mut builder, &server);
        let app = builder.build().unwrap();
        let err = app
            .run_with_args(["--publish", "--dry-run", "--features", "a"])
            .await
            .unwrap_err();
        match err {
            Error::Cli(CliError::BootFlagsMismatch { given, built }) => {
                assert_eq!(given, only(&["a"]));
                assert_eq!(built, BootFlags::all());
            }
            other => panic!("{other:?}"),
        }
        assert!(server.seen().is_empty());
    }

    /// An unknown selected flag fails `build()`.
    #[tokio::test]
    async fn unknown_flag_fails_build() {
        let err = App::builder()
            .module(Module::new("eco").flags(["economy"]))
            .boot_flags(only(&["ecnomy"]))
            .build()
            .unwrap_err();
        assert_eq!(
            err.violations(),
            [BuildViolation::UnknownBootFlag {
                flag: "ecnomy".to_owned(),
                declared: vec!["economy"],
            }]
        );
    }
}
