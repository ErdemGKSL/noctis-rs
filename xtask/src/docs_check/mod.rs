//! `docs-check [--root <path>]`: the offline link check of the launch
//! documentation (FR-74, Story 14.6). It never fetches a URL.
//!
//! # Checks
//!
//! - **Markdown links** ([`markdown`], [`link`]): in `README.md`,
//!   `CHANGELOG.md` and every `*.md` under `docs/` and `examples/` (any depth,
//!   `.claude` included; `target` and `.git` directories and symlinked
//!   directories skipped), every link, image and raw HTML `src`/`href` outside
//!   code resolves: relative paths against the linking file and same-repo
//!   URLs (`<repository>/blob/main/<path>`, `<repository>/tree/main/<path>`,
//!   `repository` from `[workspace.package]` of `Cargo.toml`) against the
//!   root, never leaving it; root-relative and empty targets are errors;
//!   other URLs are skipped;
//! - **anchors** ([`slug`]): a fragment on a Markdown target names one of its
//!   GitHub heading slugs or HTML `id`/`name` values (compared
//!   percent-decoded and lowercase); a fragment on any other target only
//!   needs the target to exist;
//! - **guides**: `README.md` and `docs/guides/README.md` link every guide
//!   (`docs/guides/*.md` but the index itself);
//! - **Rust sources** ([`rust`]): in every `*.rs` under `crates/*/src`, every
//!   same-repo URL resolves like a Markdown link, and no doc comment opens an
//!   `ignore` doctest (use `no_run` or `compile_fail`).
//!
//! # Output
//!
//! Every problem is one stderr line `docs-check: <subject>: <message>`
//! (`<subject>` is `<path>:<line>` or a bare path), sorted and deduplicated,
//! and the exit code is 1. Success prints `docs-check: ok (<m> Markdown files,
//! <l> links checked, <g> guides linked, <r> Rust files scanned)`.

mod link;
mod markdown;
mod rust;
mod slug;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use link::{SameRepo, Target};
use markdown::Doc;

/// The guides directory; its `README.md` is the index.
const GUIDES_DIR: &str = "docs/guides";
/// The guides index.
const GUIDES_INDEX: &str = "docs/guides/README.md";
/// The crate README.
const README: &str = "README.md";
/// The top-level Markdown files checked when present.
const TOP_LEVEL: [&str; 2] = [README, "CHANGELOG.md"];
/// The directories whose Markdown files are checked.
const MARKDOWN_DIRS: [&str; 2] = ["docs", "examples"];
/// The directory whose crates' `src/` are scanned.
const CRATES_DIR: &str = "crates";
/// Directory names never walked into.
const SKIPPED_DIRS: [&str; 2] = ["target", ".git"];

/// One problem; field order is the report's sort order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Problem {
    subject: String,
    message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "docs-check: {}: {}", self.subject, self.message)
    }
}

/// The sorted, deduplicated problem set (E4).
#[derive(Debug, Default)]
struct Problems(BTreeSet<Problem>);

impl Problems {
    fn add(&mut self, subject: impl Into<String>, message: impl Into<String>) {
        self.0.insert(Problem {
            subject: subject.into(),
            message: message.into(),
        });
    }
}

/// Runs the check on the repository at `root`.
pub(crate) fn run(root: &Path) -> ExitCode {
    let mut checker = Checker {
        root,
        same_repo: None,
        docs: BTreeMap::new(),
        links: 0,
        problems: Problems::default(),
    };
    let summary = checker.check();
    if checker.problems.0.is_empty() {
        eprintln!("docs-check: ok ({summary})");
        ExitCode::SUCCESS
    } else {
        for problem in &checker.problems.0 {
            eprintln!("{problem}");
        }
        ExitCode::FAILURE
    }
}

/// The check's state; paths are repo-relative with `/`.
struct Checker<'a> {
    root: &'a Path,
    same_repo: Option<SameRepo>,
    /// Parsed Markdown files by path; `None` when unreadable or not UTF-8.
    docs: BTreeMap<String, Option<Doc>>,
    /// Links checked so far (external URLs are not counted).
    links: usize,
    problems: Problems,
}

