//! The Rust-source half: same-repo URLs anywhere in a line (R1) and
//! `ignore` doctest fences in doc comments (R2, R3).

use super::link::SameRepo;

/// The characters that end a URL found in Rust source, besides whitespace.
const URL_END: &[char] = &[')', '>', '"', '\'', '`', ']'];

/// Every same-repo URL in `line`, in order: from `U/blob/main/` or
/// `U/tree/main/` up to whitespace or one of `` )>"'`] ``.
pub(super) fn repo_urls<'a>(line: &'a str, same_repo: &SameRepo) -> Vec<&'a str> {
    let mut starts: Vec<usize> = same_repo
        .prefixes()
        .iter()
        .flat_map(|prefix| line.match_indices(prefix.as_str()).map(|(i, _)| i))
        .collect();
    starts.sort_unstable();
    starts
        .into_iter()
        .map(|start| {
            let rest = &line[start..];
            let end = rest
                .find(|c: char| c.is_whitespace() || URL_END.contains(&c))
                .unwrap_or(rest.len());
            &rest[..end]
        })
        .collect()
}

/// Whether `line` is a doc comment line (`///` or `//!`) opening or closing
/// a code fence whose info string has an `ignore` token (tokens split on `,`
/// and whitespace).
pub(super) fn is_ignore_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(doc) = trimmed
        .strip_prefix("///")
        .or_else(|| trimmed.strip_prefix("//!"))
    else {
        return false;
    };
    let doc = doc.trim();
    let info = if doc.starts_with("```") {
        doc.trim_start_matches('`')
    } else if doc.starts_with("~~~") {
        doc.trim_start_matches('~')
    } else {
        return false;
    };
    info.split(|c: char| c == ',' || c.is_whitespace())
        .any(|token| token == "ignore")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ignore_fences() {
        for line in [
            "/// ```ignore",
            "    //! ```rust,ignore",
            "///```text, ignore",
            "/// ~~~ignore",
            "/// ```` rust ignore",
        ] {
            assert!(is_ignore_fence(line), "{line}");
        }
        for line in [
            "/// ```",
            "/// ```no_run",
            "/// ```compile_fail,E0277",
            "/// ```text",
            "/// ```should_panic",
            "/// ```ignored",
            "/// ```ignore-wasm32",
            "// ```ignore",
            "//// ```ignore",
            "let s = \"```ignore\";",
            "/// see ```ignore``` above",
        ] {
            assert!(!is_ignore_fence(line), "{line}");
        }
    }

    #[test]
    fn urls_end_at_delimiters() {
        let repo = SameRepo::new("https://github.com/o/r");
        let line = "/// [g](https://github.com/o/r/blob/main/docs/a.md#x) and \
                    <https://github.com/o/r/tree/main/examples> \"https://github.com/o/r/blob/main/b.md\" \
                    `https://github.com/o/r/blob/main/c.md` [https://github.com/o/r/blob/main/d.md] \
                    'https://github.com/o/r/blob/main/e.md' https://github.com/o/r/blob/main/f.md\tx \
                    https://github.com/o/r/blob/v1/g.md https://github.com/o/r/blob/main/h.md";
        assert_eq!(
            repo_urls(line, &repo),
            [
                "https://github.com/o/r/blob/main/docs/a.md#x",
                "https://github.com/o/r/tree/main/examples",
                "https://github.com/o/r/blob/main/b.md",
                "https://github.com/o/r/blob/main/c.md",
                "https://github.com/o/r/blob/main/d.md",
                "https://github.com/o/r/blob/main/e.md",
                "https://github.com/o/r/blob/main/f.md",
                "https://github.com/o/r/blob/main/h.md",
            ]
        );
    }
}
