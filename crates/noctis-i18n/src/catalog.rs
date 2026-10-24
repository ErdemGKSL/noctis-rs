//! [`Catalog`]: one `i18n!` invocation's compiled locale files, and
//! [`AppStrings`]: the App's catalogs layered over [`Builtin`].

use std::collections::HashMap;
use std::fmt::Display;

use noctis_rules::{Locale, MessageKey, Translate};

use crate::overlay::{self, OverlaySlot, OverlayViolation};
use crate::{Builtin, format};

/// The entries of one locale file: `(key, template)`, sorted by key bytes.
type Entries = &'static [(&'static str, &'static str)];

/// The compiled locale files of one `i18n!` invocation, as `catalog()`
/// returns it.
///
/// It holds every key written in each file (the reserved `cmd.*`,
/// `cmdword.*`, `ctxcmd.*` and `noctis.*` families included) without
/// fill-in. As a [`Translate`] it looks a key up in the locale's file, then
/// in its [`Locale::sibling`]'s file, then in the default locale's file, and
/// substitutes the arguments with [`format`].
///
/// Only the generated code constructs one; hand it to
/// `AppBuilder::locales`.
///
/// ```
/// use noctis_i18n::Catalog;
/// use noctis_rules::{Locale, MessageKey, Translate};
///
/// static CATALOG: Catalog = Catalog::__new(
///     "bot/locales",
///     Locale::EnUs,
///     &[
///         (Locale::EnUs, &[("shop.left", "{n} left")]),
///         (Locale::Tr, &[("shop.left", "{n} kaldı")]),
///     ],
///     &[("shop.left", &["n"])],
/// );
///
/// let n: &dyn std::fmt::Display = &3;
/// let left = MessageKey::new("shop.left");
/// assert_eq!(CATALOG.translate(Locale::Tr, left, &[("n", n)]).as_deref(), Some("3 kaldı"));
/// assert_eq!(CATALOG.translate(Locale::De, left, &[("n", n)]).as_deref(), Some("3 left"));
/// assert_eq!(CATALOG.get(Locale::De, "shop.left"), None);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    origin: &'static str,
    default: Locale,
    files: &'static [(Locale, Entries)],
    placeholders: &'static [(&'static str, &'static [&'static str])],
    /// The process-wide overlay slot (`None`: takes no overlays).
    overlay: Option<&'static OverlaySlot>,
    /// The content keys, sorted by key bytes: the overlay row order.
    content: &'static [&'static str],
}

impl Catalog {
    /// Builds a catalog; only `i18n!`'s generated code calls it.
    ///
    /// `files` is in [`Locale::ALL`] order and each file's entries are sorted
    /// by key bytes; `placeholders` is sorted by key bytes.
    #[doc(hidden)]
    pub const fn __new(
        origin: &'static str,
        default: Locale,
        files: &'static [(Locale, &'static [(&'static str, &'static str)])],
        placeholders: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        Self {
            origin,
            default,
            files,
            placeholders,
            overlay: None,
            content: &[],
        }
    }

    /// Links the catalog to its process-wide overlay `slot`; `content` is
    /// the content keys sorted by key bytes (the overlay row order). Only
    /// `i18n!`'s generated code and tests call it.
    #[doc(hidden)]
    #[must_use]
    pub const fn __with_overlay(
        self,
        slot: &'static OverlaySlot,
        content: &'static [&'static str],
    ) -> Self {
        Self {
            overlay: Some(slot),
            content,
            ..self
        }
    }

    /// The overlay row of `locale` (one value per content key, in
    /// content order) once overlays are installed and `locale`'s
    /// fallback chain holds one; `None` keeps the compiled text.
    #[doc(hidden)]
    pub fn __overlay_row(&self, locale: Locale) -> Option<&'static [&'static str]> {
        self.overlay?.row(locale)
    }

    /// The content keys that take overlays, sorted by key bytes.
    pub(crate) fn content(&self) -> &'static [&'static str] {
        self.content
    }

    /// The overlay row index of content key `key`.
    pub(crate) fn content_row(&self, key: &str) -> Option<u32> {
        self.content
            .binary_search_by(|k| k.as_bytes().cmp(key.as_bytes()))
            .ok()
            .and_then(|i| u32::try_from(i).ok())
    }

    /// Where the catalog comes from: `<crate name>/<dir as written>`, such
    /// as `my-bot/locales`.
    pub const fn origin(&self) -> &'static str {
        self.origin
    }

    /// The default locale, whose file defines the content keys.
    pub const fn default_locale(&self) -> Locale {
        self.default
    }

    /// The locales that have a file, in [`Locale::ALL`] order.
    pub fn locales(&self) -> impl Iterator<Item = Locale> + '_ {
        self.files.iter().map(|(locale, _)| *locale)
    }

    /// The text `key` has in `locale`'s own file, without fallback.
    pub fn get(&self, locale: Locale, key: &str) -> Option<&'static str> {
        let entries = self.file(locale)?;
        entries
            .binary_search_by(|(k, _)| k.as_bytes().cmp(key.as_bytes()))
            .ok()
            .map(|i| entries[i].1)
    }

    /// Every key of every file, sorted and deduplicated.
    pub fn keys(&self) -> impl Iterator<Item = &'static str> + use<> {
        let mut keys: Vec<&'static str> = self
            .files
            .iter()
            .flat_map(|(_, entries)| entries.iter().map(|(k, _)| *k))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
    }

    /// The placeholder names of a content key (in parameter order,
    /// positional ones as `"0"`, `"1"`, …) or of an overridden `noctis.*`
    /// key; `None` for any other key.
    pub fn placeholders(&self, key: &str) -> Option<&'static [&'static str]> {
        self.placeholders
            .binary_search_by(|(k, _)| k.as_bytes().cmp(key.as_bytes()))
            .ok()
            .map(|i| self.placeholders[i].1)
    }

    fn file(&self, locale: Locale) -> Option<Entries> {
        self.files
            .iter()
            .find(|(l, _)| *l == locale)
            .map(|(_, entries)| *entries)
    }

    /// The template of `key` along the fallback chain: the overlay row of
    /// `locale` for a content key once overlays apply, else the locale's
    /// file, its sibling's, then the default locale's.
    fn template(&self, locale: Locale, key: &str) -> Option<&'static str> {
        if let Some(text) = self.content_row(key).and_then(|i| {
            self.__overlay_row(locale)
                .and_then(|row| row.get(i as usize).copied())
        }) {
            return Some(text);
        }
        self.get(locale, key)
            .or_else(|| locale.sibling().and_then(|s| self.get(s, key)))
            .or_else(|| self.get(self.default, key))
    }
}