impl Checker<'_> {
    /// Runs every check; returns the success summary.
    fn check(&mut self) -> String {
        self.same_repo = self.repository().map(|url| SameRepo::new(&url));

        let markdown = self.markdown_files();
        let mut linked: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for file in &markdown {
            let doc = self.read_doc(file, true);
            let Some(links) = doc.map(|d| d.links.clone()) else {
                continue;
            };
            let base = parent(file);
            for link in links {
                let subject = format!("{file}:{}", link.line);
                if let Some(target) = self.check_link(&subject, file, base, &link.dest) {
                    linked.entry(file.clone()).or_default().insert(target);
                }
            }
        }

        let guides = self.check_guides(&linked);
        let rust = self.check_rust();
        format!(
            "{} Markdown files, {} links checked, {} guides linked, {rust} Rust files scanned",
            markdown.len(),
            self.links,
            guides,
        )
    }

    /// `[workspace.package] repository` of `Cargo.toml`, one trailing `/`
    /// trimmed (E2).
    fn repository(&mut self) -> Option<String> {
        let url = fs::read_to_string(self.root.join("Cargo.toml"))
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|manifest| {
                manifest
                    .get("workspace")?
                    .get("package")?
                    .get("repository")?
                    .as_str()
                    .map(str::to_owned)
            });
        if url.is_none() {
            self.problems
                .add("Cargo.toml", "no [workspace.package] repository");
        }
        url
    }

    /// The Markdown set M, sorted.
    fn markdown_files(&self) -> Vec<String> {
        let mut files: Vec<String> = TOP_LEVEL
            .iter()
            .filter(|rel| self.root.join(rel).is_file())
            .map(|rel| (*rel).to_owned())
            .collect();
        for dir in MARKDOWN_DIRS {
            walk(self.root, dir, "md", &mut files);
        }
        files.sort();
        files.dedup();
        files
    }

    /// The parsed Markdown file `rel`, read once. A file of the Markdown set
    /// (`report`) that is not UTF-8 or unreadable is a problem; a link
    /// target outside it only loses its anchor check.
    fn read_doc(&mut self, rel: &str, report: bool) -> Option<&Doc> {
        if !self.docs.contains_key(rel) {
            let doc = match fs::read(self.root.join(rel)).map(String::from_utf8) {
                Ok(Ok(src)) => Some(markdown::parse(&src)),
                Ok(Err(_)) => {
                    if report {
                        self.problems.add(rel, "not UTF-8");
                    }
                    None
                }
                Err(err) => {
                    if report {
                        self.problems.add(rel, format!("cannot read: {err}"));
                    }
                    None
                }
            };
            self.docs.insert(rel.to_owned(), doc);
        }
        self.docs.get(rel).and_then(Option::as_ref)
    }

    /// Checks one link of `file` (whose directory is `base`); returns the
    /// repo-relative path it resolves to.
    fn check_link(&mut self, subject: &str, file: &str, base: &str, dest: &str) -> Option<String> {
        let target = link::classify(dest, self.same_repo.as_ref());
        if target == Target::External {
            return None;
        }
        self.links += 1;
        match target {
            Target::External => None,
            Target::Empty => {
                self.problems.add(subject, "empty link target");
                None
            }
            Target::RootRelative => {
                self.problems.add(
                    subject,
                    format!("root-relative link {dest}: use a path relative to this file"),
                );
                None
            }
            Target::SameFile { fragment } => {
                self.check_anchor(subject, file, dest, &fragment);
                None
            }
            Target::Path {
                from_root,
                path,
                fragment,
            } => {
                let base = if from_root { "" } else { base };
                let Some(resolved) = link::resolve(base, &path) else {
                    self.problems
                        .add(subject, format!("link leaves the repository: {dest}"));
                    return None;
                };
                match fs::metadata(self.root.join(&resolved)) {
                    Ok(meta) => {
                        let fragment = fragment.unwrap_or_default();
                        if meta.is_file() && is_markdown(&resolved) {
                            self.check_anchor(subject, &resolved, dest, &fragment);
                        }
                    }
                    Err(err) => {
                        let reason = if err.kind() == io::ErrorKind::NotFound {
                            "no such file or directory".to_owned()
                        } else {
                            err.to_string()
                        };
                        self.problems
                            .add(subject, format!("broken link {dest}: {reason}"));
                    }
                }
                Some(resolved)
            }
        }
    }

    /// Checks that `fragment` (decoded) is an anchor of the Markdown file
    /// `target`; an empty fragment is the top of the file (A8).
    fn check_anchor(&mut self, subject: &str, target: &str, dest: &str, fragment: &str) {
        if fragment.is_empty() {
            return;
        }
        let wanted = fragment.to_lowercase();
        let Some(doc) = self.read_doc(target, false) else {
            return;
        };
        if !doc.anchors.contains(&wanted) {
            let raw = link::raw_fragment(dest);
            self.problems.add(
                subject,
                format!("broken anchor #{raw}: no such heading in {target}"),
            );
        }
    }

    /// G1-G6: `README.md` and the guides index link every guide; returns how
    /// many guides there are.
    fn check_guides(&mut self, linked: &BTreeMap<String, BTreeSet<String>>) -> usize {
        let Ok(entries) = fs::read_dir(self.root.join(GUIDES_DIR)) else {
            return 0;
        };
        let mut guides: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .map(|name| format!("{GUIDES_DIR}/{name}"))
            .filter(|path| is_markdown(path) && path != GUIDES_INDEX)
            .collect();
        guides.sort();
        if guides.is_empty() {
            return 0;
        }
        let empty = BTreeSet::new();
        for index in [README, GUIDES_INDEX] {
            if index == GUIDES_INDEX && !self.root.join(index).is_file() {
                self.problems.add(index, "missing guides index");
                continue;
            }
            let targets = linked.get(index).unwrap_or(&empty);
            for guide in guides.iter().filter(|g| !targets.contains(*g)) {
                self.problems
                    .add(index, format!("guide not linked: {guide}"));
            }
        }
        guides.len()
    }

    /// R1-R3 over every `*.rs` under `crates/*/src`; returns how many files
    /// were scanned.
    fn check_rust(&mut self) -> usize {
        let Ok(entries) = fs::read_dir(self.root.join(CRATES_DIR)) else {
            return 0;
        };
        let mut crates: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        crates.sort();
        let mut files = Vec::new();
        for name in crates {
            walk(
                self.root,
                &format!("{CRATES_DIR}/{name}/src"),
                "rs",
                &mut files,
            );
        }
        for file in &files {
            let src = match fs::read(self.root.join(file)).map(String::from_utf8) {
                Ok(Ok(src)) => src,
                Ok(Err(_)) => {
                    self.problems.add(file, "not UTF-8");
                    continue;
                }
                Err(err) => {
                    self.problems.add(file, format!("cannot read: {err}"));
                    continue;
                }
            };
            for (n, line) in src.lines().enumerate() {
                let subject = format!("{file}:{}", n + 1);
                if rust::is_ignore_fence(line) {
                    self.problems.add(
                        &subject,
                        "doctest marked ignore: make it run, no_run or compile_fail",
                    );
                }
                let urls: Vec<String> = self
                    .same_repo
                    .as_ref()
                    .map(|repo| {
                        rust::repo_urls(line, repo)
                            .into_iter()
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                for url in urls {
                    self.check_link(&subject, file, "", &url);
                }
            }
        }
        files.len()
    }
}

/// The directory of a repo-relative path (`""` for the root).
fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn is_markdown(rel: &str) -> bool {
    Path::new(rel).extension().is_some_and(|e| e == "md")
}

/// Appends every file below `rel` with extension `ext`, recursively, to
/// `out`; skips `target`, `.git` and symlinked directories. A missing
/// directory adds nothing.
fn walk(root: &Path, rel: &str, ext: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join(rel)) else {
        return;
    };
    let mut entries: Vec<(String, fs::FileType)> = entries
        .flatten()
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, kind) in entries {
        let path = format!("{rel}/{name}");
        if kind.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(root, &path, ext, out);
            }
        } else if Path::new(&name).extension().is_some_and(|e| e == ext)
            && (kind.is_file() || root.join(&path).is_file())
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_of_root_files_is_root() {
        assert_eq!(parent("README.md"), "");
        assert_eq!(parent("docs/guides/README.md"), "docs/guides");
    }

    #[test]
    fn walk_skips_target_and_git() {
        let root = std::env::temp_dir().join(format!("xtask-docs-walk-{}", std::process::id()));
        for rel in [
            "docs/a.md",
            "docs/target/b.md",
            "docs/.git/c.md",
            "docs/sub/d.md",
            "docs/e.txt",
        ] {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let mut out = Vec::new();
        walk(&root, "docs", "md", &mut out);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(out, ["docs/a.md", "docs/sub/d.md"]);
    }
}
