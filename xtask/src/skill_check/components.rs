//! The component-entry check (C1-C6, FR-49): every component builder type the
//! snapshot exports has exactly one entry in the Usage Skill.
//!
//! A builder is a `struct` or `enum` of the snapshot outside `noctis::rules::`
//! and `noctis::codec::` (the codec's `Section` is a `custom_id` layout
//! section) whose name is in [`BUILDERS`]. Its entry is a Markdown heading
//! (`#` to `######`, outside fenced code) of `references/components-v2.md` or
//! `references/modals.md` that names it as a code span (`` `Name` ``); the
//! entry's body runs to the next heading of the same or a higher level and
//! must contain each of [`LABELS`]. A heading may name at most one builder.
//!
//! Q18: the text of an entry from `**Limits:**` to the next `**Where:**` or
//! `**Builder:**` (or the body's end) has a Markdown link whose target
//! (`](…)`) contains [`CATALOG`].

use std::collections::BTreeSet;

use super::Problems;
use crate::public_api::{Kind, Line};

/// The skill files that hold component entries, relative to the skill.
const ENTRY_FILES: &[&str] = &["references/components-v2.md", "references/modals.md"];

/// The labels every entry's body must contain.
const LABELS: &[&str] = &["**Where:**", "**Limits:**", "**Builder:**"];

const CHECK: &str = "component-entries";

/// The rule catalog every entry's **Limits:** must link (Q18).
const CATALOG: &str = "docs/discord-rules.md";
const LIMITS: &str = "**Limits:**";

/// The Components V2 message and modal builder names.
pub(super) const BUILDERS: &[&str] = &[
    "Container",
    "Section",
    "TextDisplay",
    "Thumbnail",
    "MediaGallery",
    "File",
    "Separator",
    "ActionRow",
    "StaticContainer",
    "StaticSection",
    "StaticActionRow",
    "RouteButton",
    "LinkButton",
    "PremiumButton",
    "StringSelect",
    "StringSelectBuilder",
    "UserSelect",
    "RoleSelect",
    "MentionableSelect",
    "ChannelSelect",
    "Label",
    "ModalLabel",
    "TextInput",
    "ModalStringSelect",
    "ModalUserSelect",
    "ModalRoleSelect",
    "ModalMentionableSelect",
    "ModalChannelSelect",
    "FileUpload",
    "RadioGroup",
    "CheckboxGroup",
    "Checkbox",
    "ModalBuilder",
    "RawComponent",
];

/// Builder names the snapshot exports outside `noctis::rules` and
/// `noctis::codec`, deduplicated and sorted.
pub(super) fn due_builders(lines: &[Line]) -> BTreeSet<&str> {
    lines
        .iter()
        .filter(|l| {
            l.kind == Kind::Item
                && matches!(l.keyword, Some("struct" | "enum"))
                && !l.path.starts_with("noctis::rules::")
                && !l.path.starts_with("noctis::codec::")
                && BUILDERS.contains(&l.name())
        })
        .map(Line::name)
        .collect()
}

/// A Markdown heading of an entry file.
struct Heading<'a> {
    /// `<file>:<line>` (1-based).
    at: String,
    /// The [`BUILDERS`] names among its code spans, in order, deduplicated.
    builders: Vec<&'a str>,
    /// Its body, up to the next heading of the same or a higher level.
    body: String,
}

/// The code spans of one line.
pub(super) fn code_spans(line: &str) -> impl Iterator<Item = &str> {
    line.split('`').skip(1).step_by(2)
}

/// The `#` level of an ATX heading line, if it is one.
fn heading_level(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    let rest = line.get(level..)?;
    ((1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' '))).then_some(level)
}

