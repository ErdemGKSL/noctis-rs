//! Parsing `docs/discord-rules.md`: the rule tables, the Enforcement-cell
//! classification and the `custom_id` constants table.

use std::collections::BTreeSet;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use super::{Evidence, Problems, is_rule_id};

/// The catalog's repo-relative path, used as a problem subject.
pub(crate) const CATALOG: &str = "docs/discord-rules.md";

/// What a row's Enforcement cell asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Class {
    /// The evidence kinds the row requires (never empty).
    Required(BTreeSet<Evidence>),
    /// A parenthesis-only cell: permitted, out of scope, or no API in v1.
    DocOnly,
    /// `see <ID>` / `as <ID>`: covered by the target's tests.
    Alias(String),
}

impl Class {
    /// The evidence kinds required; empty for documentation-only and alias rows.
    pub(crate) fn kinds(&self) -> BTreeSet<Evidence> {
        match self {
            Self::Required(kinds) => kinds.clone(),
            Self::DocOnly | Self::Alias(_) => BTreeSet::new(),
        }
    }
}

/// One rule row, in catalog order.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub(crate) id: String,
    /// `None` when the Enforcement cell could not be classified (already reported).
    pub(crate) class: Option<Class>,
}

/// The parsed catalog.
#[derive(Debug, Default)]
pub(crate) struct Catalog {
    pub(crate) rows: Vec<Row>,
    /// The `custom_id` constants table; `None` when it is broken (already reported).
    pub(crate) constants: Option<Vec<(String, u64)>>,
}

impl Catalog {
    pub(crate) fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }
}

/// One table cell: its rendered text, the text of its first bold span, and
/// whether pulldown-cmark padded it in for a short row.
#[derive(Debug, Default)]
struct Cell {
    text: String,
    strong: Option<String>,
    padded: bool,
}

#[derive(Debug, Default)]
struct Table {
    header: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
}

/// Renders every table of `markdown`: text as is, code spans wrapped in
/// backticks, breaks as spaces.
fn tables(markdown: &str) -> Vec<Table> {
    let mut tables = Vec::new();
    let mut table: Option<Table> = None;
    let mut row: Vec<Cell> = Vec::new();
    let mut cell: Option<Cell> = None;
    // `Some(text)` while inside the cell's first bold span.
    let mut strong: Option<String> = None;
    let mut in_strong = false;
    for (event, range) in Parser::new_ext(markdown, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::Table(_)) => table = Some(Table::default()),
            Event::End(TagEnd::Table) => tables.extend(table.take()),
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = table.as_mut() {
                    t.header = std::mem::take(&mut row);
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.rows.push(std::mem::take(&mut row));
                }
            }
            Event::Start(Tag::TableCell) => {
                cell = Some(Cell {
                    padded: Range::is_empty(&range),
                    ..Cell::default()
                });
                strong = None;
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(mut c) = cell.take() {
                    let text = c.text.trim().to_owned();
                    c.text = text;
                    c.strong = strong.take();
                    row.push(c);
                }
            }
            Event::Start(Tag::Strong) => {
                if strong.is_none() {
                    strong = Some(String::new());
                    in_strong = true;
                }
            }
            Event::End(TagEnd::Strong) => in_strong = false,
            Event::Text(text) | Event::InlineHtml(text) | Event::Html(text) => {
                push_text(cell.as_mut(), &mut strong, in_strong, &text);
            }
            Event::Code(code) => {
                push_text(cell.as_mut(), &mut strong, in_strong, &format!("`{code}`"));
            }
            Event::SoftBreak | Event::HardBreak => {
                push_text(cell.as_mut(), &mut strong, in_strong, " ");
            }
            _ => {}
        }
    }
    tables
}

fn push_text(cell: Option<&mut Cell>, strong: &mut Option<String>, in_strong: bool, text: &str) {
    if let Some(c) = cell {
        c.text.push_str(text);
        if in_strong && let Some(s) = strong.as_mut() {
            s.push_str(text);
        }
    }
}

