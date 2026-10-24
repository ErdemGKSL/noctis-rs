//! Runtime locale overlays (Story 11.5): [`OverlaySlot`], the process-wide
//! storage of one `i18n!` catalog's overlays, and [`OverlayViolation`], why
//! a loaded overlay was rejected.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use noctis_rules::Locale;

use crate::catalog::Catalog;

/// The first dot-segments that can never be overlaid: the published
/// command texts and the framework notices.
const RESERVED: [&str; 4] = ["cmd", "cmdword", "ctxcmd", "noctis"];

/// One installed overlay value: `(locale, content row index, value)`.
pub(crate) type Entry = (Locale, u32, Box<str>);

/// The per-locale rows of one catalog: one per [`Locale::ALL`] position,
/// `None` where the compiled row is kept.
type Rows = Box<[Option<Box<[&'static str]>>]>;

/// The process-wide overlay storage of one catalog.
///
/// `i18n!` emits one `static` slot per invocation and links it to the
/// catalog with `Catalog::__with_overlay`; the App fills it once at
/// startup through `AppStrings::install_overlays`. The slot owns the
/// overlay strings, and the per-locale rows borrow them (the slot is a
/// `static`, so the borrows are `'static`).
#[doc(hidden)]
pub struct OverlaySlot {
    /// The installed values, sorted by locale position, then row index.
    entries: OnceLock<Box<[Entry]>>,
    /// One row per [`Locale::ALL`] position, built from `entries` and the
    /// compiled files; `None` keeps the compiled row.
    rows: OnceLock<Rows>,
}

impl OverlaySlot {
    /// An empty slot.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: OnceLock::new(),
            rows: OnceLock::new(),
        }
    }

    /// The installed values, if any.
    pub(crate) fn entries(&self) -> Option<&[Entry]> {
        self.entries.get().map(|e| &**e)
    }

    /// The row of `locale`, once rows are built.
    pub(crate) fn row(&self, locale: Locale) -> Option<&[&'static str]> {
        self.rows.get()?.get(locale as usize)?.as_deref()
    }

    /// Installs `entries` (sorted, unique) and builds the rows of
    /// `catalog`. A slot that already holds entries keeps them.
    pub(crate) fn install(&'static self, catalog: &Catalog, entries: Box<[Entry]>) {
        let entries: &'static [Entry] = self.entries.get_or_init(|| entries);
        self.rows.get_or_init(|| build_rows(catalog, entries));
    }
}

impl Default for OverlaySlot {
    fn default() -> Self {
        Self::new()
    }
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "the spec fixes the Debug output to `installed: bool`; the row cache is derived state"
)]
impl fmt::Debug for OverlaySlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlaySlot")
            .field("installed", &self.entries.get().is_some())
            .finish()
    }
}

/// The overlay value of `(locale, row)` in `entries`.
fn overlay(entries: &'static [Entry], locale: Locale, row: u32) -> Option<&'static str> {
    entries
        .binary_search_by(|(l, i, _)| (*l as usize, *i).cmp(&(locale as usize, row)))
        .ok()
        .map(|i| &*entries[i].2)
}

/// The rows of every locale: per content key, overlay(L), file(L),
/// overlay(sibling), file(sibling), overlay(default), file(default). A
/// locale whose chain holds no overlay gets `None`.
fn build_rows(catalog: &Catalog, entries: &'static [Entry]) -> Rows {
    let default = catalog.default_locale();
    let has = |locale: Locale| entries.iter().any(|(l, _, _)| *l == locale);
    Locale::ALL
        .iter()
        .map(|&locale| {
            let sibling = locale.sibling();
            let touched = has(locale) || sibling.is_some_and(has) || has(default);
            touched.then(|| {
                (0..)
                    .zip(catalog.content())
                    .map(|(row, key)| {
                        overlay(entries, locale, row)
                            .or_else(|| catalog.get(locale, key))
                            .or_else(|| sibling.and_then(|s| overlay(entries, s, row)))
                            .or_else(|| sibling.and_then(|s| catalog.get(s, key)))
                            .or_else(|| overlay(entries, default, row))
                            .or_else(|| catalog.get(default, key))
                            .unwrap_or("")
                    })
                    .collect()
            })
        })
        .collect()
}

/// Why a loaded locale overlay was rejected at startup (Story 11.5).
///
/// ```
/// use noctis_i18n::OverlayViolation;
/// use noctis_rules::Locale;
///
/// let v = OverlayViolation::Placeholders {
///     locale: Locale::Tr,
///     key: "shop.left".into(),
///     expected: vec!["n".into()],
///     found: vec!["count".into()],
/// };
/// assert_eq!(
///     v.to_string(),
///     "locale `tr`: overlay of `shop.left` must use the placeholders `{n}`; missing `{n}`, \
///      extra `{count}`",
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverlayViolation {
    /// The same key appears more than once in one locale's overlay.
    DuplicateKey {
        /// The overlay's locale.
        locale: Locale,
        /// The repeated key.
        key: String,
    },
    /// The key belongs to a reserved family (`cmd.*`, `cmdword.*`,
    /// `ctxcmd.*`, `noctis.*`): published command texts and framework
    /// notices change only with a redeploy.
    ReservedKey {
        /// The overlay's locale.
        locale: Locale,
        /// The reserved key.
        key: String,
    },
    /// The key is not a content key of any merged catalog (or its catalog
    /// takes no overlays).
    UnknownKey {
        /// The overlay's locale.
        locale: Locale,
        /// The unknown key.
        key: String,
    },
    /// The value's placeholder set differs from the compiled one.
    Placeholders {
        /// The overlay's locale.
        locale: Locale,
        /// The content key.
        key: String,
        /// The compiled placeholder names, in parameter order.
        expected: Vec<String>,
        /// The value's placeholder names, unique, in first-appearance order.
        found: Vec<String>,
    },
    /// The catalog's process-wide slot already holds different overlays
    /// (overlays install once per process).
    AlreadyInstalled {
        /// The catalog's `origin()`.
        origin: &'static str,
    },
}

/// `` `{a}`, `{b}` ``, or `(none)`.
fn braces<'a>(f: &mut fmt::Formatter<'_>, names: impl IntoIterator<Item = &'a str>) -> fmt::Result {
    let mut any = false;
    for name in names {
        if any {
            f.write_str(", ")?;
        }
        write!(f, "`{{{name}}}`")?;
        any = true;
    }
    if !any {
        f.write_str("(none)")?;
    }
    Ok(())
}

