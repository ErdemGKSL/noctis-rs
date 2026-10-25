//! The catalog's `## v1 rules audit` section (Story 14.4): its three lists
//! must equal what the classifier derives from the Enforcement column.
//!
//! This is a unit test over the real catalog, not a `rules-catalog` check, so
//! the generated catalogs of the CLI tests need no audit section.

use std::collections::{BTreeMap, BTreeSet};

use super::Evidence;
use super::Problems;
use super::catalog::{self, Class, Row};

/// The audit section's heading text.
const SECTION: &str = "v1 rules audit";

/// One audited list.
#[derive(Debug, Clone, Copy)]
enum List {
    /// Rows with a `Compile-time` tier classified by construction.
    ByConstruction,
    /// Requirement-bearing rows whose every tier is `Runtime` or `Startup`.
    RuntimeOnly,
    /// Documentation-only and alias rows.
    DocOnlyOrAlias,
}

impl List {
    const ALL: [Self; 3] = [
        Self::ByConstruction,
        Self::RuntimeOnly,
        Self::DocOnlyOrAlias,
    ];

    /// The `###` subsection that lists this set.
    const fn heading(self) -> &'static str {
        match self {
            Self::ByConstruction => "By construction",
            Self::RuntimeOnly => "Runtime only",
            Self::DocOnlyOrAlias => "Documentation only and aliases",
        }
    }

    /// "a … row", as problem messages name a member.
    const fn member(self) -> &'static str {
        match self {
            Self::ByConstruction => "a by-construction row",
            Self::RuntimeOnly => "a runtime-only row",
            Self::DocOnlyOrAlias => "a documentation-only or alias row",
        }
    }

    fn contains(self, row: &Row) -> bool {
        let Some(class) = &row.class else {
            return false;
        };
        match self {
            Self::ByConstruction => class.kinds().contains(&Evidence::ByConstruction),
            Self::RuntimeOnly => {
                matches!(class, Class::Required(_))
                    && catalog::split_tiers(&row.enforcement).is_some_and(|tiers| {
                        tiers.iter().all(|tier| {
                            let tier = tier.trim();
                            tier.starts_with("Runtime") || tier.starts_with("Startup")
                        })
                    })
            }
            Self::DocOnlyOrAlias => matches!(class, Class::DocOnly | Class::Alias(_)),
        }
    }
}

/// The audit's problems over `catalog_text`, sorted and deduplicated; empty
/// when every list matches. Problems of `catalog::parse` itself are ignored.
pub(crate) fn audit_problems(catalog_text: &str) -> Vec<String> {
    let catalog = catalog::parse(catalog_text, &mut Problems::default());
    let Some(lists) = listed(catalog_text) else {
        return vec![format!("{SECTION}: section missing")];
    };
    let mut out = BTreeSet::new();
    for list in List::ALL {
        let heading = list.heading();
        let Some(cells) = lists.get(heading) else {
            out.insert(format!("{SECTION}: subsection {heading} missing"));
            continue;
        };
        let mut seen = BTreeSet::new();
        for &cell in cells {
            let Some(row) = catalog.row(cell) else {
                out.insert(format!(
                    "{SECTION}: {cell} under {heading} is not a catalog rule id"
                ));
                continue;
            };
            if !seen.insert(cell) {
                out.insert(format!("{SECTION}: {cell} is listed twice under {heading}"));
            } else if !list.contains(row) {
                out.insert(format!(
                    "{SECTION}: {cell} is listed under {heading} but is not {}",
                    list.member()
                ));
            }
        }
        for row in catalog.rows.iter().filter(|r| list.contains(r)) {
            if !seen.contains(row.id.as_str()) {
                out.insert(format!(
                    "{SECTION}: {} is {} but is not listed under {heading}",
                    row.id,
                    list.member()
                ));
            }
        }
    }
    out.into_iter().collect()
}

/// The first cells of the table rows under each `###` subsection of the audit
/// section (header and delimiter rows skipped); `None` without the section.
fn listed(text: &str) -> Option<BTreeMap<&str, Vec<&str>>> {
    let mut found = false;
    let mut in_section = false;
    let mut in_fence = false;
    let mut subsection: Option<&str> = None;
    // 1-based index of the current line within its table; 0 outside tables.
    let mut table_line = 0_usize;
    let mut lists: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            table_line = 0;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            in_section = heading.trim() == SECTION;
            found |= in_section;
            subsection = None;
            table_line = 0;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(heading) = line.strip_prefix("### ") {
            let heading = heading.trim();
            lists.entry(heading).or_default();
            subsection = Some(heading);
            table_line = 0;
            continue;
        }
        let Some(row) = line.strip_prefix('|') else {
            table_line = 0;
            continue;
        };
        table_line += 1;
        if table_line <= 2 {
            continue;
        }
        if let Some(cells) = subsection.and_then(|s| lists.get_mut(s)) {
            cells.push(row.split('|').next().unwrap_or_default().trim());
        }
    }
    found.then_some(lists)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const RULES: &str = "\
| Rule | Enforcement |
|---|---|
| **MSG-03** Mentions | Compile-time (by construction) |
| **INT-01** Deadline | Runtime |
| **RL-02** Proxy | Startup (cluster config) / Runtime |
| **CV2-03** Total | Compile-time (literal) / Runtime (all trees) |
| **CMD-14** Mixing | (permitted; no check) |
| **LOC-05** Budget | see CV2-03 |
";

    const AUDIT: &str = "\
## v1 rules audit

