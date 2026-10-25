//! `xtask/rules-pending.toml`: requirements whose tests a later story adds.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::catalog::Catalog;
use super::{Evidence, Problems};

/// The subject of problems inside the pending file.
pub(crate) const SUBJECT: &str = "rules-pending.toml";
/// The pending file's repo-relative path.
pub(crate) const PATH: &str = "xtask/rules-pending.toml";
/// The one known `[[check]]`.
pub(crate) const CUSTOM_ID_CONSTANTS: &str = "custom-id-constants";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    rule: Vec<RuleEntry>,
    #[serde(default)]
    check: Vec<CheckEntry>,
}

/// A requirement whose test lands in `story`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleEntry {
    pub(crate) id: String,
    pub(crate) evidence: Evidence,
    pub(crate) story: String,
    /// Whether the rule belongs to a P2 feature; only such entries may stay
    /// pending (Story 14.4).
    #[serde(default)]
    pub(crate) p2: bool,
}

/// A whole check that stays pending until `story`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckEntry {
    pub(crate) name: String,
    pub(crate) story: String,
    /// Whether the check belongs to a P2 feature (see [`RuleEntry::p2`]).
    #[serde(default)]
    pub(crate) p2: bool,
}

/// The valid pending entries.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    pub(crate) rules: Vec<RuleEntry>,
    pub(crate) checks: BTreeSet<String>,
}

impl Pending {
    pub(crate) fn rule(&self, id: &str, evidence: Evidence) -> Option<&RuleEntry> {
        self.rules
            .iter()
            .find(|e| e.id == id && e.evidence == evidence)
    }
}

/// `^[1-9][0-9]*\.[1-9][0-9]*$` (P7).
pub(crate) fn is_story(story: &str) -> bool {
    let positive = |s: &str| {
        s.starts_with(|c: char| ('1'..='9').contains(&c)) && s.bytes().all(|b| b.is_ascii_digit())
    };
    story
        .split_once('.')
        .is_some_and(|(epic, n)| positive(epic) && positive(n))
}