/// Parses the catalog's rule rows and constants table, reporting C1-C6 and K6.
pub(crate) fn parse(markdown: &str, problems: &mut Problems) -> Catalog {
    let tables = tables(markdown);
    let mut catalog = Catalog::default();
    let mut seen = BTreeSet::new();
    for table in &tables {
        let enforcement = table.header.iter().position(|c| c.text == "Enforcement");
        for cells in &table.rows {
            let Some(id) = cells
                .first()
                .and_then(|c| c.strong.as_deref())
                .map(str::trim)
                .filter(|s| is_rule_id(s))
            else {
                continue;
            };
            if !seen.insert(id.to_owned()) {
                problems.add(id, "duplicate catalog row");
                continue;
            }
            let class = match enforcement {
                None => {
                    problems.add(id, "table has no Enforcement column");
                    None
                }
                Some(ix) => match cells.get(ix).filter(|c| !c.padded) {
                    None => {
                        problems.add(id, "row has no Enforcement cell");
                        None
                    }
                    Some(cell) => {
                        let class = classify(&cell.text);
                        if class.is_none() {
                            problems.add(id, format!("cannot parse Enforcement \"{}\"", cell.text));
                        }
                        class
                    }
                },
            };
            catalog.rows.push(Row {
                id: id.to_owned(),
                class,
            });
        }
    }
    if catalog.rows.is_empty() {
        problems.add(CATALOG, "no rule rows found");
    }
    check_aliases(&catalog, problems);
    catalog.constants = constants(&tables, problems);
    catalog
}

/// R9: an alias must point at a requirement-bearing catalog row.
fn check_aliases(catalog: &Catalog, problems: &mut Problems) {
    for row in &catalog.rows {
        let Some(Class::Alias(target)) = &row.class else {
            continue;
        };
        let reason = match catalog.row(target).map(|r| &r.class) {
            None => "is not a catalog rule",
            Some(Some(Class::Alias(_))) => "is itself an alias",
            Some(Some(Class::DocOnly)) => "is documentation-only",
            Some(_) => continue,
        };
        problems.add(&row.id, format!("alias target {target} {reason}"));
    }
}

/// K6: the `Constant (`noctis-codec`) | Value` table.
fn constants(tables: &[Table], problems: &mut Problems) -> Option<Vec<(String, u64)>> {
    let table = tables.iter().find(|t| {
        t.header.len() >= 2
            && t.header[0].text == "Constant (`noctis-codec`)"
            && t.header[1].text == "Value"
    });
    let mut fail = |message: String| {
        problems.add(CATALOG, format!("custom_id constants table {message}"));
        None
    };
    let Some(table) = table else {
        return fail("not found".to_owned());
    };
    if table.rows.is_empty() {
        return fail("is empty".to_owned());
    }
    let mut out: Vec<(String, u64)> = Vec::new();
    let mut broken = false;
    for cells in &table.rows {
        let name_cell = cells.first().map_or("", |c| c.text.as_str());
        let name = name_cell
            .strip_prefix('`')
            .and_then(|n| n.strip_suffix('`'))
            .unwrap_or(name_cell);
        let value = cells.get(1).map_or("", |c| c.text.as_str());
        if !is_const_name(name) {
            fail(format!("has an invalid constant name \"{name_cell}\""));
            broken = true;
            continue;
        }
        let Some(value) = parse_int(value) else {
            fail(format!("has a non-integer value \"{value}\" for {name}"));
            broken = true;
            continue;
        };
        if out.iter().any(|(n, _)| n == name) {
            fail(format!("lists {name} twice"));
            broken = true;
            continue;
        }
        out.push((name.to_owned(), value));
    }
    (!broken).then_some(out)
}

