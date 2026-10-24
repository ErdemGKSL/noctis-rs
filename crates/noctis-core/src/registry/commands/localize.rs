//! Command localization (FR-41, FR-12, FR-13, AD-16; LOC-02, LOC-03,
//! LOC-05): resolving every registered node, option, choice and keyed menu
//! against the merged [`AppStrings`] into per-locale variants, and checking
//! them.
//!
//! Key layout: `cmd.<path>.name|description` per node,
//! `cmd.<path>.opt.<o>.name|description|choice.<value>` per option of a
//! leaf, `cmdword.<word>` per command word and `ctxcmd.<key>.name` per menu,
//! where `<path>` is the Command Path with dots between its words. A word,
//! option name or choice value that is not one key segment
//! (`[A-Za-z0-9_-]+`) has no key and is never localized.
//!
//! Resolution: for locale `L` the chain is `L`, then `L.sibling()`; in each
//! chain locale the full-path key is tried first, then (node names only)
//! `cmdword.<last word>`. Every locale of [`Locale::ALL`] with a hit is
//! emitted, values verbatim.

use noctis_i18n::AppStrings;
use noctis_rules::command::{
    ChoiceValue, LocalizedField, OptionDecl, validate_localized, validate_localized_option_names,
};
use noctis_rules::{Actual, Bound, Limits, Locale, RuleId, Violation};

use crate::BuildViolation;

/// A text's localizations, in [`Locale::ALL`] order: `(emitted locale,
/// value)`.
pub(crate) type Localized = Vec<(Locale, &'static str)>;

/// One emitted localization of a text and where its value comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Variant {
    /// The emitted locale code.
    pub(crate) locale: Locale,
    /// The value, verbatim.
    pub(crate) value: &'static str,
    /// The key that supplied it.
    key: String,
    /// The locale whose file holds it (`locale` or its sibling).
    file: Locale,
}

/// Every [`Variant`] of one text, in [`Locale::ALL`] order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Variants(Vec<Variant>);

impl Variants {
    /// The `(locale, value)` pairs, for the Publish JSON.
    pub(crate) fn localized(&self) -> Localized {
        self.0.iter().map(|v| (v.locale, v.value)).collect()
    }

    /// The value emitted for `locale`, if any.
    fn get(&self, locale: Locale) -> Option<&Variant> {
        self.0.iter().find(|v| v.locale == locale)
    }

    /// The values that pass LOC-02 for `field` (what the CMD-05 budget
    /// counts; a failing value is reported as LOC-02 instead).
    pub(crate) fn valid_values(&self, field: LocalizedField, limits: &Limits) -> Vec<&'static str> {
        self.0
            .iter()
            .filter(|v| validate_localized(field, v.value, limits).is_empty())
            .map(|v| v.value)
            .collect()
    }
}

/// The localizations of one chat-input node (base, group or subcommand).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NodeTexts {
    pub(crate) name: Variants,
    pub(crate) description: Variants,
}

/// The localizations of one option of one leaf.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OptionTexts {
    pub(crate) name: Variants,
    pub(crate) description: Variants,
    /// Parallel to the option's choices.
    pub(crate) choices: Vec<Variants>,
}

/// Whether `text` is one reserved-family key segment (`[A-Za-z0-9_-]+`).
pub(crate) fn is_segment(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `path` with dots between its words, when every word is a key segment.
fn key_path(path: &str) -> Option<String> {
    path.split(' ')
        .all(is_segment)
        .then(|| path.replace(' ', "."))
}

/// A choice value as key text: `Str` as is, `Int` via `Display`; `Float`
/// and non-segment values have none.
fn choice_segment(value: ChoiceValue<'_>) -> Option<String> {
    let text = match value {
        ChoiceValue::Str(text) => text.to_owned(),
        ChoiceValue::Int(value) => value.to_string(),
        ChoiceValue::Float(_) => return None,
    };
    is_segment(&text).then_some(text)
}

/// Looks keys up in the merged strings; with none, nothing resolves.
#[derive(Clone, Copy)]
pub(crate) struct Resolver<'s> {
    strings: Option<&'s AppStrings>,
}

impl<'s> Resolver<'s> {
    pub(crate) fn new(strings: Option<&'s AppStrings>) -> Self {
        Self { strings }
    }

    /// Every locale's first hit along its chain, trying `full` before
    /// `word` in each chain locale.
    fn resolve(self, full: Option<&str>, word: Option<&str>) -> Variants {
        let Some(strings) = self.strings else {
            return Variants::default();
        };
        if full.is_none() && word.is_none() {
            return Variants::default();
        }
        let keys: Vec<&str> = full.into_iter().chain(word).collect();
        let mut out = Vec::new();
        for locale in Locale::ALL {
            let chain = std::iter::once(locale).chain(locale.sibling());
            let hit = chain
                .flat_map(|file| keys.iter().map(move |key| (file, *key)))
                .find_map(|(file, key)| strings.get(file, key).map(|value| (file, key, value)));
            if let Some((file, key, value)) = hit {
                out.push(Variant {
                    locale,
                    value,
                    key: key.to_owned(),
                    file,
                });
            }
        }
        Variants(out)
    }

