//! `noctis-rules` stays sync, I/O-free and dependency-light (AD-3).

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "std::fs",
    "std::net",
    "std::io",
    "std::process",
    "std::thread",
    "std::env",
    "async fn",
    "async move",
    "async {",
    ".await",
    "tokio",
    "unsafe",
];

const ALLOWED_DEPENDENCIES: &[&str] = &["regex"];

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
fn source_has_no_io_async_or_unsafe() {
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
            for token in FORBIDDEN {
                if line.contains(token) {
                    hits.push(format!("{}:{}: `{token}`", file.display(), number + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "forbidden tokens in noctis-rules:\n{}",
        hits.join("\n")
    );
}

#[test]
fn manifest_has_only_allowed_dependencies() {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("Cargo.toml");
    let mut section = String::new();
    let mut problems = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.to_owned();
            if section == "[build-dependencies]" || section.starts_with("[target.") {
                problems.push(format!("Cargo.toml: forbidden section {section}"));
            }
            if let Some(name) = section
                .strip_prefix("[dependencies.")
                .and_then(|rest| rest.strip_suffix(']'))
                && !ALLOWED_DEPENDENCIES.contains(&name.trim())
            {
                problems.push(format!("Cargo.toml: dependency `{name}` is not allowed"));
            }
            continue;
        }
        if section == "[dependencies]" && !line.is_empty() && !line.starts_with('#') {
            let name = line.split(['=', '.']).next().unwrap_or_default().trim();
            if !ALLOWED_DEPENDENCIES.contains(&name) {
                problems.push(format!("Cargo.toml: dependency `{name}` is not allowed"));
            }
        }
        if section == "[package]" && line.starts_with("build") {
            problems.push(format!("Cargo.toml: `{line}`"));
        }
    }
    if manifest_dir().join("build.rs").exists() {
        problems.push("build.rs exists".to_owned());
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
