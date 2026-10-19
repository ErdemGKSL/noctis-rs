//! The line grammar of `cargo public-api` output, shared by `api-snapshot` and
//! `skill-check`.
//!
//! # Lines
//!
//! Every line is one of:
//!
//! - an **item**: leading `#[…]` attributes (bracket-balanced, skipped), `pub`,
//!   any keywords (`const`, `fn`, `unsafe`, `async`, `extern "C"`, `static`,
//!   `mut`, `type`, `trait`, `auto`, `union`, `use`, `macro`, `struct`, `enum`,
//!   `mod`), then the item's path. Fields and variants have no keyword
//!   (`pub noctis::rules::Limits::button_label_len: usize`);
//! - an **impl header**: `impl[<…>] [<Trait> for ]<SelfType>[ where …]`. It is a
//!   trait impl when a ` for ` occurs at depth 0 of `<>()[]`; the self type ends
//!   at a depth-0 ` where `; its path drops a leading `&`, lifetime and `mut `;
//! - an **impl member**: an item line right after an impl header (or another
//!   member) whose parent path equals the header's self path. Any other line
//!   closes the impl context.
//!
//! Paths drop every `<…>` group (angle-depth aware, `->` is not a closer) and
//! a leading `r#` per segment; a macro's trailing `!` is not part of its path.
//!
//! # Items and owners
//!
//! The **owner** of a line decides which re-export it belongs to: the item's
//! own path, or the impl's self path for headers and members. The **items** a
//! skill must mention are item lines and inherent-impl members; trait-impl
//! members (`fmt`, `clone`, `Err`) and headers are not items.
//!
//! # Hash
//!
//! The API hash is the blake3 hash of the snapshot file's bytes, as 64
//! lowercase hex digits.

/// What a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An item outside any impl context.
    Item,
    /// `impl … [for] Self`.
    ImplHeader { trait_impl: bool },
    /// An item inside the preceding impl.
    Member { trait_impl: bool },
}

/// One parsed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    /// The line as given.
    pub(crate) text: String,
    pub(crate) kind: Kind,
    /// The item's path, or the impl's self path for a header.
    pub(crate) path: String,
    /// The path that decides inclusion in a re-export's expansion.
    pub(crate) owner: String,
    /// The item's defining keyword (`fn`, `struct`, `use`, …); `None` for
    /// fields, variants and impl headers.
    pub(crate) keyword: Option<&'static str>,
}

impl Line {
    /// The last path segment.
    pub(crate) fn name(&self) -> &str {
        last_segment(&self.path)
    }

    /// Whether the skill must mention this line's path.
    pub(crate) fn is_item(&self) -> bool {
        matches!(self.kind, Kind::Item | Kind::Member { trait_impl: false })
    }
}

/// The last `::` segment of `path`.
pub(crate) fn last_segment(path: &str) -> &str {
    path.rsplit_once("::").map_or(path, |(_, name)| name)
}

/// Words that may sit between `pub` and an item's path.
const KEYWORDS: &[&str] = &[
    "const", "fn", "unsafe", "async", "extern", "static", "mut", "type", "trait", "auto", "union",
    "use", "macro", "struct", "enum", "mod", "proc",
];

/// Keywords that do not define the item's kind on their own.
const QUALIFIERS: &[&str] = &["unsafe", "async", "extern", "mut", "auto", "proc"];

/// Parses `cargo public-api` output (LF lines, an optional final newline;
/// an empty line is an error); errors are `(1-based line, line text)`.
pub(crate) fn parse(text: &str) -> Result<Vec<Line>, Vec<(usize, String)>> {
    let mut lines = Vec::new();
    let mut errors = Vec::new();
    // The open impl context: (self path, is a trait impl).
    let mut context: Option<(String, bool)> = None;
    let text = text.strip_suffix('\n').unwrap_or(text);
    for (index, raw) in text.split('\n').enumerate() {
        if let Some(line) = parse_line(raw, &mut context) {
            lines.push(line);
        } else {
            errors.push((index + 1, raw.to_owned()));
            context = None;
        }
    }
    if errors.is_empty() {
        Ok(lines)
    } else {
        Err(errors)
    }
}