impl Translate for Catalog {
    fn translate(
        &self,
        locale: Locale,
        key: MessageKey,
        args: &[(&str, &dyn Display)],
    ) -> Option<String> {
        self.template(locale, key.as_str())
            .map(|template| format(template, args))
    }
}

/// A key that two catalogs define.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateKey {
    /// The key.
    pub key: &'static str,
    /// The [`Catalog::origin`] of the catalog registered first.
    pub first: &'static str,
    /// The [`Catalog::origin`] of the other catalog.
    pub second: &'static str,
}

/// The App's strings: its catalogs, then [`Builtin`].
///
/// A key resolves in the one catalog that defines it (in any of its files),
/// with that catalog's fallback chain; when that gives nothing (an unknown
/// key, or a `noctis.*` override missing in this locale's chain) the
/// [`Builtin`] text is used.
///
/// ```
/// use noctis_i18n::{AppStrings, Catalog, keys};
/// use noctis_rules::{Locale, Translate};
///
/// static BOT: Catalog = Catalog::__new(
///     "bot/locales",
///     Locale::EnUs,
///     &[(Locale::EnUs, &[]), (Locale::Tr, &[("noctis.expired", "Süresi doldu")])],
///     &[("noctis.expired", &[])],
/// );
///
/// let strings = AppStrings::new(&[BOT]).unwrap();
/// assert_eq!(strings.translate(Locale::Tr, keys::EXPIRED, &[]).as_deref(), Some("Süresi doldu"));
/// assert_eq!(
///     strings.translate(Locale::De, keys::EXPIRED, &[]).as_deref(),
///     Some("This interaction has expired. Run the command again."),
/// );
/// ```
#[derive(Clone, Debug, Default)]
pub struct AppStrings {
    catalogs: Vec<Catalog>,
    owners: HashMap<&'static str, usize>,
}

impl AppStrings {
    /// Layers `catalogs` over [`Builtin`]. Catalogs with an equal
    /// [`Catalog::origin`] count once.
    ///
    /// # Errors
    ///
    /// Every key that two distinct catalogs define, in registration order
    /// and then key order.
    pub fn new(catalogs: &[Catalog]) -> Result<Self, Vec<DuplicateKey>> {
        let mut unique: Vec<Catalog> = Vec::with_capacity(catalogs.len());
        for catalog in catalogs {
            if !unique.iter().any(|c| c.origin == catalog.origin) {
                unique.push(*catalog);
            }
        }
        let mut owners: HashMap<&'static str, usize> = HashMap::new();
        let mut duplicates = Vec::new();
        for (index, catalog) in unique.iter().enumerate() {
            for key in catalog.keys() {
                match owners.get(key) {
                    Some(&first) => duplicates.push(DuplicateKey {
                        key,
                        first: unique[first].origin,
                        second: catalog.origin,
                    }),
                    None => {
                        owners.insert(key, index);
                    }
                }
            }
        }
        if duplicates.is_empty() {
            Ok(Self {
                catalogs: unique,
                owners,
            })
        } else {
            Err(duplicates)
        }
    }

