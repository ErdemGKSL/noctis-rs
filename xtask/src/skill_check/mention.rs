//! The mention check (M1-M9): every public item of the snapshot is named in
//! the Usage Skill's Markdown (all `*.md` files below the skill directory).
//!
//! An item is mentioned when its last path segment appears as a whole
//! identifier token: a maximal run of ASCII `[A-Za-z0-9_]`, case-sensitive;
//! any other character, non-ASCII included, separates tokens. A rule-id
//! constant (`RuleId::REG_06`) is also mentioned by its catalog id (`REG-06`)
//! as a whole token of `[A-Za-z0-9_-]`. Trait-impl members and impl headers
//! are not items.

use std::collections::BTreeSet;

use super::{Problems, Repo};
use crate::public_api::{self, Line};

/// Reads every `*.md` file below `dir` (M8); `None` when none can be read.
pub(super) fn read_skill(
    repo: &Repo<'_>,
    dir: &str,
    problems: &mut Problems,
) -> Option<Vec<(String, String)>> {
    let Some(paths) = repo.files(dir, "md") else {
        problems.add(dir, "cannot read: not a directory");
        return None;
    };
    if paths.is_empty() {
        problems.add(dir, "cannot read: no *.md files");
        return None;
    }
    let mut files = Vec::new();
    let mut failed = false;
    for path in paths {
        match repo.read(&path, problems) {
            Some(text) => files.push((path, text)),
            None => failed = true,
        }
    }
    (!failed).then_some(files)
}

/// Splits `text` into maximal runs of bytes accepted by `keep`.
fn tokens(text: &str, keep: impl Fn(u8) -> bool) -> BTreeSet<&str> {
    text.split(|c: char| !(c.is_ascii() && keep(c as u8)))
        .filter(|t| !t.is_empty())
        .collect()
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `^[A-Z][A-Z0-9]{1,3}_[0-9]{2}$` under a `RuleId` parent → its catalog id.
fn catalog_id(line: &Line) -> Option<String> {
    let parent = line.path.rsplit_once("::")?.0;
    if !parent.ends_with("::RuleId") && parent != "RuleId" {
        return None;
    }
    let id = line.name().replacen('_', "-", 1);
    crate::rules_catalog::is_rule_id(&id).then_some(id)
}

/// Reports unmentioned items; returns the number of items required.
pub(super) fn check(lines: &[Line], files: &[(String, String)], problems: &mut Problems) -> usize {
    let text: String = files
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let idents = tokens(&text, is_ident_byte);
    let ids = tokens(&text, |b| is_ident_byte(b) || b == b'-');
    let items = public_api::items(lines);
    for item in &items {
        let mentioned = idents.contains(item.name())
            || catalog_id(item).is_some_and(|id| ids.contains(id.as_str()));
        if !mentioned {
            problems.add(
                item.path.as_str(),
                "public item not mentioned in the Usage Skill",
            );
        }
    }
    items.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems_for(snapshot: &str, skill: &str) -> Vec<String> {
        let lines = public_api::parse(snapshot).unwrap();
        let mut problems = Problems::default();
        check(
            &lines,
            &[("s.md".to_owned(), skill.to_owned())],
            &mut problems,
        );
        problems.0.iter().map(ToString::to_string).collect()
    }

    const TREE: &str = "pub fn noctis::rules::command::validate_tree()";
    const MISSING: &str = "skill-check: noctis::rules::command::validate_tree: public item not mentioned in the Usage Skill";

    #[test]
    fn m1_m2_whole_identifier() {
        assert!(problems_for(TREE, "call `validate_tree(&x)`.").is_empty());
        assert_eq!(
            problems_for(TREE, "validate_trees xvalidate_tree"),
            [MISSING]
        );
    }

    #[test]
    fn m3_rule_id_consts() {
        let snap = "pub const noctis::rules::RuleId::REG_06: noctis::rules::RuleId";
        assert!(problems_for(snap, "| REG-06 | x |").is_empty());
        assert!(problems_for(snap, "RuleId::REG_06").is_empty());
        assert_eq!(problems_for(snap, "REG-061 XREG-06 REG-06-x").len(), 1);
        // Not under RuleId: only the identifier counts.
        assert_eq!(
            problems_for("pub const noctis::X::REG_06: u8", "REG-06").len(),
            1
        );
    }

    #[test]
    fn m4_trait_impl_members_not_required() {
        let snap = "pub struct noctis::X\nimpl core::fmt::Debug for noctis::X\npub fn noctis::X::fmt(&self)";
        assert!(problems_for(snap, "X").is_empty());
    }

    #[test]
    fn m5_duplicates_once() {
        assert_eq!(problems_for(&format!("{TREE}\n{TREE}"), ""), [MISSING]);
    }

    #[test]
    fn m6_case_sensitive() {
        assert_eq!(problems_for("pub struct noctis::Limits", "limits").len(), 1);
    }

    #[test]
    fn m7_unicode_separators() {
        let snap = "pub fn noctis::discord_len()";
        assert!(problems_for(snap, "ʼdiscord_len… 😀discord_len").is_empty());
        assert!(problems_for(snap, "édiscord_lenʼ").is_empty());
    }
}
