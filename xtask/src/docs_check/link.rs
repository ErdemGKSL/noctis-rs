//! Link targets: what a link points at (L5-L9, L14-L16) and how a relative
//! path resolves inside the repository.

/// The same-repo URL forms of the repository at `U` (L15): `U/blob/main/`
/// and `U/tree/main/`.
#[derive(Debug, Clone)]
pub(super) struct SameRepo {
    prefixes: [String; 2],
}

impl SameRepo {
    /// The forms of the repository URL `url` (one trailing `/` trimmed).
    pub(super) fn new(url: &str) -> Self {
        let url = url.strip_suffix('/').unwrap_or(url);
        Self {
            prefixes: [format!("{url}/blob/main/"), format!("{url}/tree/main/")],
        }
    }

    /// The `blob` and `tree` prefixes, each ending in `/main/`.
    pub(super) fn prefixes(&self) -> &[String; 2] {
        &self.prefixes
    }

    /// The repo-relative rest of a same-repo URL, or `None` for any other
    /// string (another ref, another form, another letter case: L16).
    fn strip<'a>(&self, dest: &'a str) -> Option<&'a str> {
        self.prefixes
            .iter()
            .find_map(|p| dest.strip_prefix(p.as_str()))
    }
}

/// What a link target points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// `[x]()` (L7).
    Empty,
    /// A URL with a scheme, or `//host`, that is not a same-repo URL (L14,
    /// L16): never fetched, not counted.
    External,
    /// `/docs/a.md` (L6).
    RootRelative,
    /// `#fragment` in the linking file (A1, A8).
    SameFile {
        /// The fragment, percent-decoded.
        fragment: String,
    },
    /// A path, relative to the linking file or (for a same-repo URL) to the
    /// repository root.
    Path {
        /// `true` for a same-repo URL (L15).
        from_root: bool,
        /// The path, query dropped (L9) and percent-decoded (L8).
        path: String,
        /// The fragment, percent-decoded; `None` without a `#`.
        fragment: Option<String>,
    },
}

/// Classifies a link target as written.
pub(super) fn classify(dest: &str, same_repo: Option<&SameRepo>) -> Target {
    if dest.is_empty() {
        return Target::Empty;
    }
    if let Some(fragment) = dest.strip_prefix('#') {
        return Target::SameFile {
            fragment: percent_decode(fragment),
        };
    }
    if let Some(rest) = same_repo.and_then(|r| r.strip(dest)) {
        return path_target(rest, true);
    }
    if dest.starts_with("//") || has_scheme(dest) {
        return Target::External;
    }
    if dest.starts_with('/') {
        return Target::RootRelative;
    }
    path_target(dest, false)
}

/// The raw (not decoded) fragment of a target, for messages.
pub(super) fn raw_fragment(dest: &str) -> &str {
    dest.split_once('#').map_or("", |(_, fragment)| fragment)
}

fn path_target(rest: &str, from_root: bool) -> Target {
    let (before, fragment) = match rest.split_once('#') {
        Some((before, fragment)) => (before, Some(percent_decode(fragment))),
        None => (rest, None),
    };
    let path = before.split_once('?').map_or(before, |(path, _)| path);
    Target::Path {
        from_root,
        path: percent_decode(path),
        fragment,
    }
}

/// Whether `dest` starts with a URI scheme (`[A-Za-z][A-Za-z0-9+.-]*:`).
fn has_scheme(dest: &str) -> bool {
    let Some((scheme, _)) = dest.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Decodes `%XX` escapes; an invalid escape (`%G1`, a trailing `%`) stays as
/// written, and invalid UTF-8 is replaced (L8, A7).
pub(super) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = decoded {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Joins `path` onto the repo-relative directory `base` lexically; `None`
/// when a `..` leaves the repository (L5). The result has no `.`, `..` or
/// empty segments (`""` is the root).
pub(super) fn resolve(base: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const U: &str = "https://github.com/o/r";

    fn path(from_root: bool, path: &str, fragment: Option<&str>) -> Target {
        Target::Path {
            from_root,
            path: path.to_owned(),
            fragment: fragment.map(str::to_owned),
        }
    }

    #[test]
    fn classifies_targets() {
        let repo = SameRepo::new("https://github.com/o/r/");
        let cases = [
            ("", Target::Empty),
            ("/docs/a.md", Target::RootRelative),
            ("my%20file.md", path(false, "my file.md", None)),
            ("a%G1b%", path(false, "a%G1b%", None)),
            ("a.md?plain=1", path(false, "a.md", None)),
            ("a.md?plain=1#Top", path(false, "a.md", Some("Top"))),
            ("a.md#", path(false, "a.md", Some(""))),
            ("../../outside.md", path(false, "../../outside.md", None)),
            (
                "#%C3%A7al",
                Target::SameFile {
                    fragment: "çal".to_owned(),
                },
            ),
            (
                "#",
                Target::SameFile {
                    fragment: String::new(),
                },
            ),
            ("https://discord.com/developers#x", Target::External),
            ("http://example.com", Target::External),
            ("mailto:a@b.c", Target::External),
            ("ftp+x.y-z:abc", Target::External),
            ("//cdn.example.com/a.png", Target::External),
            (
                "https://github.com/o/r/blob/main/docs/guides/cluster.md#shard-ranges",
                path(true, "docs/guides/cluster.md", Some("shard-ranges")),
            ),
            (
                "https://github.com/o/r/tree/main/examples/",
                path(true, "examples/", None),
            ),
            (
                "https://github.com/o/r/blob/0123abc/docs/a.md",
                Target::External,
            ),
            ("https://github.com/o/r/issues/1", Target::External),
            ("https://github.com/o/r", Target::External),
            (
                "https://github.com/O/r/blob/main/docs/a.md",
                Target::External,
            ),
        ];
        for (dest, expected) in cases {
            assert_eq!(classify(dest, Some(&repo)), expected, "{dest}");
        }
        assert_eq!(
            classify(&format!("{U}/blob/main/a.md"), None),
            Target::External
        );
    }

    #[test]
    fn resolves_lexically() {
        assert_eq!(
            resolve("docs/guides", "voice.md").as_deref(),
            Some("docs/guides/voice.md")
        );
        assert_eq!(
            resolve("docs/guides", "../discord-rules.md").as_deref(),
            Some("docs/discord-rules.md")
        );
        assert_eq!(
            resolve("", "./examples/project-template/").as_deref(),
            Some("examples/project-template")
        );
        assert_eq!(resolve("docs", "..").as_deref(), Some(""));
        assert_eq!(resolve("", "../../outside.md"), None);
        assert_eq!(resolve("docs", "../../x.md"), None);
        assert_eq!(resolve("", "a/../../x.md"), None);
    }

    #[test]
    fn raw_fragment_is_as_written() {
        assert_eq!(raw_fragment("a.md#%C3%A7"), "%C3%A7");
        assert_eq!(raw_fragment("a.md"), "");
    }
}
