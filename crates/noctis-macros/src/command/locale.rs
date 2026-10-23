//! The default-locale file of `#[command]` with the `i18n` feature (Story
//! 11.4; AD-16, CMD-01, CMD-04): where it is, reading it, and the pure
//! description resolution over its entries.
//!
//! A command description comes from `cmd.<path>.description`, an option
//! description from `cmd.<path>.opt.<option>.description`, where `<path>`
//! is the Command Path with dots between its words. A literal equal to the
//! file's text is fine, a differing one is CMD-04; with neither, the
//! existing missing-description CMD-04 gains a note naming the key.

use std::collections::BTreeMap;
use std::path::PathBuf;

use noctis_rules::{Actual, Bound, Locale, RuleId, Violation};

/// The directory `#[command]` reads when `locales` is not given.
pub(crate) const DEFAULT_DIR: &str = "locales";

/// A read default-locale file.
#[derive(Debug, Default)]
pub(crate) struct LocaleFile {
    /// `<dir>/<code>.yaml`, for messages.
    pub(crate) shown: String,
    /// The absolute path, for `include_bytes!` (`None` in unit tests).
    pub(crate) path: Option<PathBuf>,
    /// Every key of the file.
    pub(crate) entries: BTreeMap<String, String>,
}

impl LocaleFile {
    /// A file with `entries`, for unit tests.
    #[cfg(test)]
    pub(crate) fn of(shown: &str, entries: &[(&str, &str)]) -> Self {
        Self {
            shown: shown.to_owned(),
            path: None,
            entries: entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

/// The `locales`, `default_locale` and hidden `__locales_from_source_file`
/// arguments.
#[derive(Debug, Default)]
pub(crate) struct LocaleArgs {
    /// `locales = "<dir>"`.
    pub(crate) dir: Option<syn::LitStr>,
    /// `default_locale = "<code>"`, parsed.
    pub(crate) default: Option<(Locale, proc_macro2::Span)>,
    /// `__locales_from_source_file`.
    pub(crate) from_source_file: bool,
}

impl LocaleArgs {
    /// Whether `locales` or `default_locale` was written.
    fn explicit(&self) -> bool {
        self.dir.is_some() || self.default.is_some()
    }
}

impl LocaleArgs {
    /// The directory as written (`locales` when omitted).
    fn dir(&self) -> String {
        self.dir
            .as_ref()
            .map_or_else(|| DEFAULT_DIR.to_owned(), syn::LitStr::value)
    }

    /// The file name, `<code>.yaml` (`en-US.yaml` when omitted).
    fn file_name(&self) -> String {
        let code = self.default.map_or(Locale::EnUs, |(l, _)| l);
        format!("{code}.yaml")
    }
}

/// Reads the default-locale file with the `i18n` feature: `None` without
/// the feature, or when the file is missing and neither argument was
/// written (Story 6.1's literal rule applies); an error for a missing
/// explicit file or a file with errors.
pub(crate) fn load(args: &LocaleArgs) -> syn::Result<Option<LocaleFile>> {
    if !cfg!(feature = "i18n") {
        return Ok(None);
    }
    let span = args
        .dir
        .as_ref()
        .map(syn::LitStr::span)
        .or(args.default.map(|(_, s)| s))
        .unwrap_or_else(proc_macro2::Span::call_site);
    let dir = args.dir();
    let name = args.file_name();
    let shown = format!("{}/{name}", crate::i18n::shown(&dir));
    let root = crate::i18n::root(args.from_source_file).map_err(|e| syn::Error::new(span, e))?;
    let path = root.join(&dir).join(&name);
    if !path.is_file() {
        if args.explicit() {
            let from = if args.from_source_file {
                "the source file"
            } else {
                "the crate root"
            };
            return Err(syn::Error::new(
                span,
                format!("locale file `{shown}` not found (relative to {from})"),
            ));
        }
        return Ok(None);
    }
    let path = path.canonicalize().unwrap_or(path);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| syn::Error::new(span, format!("locale file `{shown}` cannot be read: {e}")))?;
    let entries = crate::i18n::file_entries(&text).map_err(|e| {
        syn::Error::new(
            span,
            format!("locale file `{shown}` has errors (see `i18n!`): {e}"),
        )
    })?;
    Ok(Some(LocaleFile {
        shown,
        path: Some(path),
        entries,
    }))
}

/// `path` with dots between its words.
pub(crate) fn key_path(path: &str) -> String {
    path.replace(' ', ".")
}

/// `cmd.<path>.description`.
pub(crate) fn description_key(path: &str) -> String {
    format!("cmd.{}.description", key_path(path))
}

/// `cmd.<path>.opt.<option>.description`.
pub(crate) fn option_key(path: &str, option: &str) -> String {
    format!("cmd.{}.opt.{option}.description", key_path(path))
}

/// The note of a description missing from the file.
pub(crate) fn not_found(key: &str, file: &LocaleFile) -> String {
    format!("not found as `{key}` in `{}`", file.shown)
}

/// CMD-01 for every path word that is a reserved command-key segment
/// (`name`, `description`, `opt`), once per word.
pub(crate) fn reserved_words(paths: &[String], file: &LocaleFile) -> Vec<Violation> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for path in paths {
        for word in path.split(' ') {
            if crate::i18n::COMMAND_KEY_WORDS.contains(&word) && !seen.contains(&word) {
                seen.push(word);
                out.push(
                    Violation::new(
                        RuleId::CMD_01,
                        path.as_str(),
                        Bound::Describe(
                            "a word other than `name`, `description` or `opt` when locale files are read",
                        ),
                        Actual::Text(word.to_owned()),
                    )
                    .with_note(format!(
                        "`{word}` is ambiguous in the `cmd.<path>` keys of `{}`",
                        file.shown
                    ))
                    .with_help(format!("rename the word `{word}`")),
                );
            }
        }
    }
    out
}

/// The CMD-04 conflict of a literal and the file's text.
fn conflict(path: &str, key: &str, literal: &str, text: &str, file: &LocaleFile) -> Violation {
    Violation::new(
        RuleId::CMD_04,
        path,
        Bound::Describe("the locale file's text"),
        Actual::Text(literal.to_owned()),
    )
    .with_note(format!(
        "description differs from `{key}` in `{}`",
        file.shown
    ))
    .with_note(format!("the file has `{}`", text.escape_debug()))
    .with_help("remove the `description` literal or make it equal to the file's text")
}

/// Every path's description (M1-M8): the literal, else
/// `cmd.<path>.description` from `file`; `None` where there is neither
/// (the tree check reports it). A literal differing from the file's text
/// is one CMD-04 violation per path.
pub(crate) fn descriptions(
    paths: &[String],
    literal: Option<&str>,
    file: Option<&LocaleFile>,
) -> (Vec<Option<String>>, Vec<Violation>) {
    let mut violations = Vec::new();
    let texts = paths
        .iter()
        .map(|path| {
            let key = description_key(path);
            let text = file.and_then(|f| f.get(&key));
            match (literal, text, file) {
                (Some(literal), Some(text), Some(file)) if literal != text => {
                    violations.push(conflict(path, &key, literal, text, file));
                    Some(literal.to_owned())
                }
                (Some(literal), ..) => Some(literal.to_owned()),
                (None, Some(text), _) => Some(text.to_owned()),
                (None, None, _) => None,
            }
        })
        .collect();
    (texts, violations)
}

/// An option description resolved from a literal and the file (M12-M14).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OptionText {
    /// The text every path agrees on.
    Text(String),
    /// No literal and these keys are missing from the file.
    Missing(Vec<String>),
}

