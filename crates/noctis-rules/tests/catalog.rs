//! `RuleId::ALL` lists exactly the rule rows of `docs/discord-rules.md`, in order.

use noctis_rules::RuleId;

const CATALOG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/discord-rules.md"
));

/// Whether `id` has the `<AREA>-<nn>` shape (`^[A-Z][A-Z0-9]{1,3}-[0-9]{2}$`).
/// Table rows such as `| **Compile-time** |` also start with `| **`, but they
/// define enforcement tiers, not rules.
fn is_rule_id(id: &str) -> bool {
    let Some((area, num)) = id.split_once('-') else {
        return false;
    };
    area.len() >= 2
        && area.len() <= 4
        && area.starts_with(|c: char| c.is_ascii_uppercase())
        && area
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && num.len() == 2
        && num.bytes().all(|b| b.is_ascii_digit())
}

fn catalog_ids() -> Vec<&'static str> {
    CATALOG
        .lines()
        .filter_map(|line| line.strip_prefix("| **"))
        .filter_map(|rest| rest.split_once("**").map(|(id, _)| id))
        .filter(|id| is_rule_id(id))
        .collect()
}

#[test]
fn rule_ids_match_the_catalog() {
    let catalog = catalog_ids();
    let code: Vec<&str> = RuleId::ALL.iter().map(|r| r.as_str()).collect();
    let missing: Vec<_> = catalog.iter().filter(|id| !code.contains(id)).collect();
    let extra: Vec<_> = code.iter().filter(|id| !catalog.contains(id)).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "RuleId::ALL and docs/discord-rules.md differ; in the catalog but not in RuleId: \
         {missing:?}; in RuleId but not in the catalog: {extra:?}"
    );
    assert_eq!(code, catalog, "RuleId::ALL is not in catalog row order");
    assert_eq!(code.len(), 133);
}