    /// The catalogs, deduplicated, in registration order.
    pub fn catalogs(&self) -> &[Catalog] {
        &self.catalogs
    }

    /// The catalog that defines `key` (in any of its files), if any.
    ///
    /// ```
    /// use noctis_i18n::{AppStrings, Catalog};
    /// use noctis_rules::Locale;
    ///
    /// static BOT: Catalog = Catalog::__new(
    ///     "bot/locales",
    ///     Locale::EnUs,
    ///     &[(Locale::Tr, &[("cmdword.ban", "yasakla")])],
    ///     &[],
    /// );
    ///
    /// let strings = AppStrings::new(&[BOT]).unwrap();
    /// assert_eq!(strings.owner("cmdword.ban").map(Catalog::origin), Some("bot/locales"));
    /// assert!(strings.owner("cmdword.kick").is_none());
    /// ```
    pub fn owner(&self, key: &str) -> Option<&Catalog> {
        self.owners.get(key).map(|&i| &self.catalogs[i])
    }

    /// The text `key` has in `locale`'s own file of the catalog that
    /// defines it, without fallback (neither the sibling, the default
    /// locale nor [`Builtin`]).
    ///
    /// ```
    /// use noctis_i18n::{AppStrings, Catalog};
    /// use noctis_rules::Locale;
    ///
    /// static BOT: Catalog = Catalog::__new(
    ///     "bot/locales",
    ///     Locale::EnUs,
    ///     &[
    ///         (Locale::EnUs, &[("cmd.ban.description", "Ban a member")]),
    ///         (Locale::Tr, &[("cmdword.ban", "yasakla")]),
    ///     ],
    ///     &[],
    /// );
    ///
    /// let strings = AppStrings::new(&[BOT]).unwrap();
    /// assert_eq!(strings.get(Locale::Tr, "cmdword.ban"), Some("yasakla"));
    /// assert_eq!(strings.get(Locale::EnGb, "cmd.ban.description"), None);
    /// ```
    pub fn get(&self, locale: Locale, key: &str) -> Option<&'static str> {
        self.owner(key)?.get(locale, key)
    }

    /// Validates the loaded runtime `overlays` (one `(locale, pairs)` per
    /// loaded locale, each pair `(key, value)`) and installs them into
    /// their owner catalogs' process-wide slots (Story 11.5).
    ///
    /// A pair is checked in this order, the first match wins: a key twice
    /// in one locale ([`OverlayViolation::DuplicateKey`]), a reserved
    /// family `cmd`, `cmdword`, `ctxcmd` or `noctis` as first segment
    /// ([`OverlayViolation::ReservedKey`]), not a content key of an
    /// overlay-taking catalog ([`OverlayViolation::UnknownKey`]), a
    /// placeholder set other than the compiled one
    /// ([`OverlayViolation::Placeholders`]). Nothing is installed unless
    /// every pair passes. A catalog whose slot already holds the same
    /// values is left as it is; one holding other values is
    /// [`OverlayViolation::AlreadyInstalled`].
    ///
    /// # Errors
    ///
    /// Every violation, sorted by [`Locale::ALL`] position, then key bytes,
    /// then the `AlreadyInstalled` ones in catalog registration order.
    #[doc(hidden)]
    pub fn install_overlays(
        &self,
        overlays: &[(Locale, Vec<(String, String)>)],
    ) -> Result<(), Vec<OverlayViolation>> {
        let valid = overlay::validate(overlays, |key| {
            self.owners.get(key).map(|&i| (i, &self.catalogs[i]))
        })?;
        let mut groups: Vec<Vec<overlay::Entry>> = vec![Vec::new(); self.catalogs.len()];
        for (index, entry) in valid {
            groups[index].push(entry);
        }
        let mut installs = Vec::new();
        let mut taken = Vec::new();
        for (catalog, mut group) in self.catalogs.iter().zip(groups) {
            // Only a catalog with an overlay slot owns validated content keys.
            let Some(slot) = catalog.overlay else {
                continue;
            };
            if group.is_empty() {
                continue;
            }
            group.sort_by_key(|(l, i, _)| (*l as usize, *i));
            match slot.entries() {
                Some(existing) if existing == group.as_slice() => {}
                Some(_) => taken.push(OverlayViolation::AlreadyInstalled {
                    origin: catalog.origin,
                }),
                None => installs.push((catalog, slot, group)),
            }
        }
        if !taken.is_empty() {
            return Err(taken);
        }
        for (catalog, slot, group) in installs {
            slot.install(catalog, group.into_boxed_slice());
        }
        Ok(())
    }
}

