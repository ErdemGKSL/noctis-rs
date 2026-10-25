//! `noctis-ports` `src/` does no I/O, has no `unsafe`, names no secret, and depends only on the allowed crates (AD-1 layer 1, NFR-6).

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs",
    "std::net",
    "std::io",
    "std::process",
    "std::thread",
    "std::env",
    "unsafe",
    "PayloadKey",
];

/// Matched case-insensitively: no port method may take a secret (NFR-6).
const FORBIDDEN_ANY_CASE: &[&str] = &["token", "secret", "password", "credential", "api_key"];

/// `blake3` is pure hashing (`RenderJob::key`, Story 15.1); it does no I/O.
const ALLOWED_DEPENDENCIES: &[&str] = &[
    "blake3",
    "bytes",
    "moka",
    "postcard",
    "serde",
    "thiserror",
    "tokio",
    "twilight-cache-inmemory",
    "twilight-model",
];

/// The one file allowed to name tokio, and the only paths it may use (timers
/// and the FIFO mutex do no I/O).
const TOKIO_FILE: &str = "identify.rs";
const TOKIO_ALLOWED_PATHS: &[&str] = &["::sync::Mutex", "::time::"];

/// The exact `[dependencies]` line for tokio: no runtime, I/O or macros.
const TOKIO_DEPENDENCY: &str = r#"tokio = { workspace = true, features = ["sync", "time"] }"#;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn source_has_no_io_unsafe_or_secret() {
    let mut files = Vec::new();
    rust_files(&manifest_dir().join("src"), &mut files).expect("src/ is readable");
    assert!(!files.is_empty());
    let mut hits = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("source file is readable");
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let lower = line.to_lowercase();
            let any_case = FORBIDDEN_ANY_CASE
                .iter()
                .filter(|word| lower.contains(*word));
            check_tokio(file, number + 1, line, &mut hits);
            for word in FORBIDDEN
                .iter()
                .filter(|word| line.contains(*word))
                .chain(any_case)
            {
                hits.push(format!("{}:{}: `{word}`", file.display(), number + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "forbidden tokens in noctis-ports:\n{}",
        hits.join("\n")
    );
}

/// tokio may appear only in `src/memory/identify.rs`, and only as
/// `tokio::sync::Mutex` or `tokio::time::…`.
fn check_tokio(file: &Path, number: usize, line: &str, hits: &mut Vec<String>) {
    let in_allowed_file = file.ends_with(Path::new("memory").join(TOKIO_FILE));
    for (at, _) in line.match_indices("tokio") {
        let rest = &line[at + "tokio".len()..];
        if !in_allowed_file {
            hits.push(format!(
                "{}:{number}: `tokio` outside src/memory/{TOKIO_FILE}",
                file.display()
            ));
        } else if !TOKIO_ALLOWED_PATHS
            .iter()
            .any(|path| rest.starts_with(path))
        {
            hits.push(format!(
                "{}:{number}: `tokio{rest}` is not tokio::sync::Mutex or tokio::time::",
                file.display()
            ));
        }
    }
}

#[test]
fn manifest_has_only_allowed_dependencies() {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("Cargo.toml");
    let mut section = String::new();
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_owned();
            if section == "[build-dependencies]" || section.starts_with("[target.") {
                problems.push(format!("Cargo.toml: forbidden section {section}"));
            }
            if let Some(name) = section
                .strip_prefix("[dependencies.")
                .and_then(|rest| rest.strip_suffix(']'))
            {
                found.push(name.trim().to_owned());
                if name.trim() == "tokio" {
                    problems.push(format!("Cargo.toml: write tokio as `{TOKIO_DEPENDENCY}`"));
                }
                if !ALLOWED_DEPENDENCIES.contains(&name.trim()) {
                    problems.push(format!("Cargo.toml: dependency `{name}` is not allowed"));
                }
            }
            continue;
        }
        if section == "[dependencies]" && !line.is_empty() && !line.starts_with('#') {
            let name = line.split(['=', '.']).next().unwrap_or_default().trim();
            if name == "tokio" && line != TOKIO_DEPENDENCY {
                problems.push(format!(
                    "Cargo.toml: `{line}`, expected `{TOKIO_DEPENDENCY}`"
                ));
            }
            found.push(name.to_owned());
            if !ALLOWED_DEPENDENCIES.contains(&name) {
                problems.push(format!("Cargo.toml: dependency `{name}` is not allowed"));
            }
        }
        if section == "[package]" && line.starts_with("build") {
            problems.push(format!("Cargo.toml: `{line}`"));
        }
    }
    found.sort();
    if found != ALLOWED_DEPENDENCIES {
        problems.push(format!(
            "Cargo.toml: [dependencies] is {found:?}, expected exactly {ALLOWED_DEPENDENCIES:?}"
        ));
    }
    if manifest_dir().join("build.rs").exists() {
        problems.push("build.rs exists".to_owned());
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