fn parse_line(raw: &str, context: &mut Option<(String, bool)>) -> Option<Line> {
    let rest = skip_attributes(raw)?;
    if let Some((self_path, trait_impl)) = parse_impl(rest) {
        *context = Some((self_path.clone(), trait_impl));
        return Some(Line {
            text: raw.to_owned(),
            kind: Kind::ImplHeader { trait_impl },
            owner: self_path.clone(),
            path: self_path,
            keyword: None,
        });
    }
    let (keyword, path) = parse_item(rest)?;
    let parent = path.rsplit_once("::").map(|(parent, _)| parent);
    let (kind, owner) = match context.as_ref() {
        Some((self_path, trait_impl)) if parent == Some(self_path.as_str()) => (
            Kind::Member {
                trait_impl: *trait_impl,
            },
            self_path.clone(),
        ),
        _ => {
            *context = None;
            (Kind::Item, path.clone())
        }
    };
    Some(Line {
        text: raw.to_owned(),
        kind,
        path,
        owner,
        keyword,
    })
}

/// Skips leading `#[…]` attributes; `None` when one is unbalanced.
fn skip_attributes(mut s: &str) -> Option<&str> {
    while s.starts_with("#[") {
        let mut depth = 0_usize;
        let mut end = None;
        for (i, c) in s.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        s = s[end? + 1..].trim_start();
    }
    Some(s)
}

/// `pub [keywords] <path>…` → (kind keyword, path).
fn parse_item(s: &str) -> Option<(Option<&'static str>, String)> {
    let mut rest = s.strip_prefix("pub ")?;
    let mut keyword = None;
    loop {
        let word = rest.split(' ').next().unwrap_or_default();
        let Some(kw) = KEYWORDS.iter().find(|k| **k == word) else {
            break;
        };
        rest = rest[word.len()..].trim_start();
        if *kw == "extern" && rest.starts_with('"') {
            let close = rest[1..].find('"')? + 2;
            rest = rest[close..].trim_start();
        }
        if !QUALIFIERS.contains(kw) {
            keyword = Some(*kw);
        }
    }
    let canonical = proc_macro_path(rest);
    let (path, _) = parse_path(canonical.as_deref().unwrap_or(rest))?;
    Some((keyword, path))
}

/// The plain path of a derive or attribute proc macro, which the tool spells
/// `parent::#[derive(Name)]` or `parent::#[Name]`: `parent::Name`.
fn proc_macro_path(s: &str) -> Option<String> {
    let (parent, attr) = s.split_once("::#[")?;
    let inner = attr.strip_suffix(']')?;
    let name = inner
        .strip_prefix("derive(")
        .and_then(|n| n.strip_suffix(')'))
        .unwrap_or(inner);
    Some(format!("{parent}::{name}"))
}

/// How `cargo public-api` spells derive (`parent::#[derive(Name)]`) and
/// attribute (`parent::#[Name]`) proc macro `path`; `None` without a parent.
pub(crate) fn proc_macro_spellings(path: &str) -> Option<[String; 2]> {
    let (parent, name) = path.rsplit_once("::")?;
    Some([
        format!("{parent}::#[derive({name})]"),
        format!("{parent}::#[{name}]"),
    ])
}