impl Translate for AppStrings {
    fn translate(
        &self,
        locale: Locale,
        key: MessageKey,
        args: &[(&str, &dyn Display)],
    ) -> Option<String> {
        self.owners
            .get(key.as_str())
            .and_then(|&i| self.catalogs[i].translate(locale, key, args))
            .or_else(|| Builtin.translate(locale, key, args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;

    fn d(value: &dyn Display) -> &dyn Display {
        value
    }

    static BASIC: Catalog = Catalog::__new(
        "bot/locales",
        Locale::EnUs,
        &[
            (
                Locale::EnUs,
                &[
                    ("cmd.setup.description", "Set {x} up"),
                    ("common.refresh", "Refresh"),
                    ("shop.left", "{n} left, {n}!"),
                ],
            ),
            (Locale::Tr, &[("common.refresh", "Yenile")]),
        ],
        &[("common.refresh", &[]), ("shop.left", &["n"])],
    );

    /// Matrix 58.
    #[test]
    fn catalog_lookups() {
        assert_eq!(BASIC.origin(), "bot/locales");
        assert_eq!(BASIC.default_locale(), Locale::EnUs);
        assert_eq!(
            BASIC.locales().collect::<Vec<_>>(),
            [Locale::EnUs, Locale::Tr]
        );
        assert_eq!(BASIC.get(Locale::Tr, "common.refresh"), Some("Yenile"));
        assert_eq!(BASIC.get(Locale::Tr, "shop.left"), None);
        assert_eq!(BASIC.get(Locale::De, "common.refresh"), None);
        assert_eq!(
            BASIC.keys().collect::<Vec<_>>(),
            ["cmd.setup.description", "common.refresh", "shop.left"]
        );
        assert_eq!(BASIC.placeholders("shop.left"), Some(&["n"][..]));
        assert_eq!(BASIC.placeholders("common.refresh"), Some(&[][..]));
        assert_eq!(BASIC.placeholders("cmd.setup.description"), None);
        assert_eq!(BASIC.placeholders("nope"), None);
    }

    /// Matrix 59: the chain is the locale, its sibling, then the default.
    #[test]
    fn catalog_translate_falls_back() {
        static SIBLING: Catalog = Catalog::__new(
            "s/l",
            Locale::EnUs,
            &[
                (Locale::EnUs, &[("a", "us")]),
                (Locale::EsEs, &[]),
                (Locale::Es419, &[("a", "419")]),
            ],
            &[("a", &[])],
        );
        let left = MessageKey::new("shop.left");
        assert_eq!(
            BASIC
                .translate(Locale::EnGb, left, &[("n", d(&2))])
                .as_deref(),
            Some("2 left, 2!")
        );
        assert_eq!(
            BASIC
                .translate(Locale::Tr, MessageKey::new("common.refresh"), &[])
                .as_deref(),
            Some("Yenile")
        );
        assert_eq!(
            BASIC.translate(Locale::Tr, MessageKey::new("shop.unknown"), &[]),
            None
        );
        let a = MessageKey::new("a");
        assert_eq!(
            SIBLING.translate(Locale::EsEs, a, &[]).as_deref(),
            Some("419")
        );
        assert_eq!(
            SIBLING.translate(Locale::PtBr, a, &[]).as_deref(),
            Some("us")
        );
    }

    /// Matrix 60.
    #[test]
    fn app_strings_layers_over_builtin() {
        let strings = AppStrings::new(&[BASIC]).unwrap_or_default();
        assert_eq!(
            strings
                .translate(Locale::Tr, MessageKey::new("common.refresh"), &[])
                .as_deref(),
            Some("Yenile")
        );
        assert_eq!(
            strings.translate(Locale::Tr, keys::EXPIRED, &[]),
            Builtin.translate(Locale::Tr, keys::EXPIRED, &[])
        );
        assert_eq!(
            strings.translate(Locale::Tr, MessageKey::new("nope.x"), &[]),
            None
        );
        let empty = AppStrings::new(&[]).unwrap_or_default();
        for key in keys::ALL {
            assert_eq!(
                empty.translate(Locale::Ja, key, &[]),
                Builtin.translate(Locale::Ja, key, &[])
            );
        }
    }

    /// Matrix 61, 62.
    #[test]
    fn duplicate_keys_and_idempotent_registration() {
        static OTHER: Catalog = Catalog::__new(
            "other/locales",
            Locale::EnUs,
            &[(Locale::Tr, &[("common.refresh", "x"), ("z.z", "z")])],
            &[],
        );
        assert_eq!(
            AppStrings::new(&[BASIC, OTHER]).err(),
            Some(vec![DuplicateKey {
                key: "common.refresh",
                first: "bot/locales",
                second: "other/locales",
            }])
        );
        let twice = AppStrings::new(&[BASIC, BASIC]).unwrap_or_default();
        assert_eq!(twice.catalogs().len(), 1);
    }

    /// Story 11.4: `get` and `owner` look in the owner catalog's own file.
    #[test]
    fn app_strings_get_and_owner() {
        static WORDS: Catalog = Catalog::__new(
            "words/locales",
            Locale::EnUs,
            &[(Locale::Tr, &[("cmdword.ban", "yasakla")])],
            &[],
        );
        let strings = AppStrings::new(&[BASIC, WORDS]).unwrap_or_default();
        assert_eq!(
            strings.owner("cmdword.ban").map(Catalog::origin),
            Some("words/locales")
        );
        assert_eq!(
            strings.owner("common.refresh").map(Catalog::origin),
            Some("bot/locales")
        );
        assert!(strings.owner("nope").is_none());
        assert_eq!(strings.get(Locale::Tr, "cmdword.ban"), Some("yasakla"));
        assert_eq!(strings.get(Locale::EnUs, "cmdword.ban"), None);
        assert_eq!(strings.get(Locale::Tr, "common.refresh"), Some("Yenile"));
        // No fallback to the sibling or the default locale.
        assert_eq!(strings.get(Locale::EnGb, "cmd.setup.description"), None);
        assert_eq!(strings.get(Locale::De, "common.refresh"), None);
        assert_eq!(strings.get(Locale::Tr, "noctis.expired"), None);
    }

    /// `FRAMEWORK_KEYS` lists `keys::ALL` with each `Builtin` template's
    /// placeholders.
    #[test]
    fn framework_keys_match_builtin() {
        let listed: Vec<&str> = noctis_rules::FRAMEWORK_KEYS
            .iter()
            .map(|(k, _)| *k)
            .collect();
        let all: Vec<&str> = keys::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(listed, all);
        for (key, names) in noctis_rules::FRAMEWORK_KEYS {
            let template = Builtin::template(Locale::EnUs, MessageKey::new(key)).unwrap_or("");
            let found: Vec<&str> = noctis_rules::placeholder_names(template).collect();
            assert_eq!(found, *names, "{key}");
        }
    }

    // Story 11.5: runtime overlays. Every installing test owns its catalog
    // (and so its process-wide slot): tests run in parallel.

    const C_EN_US: Entries = &[
        ("cmd.greet.description", "Greet"),
        ("greet.hello", "Hello"),
        ("pos.x", "{0}/{1}"),
        ("shop.left", "{n} left"),
    ];
    const C_TR: Entries = &[("greet.hello", "Merhaba")];
    const C_FILES: &[(Locale, Entries)] = &[(Locale::EnUs, C_EN_US), (Locale::Tr, C_TR)];
    const C_PLACEHOLDERS: &[(&str, &[&str])] = &[
        ("greet.hello", &[]),
        ("pos.x", &["0", "1"]),
        ("shop.left", &["n"]),
    ];
    const C_CONTENT: &[&str] = &["greet.hello", "pos.x", "shop.left"];

    /// Defines a static copy of the Matrix's catalog `C` with its own slot.
    macro_rules! catalog_c {
        ($name:ident) => {
            catalog_c!($name, C_FILES);
        };
        ($name:ident, $files:expr) => {
            static $name: Catalog = {
                static SLOT: OverlaySlot = OverlaySlot::new();
                Catalog::__new("bot/locales", Locale::EnUs, $files, C_PLACEHOLDERS)
                    .__with_overlay(&SLOT, C_CONTENT)
            };
        };
    }

    fn ov(locale: Locale, pairs: &[(&str, &str)]) -> (Locale, Vec<(String, String)>) {
        (
            locale,
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    fn text(strings: &AppStrings, locale: Locale, key: &'static str) -> Option<String> {
        strings.translate(
            locale,
            MessageKey::new(key),
            &[("n", d(&3)), ("0", d(&"a")), ("1", d(&"b"))],
        )
    }

    fn strings(catalogs: &[Catalog]) -> AppStrings {
        AppStrings::new(catalogs).unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// The validation error of `pairs` in `locale` on a fresh catalog `C`
    /// that installs nothing.
    fn rejected(catalog: Catalog, locale: Locale, pairs: &[(&str, &str)]) -> Vec<OverlayViolation> {
        let app = strings(&[catalog]);
        let err = app
            .install_overlays(&[ov(locale, pairs)])
            .expect_err("the overlay is invalid");
        assert_eq!(catalog.__overlay_row(locale), None, "nothing is installed");
        err
    }

    fn placeholders(key: &str, expected: &[&str], found: &[&str]) -> OverlayViolation {
        OverlayViolation::Placeholders {
            locale: Locale::Tr,
            key: key.to_owned(),
            expected: expected.iter().map(|n| (*n).to_owned()).collect(),
            found: found.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    fn reserved(locale: Locale, key: &str) -> OverlayViolation {
        OverlayViolation::ReservedKey {
            locale,
            key: key.to_owned(),
        }
    }

    fn unknown(locale: Locale, key: &str) -> OverlayViolation {
        OverlayViolation::UnknownKey {
            locale,
            key: key.to_owned(),
        }
    }

    /// Story 11.5 Matrix 1
    #[test]
    fn overlay_fixes_a_translation() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("greet.hello", "Selam")])]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("Selam")
        );
        assert_eq!(
            text(&app, Locale::EnUs, "greet.hello").as_deref(),
            Some("Hello")
        );
        assert_eq!(
            C.__overlay_row(Locale::Tr),
            Some(&["Selam", "{0}/{1}", "{n} left"][..])
        );
        // en-US's chain (en-US, en-GB, en-US) holds no overlay.
        assert_eq!(C.__overlay_row(Locale::EnUs), None);
    }

    /// Story 11.5 Matrix 2
    #[test]
    fn overlay_adds_a_missing_translation() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("shop.left", "{n} kaldı")])]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::Tr, "shop.left").as_deref(),
            Some("3 kaldı")
        );
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("Merhaba")
        );
        assert_eq!(C.__overlay_row(Locale::Tr).map(|r| r[2]), Some("{n} kaldı"));
    }

    /// Story 11.5 Matrix 3
    #[test]
    fn default_locale_overlay_reaches_fallbacks_but_not_own_files() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::EnUs, &[("greet.hello", "Hi")])]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::EnUs, "greet.hello").as_deref(),
            Some("Hi")
        );
        assert_eq!(text(&app, Locale::De, "greet.hello").as_deref(), Some("Hi"));
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("Merhaba")
        );
        assert_eq!(C.__overlay_row(Locale::De).map(|r| r[0]), Some("Hi"));
        assert_eq!(C.__overlay_row(Locale::Tr).map(|r| r[0]), Some("Merhaba"));
    }

    /// Story 11.5 Matrix 4
    #[test]
    fn sibling_chain_reaches_the_overlay() {
        catalog_c!(
            C,
            &[
                (Locale::EnUs, C_EN_US),
                (Locale::EnGb, &[]),
                (Locale::Tr, C_TR)
            ]
        );
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::EnUs, &[("greet.hello", "Hi")])]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::EnGb, "greet.hello").as_deref(),
            Some("Hi")
        );
        assert_eq!(C.__overlay_row(Locale::EnGb).map(|r| r[0]), Some("Hi"));
    }

    /// Story 11.5 Matrix 5
    #[test]
    fn overlay_for_a_locale_without_a_file_in_its_owner() {
        catalog_c!(C);
        static D: Catalog = Catalog::__new(
            "other/locales",
            Locale::EnUs,
            &[
                (Locale::EnUs, &[("d.x", "x")]),
                (Locale::De, &[("d.x", "X")]),
            ],
            &[("d.x", &[])],
        );
        let app = strings(&[C, D]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::De, &[("greet.hello", "Hallo")])]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::De, "greet.hello").as_deref(),
            Some("Hallo")
        );
        assert_eq!(text(&app, Locale::De, "d.x").as_deref(), Some("X"));
    }

    /// Story 11.5 Matrix 9
    #[test]
    fn empty_value_is_accepted() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("greet.hello", "")])]),
            Ok(())
        );
        assert_eq!(text(&app, Locale::Tr, "greet.hello").as_deref(), Some(""));
    }

    /// Story 11.5 Matrix 10
    #[test]
    fn unicode_and_long_values_are_accepted_verbatim() {
        catalog_c!(C);
        let long = "ç".repeat(10_000);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[
                ov(Locale::Tr, &[("greet.hello", "😀 Çağ")]),
                ov(Locale::EnUs, &[("greet.hello", &long)]),
            ]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("😀 Çağ")
        );
        assert_eq!(text(&app, Locale::EnUs, "greet.hello"), Some(long));
    }

    /// Story 11.5 Matrix 11
    #[test]
    fn repeated_placeholder_is_accepted() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("shop.left", "{n}-{n}")])]),
            Ok(())
        );
        assert_eq!(
            app.translate(Locale::Tr, MessageKey::new("shop.left"), &[("n", d(&2))])
                .as_deref(),
            Some("2-2")
        );
    }

    /// Story 11.5 Matrix 12
    #[test]
    fn positional_placeholders() {
        catalog_c!(OK);
        let app = strings(&[OK]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("pos.x", "{1} / {0}")])]),
            Ok(())
        );
        assert_eq!(text(&app, Locale::Tr, "pos.x").as_deref(), Some("b / a"));
        catalog_c!(BAD);
        let cases: [(&str, &[&str]); 3] = [
            ("{0}", &["0"]),
            ("{00}/{1}", &["00", "1"]),
            ("{0}/{1}/{2}", &["0", "1", "2"]),
        ];
        for (value, found) in cases {
            assert_eq!(
                rejected(BAD, Locale::Tr, &[("pos.x", value)]),
                [placeholders("pos.x", &["0", "1"], found)],
                "{value}"
            );
        }
        assert_eq!(
            placeholders("pos.x", &["0", "1"], &["00", "1"]).to_string(),
            "locale `tr`: overlay of `pos.x` must use the placeholders `{0}`, `{1}`; missing `{0}`, extra `{00}`"
        );
    }

    /// Story 11.5 Matrix 13
    #[test]
    fn missing_placeholder_is_rejected() {
        catalog_c!(C);
        let err = rejected(C, Locale::Tr, &[("shop.left", "kaldı")]);
        assert_eq!(err, [placeholders("shop.left", &["n"], &[])]);
        assert_eq!(
            err[0].to_string(),
            "locale `tr`: overlay of `shop.left` must use the placeholders `{n}`; missing `{n}`"
        );
    }

    /// Story 11.5 Matrix 14
    #[test]
    fn extra_or_renamed_placeholder_is_rejected() {
        catalog_c!(C);
        let cases: [(&str, &str, &[&str], &[&str]); 3] = [
            ("greet.hello", "Hi {name}", &[], &["name"]),
            ("shop.left", "{count}", &["n"], &["count"]),
            ("shop.left", "{N}", &["n"], &["N"]),
        ];
        for (key, value, expected, found) in cases {
            assert_eq!(
                rejected(C, Locale::Tr, &[(key, value)]),
                [placeholders(key, expected, found)],
                "{value}"
            );
        }
        assert_eq!(
            placeholders("greet.hello", &[], &["name"]).to_string(),
            "locale `tr`: overlay of `greet.hello` must use the placeholders (none); extra `{name}`"
        );
        assert_eq!(
            placeholders("shop.left", &["n"], &["N"]).to_string(),
            "locale `tr`: overlay of `shop.left` must use the placeholders `{n}`; missing `{n}`, extra `{N}`"
        );
    }

    /// Story 11.5 Matrix 15
    #[test]
    fn literal_braces_are_accepted() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("greet.hello", "a {} {a b} {x")])]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("a {} {a b} {x")
        );
    }

    /// Story 11.5 Matrix 16
    #[test]
    fn reserved_families_are_rejected() {
        catalog_c!(C);
        let keys = [
            "cmd.greet.description",
            "cmdword.greet",
            "ctxcmd.k.name",
            "noctis.expired",
            "cmd",
            "noctis",
        ];
        let pairs: Vec<(&str, &str)> = keys.iter().map(|k| (*k, "x")).collect();
        let mut sorted = keys;
        sorted.sort_unstable();
        let expected: Vec<OverlayViolation> =
            sorted.iter().map(|k| reserved(Locale::Tr, k)).collect();
        assert_eq!(rejected(C, Locale::Tr, &pairs), expected);
    }

    /// Story 11.5 Matrix 17, 18
    #[test]
    fn unknown_keys_are_rejected() {
        catalog_c!(C);
        let mut keys = [
            "cmdx.y",
            "CMD.greet",
            "shop.titel",
            "",
            " greet.hello",
            "greet",
            "greet.hello.x",
            "çay.x",
        ];
        let pairs: Vec<(&str, &str)> = keys.iter().map(|k| (*k, "x")).collect();
        keys.sort_unstable();
        let expected: Vec<OverlayViolation> = keys.iter().map(|k| unknown(Locale::Tr, k)).collect();
        assert_eq!(rejected(C, Locale::Tr, &pairs), expected);
    }

    /// Story 11.5 Matrix 19
    #[test]
    fn catalog_without_slot_takes_no_overlays() {
        static PLAIN: Catalog =
            Catalog::__new("bot/locales", Locale::EnUs, C_FILES, C_PLACEHOLDERS);
        assert_eq!(
            rejected(PLAIN, Locale::Tr, &[("greet.hello", "Selam")]),
            [unknown(Locale::Tr, "greet.hello")]
        );
    }

    /// Story 11.5 Matrix 20
    #[test]
    fn duplicate_key_in_one_locale() {
        catalog_c!(C);
        for pairs in [
            &[("greet.hello", "a"), ("greet.hello", "a")][..],
            &[("greet.hello", "a"), ("greet.hello", "b")],
            &[
                ("shop.left", "{n}"),
                ("shop.left", "bad"),
                ("shop.left", "{x}"),
            ],
        ] {
            let key = pairs[0].0;
            assert_eq!(
                rejected(C, Locale::Tr, pairs),
                [OverlayViolation::DuplicateKey {
                    locale: Locale::Tr,
                    key: key.to_owned()
                }]
            );
        }
    }

    /// Story 11.5 Matrix 21
    #[test]
    fn same_key_in_two_locales() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(
            app.install_overlays(&[
                ov(Locale::EnUs, &[("greet.hello", "Hi")]),
                ov(Locale::Tr, &[("greet.hello", "Selam")]),
            ]),
            Ok(())
        );
        assert_eq!(
            text(&app, Locale::EnUs, "greet.hello").as_deref(),
            Some("Hi")
        );
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("Selam")
        );
        assert_eq!(text(&app, Locale::Ja, "greet.hello").as_deref(), Some("Hi"));
    }

    /// Story 11.5 Matrix 22
    #[test]
    fn every_problem_is_reported_sorted_and_nothing_installs() {
        catalog_c!(C);
        let app = strings(&[C]);
        let err = app
            .install_overlays(&[
                ov(
                    Locale::Tr,
                    &[
                        ("shop.left", "x"),
                        ("greet.hello", "Selam"),
                        ("cmd.greet.description", "x"),
                    ],
                ),
                ov(Locale::EnUs, &[("zz.nope", "x")]),
            ])
            .expect_err("invalid");
        assert_eq!(
            err,
            [
                unknown(Locale::EnUs, "zz.nope"),
                reserved(Locale::Tr, "cmd.greet.description"),
                placeholders("shop.left", &["n"], &[]),
            ]
        );
        assert_eq!(
            text(&app, Locale::Tr, "greet.hello").as_deref(),
            Some("Merhaba")
        );
        assert_eq!(C.__overlay_row(Locale::Tr), None);
    }

    /// Story 11.5 Matrix 24, 25, 26
    #[test]
    fn overlays_install_once_per_process() {
        catalog_c!(C);
        let first = strings(&[C]);
        let load = [ov(
            Locale::Tr,
            &[("greet.hello", "Selam"), ("shop.left", "{n} kaldı")],
        )];
        assert_eq!(first.install_overlays(&load), Ok(()));
        // Matrix 24: equal overlays, in any order.
        let second = strings(&[C]);
        let reordered = [ov(
            Locale::Tr,
            &[("shop.left", "{n} kaldı"), ("greet.hello", "Selam")],
        )];
        assert_eq!(second.install_overlays(&reordered), Ok(()));
        // Matrix 25: different overlays.
        assert_eq!(
            second.install_overlays(&[ov(Locale::Tr, &[("greet.hello", "Merhabalar")])]),
            Err(vec![OverlayViolation::AlreadyInstalled {
                origin: "bot/locales"
            }])
        );
        assert_eq!(
            text(&second, Locale::Tr, "greet.hello").as_deref(),
            Some("Selam")
        );
        // Matrix 26: no overlays leaves the slot alone, and it is process-wide.
        let third = strings(&[C]);
        assert_eq!(
            third.install_overlays(&[ov(Locale::Tr, &[]), ov(Locale::EnUs, &[])]),
            Ok(())
        );
        assert_eq!(
            text(&third, Locale::Tr, "greet.hello").as_deref(),
            Some("Selam")
        );
    }

    /// Story 11.5 Matrix 28, 29
    #[test]
    fn get_stays_compiled_and_rows_appear_after_install() {
        catalog_c!(C);
        let app = strings(&[C]);
        assert_eq!(C.__overlay_row(Locale::Tr), None);
        assert_eq!(
            app.install_overlays(&[ov(Locale::Tr, &[("greet.hello", "Selam")])]),
            Ok(())
        );
        assert!(C.__overlay_row(Locale::Tr).is_some());
        assert_eq!(C.get(Locale::Tr, "greet.hello"), Some("Merhaba"));
        assert_eq!(app.get(Locale::Tr, "greet.hello"), Some("Merhaba"));
        assert_eq!(
            app.get(Locale::EnUs, "cmd.greet.description"),
            Some("Greet")
        );
        // Keys outside the content list keep the compiled chain.
        assert_eq!(
            text(&app, Locale::Tr, "cmd.greet.description").as_deref(),
            Some("Greet")
        );
    }
}