    /// The name and description of the node at `path`.
    pub(crate) fn node(self, path: &str) -> NodeTexts {
        let full = key_path(path);
        let last = path.rsplit(' ').next().unwrap_or_default();
        let word = is_segment(last).then(|| format!("cmdword.{last}"));
        NodeTexts {
            name: self.resolve(
                full.as_ref().map(|p| format!("cmd.{p}.name")).as_deref(),
                word.as_deref(),
            ),
            description: self.resolve(
                full.as_ref()
                    .map(|p| format!("cmd.{p}.description"))
                    .as_deref(),
                None,
            ),
        }
    }

    /// The localizations of the options of the leaf at `path`.
    pub(crate) fn options(self, path: &str, options: &[OptionDecl<'_>]) -> Vec<OptionTexts> {
        let full = key_path(path);
        options
            .iter()
            .map(|option| {
                let Some(prefix) = full
                    .as_ref()
                    .filter(|_| is_segment(option.name))
                    .map(|p| format!("cmd.{p}.opt.{}", option.name))
                else {
                    return OptionTexts {
                        choices: vec![Variants::default(); option.choices.len()],
                        ..OptionTexts::default()
                    };
                };
                OptionTexts {
                    name: self.resolve(Some(&format!("{prefix}.name")), None),
                    description: self.resolve(Some(&format!("{prefix}.description")), None),
                    choices: option
                        .choices
                        .iter()
                        .map(|choice| match choice_segment(choice.value) {
                            Some(value) => {
                                self.resolve(Some(&format!("{prefix}.choice.{value}")), None)
                            }
                            None => Variants::default(),
                        })
                        .collect(),
                }
            })
            .collect()
    }

    /// The name of a context menu declared with `key`.
    pub(crate) fn menu(self, key: Option<&str>) -> Variants {
        match key.filter(|k| is_segment(k)) {
            Some(key) => self.resolve(Some(&format!("ctxcmd.{key}.name")), None),
            None => Variants::default(),
        }
    }

    /// `cmd.<path>.description` in its owner catalog's default-locale file,
    /// with that catalog's origin (LOC-06's default description).
    pub(crate) fn default_description(self, path: &str) -> Option<(&'static str, &'static str)> {
        let key = format!("cmd.{}.description", key_path(path)?);
        let found = self.default_text(&key)?;
        Some((found.text, found.origin))
    }

    /// `key` in its owner catalog's own default-locale file (no sibling).
    fn default_text(self, key: &str) -> Option<DefaultText> {
        let catalog = self.strings?.owner(key)?;
        let locale = catalog.default_locale();
        let text = catalog.get(locale, key)?;
        Some(DefaultText {
            text,
            origin: catalog.origin(),
            locale,
        })
    }
}

/// A key's text in its owner catalog's default-locale file.
struct DefaultText {
    text: &'static str,
    origin: &'static str,
    locale: Locale,
}

/// One declared description: a node's at `path` (`option` unset) or the
/// description of option `option` of the leaf at `path`.
pub(crate) struct Declared<'a> {
    /// The Command Path of the node or leaf.
    pub(crate) path: &'static str,
    /// The option's Discord name, for an option description.
    pub(crate) option: Option<&'a str>,
    /// The declared text.
    pub(crate) text: &'a str,
    /// The Module (or catalog) that declared it.
    pub(crate) source: &'static str,
}

/// CMD-04 for every declared description that differs from its owner
/// catalog's default-locale `cmd.<path>.description` (node) or
/// `cmd.<path>.opt.<o>.description` (option): one violation per key, the
/// first differing declaration in `declared` order, sorted by key bytes. Publish
/// emits the file's text as the default locale's localization, so a
/// differing declaration would show those users another text than
/// `description`. A key without an owner, or missing from that owner's
/// default-locale file, is not checked; the comparison is exact.
pub(crate) fn declared_descriptions<'a>(
    resolver: Resolver<'_>,
    declared: impl IntoIterator<Item = Declared<'a>>,
) -> Vec<Violation> {
    let mut found: Vec<(String, Violation)> = Vec::new();
    for decl in declared {
        let Some(path) = key_path(decl.path) else {
            continue;
        };
        let key = match decl.option {
            None => format!("cmd.{path}.description"),
            Some(option) if is_segment(option) => format!("cmd.{path}.opt.{option}.description"),
            Some(_) => continue,
        };
        if found.iter().any(|(k, _)| *k == key) {
            continue;
        }
        let Some(file) = resolver.default_text(&key) else {
            continue;
        };
        if file.text == decl.text {
            continue;
        }
        let at = match decl.option {
            None => decl.path.to_owned(),
            Some(option) => format!("{}.{option}", decl.path),
        };
        let declared_by = if decl.source.is_empty() {
            "declared by (unnamed)".to_owned()
        } else {
            format!("declared by `{}`", decl.source)
        };
        let violation = Violation::new(
            RuleId::CMD_04,
            at,
            Bound::Describe("the catalog's default-locale text"),
            Actual::Text(decl.text.to_owned()),
        )
        .with_note(format!(
            "description differs from `{key}` in catalog `{}` ({})",
            file.origin, file.locale
        ))
        .with_note(format!("the file has `{}`", file.text.escape_debug()))
        .with_note(declared_by)
        .with_help("make them equal or remove the declaration");
        found.push((key, violation));
    }
    found.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    found.into_iter().map(|(_, v)| v).collect()
}