impl fmt::Display for OverlayViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey { locale, key } => {
                write!(
                    f,
                    "locale `{locale}`: overlay key `{key}` appears more than once"
                )
            }
            Self::ReservedKey { locale, key } => write!(
                f,
                "locale `{locale}`: overlay key `{key}` is reserved: `cmd.*`, `cmdword.*`, \
                 `ctxcmd.*` and `noctis.*` cannot be overlaid (published command texts change \
                 only with a redeploy)"
            ),
            Self::UnknownKey { locale, key } => write!(
                f,
                "locale `{locale}`: overlay key `{key}` is not a content key of any catalog"
            ),
            Self::Placeholders {
                locale,
                key,
                expected,
                found,
            } => {
                write!(
                    f,
                    "locale `{locale}`: overlay of `{key}` must use the placeholders "
                )?;
                braces(f, expected.iter().map(String::as_str))?;
                let missing: Vec<&str> = expected
                    .iter()
                    .filter(|n| !found.contains(n))
                    .map(String::as_str)
                    .collect();
                let extra: Vec<&str> = found
                    .iter()
                    .filter(|n| !expected.contains(n))
                    .map(String::as_str)
                    .collect();
                let mut sep = "; ";
                if !missing.is_empty() {
                    write!(f, "{sep}missing ")?;
                    braces(f, missing)?;
                    sep = ", ";
                }
                if !extra.is_empty() {
                    write!(f, "{sep}extra ")?;
                    braces(f, extra)?;
                }
                Ok(())
            }
            Self::AlreadyInstalled { origin } => write!(
                f,
                "catalog `{origin}` already holds different overlays in this process (overlays \
                 install once per process)"
            ),
        }
    }
}

impl std::error::Error for OverlayViolation {}

/// Whether `key`'s first dot-segment is a reserved family.
fn is_reserved(key: &str) -> bool {
    key.split('.').next().is_some_and(|s| RESERVED.contains(&s))
}