Intro.

### By construction
| Rule | Evidence |
|---|---|
| MSG-03 | fixture |

### Runtime only
| Rule | Why |
|---|---|
| INT-01 | timer |
| RL-02 | environment |

### Documentation only and aliases
| Rule | Status |
|---|---|
| CMD-14 | permitted |
| LOC-05 | alias |

## Unconfirmed values
";

    fn audit(section: &str) -> Vec<String> {
        audit_problems(&format!("{RULES}\n{section}"))
    }

    /// `AUDIT` with `from` replaced by `to`.
    fn edited(from: &str, to: &str) -> Vec<String> {
        assert!(AUDIT.contains(from), "audit lacks {from}");
        audit(&AUDIT.replace(from, to))
    }

    #[test]
    fn a1_matching_lists() {
        assert_eq!(audit(AUDIT), Vec::<String>::new());
    }

    #[test]
    fn a2_section_missing() {
        assert_eq!(audit(""), ["v1 rules audit: section missing"]);
        assert_eq!(
            edited("## v1 rules audit", "## Other"),
            ["v1 rules audit: section missing"]
        );
    }

    #[test]
    fn a3_subsection_missing() {
        assert_eq!(
            edited("### Runtime only", "### Runtime"),
            ["v1 rules audit: subsection Runtime only missing"]
        );
    }

    #[test]
    fn a4_unlisted() {
        assert_eq!(
            edited("| INT-01 | timer |\n", ""),
            ["v1 rules audit: INT-01 is a runtime-only row but is not listed under Runtime only"]
        );
        assert_eq!(
            edited("| MSG-03 | fixture |\n", ""),
            [
                "v1 rules audit: MSG-03 is a by-construction row but is not listed under By \
                 construction"
            ]
        );
        assert_eq!(
            edited("| LOC-05 | alias |\n", ""),
            [
                "v1 rules audit: LOC-05 is a documentation-only or alias row but is not listed \
                 under Documentation only and aliases"
            ]
        );
    }

    #[test]
    fn a5_wrong_list() {
        assert_eq!(
            edited(
                "| RL-02 | environment |",
                "| RL-02 | environment |\n| CV2-03 | count |"
            ),
            ["v1 rules audit: CV2-03 is listed under Runtime only but is not a runtime-only row"]
        );
    }

    #[test]
    fn a6_listed_twice() {
        assert_eq!(
            edited(
                "| INT-01 | timer |",
                "| INT-01 | timer |\n| INT-01 | again |"
            ),
            ["v1 rules audit: INT-01 is listed twice under Runtime only"]
        );
    }

    #[test]
    fn a7_unknown_or_malformed_ids() {
        for (cell, unlisted) in [
            ("ZZZ-01", false),
            ("**MSG-03**", true),
            ("`MSG-03`", true),
            ("", true),
        ] {
            let mut want = vec![format!(
                "v1 rules audit: {cell} under By construction is not a catalog rule id"
            )];
            if unlisted {
                want.push(
                    "v1 rules audit: MSG-03 is a by-construction row but is not listed under By \
                     construction"
                        .to_owned(),
                );
            }
            let got = if unlisted {
                edited("| MSG-03 | fixture |", &format!("| {cell} | fixture |"))
            } else {
                edited(
                    "| MSG-03 | fixture |",
                    &format!("| MSG-03 | fixture |\n| {cell} | x |"),
                )
            };
            want.sort();
            assert_eq!(got, want, "{cell:?}");
        }
    }

    #[test]
    fn a8_empty_catalog() {
        assert_eq!(audit_problems(""), ["v1 rules audit: section missing"]);
    }

    #[test]
    fn real_catalog_audit_is_complete() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/discord-rules.md");
        let text = std::fs::read_to_string(path).expect("read the catalog");
        let mut problems = Problems::default();
        let catalog = catalog::parse(&text, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(audit_problems(&text), Vec::<String>::new());
        let mut runtime_only: Vec<&str> = catalog
            .rows
            .iter()
            .filter(|r| List::RuntimeOnly.contains(r))
            .map(|r| r.id.as_str())
            .collect();
        let mut expected = vec![
            "LOC-06", "META-04", "REG-05", "INT-01", "INT-02", "INT-07", "INT-11", "INT-12",
            "INT-13", "CV2-16", "MOD-09", "MSG-04", "MSG-06", "MSG-08", "MSG-09", "ATT-01",
            "ATT-02", "ATT-03", "ATT-04", "WH-05", "RL-01", "RL-02", "RL-03", "RL-04", "GW-04",
            "VC-01", "VC-02",
        ];
        runtime_only.sort_unstable();
        expected.sort_unstable();
        assert_eq!(runtime_only, expected);
        let mut by_construction: Vec<&str> = catalog
            .rows
            .iter()
            .filter(|r| List::ByConstruction.contains(r))
            .map(|r| r.id.as_str())
            .collect();
        let mut expected = vec![
            "OPT-12", "OPT-14", "LOC-04", "CTX-02", "CTX-03", "META-05", "INT-03", "INT-04",
            "INT-06", "INT-08", "INT-09", "INT-10", "CID-05", "CID-07", "CV2-01", "CV2-02",
            "CV2-12", "CV2-14", "MSG-03", "MSG-05", "MSG-07", "ATT-05", "ATT-07", "WH-01", "WH-03",
            "GW-07",
        ];
        by_construction.sort_unstable();
        expected.sort_unstable();
        assert_eq!(by_construction, expected);
    }
}