/// One localization problem before it is typed.
struct Problem {
    rule: RuleId,
    file: Locale,
    key: String,
    violation: Violation,
}

/// The LOC-02 / LOC-03 problems found so far.
#[derive(Default)]
pub(crate) struct Problems(Vec<Problem>);

impl Problems {
    /// LOC-02 over every value of `variants`.
    pub(crate) fn values(&mut self, field: LocalizedField, variants: &Variants, limits: &Limits) {
        for variant in &variants.0 {
            for violation in validate_localized(field, variant.value, limits) {
                self.0.push(Problem {
                    rule: RuleId::LOC_02,
                    file: variant.file,
                    key: variant.key.clone(),
                    violation,
                });
            }
        }
    }

    /// LOC-03 over one options or children array: per locale, the
    /// localized names against the defaults and each other.
    pub(crate) fn names(&mut self, array: &[(&str, &Variants)]) {
        if array.iter().all(|(_, v)| v.0.is_empty()) {
            return;
        }
        let defaults: Vec<&str> = array.iter().map(|(name, _)| *name).collect();
        for locale in Locale::ALL {
            let variants: Vec<Option<&Variant>> =
                array.iter().map(|(_, v)| v.get(locale)).collect();
            if variants.iter().all(Option::is_none) {
                continue;
            }
            let localized: Vec<Option<&str>> =
                variants.iter().map(|v| v.map(|v| v.value)).collect();
            for (i, violation) in validate_localized_option_names(&defaults, &localized) {
                if let Some(variant) = variants[i] {
                    self.0.push(Problem {
                        rule: RuleId::LOC_03,
                        file: variant.file,
                        key: variant.key.clone(),
                        violation,
                    });
                }
            }
        }
    }

    /// The typed violations: one per `(key, file locale, rule, base rule)`,
    /// sorted by key bytes, then [`Locale::ALL`] order, then LOC-02 before
    /// LOC-03 (input order otherwise).
    pub(crate) fn finish(self, resolver: Resolver<'_>) -> Vec<BuildViolation> {
        let mut problems: Vec<Problem> = Vec::with_capacity(self.0.len());
        for problem in self.0 {
            let seen = problems.iter().any(|p| {
                p.key == problem.key
                    && p.file == problem.file
                    && p.rule == problem.rule
                    && p.violation.rule == problem.violation.rule
            });
            if !seen {
                problems.push(problem);
            }
        }
        let rank = |rule: RuleId| u8::from(rule != RuleId::LOC_02);
        problems.sort_by(|a, b| {
            a.key
                .as_bytes()
                .cmp(b.key.as_bytes())
                .then(a.file.cmp(&b.file))
                .then(rank(a.rule).cmp(&rank(b.rule)))
        });
        let Some(strings) = resolver.strings else {
            return Vec::new();
        };
        problems
            .into_iter()
            .filter_map(|problem| {
                // Every problem comes from a hit, so the key has an owner.
                let catalog = strings.owner(&problem.key)?;
                let key = catalog.keys().find(|k| *k == problem.key)?;
                Some(BuildViolation::CommandLocalization {
                    rule: problem.rule,
                    locale: problem.file,
                    key,
                    origin: catalog.origin(),
                    violation: problem.violation,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_and_key_paths() {
        for ok in ["ban", "show_profile", "a-b", "Red", "-1", "x1"] {
            assert!(is_segment(ok), "{ok}");
        }
        for bad in ["", "çay", "1.5", "big red", "a.b"] {
            assert!(!is_segment(bad), "{bad}");
        }
        assert_eq!(key_path("mod ban").as_deref(), Some("mod.ban"));
        assert_eq!(key_path("çay iç"), None);
        assert_eq!(choice_segment(ChoiceValue::Int(-1)).as_deref(), Some("-1"));
        assert_eq!(choice_segment(ChoiceValue::Float(1.5)), None);
        assert_eq!(choice_segment(ChoiceValue::Str("big red")), None);
    }
}