/// `impl[<…>] …` → (self path, is a trait impl); `None` when not an impl
/// header or when it is malformed (the item parser then rejects it too).
fn parse_impl(s: &str) -> Option<(String, bool)> {
    let s = s.strip_prefix("unsafe ").unwrap_or(s);
    let mut rest = s.strip_prefix("impl")?;
    if rest.starts_with('<') {
        rest = &rest[skip_angle(rest)?..];
    } else if !rest.starts_with(' ') {
        return None;
    }
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    let (self_ty, trait_impl) = match find_depth0(rest, " for ") {
        Some(i) => (&rest[i + " for ".len()..], true),
        None => (rest, false),
    };
    let self_ty = match find_depth0(self_ty, " where ") {
        Some(i) => &self_ty[..i],
        None => self_ty,
    };
    let mut self_ty = self_ty.trim();
    loop {
        if let Some(r) = self_ty.strip_prefix('&') {
            self_ty = r.trim_start();
        } else if self_ty.starts_with('\'') {
            self_ty = self_ty.split_once(' ')?.1.trim_start();
        } else if let Some(r) = self_ty.strip_prefix("mut ") {
            self_ty = r.trim_start();
        } else {
            break;
        }
    }
    let (path, _) = parse_path(self_ty)?;
    Some((path, trait_impl))
}

/// The byte index of the first `needle` at depth 0 of `<>()[]`.
fn find_depth0(s: &str, needle: &str) -> Option<usize> {
    let mut depth = 0_i32;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' if prev != '-' => depth -= 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && s[i..].starts_with(needle) {
            return Some(i);
        }
        prev = c;
    }
    None
}

/// The byte length of the balanced `<…>` group starting `s`.
fn skip_angle(s: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if prev != '-' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        prev = c;
    }
    None
}

/// Reads a path at the start of `s`, dropping `<…>` groups and `r#`; returns
/// the path and the rest.
fn parse_path(s: &str) -> Option<(String, &str)> {
    let mut path = String::new();
    let mut rest = s;
    loop {
        rest = rest.strip_prefix("r#").unwrap_or(rest);
        let len = if rest.starts_with('*') {
            1
        } else {
            rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len())
        };
        if len == 0 {
            return None;
        }
        path.push_str(&rest[..len]);
        rest = &rest[len..];
        while rest.starts_with('<') {
            rest = &rest[skip_angle(rest)?..];
        }
        match rest.strip_prefix("::") {
            Some(r) if !r.starts_with('<') => {
                path.push_str("::");
                rest = r;
            }
            _ => break,
        }
    }
    Some((path, rest))
}

/// The item paths the skill must mention, in order, deduplicated.
pub(crate) fn items(lines: &[Line]) -> Vec<&Line> {
    let mut seen = std::collections::BTreeSet::new();
    lines
        .iter()
        .filter(|l| l.is_item() && seen.insert(l.path.as_str()))
        .collect()
}

