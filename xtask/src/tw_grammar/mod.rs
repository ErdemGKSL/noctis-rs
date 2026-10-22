//! `tw-grammar [--check]`: generates `crates/noctis-image/src/tw/generated.rs`
//! from the pinned takumi-core sources and the closed suffix table (FR-32,
//! FR-66, AD-15, AD-28).
//!
//! The pin is read from the root `Cargo.toml` (`takumi-core = "=X.Y.Z"`); the
//! crate is located with `cargo metadata` (all features). `syn` reads its
//! Tailwind tables; anything that does not have the expected shape, a parse
//! type without a closed-table entry, an unknown variant or namespace, or a
//! changed fingerprint fails, naming the file and the item. `--check` compares
//! the generated text with the committed file and writes nothing.

mod emit;
mod extract;
mod table;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cargo_metadata::{CargoOpt, MetadataCommand};

/// The generated file, relative to the repository root.
const GENERATED: &str = "crates/noctis-image/src/tw/generated.rs";
/// The pinned crate.
const TAKUMI_CORE: &str = "takumi-core";

/// A generator failure: the file and item it concerns, and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrammarError {
    file: String,
    item: String,
    message: String,
}

impl GrammarError {
    pub(crate) fn new(file: &str, item: &str, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            item: item.to_owned(),
            message: message.into(),
        }
    }
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tw-grammar: {}: {}: {}",
            self.file, self.item, self.message
        )
    }
}

/// Runs the generator for the repository at `root`.
pub(crate) fn run(root: &Path, check: bool) -> ExitCode {
    match generate(root).and_then(|text| write_or_check(&root.join(GENERATED), &text, check)) {
        Ok(message) => {
            eprintln!("tw-grammar: {message}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// The formatted text of `generated.rs`.
fn generate(root: &Path) -> Result<String, GrammarError> {
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| GrammarError::new("Cargo.toml", TAKUMI_CORE, format!("cannot read: {e}")))?;
    let version = read_pin(&text)?;
    let crate_root = locate(&manifest, &version)?;
    let sources = read_sources(&crate_root)?;
    let extracted = extract::extract(&sources)?;
    emit::check(&extracted)?;
    emit::rustfmt(&emit::render(&extracted, &version), root)
}

/// The exact `takumi-core` version pinned in `[workspace.dependencies]`.
fn read_pin(manifest: &str) -> Result<String, GrammarError> {
    let fail = |message: &str| {
        GrammarError::new(
            "Cargo.toml",
            "[workspace.dependencies] takumi-core",
            message,
        )
    };
    let doc: toml::Table = manifest
        .parse()
        .map_err(|e| fail(&format!("does not parse: {e}")))?;
    let dep = doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.get(TAKUMI_CORE))
        .ok_or_else(|| fail("is missing"))?;
    let requirement = match dep {
        toml::Value::String(s) => Some(s.as_str()),
        toml::Value::Table(t) => t.get("version").and_then(toml::Value::as_str),
        _ => None,
    }
    .ok_or_else(|| fail("has no version"))?;
    let version = requirement
        .strip_prefix('=')
        .filter(|v| {
            let parts: Vec<&str> = v.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .ok_or_else(|| fail(&format!("`{requirement}` is not an exact `=X.Y.Z` pin")))?;
    Ok(version.to_owned())
}

/// The root directory of the `takumi-core` package at `version`.
fn locate(manifest: &Path, version: &str) -> Result<PathBuf, GrammarError> {
    let fail = |message: String| GrammarError::new("Cargo.toml", TAKUMI_CORE, message);
    let metadata = MetadataCommand::new()
        .manifest_path(manifest)
        .features(CargoOpt::AllFeatures)
        .exec()
        .map_err(|e| fail(format!("cargo metadata failed: {e}")))?;
    let package = metadata
        .packages
        .iter()
        .find(|p| p.name.as_str() == TAKUMI_CORE && p.version.to_string() == version)
        .ok_or_else(|| {
            fail(format!(
                "package {TAKUMI_CORE} {version} is not in the dependency graph"
            ))
        })?;
    package
        .manifest_path
        .parent()
        .map(|dir| dir.as_std_path().to_path_buf())
        .ok_or_else(|| fail("manifest path has no parent".to_owned()))
}

/// Every `src/**/*.rs` of the crate, keyed by its path relative to the crate root.
fn read_sources(crate_root: &Path) -> Result<BTreeMap<String, String>, GrammarError> {
    let mut files = Vec::new();
    collect_rs(&crate_root.join("src"), &mut files);
    let mut out = BTreeMap::new();
    for file in files {
        let relative = file
            .strip_prefix(crate_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&file)
            .map_err(|e| GrammarError::new(&relative, "file", format!("cannot read: {e}")))?;
        out.insert(relative, text);
    }
    Ok(out)
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Writes `text` to `path`, or with `check` compares it and writes nothing.
fn write_or_check(path: &Path, text: &str, check: bool) -> Result<String, GrammarError> {
    let current = std::fs::read_to_string(path).ok();
    if check {
        return if current.as_deref() == Some(text) {
            Ok(format!("ok ({GENERATED} is up to date)"))
        } else {
            Err(GrammarError::new(
                GENERATED,
                "--check",
                "is stale; run `cargo run --release -p xtask -- tw-grammar` and commit the result",
            ))
        };
    }
    if current.as_deref() == Some(text) {
        return Ok(format!("{GENERATED} unchanged"));
    }
    std::fs::write(path, text)
        .map_err(|e| GrammarError::new(GENERATED, "write", format!("cannot write: {e}")))?;
    Ok(format!("wrote {GENERATED}"))
}

#[cfg(test)]
mod tests;