/// The unique placeholder names of `value`, in first-appearance order.
fn unique_names(value: &str) -> Vec<&str> {
    let mut seen = Vec::new();
    for name in noctis_rules::placeholder_names(value) {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// A validated overlay value: its owner catalog's index, then the entry.
pub(crate) type Valid = (usize, Entry);

/// Validates every `(locale, key, value)` pair of `overlays` against the
/// merged catalogs; `owner` maps a key to its owner catalog's index.
///
/// Returns the accepted values, or every violation sorted by locale
/// position, then key bytes.
pub(crate) fn validate<'c>(
    overlays: &[(Locale, Vec<(String, String)>)],
    owner: impl Fn(&str) -> Option<(usize, &'c Catalog)>,
) -> Result<Vec<Valid>, Vec<OverlayViolation>> {
    let mut by_locale: HashMap<Locale, Vec<(&str, &str)>> = HashMap::new();
    for (locale, pairs) in overlays {
        by_locale
            .entry(*locale)
            .or_default()
            .extend(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    let mut violations: Vec<(usize, OverlayViolation)> = Vec::new();
    let mut valid = Vec::new();
    for (&locale, pairs) in &by_locale {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for &(key, _) in pairs {
            *counts.entry(key).or_default() += 1;
        }
        let mut reported: Vec<&str> = Vec::new();
        for &(key, value) in pairs {
            let mut reject = |v: OverlayViolation| violations.push((locale as usize, v));
            if counts[key] > 1 {
                if !reported.contains(&key) {
                    reported.push(key);
                    reject(OverlayViolation::DuplicateKey {
                        locale,
                        key: key.to_owned(),
                    });
                }
                continue;
            }
            if is_reserved(key) {
                reject(OverlayViolation::ReservedKey {
                    locale,
                    key: key.to_owned(),
                });
                continue;
            }
            let Some((index, catalog, row)) =
                owner(key).and_then(|(i, c)| c.content_row(key).map(|row| (i, c, row)))
            else {
                reject(OverlayViolation::UnknownKey {
                    locale,
                    key: key.to_owned(),
                });
                continue;
            };
            let expected = catalog.placeholders(key).unwrap_or(&[]);
            let found = unique_names(value);
            let same = expected.len() == found.len() && expected.iter().all(|n| found.contains(n));
            if !same {
                reject(OverlayViolation::Placeholders {
                    locale,
                    key: key.to_owned(),
                    expected: expected.iter().map(|n| (*n).to_owned()).collect(),
                    found: found.into_iter().map(str::to_owned).collect(),
                });
                continue;
            }
            valid.push((index, (locale, row, Box::from(value))));
        }
    }
    if violations.is_empty() {
        return Ok(valid);
    }
    violations.sort_by(|(la, a), (lb, b)| la.cmp(lb).then_with(|| key_of(a).cmp(key_of(b))));
    Err(violations.into_iter().map(|(_, v)| v).collect())
}

/// The key bytes of a per-key violation.
fn key_of(v: &OverlayViolation) -> &[u8] {
    match v {
        OverlayViolation::DuplicateKey { key, .. }
        | OverlayViolation::ReservedKey { key, .. }
        | OverlayViolation::UnknownKey { key, .. }
        | OverlayViolation::Placeholders { key, .. } => key.as_bytes(),
        OverlayViolation::AlreadyInstalled { .. } => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_families() {
        for key in [
            "cmd",
            "cmd.x",
            "cmdword.a",
            "ctxcmd.k.name",
            "noctis",
            "noctis.expired",
        ] {
            assert!(is_reserved(key), "{key}");
        }
        for key in ["cmdx.y", "CMD.x", "", " cmd.x", "shop.cmd"] {
            assert!(!is_reserved(key), "{key}");
        }
    }

    #[test]
    fn displays() {
        let tr = Locale::Tr;
        let cases = [
            (
                OverlayViolation::DuplicateKey {
                    locale: tr,
                    key: "shop.title".into(),
                },
                "locale `tr`: overlay key `shop.title` appears more than once",
            ),
            (
                OverlayViolation::ReservedKey {
                    locale: tr,
                    key: "cmd.ban.description".into(),
                },
                "locale `tr`: overlay key `cmd.ban.description` is reserved: `cmd.*`, \
                 `cmdword.*`, `ctxcmd.*` and `noctis.*` cannot be overlaid (published command \
                 texts change only with a redeploy)",
            ),
            (
                OverlayViolation::UnknownKey {
                    locale: tr,
                    key: "shop.titel".into(),
                },
                "locale `tr`: overlay key `shop.titel` is not a content key of any catalog",
            ),
            (
                OverlayViolation::Placeholders {
                    locale: tr,
                    key: "shop.left".into(),
                    expected: vec!["n".into()],
                    found: vec![],
                },
                "locale `tr`: overlay of `shop.left` must use the placeholders `{n}`; missing `{n}`",
            ),
            (
                OverlayViolation::Placeholders {
                    locale: tr,
                    key: "greet.hello".into(),
                    expected: vec![],
                    found: vec!["name".into()],
                },
                "locale `tr`: overlay of `greet.hello` must use the placeholders (none); extra \
                 `{name}`",
            ),
            (
                OverlayViolation::AlreadyInstalled {
                    origin: "bot/locales",
                },
                "catalog `bot/locales` already holds different overlays in this process \
                 (overlays install once per process)",
            ),
        ];
        for (v, text) in cases {
            assert_eq!(v.to_string(), text);
        }
        assert_eq!(
            format!("{:?}", OverlaySlot::new()),
            "OverlaySlot { installed: false }"
        );
    }
}