/// The blake3 hash of `bytes` as 64 lowercase hex digits.
pub(crate) fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Line {
        let mut lines = parse(text).unwrap();
        assert_eq!(lines.len(), 1, "{text}");
        lines.remove(0)
    }

    fn item(text: &str, path: &str) {
        let line = one(text);
        assert_eq!(line.kind, Kind::Item, "{text}");
        assert_eq!(line.path, path, "{text}");
        assert!(line.is_item());
    }

    #[test]
    fn l1_module() {
        let line = one("pub mod noctis::rules");
        assert_eq!(line.path, "noctis::rules");
        assert_eq!(line.name(), "rules");
        assert_eq!(line.keyword, Some("mod"));
    }

    #[test]
    fn l2_attributes() {
        item(
            "#[non_exhaustive] pub struct noctis::rules::Limits",
            "noctis::rules::Limits",
        );
        let line = one("#[repr(u8)] #[non_exhaustive] pub enum noctis::rules::Actual");
        assert_eq!(line.path, "noctis::rules::Actual");
        assert_eq!(line.keyword, Some("enum"));
        item("#[doc = \"[x]\"] pub fn noctis::f()", "noctis::f");
    }

    #[test]
    fn l3_field() {
        let line = one("pub noctis::rules::Limits::button_label_len: usize");
        assert_eq!(line.path, "noctis::rules::Limits::button_label_len");
        assert_eq!(line.name(), "button_label_len");
        assert_eq!(line.keyword, None);
    }

    #[test]
    fn l4_variants() {
        item(
            "pub noctis::rules::Actual::Count(usize)",
            "noctis::rules::Actual::Count",
        );
        item(
            "pub noctis::rules::command::DeclKind::Handler::options: usize",
            "noctis::rules::command::DeclKind::Handler::options",
        );
    }

    #[test]
    fn l5_generic_path() {
        item(
            "pub const fn noctis::rules::command::CommandDecl<'a>::with_options(self, usize, usize) -> Self",
            "noctis::rules::command::CommandDecl::with_options",
        );
        item(
            "pub fn noctis::f<F: Fn(u8) -> u8, T: Into<Vec<u8>>>(F) -> T",
            "noctis::f",
        );
    }

    #[test]
    fn l6_keywords() {
        for (prefix, keyword) in [
            ("pub const", "const"),
            ("pub fn", "fn"),
            ("pub const fn", "fn"),
            ("pub unsafe fn", "fn"),
            ("pub async fn", "fn"),
            ("pub extern \"C\" fn", "fn"),
            ("pub static", "static"),
            ("pub static mut", "static"),
            ("pub type", "type"),
            ("pub trait", "trait"),
            ("pub auto trait", "trait"),
            ("pub union", "union"),
            ("pub use", "use"),
            ("pub macro", "macro"),
        ] {
            let line = one(&format!("{prefix} noctis::X::y: u8"));
            assert_eq!(line.path, "noctis::X::y", "{prefix}");
            assert_eq!(line.keyword, Some(keyword), "{prefix}");
        }
    }

    #[test]
    fn l7_macro() {
        let line = one("pub macro noctis::rgb!");
        assert_eq!(line.path, "noctis::rgb");
        assert_eq!(line.name(), "rgb");
    }

    #[test]
    fn l7_proc_macros() {
        for (text, name) in [
            ("pub proc macro noctis::prelude::#[derive(Route)]", "Route"),
            ("pub proc macro noctis::prelude::#[handler]", "handler"),
            ("pub proc macro noctis::prelude::command!", "command"),
        ] {
            let line = one(text);
            assert_eq!(line.kind, Kind::Item, "{text}");
            assert_eq!(line.keyword, Some("macro"), "{text}");
            assert_eq!(line.path, format!("noctis::prelude::{name}"), "{text}");
            assert!(line.is_item());
        }
        assert_eq!(
            proc_macro_spellings("noctis_macros::Route"),
            Some([
                "noctis_macros::#[derive(Route)]".to_owned(),
                "noctis_macros::#[Route]".to_owned()
            ])
        );
        assert_eq!(proc_macro_spellings("Route"), None);
    }

    #[test]
    fn l8_tuple_struct_and_alias() {
        item(
            "pub struct noctis::rules::MessageKey(_)",
            "noctis::rules::MessageKey",
        );
        item("pub type noctis::X = u8", "noctis::X");
        item("pub struct noctis::r#X::r#type", "noctis::X::type");
    }

    #[test]
    fn l9_inherent_impl() {
        let lines = parse(
            "impl<'a> noctis::rules::command::Choice<'a>\n\
             pub const fn noctis::rules::command::Choice<'a>::new(&'a str) -> Self",
        )
        .unwrap();
        assert_eq!(lines[0].kind, Kind::ImplHeader { trait_impl: false });
        assert_eq!(lines[0].owner, "noctis::rules::command::Choice");
        assert!(!lines[0].is_item());
        assert_eq!(lines[1].kind, Kind::Member { trait_impl: false });
        assert_eq!(lines[1].owner, "noctis::rules::command::Choice");
        assert!(lines[1].is_item());
        assert_eq!(items(&lines).len(), 1);
    }

    #[test]
    fn l10_trait_impl() {
        let lines = parse(
            "impl core::fmt::Display for noctis::rules::Violation\n\
             pub fn noctis::rules::Violation::fmt(&self) -> core::fmt::Result\n\
             impl core::str::FromStr for noctis::rules::Locale\n\
             pub type noctis::rules::Locale::Err = noctis::rules::ParseLocaleError",
        )
        .unwrap();
        assert_eq!(lines[0].kind, Kind::ImplHeader { trait_impl: true });
        assert_eq!(lines[0].owner, "noctis::rules::Violation");
        assert_eq!(lines[1].kind, Kind::Member { trait_impl: true });
        assert_eq!(lines[3].kind, Kind::Member { trait_impl: true });
        assert_eq!(lines[3].owner, "noctis::rules::Locale");
        assert!(items(&lines).is_empty());
    }

    #[test]
    fn l11_for_detection() {
        let line = one("impl<T> core::convert::From<T> for noctis::X where T: for<'a> Fn(&'a str)");
        assert_eq!(line.kind, Kind::ImplHeader { trait_impl: true });
        assert_eq!(line.path, "noctis::X");
        let line = one("impl<'a> noctis::X<'a>");
        assert_eq!(line.kind, Kind::ImplHeader { trait_impl: false });
        assert_eq!(line.path, "noctis::X");
        let line = one("impl<F: for<'a> Fn(&'a u8)> noctis::X<F>");
        assert_eq!(line.kind, Kind::ImplHeader { trait_impl: false });
        let line = one("impl<F: Fn() -> u8> noctis::T<F> for noctis::Y");
        assert_eq!(line.kind, Kind::ImplHeader { trait_impl: true });
        assert_eq!(line.path, "noctis::Y");
    }

    #[test]
    fn l12_reference_and_foreign_self() {
        assert_eq!(
            one("impl<'a> core::convert::From<u8> for &'a noctis::X").path,
            "noctis::X"
        );
        assert_eq!(
            one("impl core::convert::From<u8> for &mut noctis::X").path,
            "noctis::X"
        );
        assert_eq!(
            one("impl<T> core::convert::From<noctis::X> for alloc::sync::Arc<T>").path,
            "alloc::sync::Arc"
        );
    }

    #[test]
    fn l13_context_ends() {
        let lines = parse(
            "impl core::clone::Clone for noctis::rules::X\n\
             pub fn noctis::rules::X::clone(&self) -> Self\n\
             impl noctis::rules::X\n\
             pub fn noctis::rules::X::new() -> Self\n\
             pub fn noctis::rules::attachment::validate_file_size(usize) -> u8\n\
             pub fn noctis::rules::X::stray()",
        )
        .unwrap();
        assert_eq!(lines[1].kind, Kind::Member { trait_impl: true });
        assert_eq!(lines[3].kind, Kind::Member { trait_impl: false });
        assert_eq!(lines[4].kind, Kind::Item);
        assert_eq!(lines[5].kind, Kind::Item);
    }

    #[test]
    fn l14_trait_methods() {
        let lines = parse(
            "pub trait noctis::rules::Translate: core::marker::Send + core::marker::Sync\n\
             pub fn noctis::rules::Translate::translate(&self) -> u8",
        )
        .unwrap();
        assert!(lines.iter().all(|l| l.kind == Kind::Item));
        assert_eq!(lines[0].keyword, Some("trait"));
        assert_eq!(items(&lines).len(), 2);
    }

    #[test]
    fn l15_unicode() {
        item("pub const noctis::X::Y: &str = \"é…\"", "noctis::X::Y");
    }

    #[test]
    fn l16_garbage() {
        for bad in [
            "",
            "foo",
            "pub",
            "impl",
            "#[non_exhaustive pub struct noctis::X",
            "pub é",
        ] {
            let errors = parse(bad).unwrap_err();
            assert_eq!(errors, vec![(1, bad.to_owned())], "{bad:?}");
        }
        let errors = parse("pub mod noctis\n\npub mod noctis::x").unwrap_err();
        assert_eq!(errors, vec![(2, String::new())]);
    }

    #[test]
    fn glob_use() {
        let line = one("pub use noctis::x::*");
        assert_eq!(line.path, "noctis::x::*");
    }

    #[test]
    fn hash_is_lowercase_hex() {
        let h = hash(b"abc");
        assert_eq!(h.len(), 64);
        assert!(
            h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(
            h,
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }
}
