//! `xtask/skill-pending.toml` (P1-P7): skill checks that a later story
//! implements. The only known check is `component-entries` (Story 4.1: every
//! component builder has its entry in the skill). It must stay pending until
//! then, and it is due, which fails, once the snapshot exports a component
//! builder type (a `pub struct`/`pub enum` outside `noctis::rules::` named
//! like a Components V2 component). This module implements only that
//! registration and trigger, never the check itself.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::{Problems, Repo};
use crate::public_api::Line;
use crate::rules_catalog::pending::is_story;

const SUBJECT: &str = "skill-pending.toml";
const PATH: &str = "xtask/skill-pending.toml";
const COMPONENT_ENTRIES: &str = "component-entries";

/// The component builder names that make `component-entries` due.
const BUILDERS: &[&str] = &[
    "Container",
    "Section",
    "TextDisplay",
    "Thumbnail",
    "MediaGallery",
    "File",
    "Separator",
    "ActionRow",
    "RouteButton",
    "LinkButton",
    "PremiumButton",
    "StringSelect",
    "UserSelect",
    "RoleSelect",
    "MentionableSelect",
    "ChannelSelect",
    "Label",
    "TextInput",
    "FileUpload",
    "RadioGroup",
    "CheckboxGroup",
    "Checkbox",
];

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    check: Vec<CheckEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckEntry {
    name: String,
    story: String,
}

/// Validates the pending file; returns the number of pending checks.
pub(super) fn check(repo: &Repo<'_>, snapshot: Option<&[Line]>, problems: &mut Problems) -> usize {
    let Some(text) = repo.read(PATH, problems) else {
        return 0;
    };
    let file: File = match toml::from_str(&text) {
        Ok(file) => file,
        Err(err) => {
            problems.add(SUBJECT, err.message().replace('\n', " ").trim().to_owned());
            return 0;
        }
    };
    let mut pending = BTreeSet::new();
    for entry in file.check {
        if entry.name != COMPONENT_ENTRIES {
            problems.add(SUBJECT, format!("unknown check {}", entry.name));
        } else if !is_story(&entry.story) {
            problems.add(
                SUBJECT,
                format!("{}: story must be <epic>.<story>", entry.name),
            );
        } else if !pending.insert(entry.name.clone()) {
            problems.add(SUBJECT, format!("duplicate entry {}", entry.name));
        }
    }
    if !pending.contains(COMPONENT_ENTRIES) {
        problems.add(
            SUBJECT,
            format!("{COMPONENT_ENTRIES} must stay pending until Story 4.1 implements it"),
        );
    } else if let Some(lines) = snapshot {
        for builder in due_builders(lines) {
            problems.add(
                SUBJECT,
                format!(
                    "{COMPONENT_ENTRIES}: the snapshot exports component builder {builder}; implement the component-entry check and remove this entry (Story 4.1)"
                ),
            );
        }
    }
    pending.len()
}

/// Component builder types the snapshot exports outside `noctis::rules`.
fn due_builders(lines: &[Line]) -> impl Iterator<Item = &str> {
    lines
        .iter()
        .filter(|l| {
            l.kind == crate::public_api::Kind::Item
                && matches!(l.keyword, Some("struct" | "enum"))
                && !l.path.starts_with("noctis::rules::")
                && BUILDERS.contains(&l.name())
        })
        .map(|l| l.path.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_api::parse;

    #[test]
    fn p1_p2_due_trigger() {
        let lines = parse(
            "#[non_exhaustive] pub struct noctis::rules::component::Container<'a>\n\
             pub fn noctis::ui::Container()\n\
             pub struct noctis::ui::Containers",
        )
        .unwrap();
        assert_eq!(due_builders(&lines).count(), 0);
        let lines = parse("pub struct noctis::ui::Container\npub enum noctis::ui::Label").unwrap();
        assert_eq!(
            due_builders(&lines).collect::<Vec<_>>(),
            ["noctis::ui::Container", "noctis::ui::Label"]
        );
    }
}