/// `[A-Z][A-Z0-9_]*`.
pub(crate) fn is_const_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// A decimal integer, `_` separators allowed.
pub(crate) fn parse_int(text: &str) -> Option<u64> {
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Byte ranges of `text` that lie outside backtick code spans, with the paren
/// depth at each character, fed to `f(index, char, depth)`. Returns `None`
/// when parentheses are unbalanced.
fn scan(text: &str, mut f: impl FnMut(usize, char, usize)) -> Option<()> {
    let mut depth = 0_usize;
    let mut in_code = false;
    for (i, c) in text.char_indices() {
        if c == '`' {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        match c {
            '(' => {
                f(i, c, depth);
                depth += 1;
            }
            ')' => {
                depth = depth.checked_sub(1)?;
                f(i, c, depth);
            }
            _ => f(i, c, depth),
        }
    }
    (depth == 0).then_some(())
}

/// Splits an Enforcement cell into tiers at ` / ` at paren depth 0 outside
/// code spans. `None` when parentheses are unbalanced.
pub(crate) fn split_tiers(cell: &str) -> Option<Vec<&str>> {
    let mut cuts = Vec::new();
    scan(cell, |i, c, depth| {
        if c == '/'
            && depth == 0
            && i > 0
            && cell[..i].ends_with(' ')
            && cell[i + 1..].starts_with(' ')
        {
            cuts.push(i);
        }
    })?;
    let mut tiers = Vec::new();
    let mut start = 0;
    for cut in cuts {
        tiers.push(&cell[start..cut - 1]);
        start = cut + 2;
    }
    tiers.push(&cell[start..]);
    Some(tiers)
}

/// The byte index of the `)` closing the `(` at the start of `text`, if any.
fn closing_paren(text: &str) -> Option<usize> {
    let mut close = None;
    scan(text, |i, c, depth| {
        if c == ')' && depth == 0 && close.is_none() {
            close = Some(i);
        }
    })?;
    close
}

/// Tier heads, longest first so `App-build or Runtime` wins over `App-build`.
const HEADS: [&str; 5] = [
    "App-build or Runtime",
    "Compile-time",
    "App-build",
    "Runtime",
    "Startup",
];

/// Classifies a rendered Enforcement cell (R1-R14); `None` is a parse error.
pub(crate) fn classify(cell: &str) -> Option<Class> {
    let cell = cell.trim();
    for prefix in ["see ", "as "] {
        if let Some(target) = cell.strip_prefix(prefix)
            && is_rule_id(target)
        {
            return Some(Class::Alias(target.to_owned()));
        }
    }
    if cell.starts_with('(') && closing_paren(cell)? + 1 == cell.len() {
        return Some(Class::DocOnly);
    }
    let mut kinds = BTreeSet::new();
    for tier in split_tiers(cell)? {
        kinds.insert(tier_kind(tier.trim())?);
    }
    Some(Class::Required(kinds))
}

/// One tier: `<Head>` or `<Head> (<inner>)` with nothing after the `)`.
fn tier_kind(tier: &str) -> Option<Evidence> {
    let head = HEADS.iter().find(|h| tier.starts_with(**h))?;
    let rest = &tier[head.len()..];
    let inner = if rest.is_empty() {
        ""
    } else {
        let group = rest.strip_prefix(' ')?;
        if !group.starts_with('(') || closing_paren(group)? + 1 != group.len() {
            return None;
        }
        &group[1..group.len() - 1]
    };
    if *head != "Compile-time" {
        return Some(Evidence::Unit);
    }
    // Only the text before the first `;` (outside code) decides (R3).
    let mut first_semicolon = None;
    scan(inner, |i, c, _| {
        if c == ';' && first_semicolon.is_none() {
            first_semicolon = Some(i);
        }
    })?;
    let clause = &inner[..first_semicolon.unwrap_or(inner.len())];
    Some(if clause.contains("by construction") {
        Evidence::ByConstruction
    } else {
        Evidence::Diagnosed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Evidence::{ByConstruction as B, Diagnosed as D, Unit as U};

    #[expect(
        clippy::unnecessary_wraps,
        reason = "compared with `classify`'s result"
    )]
    fn req(kinds: &[Evidence]) -> Option<Class> {
        Some(Class::Required(kinds.iter().copied().collect()))
    }

    #[test]
    fn r1_diagnosed() {
        assert_eq!(classify("Compile-time"), req(&[D]));
        assert_eq!(classify("Compile-time (literal)"), req(&[D]));
    }

    #[test]
    fn r2_by_construction() {
        for cell in [
            "Compile-time (by construction)",
            "Compile-time (types, by construction)",
            "Compile-time (by construction: Renderer output enum)",
        ] {
            assert_eq!(classify(cell), req(&[B]), "{cell}");
        }
    }

    #[test]
    fn r3_first_semicolon_clause_decides() {
        let cell = "Compile-time (`#[command]`/`#[handler]` check; typestate by construction) / Runtime (auto-defer race)";
        assert_eq!(classify(cell), req(&[D, U]));
        assert_eq!(
            classify("Compile-time (by construction; checked)"),
            req(&[B])
        );
    }

    #[test]
    fn r4_multi_tier() {
        assert_eq!(
            classify("Compile-time (literal) / App-build (loaded locales)"),
            req(&[D, U])
        );
    }

    #[test]
    fn r5_unit_heads() {
        for cell in [
            "App-build",
            "Runtime",
            "Startup (cluster config) / Runtime",
            "App-build or Runtime (localized)",
            "Runtime (by construction)",
            "Runtime (Publish)",
        ] {
            assert_eq!(classify(cell), req(&[U]), "{cell}");
        }
    }

    #[test]
    fn r6_nested_and_code_parens() {
        let cmd12 = "Compile-time (one pattern, e.g. `config (set)?`) / App-build (Module A registers `config`, Module B registers `config set`)";
        assert_eq!(classify(cmd12), req(&[D, U]));
        assert_eq!(
            split_tiers("Compile-time (a / b) / Runtime"),
            Some(vec!["Compile-time (a / b)", "Runtime"])
        );
        assert_eq!(
            split_tiers("App-build (fetch `GET / x`) / Runtime"),
            Some(vec!["App-build (fetch `GET / x`)", "Runtime"])
        );
        assert_eq!(
            classify("App-build (fetch `GET /gateway/bot` at startup) / Runtime"),
            req(&[U])
        );
        assert_eq!(classify("Compile-time (`)` in code)"), req(&[D]));
    }

    #[test]
    fn r7_escaped_pipes_keep_five_cells() {
        let md = "| Rule | Limit/value | Source | Enforcement | How |\n|---|---|---|---|---|\n\
                  | **CMD-16** x | `(a\\|b)` | — | Compile-time (literal patterns) | `A \\| B` |\n";
        let tables = tables(md);
        assert_eq!(tables[0].rows[0].len(), 5);
        assert_eq!(tables[0].rows[0][3].text, "Compile-time (literal patterns)");
        assert_eq!(tables[0].rows[0][4].text, "`A | B`");
    }

    #[test]
    fn r8_doc_only() {
        for cell in [
            "(permitted; no check)",
            "(out of scope)",
            "(no embed API in v1; documented)",
            "(permitted)",
        ] {
            assert_eq!(classify(cell), Some(Class::DocOnly), "{cell}");
        }
    }

    #[test]
    fn r9_alias() {
        assert_eq!(
            classify("see CMD-05"),
            Some(Class::Alias("CMD-05".to_owned()))
        );
        assert_eq!(
            classify("as CID-01"),
            Some(Class::Alias("CID-01".to_owned()))
        );
        assert_eq!(classify("see cmd-05"), None);
    }

    #[test]
    fn r10_unknown_head() {
        assert_eq!(classify("Linktime"), None);
        assert_eq!(classify("compile-time"), None);
        assert_eq!(classify("Compile-timely"), None);
    }

    #[test]
    fn r11_empty_or_malformed() {
        for cell in [
            "",
            "Compile-time / ",
            "Compile-time (x",
            "Compile-time x)",
            "Compile-time (x) y",
            "Compile-time(x)",
        ] {
            assert_eq!(classify(cell), None, "{cell:?}");
        }
    }

    #[test]
    fn r12_duplicate_kinds() {
        assert_eq!(
            classify("Compile-time (literal) / Compile-time (by construction)"),
            req(&[D, B])
        );
    }

    #[test]
    fn r13_parenthesis_then_tier() {
        assert_eq!(classify("(x) / Runtime"), None);
    }

    #[test]
    fn r14_unicode() {
        assert_eq!(classify("Compile-time (ʼ ≤ …)"), req(&[D]));
        assert_eq!(classify("(≤ … ʼ)"), Some(Class::DocOnly));
    }

    #[test]
    fn real_catalog_classification() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/discord-rules.md");
        let text = std::fs::read_to_string(path).expect("read the catalog");
        let mut problems = Problems::default();
        let catalog = parse(&text, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(catalog.rows.len(), 126);
        assert_eq!(catalog.rows[0].id, "CMD-01");
        let ids = |f: &dyn Fn(&Class) -> bool| -> Vec<&str> {
            catalog
                .rows
                .iter()
                .filter(|r| r.class.as_ref().is_some_and(f))
                .map(|r| r.id.as_str())
                .collect()
        };
        let by_construction = ids(&|c| c.kinds().contains(&B));
        let mut expected = vec![
            "CV2-01", "CV2-02", "CV2-12", "CV2-14", "CID-05", "CID-07", "INT-03", "INT-04",
            "INT-06", "INT-08", "INT-09", "INT-10", "MSG-03", "MSG-05", "MSG-07", "WH-01", "WH-03",
            "META-05", "OPT-12", "OPT-14", "CTX-02", "CTX-03", "ATT-05", "ATT-07", "GW-07",
        ];
        let mut got = by_construction.clone();
        got.sort_unstable();
        expected.sort_unstable();
        assert_eq!(got, expected);
        assert_eq!(
            ids(&|c| *c == Class::DocOnly),
            ["CMD-14", "LOC-04", "REG-06", "MSG-02"]
        );
        assert_eq!(ids(&|c| matches!(c, Class::Alias(_))), ["LOC-05", "MOD-08"]);
        assert_eq!(
            catalog.row("INT-05").and_then(|r| r.class.clone()),
            req(&[D, U])
        );
        assert_eq!(catalog.constants.as_ref().map(Vec::len), Some(7));
    }
}
