//! The file system half of `i18n!`: listing `dir` and reading its locale
//! files. Everything else is pure (`model`).

use std::path::{Path, PathBuf};

use noctis_rules::RuleId;

use super::args::Args;

/// What an entry of `dir` is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// A locale file: its extension is exactly `yaml`.
    Locale,
    /// `yml`, or `yaml` in another case: a LOC-01 error.
    WrongExtension,
    /// Anything else, ignored.
    Ignored,
}

/// Classifies a file name.
pub(crate) fn classify(name: &str) -> Entry {
    match Path::new(name).extension().and_then(|e| e.to_str()) {
        Some("yaml") => Entry::Locale,
        Some(ext) if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") => {
            Entry::WrongExtension
        }
        _ => Entry::Ignored,
    }
}

/// `dir` as written, without trailing separators, for messages.
pub(crate) fn shown(dir: &str) -> &str {
    let trimmed = dir.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() { dir } else { trimmed }
}

/// The read locale files and every problem found.
#[derive(Default)]
pub(crate) struct Loaded {
    /// `(file name, contents)`, sorted by file name.
    pub(crate) files: Vec<(String, String)>,
    /// The absolute path of every file read, for `include_bytes!`.
    pub(crate) read: Vec<PathBuf>,
    /// The problems, as messages.
    pub(crate) errors: Vec<String>,
}

/// The directory relative paths start from: the invoking source file's
/// directory with `from_source_file` (Noctis's own fixtures), else the
/// crate root.
pub(crate) fn root(from_source_file: bool) -> Result<PathBuf, String> {
    if from_source_file {
        proc_macro::Span::call_site()
            .local_file()
            .and_then(|file| file.parent().map(Path::to_path_buf))
            .ok_or_else(|| "i18n!: the invoking source file is unknown".to_owned())
    } else {
        std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "i18n!: `CARGO_MANIFEST_DIR` is not set".to_owned())
    }
}

/// The directory `dir` resolves to.
fn base(args: &Args) -> Result<PathBuf, String> {
    Ok(root(args.from_source_file)?.join(&args.dir))
}

/// Lists and reads the locale files of `args.dir`.
pub(crate) fn load(args: &Args) -> Loaded {
    let mut loaded = Loaded::default();
    let dir = shown(&args.dir);
    let base = match base(args) {
        Ok(base) => base,
        Err(e) => {
            loaded.errors.push(e);
            return loaded;
        }
    };
    let relative = if args.from_source_file {
        "the source file"
    } else {
        "the crate root"
    };
    if !base.is_dir() {
        loaded.errors.push(format!(
            "i18n!: `{dir}` is not a directory (relative to {relative})"
        ));
        return loaded;
    }
    let entries = match std::fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(e) => {
            loaded
                .errors
                .push(format!("i18n!: `{dir}`: cannot read: {e}"));
            return loaded;
        }
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        match classify(&name) {
            Entry::Ignored => {}
            Entry::WrongExtension => loaded.errors.push(format!(
                "[{}] locale file `{dir}/{name}` must use the `.yaml` extension",
                RuleId::LOC_01
            )),
            Entry::Locale => {
                let path = base.join(&name);
                let path = path.canonicalize().unwrap_or(path);
                match std::fs::read(&path) {
                    Err(e) => loaded
                        .errors
                        .push(format!("i18n!: `{dir}/{name}`: cannot read: {e}")),
                    Ok(bytes) => {
                        loaded.read.push(path);
                        match String::from_utf8(bytes) {
                            Ok(text) => loaded.files.push((name, text)),
                            Err(_) => loaded
                                .errors
                                .push(format!("i18n!: `{dir}/{name}`: not UTF-8")),
                        }
                    }
                }
            }
        }
    }
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Matrix 13, 15.
    #[test]
    fn entries_are_classified_by_extension() {
        assert_eq!(classify("tr.yaml"), Entry::Locale);
        assert_eq!(classify(".tr.yaml"), Entry::Locale);
        assert_eq!(classify("tr.yml"), Entry::WrongExtension);
        assert_eq!(classify("en-US.YAML"), Entry::WrongExtension);
        assert_eq!(classify("en-US.Yml"), Entry::WrongExtension);
        for ignored in ["README.md", ".gitkeep", "old", "tr", "yaml"] {
            assert_eq!(classify(ignored), Entry::Ignored, "{ignored}");
        }
    }

    #[test]
    fn shown_trims_trailing_separators() {
        assert_eq!(shown("locales/"), "locales");
        assert_eq!(shown("a/b"), "a/b");
        assert_eq!(shown("/"), "/");
    }
}