/// Every heading of `text` (fenced code skipped), with its body.
fn headings<'a>(path: &str, text: &'a str) -> Vec<Heading<'a>> {
    let mut found: Vec<(usize, usize, Vec<&'a str>)> = Vec::new();
    let lines: Vec<&'a str> = text.lines().collect();
    let mut fence: Option<&str> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (Some(open), Some(m)) if open == m => fence = None,
            (None, Some(m)) => fence = Some(m),
            (None, None) => {
                if let Some(level) = heading_level(line) {
                    let mut builders: Vec<&str> = Vec::new();
                    for span in code_spans(line) {
                        if let Some(name) = BUILDERS.iter().find(|b| **b == span)
                            && !builders.contains(name)
                        {
                            builders.push(name);
                        }
                    }
                    found.push((i, level, builders));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::with_capacity(found.len());
    for (k, (start, level, builders)) in found.iter().enumerate() {
        let end = found
            .iter()
            .skip(k + 1)
            .find(|(_, l, _)| l <= level)
            .map_or(lines.len(), |(e, _, _)| *e);
        let body = lines.get(start + 1..end).unwrap_or_default().join("\n");
        out.push(Heading {
            at: format!("{path}:{}", start + 1),
            builders: builders.clone(),
            body,
        });
    }
    out
}

/// Checks the entries of the `due` builders in the skill files `files`
/// (paths relative to the root) of the skill at `skill_dir`; returns the
/// number of builders checked.
pub(super) fn check(
    skill_dir: &str,
    due: &BTreeSet<&str>,
    files: &[(String, String)],
    problems: &mut Problems,
) -> usize {
    let entry_paths: Vec<String> = ENTRY_FILES
        .iter()
        .map(|f| format!("{skill_dir}/{f}"))
        .collect();
    let mut all = Vec::new();
    for (path, text) in files {
        if entry_paths.contains(path) {
            all.extend(headings(path, text));
        }
    }
    for heading in &all {
        if heading.builders.len() > 1 {
            problems.add(
                heading.at.as_str(),
                format!("{CHECK}: heading names more than one component builder"),
            );
        }
    }
    for name in due {
        let entries: Vec<&Heading<'_>> = all.iter().filter(|h| h.builders.contains(name)).collect();
        match entries.as_slice() {
            [] => problems.add(skill_dir, format!("{CHECK}: {name} has no entry")),
            [entry] => {
                for label in LABELS {
                    if !entry.body.contains(label) {
                        problems.add(
                            entry.at.as_str(),
                            format!("{CHECK}: entry for {name} lacks {label}"),
                        );
                    }
                }
                if !limits_link_catalog(&entry.body) {
                    problems.add(
                        entry.at.as_str(),
                        format!("{CHECK}: entry for {name}: {LIMITS} does not link {CATALOG}"),
                    );
                }
            }
            many => {
                for entry in many {
                    problems.add(
                        entry.at.as_str(),
                        format!("{CHECK}: {name} has {} entries", many.len()),
                    );
                }
            }
        }
    }
    due.len()
}

/// Q18: whether the entry's **Limits:** text links the catalog; `true` when
/// the label is absent (reported as a missing label).
fn limits_link_catalog(body: &str) -> bool {
    let Some(start) = body.find(LIMITS) else {
        return true;
    };
    let rest = &body[start + LIMITS.len()..];
    let end = ["**Where:**", "**Builder:**"]
        .iter()
        .filter_map(|label| rest.find(label))
        .min()
        .unwrap_or(rest.len());
    let limits = &rest[..end];
    limits.match_indices("](").any(|(i, _)| {
        limits[i + 2..]
            .split(')')
            .next()
            .is_some_and(|target| target.contains(CATALOG))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_api::parse;

    #[test]
    fn c1_due_builders() {
        let lines = parse(
            "#[non_exhaustive] pub struct noctis::rules::component::Container<'a>\n\
             pub fn noctis::ui::Container()\n\
             pub struct noctis::ui::Containers\n\
             #[non_exhaustive] pub enum noctis::codec::Section",
        )
        .unwrap();
        assert!(due_builders(&lines).is_empty());
        let lines = parse(
            "pub struct noctis::ui::Container\npub enum noctis::ui::Label\npub struct noctis::x::Container",
        )
        .unwrap();
        assert_eq!(
            due_builders(&lines).into_iter().collect::<Vec<_>>(),
            ["Container", "Label"]
        );
    }

    #[test]
    fn c2_headings_skip_fences_and_scope_bodies() {
        let text = "# Top\n## A `Container`\nx\n```\n# `File` in code\n```\n### Sub\n**Where:**\n## B `File`\ny\n";
        let hs = headings("f.md", text);
        let names: Vec<_> = hs
            .iter()
            .map(|h| (h.at.as_str(), h.builders.clone()))
            .collect();
        assert_eq!(
            names,
            [
                ("f.md:1", vec![]),
                ("f.md:2", vec!["Container"]),
                ("f.md:7", vec![]),
                ("f.md:9", vec!["File"]),
            ]
        );
        assert!(hs[1].body.contains("**Where:**"));
        assert!(!hs[1].body.contains('y'));
        assert_eq!(heading_level("#[derive(Button)]"), None);
        assert_eq!(heading_level("####### seven"), None);
        assert_eq!(heading_level("###"), Some(3));
    }

    #[test]
    fn q18_limits_link_the_catalog() {
        let ok = [
            "**Where:** x\n**Limits:** 40 ([catalog](../../../../../../docs/discord-rules.md#9-components-v2-messages)).\n**Builder:** y",
            "**Limits:** see [CV2-04](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)",
            "no label here",
        ];
        for body in ok {
            assert!(limits_link_catalog(body), "{body:?}");
        }
        let bad = [
            "**Limits:** 40, see docs/discord-rules.md.\n**Builder:** y",
            "**Limits:** 40.\n**Where:** [catalog](docs/discord-rules.md)",
            "**Limits:** 40 ([cheatsheet](limits-cheatsheet.md)).",
        ];
        for body in bad {
            assert!(!limits_link_catalog(body), "{body:?}");
        }
    }
}