/// Parses and validates the pending file's text (P4-P11). Invalid entries are
/// reported and dropped; invalid TOML yields no pending entries. A valid entry
/// without `p2 = true` is reported last and still kept pending (Story 14.4).
pub(crate) fn parse(text: &str, catalog: &Catalog, problems: &mut Problems) -> Pending {
    let file: File = match toml::from_str(text) {
        Ok(file) => file,
        Err(err) => {
            problems.add(SUBJECT, err.message().replace('\n', " ").trim().to_owned());
            return Pending::default();
        }
    };
    let mut pending = Pending::default();
    let mut seen = BTreeSet::new();
    for entry in file.rule {
        if !is_story(&entry.story) {
            problems.add(
                SUBJECT,
                format!("{}: story must be <epic>.<story>", entry.id),
            );
            continue;
        }
        let Some(row) = catalog.row(&entry.id) else {
            problems.add(SUBJECT, format!("{} is not a catalog rule", entry.id));
            continue;
        };
        let required = row
            .class
            .as_ref()
            .is_some_and(|c| c.kinds().contains(&entry.evidence));
        if !required {
            problems.add(
                SUBJECT,
                format!("{} does not require {} evidence", entry.id, entry.evidence),
            );
            continue;
        }
        if !seen.insert((entry.id.clone(), entry.evidence)) {
            problems.add(
                SUBJECT,
                format!("duplicate entry {} ({})", entry.id, entry.evidence),
            );
            continue;
        }
        if !entry.p2 {
            problems.add(
                SUBJECT,
                format!(
                    "{} ({}): only a P2-feature rule may stay pending; add its test or tag \
                     the entry p2 = true",
                    entry.id, entry.evidence
                ),
            );
        }
        pending.rules.push(entry);
    }
    for check in file.check {
        if check.name != CUSTOM_ID_CONSTANTS {
            problems.add(SUBJECT, format!("unknown check {}", check.name));
            continue;
        }
        if !is_story(&check.story) {
            problems.add(
                SUBJECT,
                format!("{}: story must be <epic>.<story>", check.name),
            );
            continue;
        }
        if !pending.checks.insert(check.name.clone()) {
            problems.add(SUBJECT, format!("duplicate entry check {}", check.name));
            continue;
        }
        if !check.p2 {
            problems.add(
                SUBJECT,
                format!(
                    "check {}: only a P2-feature check may stay pending; tag the entry p2 = true",
                    check.name
                ),
            );
        }
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::super::catalog;
    use super::*;

    const CATALOG: &str = "\
| Rule | Enforcement |
|---|---|
| **INT-01** Deadline | Runtime |
";

    /// Parses `toml` against a one-row catalog: the pending entries and the
    /// problem messages.
    fn parse_pending(toml: &str) -> (Pending, Vec<String>) {
        let mut problems = Problems::default();
        let catalog = catalog::parse(CATALOG, &mut problems);
        let mut problems = Problems::default();
        let pending = parse(toml, &catalog, &mut problems);
        let messages = problems
            .0
            .iter()
            .map(|p| format!("{}: {}", p.subject, p.message))
            .collect();
        (pending, messages)
    }

    const UNTAGGED_RULE: &str = "rules-pending.toml: INT-01 (unit): only a P2-feature rule may \
                                 stay pending; add its test or tag the entry p2 = true";
    const UNTAGGED_CHECK: &str = "rules-pending.toml: check custom-id-constants: only a \
                                  P2-feature check may stay pending; tag the entry p2 = true";

    #[test]
    fn pd1_empty() {
        let (pending, messages) = parse_pending("# nothing pending\n");
        assert!(pending.rules.is_empty() && pending.checks.is_empty());
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn pd2_tagged_rule_is_pending() {
        let (pending, messages) = parse_pending(
            "[[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"15.4\"\np2 = true\n",
        );
        assert_eq!(pending.rules.len(), 1);
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn pd3_pd4_untagged_rule_is_reported_and_kept() {
        for p2 in ["", "p2 = false\n"] {
            let (pending, messages) = parse_pending(&format!(
                "[[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"15.4\"\n{p2}"
            ));
            assert_eq!(pending.rules.len(), 1, "{p2:?}");
            assert_eq!(messages, [UNTAGGED_RULE], "{p2:?}");
        }
    }

    #[test]
    fn pd7_invalid_entry_has_no_p2_line() {
        let (pending, messages) =
            parse_pending("[[rule]]\nid = \"INT-01\"\nevidence = \"unit\"\nstory = \"4\"\n");
        assert!(pending.rules.is_empty());
        assert_eq!(
            messages,
            ["rules-pending.toml: INT-01: story must be <epic>.<story>"]
        );
    }

    #[test]
    fn pd8_untagged_check_is_reported_and_kept() {
        let (pending, messages) =
            parse_pending("[[check]]\nname = \"custom-id-constants\"\nstory = \"2.2\"\n");
        assert!(pending.checks.contains(CUSTOM_ID_CONSTANTS));
        assert_eq!(messages, [UNTAGGED_CHECK]);
    }

    #[test]
    fn pd9_tagged_check_is_pending() {
        let (pending, messages) = parse_pending(
            "[[check]]\nname = \"custom-id-constants\"\nstory = \"2.2\"\np2 = true\n",
        );
        assert!(pending.checks.contains(CUSTOM_ID_CONSTANTS));
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn p7_story_format() {
        for ok in ["4.4", "15.3", "1.10"] {
            assert!(is_story(ok), "{ok}");
        }
        for bad in ["4", "4.4.1", "0.3", "4.0", "v4.4", "", "04.4", "4."] {
            assert!(!is_story(bad), "{bad}");
        }
    }
}