/// The description of option `option` (one per struct) over every path:
/// the literal (each path's key, when present, must equal it), else the
/// keys' common text. A conflict or differing keys is the CMD-04
/// violation.
pub(crate) fn option_description(
    paths: &[String],
    option: &str,
    literal: Option<&str>,
    file: &LocaleFile,
) -> Result<OptionText, Box<Violation>> {
    let found: Vec<(String, Option<&str>)> = paths
        .iter()
        .map(|path| {
            let key = option_key(path, option);
            let text = file.get(&key);
            (key, text)
        })
        .collect();
    if let Some(literal) = literal {
        if let Some((key, text)) = found
            .iter()
            .find_map(|(k, t)| t.filter(|t| *t != literal).map(|t| (k, t)))
        {
            return Err(Box::new(conflict(option, key, literal, text, file)));
        }
        return Ok(OptionText::Text(literal.to_owned()));
    }
    let missing: Vec<String> = found
        .iter()
        .filter(|(_, t)| t.is_none())
        .map(|(k, _)| k.clone())
        .collect();
    if !missing.is_empty() {
        return Ok(OptionText::Missing(missing));
    }
    let mut texts = found.iter().filter_map(|(k, t)| t.map(|t| (k, t)));
    let Some((first_key, first)) = texts.next() else {
        return Ok(OptionText::Missing(Vec::new()));
    };
    if let Some((key, text)) = texts.find(|(_, t)| *t != first) {
        return Err(Box::new(Violation::new(
            RuleId::CMD_04,
            option,
            Bound::Describe("one description across the Pattern's expansions"),
            Actual::Text(text.to_owned()),
        )
        .with_note(format!(
            "different descriptions across the Pattern's expansions: `{first_key}` has `{}`, `{key}` has `{}`",
            first.escape_debug(),
            text.escape_debug()
        ))
        .with_help("write `#[option(description = \"…\")]` or make the texts equal")));
    }
    Ok(OptionText::Text(first.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| (*p).to_owned()).collect()
    }

    const SHOWN: &str = "locales/en-US.yaml";

    /// Matrix M1-M5.
    #[test]
    fn command_descriptions() {
        let p = paths(&["ping"]);
        // M1: no file.
        assert_eq!(
            descriptions(&p, Some("d"), None),
            (vec![Some("d".to_owned())], vec![])
        );
        let file = LocaleFile::of(SHOWN, &[("cmd.ping.description", "Pong")]);
        // M2, M3.
        assert_eq!(
            descriptions(&p, None, Some(&file)),
            (vec![Some("Pong".to_owned())], vec![])
        );
        assert_eq!(
            descriptions(&p, Some("Pong"), Some(&file)),
            (vec![Some("Pong".to_owned())], vec![])
        );
        // M4.
        let (_, v) = descriptions(&p, Some("X"), Some(&file));
        assert_eq!(v.len(), 1);
        let text = v[0].to_string();
        assert!(text.starts_with("[CMD-04] ping:"), "{text}");
        assert!(
            text.contains(
                "description differs from `cmd.ping.description` in `locales/en-US.yaml`"
            ),
            "{text}"
        );
        assert!(text.contains("`X`") && text.contains("`Pong`"), "{text}");
        assert!(
            text.contains("help: remove the `description` literal"),
            "{text}"
        );
        // M5.
        let empty = LocaleFile::of(SHOWN, &[]);
        assert_eq!(descriptions(&p, None, Some(&empty)), (vec![None], vec![]));
        assert_eq!(
            not_found(&description_key("ping"), &empty),
            "not found as `cmd.ping.description` in `locales/en-US.yaml`"
        );
    }

    /// Matrix M6-M8.
    #[test]
    fn pattern_descriptions_per_path() {
        let p = paths(&["config set", "config get"]);
        let file = LocaleFile::of(
            SHOWN,
            &[
                ("cmd.config.set.description", "Set"),
                ("cmd.config.get.description", "Get"),
            ],
        );
        assert_eq!(
            descriptions(&p, None, Some(&file)).0,
            [Some("Set".to_owned()), Some("Get".to_owned())]
        );
        let one = LocaleFile::of(SHOWN, &[("cmd.config.set.description", "Set")]);
        assert_eq!(
            descriptions(&p, None, Some(&one)).0,
            [Some("Set".to_owned()), None]
        );
        let (texts, v) = descriptions(&p, Some("Set"), Some(&file));
        assert_eq!(texts, [Some("Set".to_owned()), Some("Set".to_owned())]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path, "config get");
    }

    /// Matrix M10, M19.
    #[test]
    fn reserved_words_are_cmd_01() {
        let file = LocaleFile::of(SHOWN, &[]);
        let v = reserved_words(&paths(&["config opt", "name", "x description"]), &file);
        let words: Vec<_> = v.iter().map(|v| v.actual.clone()).collect();
        assert_eq!(
            words,
            [
                Actual::Text("opt".into()),
                Actual::Text("name".into()),
                Actual::Text("description".into())
            ]
        );
        assert!(v.iter().all(|v| v.rule == RuleId::CMD_01));
        assert!(v[0].to_string().contains("ambiguous"), "{}", v[0]);
        assert!(reserved_words(&paths(&["çay", "names"]), &file).is_empty());
        assert_eq!(description_key("çay"), "cmd.çay.description");
    }

    /// Matrix M12-M14.
    #[test]
    fn option_descriptions() {
        let p = paths(&["ban"]);
        let file = LocaleFile::of(SHOWN, &[("cmd.ban.opt.user.description", "Who")]);
        assert_eq!(
            option_description(&p, "user", None, &file),
            Ok(OptionText::Text("Who".into()))
        );
        assert_eq!(
            option_description(&p, "user", Some("Who"), &file),
            Ok(OptionText::Text("Who".into()))
        );
        let v = option_description(&p, "user", Some("X"), &file).unwrap_err();
        assert_eq!(v.rule, RuleId::CMD_04);
        assert!(v.to_string().contains("cmd.ban.opt.user.description"));
        assert_eq!(
            option_description(&p, "reason", None, &file),
            Ok(OptionText::Missing(vec![
                "cmd.ban.opt.reason.description".into()
            ]))
        );
        assert_eq!(
            option_description(&p, "reason", Some("Why"), &file),
            Ok(OptionText::Text("Why".into()))
        );
        let two = paths(&["config set", "config get"]);
        let file = LocaleFile::of(
            SHOWN,
            &[
                ("cmd.config.set.opt.v.description", "A"),
                ("cmd.config.get.opt.v.description", "B"),
            ],
        );
        let v = option_description(&two, "v", None, &file).unwrap_err();
        let text = v.to_string();
        assert!(
            text.contains("different descriptions across the Pattern's expansions")
                && text.contains("cmd.config.set.opt.v.description")
                && text.contains("cmd.config.get.opt.v.description"),
            "{text}"
        );
    }
}
