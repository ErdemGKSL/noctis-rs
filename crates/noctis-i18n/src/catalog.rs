//! [`Catalog`]: one `i18n!` invocation's compiled locale files, and
//! [`AppStrings`]: the App's catalogs layered over [`Builtin`].

use std::collections::HashMap;
use std::fmt::Display;

use noctis_rules::{Locale, MessageKey, Translate};

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
        }
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

    /// The template of `key` along the fallback chain.
    fn template(&self, locale: Locale, key: &str) -> Option<&'static str> {
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
}
