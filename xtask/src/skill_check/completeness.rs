//! The completeness check (Q1-Q17, Q19; FR-50, SM-3): the Usage Skill has
//! its required references and headings, lists every reference in
//! `SKILL.md`, carries no planning markers, indexes every catalog rule id in
//! `references/troubleshooting.md` and names every id in
//! `references/limits-cheatsheet.md`, and its Markdown Rust examples (and
//! those of `docs/`) follow UD-7.
//!
//! - **References** (Q1-Q4): each file of [`REQUIRED`] exists and has its
//!   level-2 headings (exact text, trailing spaces trimmed, outside fenced
//!   code); every `references/*.md` file is named as a code span in
//!   `SKILL.md`, and every such span names an existing file.
//! - **Markers** (Q5-Q7), on every line of every skill file, fences
//!   included: the identifier token (`[A-Za-z0-9_]` run) `planned` in any
//!   case, `v0` in any case unless `.` and a digit follow, and `Epic <n>` /
//!   `Story <n>[.<m>]`.
//! - **Catalog ids** (Q17): the bold ids opening the first cell of
//!   `docs/discord-rules.md` table rows, outside fenced code, in document
//!   order (the rows `rules_catalog` reads).
//! - **Rule id index** (Q8-Q15): the table rows under `## Rule id index`
//!   (up to the next level-1 or level-2 heading) list each catalog id once,
//!   in catalog order, with an id, a meaning and a fix cell naming a skill
//!   reference (`.md`).
//! - **Cheatsheet** (Q16): every catalog id is a `[A-Za-z0-9_-]` token of
//!   `references/limits-cheatsheet.md`.
//! - **UD-7 in examples** (Q19): in ```` ```rust ```` fences (info string's
//!   first word `rust`) of the skill and of `docs/**/*.md`, an identifier
//!   outside `//`/`/* */` comments and `"…"` literals with the word
//!   `collector(s)` or `listener(s)`, or an await-loop name, is reported with
//!   the doctrine lint's UD-7 messages.

use std::collections::{BTreeMap, BTreeSet};

use super::components::code_spans;
use super::doctrine::{AWAIT_LOOPS, UD7_AWAIT, UD7_COLLECTOR, UD7_LISTENER, words};
use super::{Problems, Repo};
use crate::rules_catalog::is_rule_id;

/// The rule catalog, relative to the root.
const CATALOG: &str = "docs/discord-rules.md";
/// The directory whose Markdown Rust examples Q19 also scans.
const DOCS_DIR: &str = "docs";
/// Required reference files (relative to the skill) and their level-2 headings.
const REQUIRED: &[(&str, &[&str])] = &[
    ("references/concept-map.md", &["## tescord / dbi → Noctis"]),
    ("references/limits-cheatsheet.md", &[]),
    (
        "references/patterns.md",
        &["## Patterns", "## Anti-patterns", "## Common mistakes"],
    ),
    ("references/troubleshooting.md", &[INDEX_HEADING]),
];
/// The file holding the rule id index, relative to the skill.
const INDEX_FILE: &str = "references/troubleshooting.md";
const INDEX_HEADING: &str = "## Rule id index";
/// The file that must name every rule id, relative to the skill.
const CHEATSHEET: &str = "references/limits-cheatsheet.md";
const INDEX: &str = "rule id index";

/// How a Markdown line relates to fenced code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Prose,
    /// An opening or closing fence line.
    Fence,
    /// A line inside a fence; `rust` when the info string's first word is `rust`.
    Code {
        rust: bool,
    },
}

/// Classifies every line of `text` (1-based numbers).
fn md_lines(text: &str) -> Vec<(usize, &str, LineKind)> {
    let mut open: Option<(u8, usize, bool)> = None;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let first = trimmed.bytes().next().filter(|b| matches!(b, b'`' | b'~'));
        let run = first.map_or(0, |c| trimmed.bytes().take_while(|&b| b == c).count());
        let after = trimmed.get(run..).unwrap_or_default();
        let kind = match open {
            Some((c, len, rust)) => {
                if first == Some(c) && run >= len && after.trim().is_empty() {
                    open = None;
                    LineKind::Fence
                } else {
                    LineKind::Code { rust }
                }
            }
            None => match first {
                Some(c) if run >= 3 => {
                    let lang = after
                        .trim()
                        .split(|ch: char| ch.is_whitespace() || ch == ',')
                        .next()
                        .unwrap_or_default();
                    open = Some((c, run, lang == "rust"));
                    LineKind::Fence
                }
                _ => LineKind::Prose,
            },
        };
        out.push((i + 1, line, kind));
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Maximal runs of bytes accepted by `keep`, with their byte offsets.
fn runs(text: &str, keep: impl Fn(u8) -> bool) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (keep(b), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out
}

/// Splits a table row into trimmed cells at unescaped `|`.
fn cells(row: &str) -> Vec<String> {
    let row = row.trim();
    let row = row.strip_prefix('|').unwrap_or(row);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for ch in row.chars() {
        if ch == '|' && !escaped {
            out.push(cell.trim().to_owned());
            cell.clear();
        } else {
            cell.push(ch);
        }
        escaped = ch == '\\' && !escaped;
    }
    if !cell.trim().is_empty() {
        out.push(cell.trim().to_owned());
    }
    out
}

/// `|---|:--:|` style separator rows.
fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim_start_matches(':').trim_end_matches(':');
            !c.is_empty() && c.bytes().all(|b| b == b'-')
        })
}

/// The catalog's rule ids in document order (Q17); duplicates and an empty
/// catalog are reported, and a duplicate is kept once.
pub(super) fn catalog_ids(text: &str, problems: &mut Problems) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for (_, line, kind) in md_lines(text) {
        if kind != LineKind::Prose || !line.trim_start().starts_with('|') {
            continue;
        }
        let row = cells(line);
        let Some(id) = row
            .first()
            .and_then(|c| c.strip_prefix("**"))
            .and_then(|c| c.split_once("**"))
            .map(|(id, _)| id.trim())
            .filter(|id| is_rule_id(id))
        else {
            continue;
        };
        if ids.iter().any(|known| known == id) {
            problems.add(CATALOG, format!("{id} is listed twice"));
        } else {
            ids.push(id.to_owned());
        }
    }
    if ids.is_empty() {
        problems.add(CATALOG, "no rule rows found");
    }
    ids
}

/// Runs the completeness checks over the skill files `files` (paths relative
/// to the root) of the skill at `skill_dir`; returns how many catalog ids the
/// rule id index lists.
pub(super) fn check(
    repo: &Repo<'_>,
    skill_dir: &str,
    files: &[(String, String)],
    problems: &mut Problems,
) -> usize {
    let by_rel: BTreeMap<&str, &str> = files
        .iter()
        .filter_map(|(path, text)| {
            let rel = path.strip_prefix(skill_dir)?.strip_prefix('/')?;
            Some((rel, text.as_str()))
        })
        .collect();
    references(skill_dir, &by_rel, problems);
    for (path, text) in files {
        markers(path, text, problems);
        ud7_examples(path, text, problems);
    }
    for path in repo.files(DOCS_DIR, "md").unwrap_or_default() {
        if let Some(text) = repo.read(&path, problems) {
            ud7_examples(&path, &text, problems);
        }
    }

    let Some(catalog) = repo.read(CATALOG, problems) else {
        return 0;
    };
    let mut catalog_problems = Problems::default();
    let ids = catalog_ids(&catalog, &mut catalog_problems);
    let broken = !catalog_problems.0.is_empty();
    problems.0.extend(catalog_problems.0);
    if broken {
        return 0;
    }
    if let Some(text) = by_rel.get(CHEATSHEET) {
        cheatsheet(&format!("{skill_dir}/{CHEATSHEET}"), text, &ids, problems);
    }
    match by_rel.get(INDEX_FILE) {
        Some(text) => index(&format!("{skill_dir}/{INDEX_FILE}"), text, &ids, problems),
        None => 0,
    }
}

/// Q1-Q4.
fn references(skill_dir: &str, by_rel: &BTreeMap<&str, &str>, problems: &mut Problems) {
    for (rel, headings) in REQUIRED {
        let subject = format!("{skill_dir}/{rel}");
        let Some(text) = by_rel.get(rel) else {
            problems.add(subject, "required reference missing");
            continue;
        };
        let present: BTreeSet<&str> = md_lines(text)
            .into_iter()
            .filter(|(_, _, kind)| *kind == LineKind::Prose)
            .map(|(_, line, _)| line.trim_end())
            .collect();
        for heading in *headings {
            if !present.contains(heading) {
                problems.add(subject.as_str(), format!("missing heading \"{heading}\""));
            }
        }
    }
    let skill_md = format!("{skill_dir}/SKILL.md");
    let listed: BTreeSet<&str> = by_rel.get("SKILL.md").map_or_else(BTreeSet::new, |text| {
        md_lines(text)
            .into_iter()
            .filter(|(_, _, kind)| *kind == LineKind::Prose)
            .flat_map(|(_, line, _)| code_spans(line))
            .filter(|span| {
                span.starts_with("references/")
                    && std::path::Path::new(span)
                        .extension()
                        .is_some_and(|e| e == "md")
                    && !span.contains(char::is_whitespace)
            })
            .collect()
    });
    for rel in by_rel.keys().filter(|rel| rel.starts_with("references/")) {
        if !listed.contains(rel) {
            problems.add(
                skill_md.as_str(),
                format!("{rel} is not listed in the reference index"),
            );
        }
    }
    for rel in &listed {
        if !by_rel.contains_key(rel) {
            problems.add(
                skill_md.as_str(),
                format!("lists {rel}, which does not exist"),
            );
        }
    }
}

/// Q5-Q7 on every line of one skill file.
fn markers(path: &str, text: &str, problems: &mut Problems) {
    for (n, line, _) in md_lines(text) {
        let at = || format!("{path}:{n}");
        for (start, token) in runs(line, is_ident_byte) {
            let rest = &line[start + token.len()..];
            if token.eq_ignore_ascii_case("planned") {
                problems.add(
                    at(),
                    "\"planned\" marker; document what exists or say it is not provided",
                );
            }
            if token.eq_ignore_ascii_case("v0") {
                let mut after = rest.bytes();
                let release =
                    after.next() == Some(b'.') && after.next().is_some_and(|b| b.is_ascii_digit());
                if !release {
                    problems.add(at(), "\"v0\" status marker");
                }
            }
            if matches!(token, "Epic" | "Story")
                && let Some(number) = planning_number(rest)
            {
                problems.add(
                    at(),
                    format!("internal planning reference \"{token} {number}\""),
                );
            }
        }
    }
}

/// ` <digits>[.<digits>]` at the start of `rest` → the number text.
fn planning_number(rest: &str) -> Option<&str> {
    let tail = rest.strip_prefix(' ')?;
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let major = digits(tail);
    if major == 0 {
        return None;
    }
    let minor = tail[major..].strip_prefix('.').map_or(0, digits);
    let len = if minor > 0 { major + 1 + minor } else { major };
    Some(&tail[..len])
}

/// Q19: UD-7 identifiers in the Rust fences of one Markdown file.
fn ud7_examples(path: &str, text: &str, problems: &mut Problems) {
    let mut state = Lexer::default();
    for (n, line, kind) in md_lines(text) {
        if kind != (LineKind::Code { rust: true }) {
            state = Lexer::default();
            continue;
        }
        let code = state.strip(line);
        for (_, ident) in runs(&code, is_ident_byte) {
            if ident.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let w = words(ident);
            let has = |set: &[&str]| w.iter().any(|w| set.contains(&w.as_str()));
            if has(&["collector", "collectors"]) {
                problems.add(format!("{path}:{n}"), UD7_COLLECTOR);
            }
            if AWAIT_LOOPS.contains(&ident) {
                problems.add(format!("{path}:{n}"), UD7_AWAIT);
            }
            if has(&["listener", "listeners"]) {
                problems.add(format!("{path}:{n}"), UD7_LISTENER);
            }
        }
    }
}

/// Comment and string state carried across the lines of one fence.
#[derive(Debug, Default)]
struct Lexer {
    in_string: bool,
    in_comment: bool,
}

impl Lexer {
    /// `line` with comments and string literal contents replaced by spaces.
    fn strip(&mut self, line: &str) -> String {
        let chars: Vec<char> = line.chars().collect();
        let mut out = String::with_capacity(line.len());
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            let next = chars.get(i + 1).copied();
            if self.in_comment {
                if c == '*' && next == Some('/') {
                    self.in_comment = false;
                    i += 1;
                }
            } else if self.in_string {
                if c == '\\' {
                    i += 1;
                } else if c == '"' {
                    self.in_string = false;
                }
            } else if c == '/' && next == Some('/') {
                break;
            } else if c == '/' && next == Some('*') {
                self.in_comment = true;
                i += 1;
            } else if c == '"' {
                self.in_string = true;
            } else if c == '\'' && next == Some('"') && chars.get(i + 2) == Some(&'\'') {
                // The char literal `'"'`.
                i += 2;
            } else {
                out.push(c);
                i += 1;
                continue;
            }
            out.push(' ');
            i += 1;
        }
        out
    }
}

/// Q16.
fn cheatsheet(path: &str, text: &str, ids: &[String], problems: &mut Problems) {
    let tokens: BTreeSet<&str> = runs(text, |b| is_ident_byte(b) || b == b'-')
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    for id in ids {
        if !tokens.contains(id.as_str()) {
            problems.add(path, format!("{id} is missing"));
        }
    }
}

/// Q8-Q15; returns how many catalog ids the index lists.
fn index(path: &str, text: &str, ids: &[String], problems: &mut Problems) -> usize {
    let lines = md_lines(text);
    let Some(start) = lines
        .iter()
        .position(|(_, line, kind)| *kind == LineKind::Prose && line.trim_end() == INDEX_HEADING)
    else {
        // Reported by Q2.
        return 0;
    };
    let section: Vec<Vec<String>> = lines[start + 1..]
        .iter()
        .take_while(|(_, line, kind)| {
            *kind != LineKind::Prose || !(line.starts_with("# ") || line.starts_with("## "))
        })
        .filter(|(_, line, kind)| *kind == LineKind::Prose && line.trim_start().starts_with('|'))
        .map(|(_, line, _)| cells(line))
        .collect();
    let catalog: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let mut listed: Vec<&str> = Vec::new();
    let mut bad = false;
    for (k, row) in section.iter().enumerate() {
        let header = section.get(k + 1).is_some_and(|next| is_separator(next));
        if header || is_separator(row) {
            continue;
        }
        let raw = row.first().map_or("", String::as_str);
        let id = unwrap_id(raw);
        let Some(&id) = catalog.get(id) else {
            problems.add(path, format!("{INDEX}: {id} is not a catalog rule"));
            bad = true;
            continue;
        };
        if listed.contains(&id) {
            problems.add(path, format!("{INDEX}: {id} listed twice"));
            bad = true;
            continue;
        }
        listed.push(id);
        if row.len() < 3 || row.iter().take(3).any(String::is_empty) {
            problems.add(
                path,
                format!("{INDEX}: {id} row needs id, meaning and fix cells"),
            );
        } else if !row[2].contains(".md") {
            problems.add(
                path,
                format!("{INDEX}: {id} fix cell must name a skill reference (*.md)"),
            );
        }
    }
    for id in ids {
        if !listed.contains(&id.as_str()) {
            problems.add(path, format!("{INDEX}: {id} has no row"));
            bad = true;
        }
    }
    if !bad
        && let Some(first) = listed
            .iter()
            .zip(ids)
            .find(|(got, want)| **got != want.as_str())
            .map(|(got, _)| *got)
    {
        problems.add(
            path,
            format!("{INDEX}: rows are not in catalog order (first difference: {first})"),
        );
    }
    listed.len()
}

/// An id cell's text without one wrapping `**…**` or `` `…` ``.
fn unwrap_id(cell: &str) -> &str {
    cell.strip_prefix("**")
        .and_then(|c| c.strip_suffix("**"))
        .or_else(|| cell.strip_prefix('`').and_then(|c| c.strip_suffix('`')))
        .unwrap_or(cell)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "s";

    fn lines(problems: &Problems) -> Vec<String> {
        problems.0.iter().map(ToString::to_string).collect()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A skill whose every structure check passes.
    fn skill() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([
            (
                "SKILL.md",
                "# S\n`references/concept-map.md` `references/limits-cheatsheet.md` `references/patterns.md` `references/troubleshooting.md`\n",
            ),
            ("references/concept-map.md", "## tescord / dbi → Noctis\n"),
            ("references/limits-cheatsheet.md", ""),
            (
                "references/patterns.md",
                "## Patterns\n## Anti-patterns\n## Common mistakes  \n",
            ),
            ("references/troubleshooting.md", "## Rule id index\n"),
        ])
    }

    fn structure(skill: &BTreeMap<&str, &str>) -> Vec<String> {
        let mut problems = Problems::default();
        references(S, skill, &mut problems);
        lines(&problems)
    }

    #[test]
    fn q1_required_reference_missing() {
        assert!(structure(&skill()).is_empty());
        let mut s = skill();
        s.remove("references/patterns.md");
        assert_eq!(
            structure(&s),
            [
                "skill-check: s/SKILL.md: lists references/patterns.md, which does not exist",
                "skill-check: s/references/patterns.md: required reference missing",
            ]
        );
    }

    #[test]
    fn q2_required_heading() {
        for text in [
            "## Patterns\n## Anti-patterns\n### Common mistakes\n",
            "## Patterns\n## Anti-patterns\n```\n## Common mistakes\n```\n",
        ] {
            let mut s = skill();
            s.insert("references/patterns.md", text);
            assert_eq!(
                structure(&s),
                ["skill-check: s/references/patterns.md: missing heading \"## Common mistakes\""]
            );
        }
    }

    #[test]
    fn q3_q4_reference_index() {
        let mut s = skill();
        s.insert("references/x.md", "");
        s.insert(
            "SKILL.md",
            "`references/concept-map.md` `references/limits-cheatsheet.md` `references/patterns.md` `references/troubleshooting.md` references/x.md `references/gone.md`\n",
        );
        assert_eq!(
            structure(&s),
            [
                "skill-check: s/SKILL.md: lists references/gone.md, which does not exist",
                "skill-check: s/SKILL.md: references/x.md is not listed in the reference index",
            ]
        );
    }

    fn marker_lines(text: &str) -> Vec<String> {
        let mut problems = Problems::default();
        markers("f.md", text, &mut problems);
        lines(&problems)
    }

    #[test]
    fn q5_planned_marker() {
        let msg = "\"planned\" marker; document what exists or say it is not provided";
        for text in [
            "x [planned] y",
            "**planned**",
            "Planned (later)",
            "```\nPLANNED\n```",
        ] {
            let got = marker_lines(text);
            assert_eq!(got.len(), 1, "{text:?}: {got:?}");
            assert!(got[0].ends_with(msg), "{got:?}");
        }
        assert!(marker_lines("unplanned planner planned_x").is_empty());
    }

    #[test]
    fn q6_v0_marker() {
        assert_eq!(
            marker_lines("a\nStatus: v0\n"),
            ["skill-check: f.md:2: \"v0\" status marker"]
        );
        assert!(marker_lines("COLRv0 font, v0.1 tag, v0.12").is_empty());
        assert_eq!(marker_lines("v0. end").len(), 1);
    }

    #[test]
    fn q7_planning_reference() {
        assert_eq!(
            marker_lines("see Epic 7 and Story 14.6."),
            [
                "skill-check: f.md:1: internal planning reference \"Epic 7\"",
                "skill-check: f.md:1: internal planning reference \"Story 14.6\"",
            ]
        );
        assert!(marker_lines("epic 7, Epics 7, Epic x, Story  2, MyStory 3").is_empty());
    }

    fn catalog(text: &str) -> (Vec<String>, Vec<String>) {
        let mut problems = Problems::default();
        let ids = catalog_ids(text, &mut problems);
        (ids, lines(&problems))
    }

    #[test]
    fn q17_catalog_ids() {
        let text = "| Id | x |\n|---|---|\n| **CMD-01** a | b |\n| CMD-02 | c |\n```\n| **CMD-03** | d |\n```\n| **Note** | e |\n| **REG-06** | f |\n";
        assert_eq!(catalog(text), (ids(&["CMD-01", "REG-06"]), vec![]));
        assert_eq!(
            catalog("| x |\n"),
            (
                vec![],
                vec!["skill-check: docs/discord-rules.md: no rule rows found".to_owned()]
            )
        );
        assert_eq!(
            catalog("| **CMD-01** | a |\n| **CMD-01** | b |\n").1,
            ["skill-check: docs/discord-rules.md: CMD-01 is listed twice"]
        );
    }

    #[test]
    fn real_catalog_ids_match_rules_catalog() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/discord-rules.md");
        let text = std::fs::read_to_string(path).expect("read the catalog");
        let (got, problems) = catalog(&text);
        assert!(problems.is_empty(), "{problems:?}");
        let mut catalog_problems = crate::rules_catalog::Problems::default();
        let want: Vec<String> = crate::rules_catalog::catalog::parse(&text, &mut catalog_problems)
            .rows
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(got, want);
        assert_eq!(got.len(), 135);
    }

    const CAT: &[&str] = &["CMD-01", "CMD-02", "VC-02"];

    fn index_lines(rows: &str) -> (usize, Vec<String>) {
        let text = format!(
            "# T\n## Rule id index\n\n| Rule | Meaning | Fix |\n|---|---|---|\n{rows}\n## Next\n| ZZ-09 | x | y.md |\n"
        );
        let mut problems = Problems::default();
        let n = index("ts.md", &text, &ids(CAT), &mut problems);
        (n, lines(&problems))
    }

    const CMD1: &str = "| CMD-01 | a | `references/commands.md` |\n";
    const CMD2: &str = "| `CMD-02` | b | commands.md §1 |\n";
    const VC2: &str = "| **VC-02** | c \\| d | voice.md |\n";

    #[test]
    fn q8_index_happy() {
        assert_eq!(index_lines(&format!("{CMD1}{CMD2}{VC2}")), (3, vec![]));
    }

    #[test]
    fn q9_q15_missing_ids() {
        assert_eq!(
            index_lines(&format!("{CMD2}{CMD1}")).1,
            ["skill-check: ts.md: rule id index: VC-02 has no row"]
        );
        assert_eq!(
            index_lines("").1,
            [
                "skill-check: ts.md: rule id index: CMD-01 has no row",
                "skill-check: ts.md: rule id index: CMD-02 has no row",
                "skill-check: ts.md: rule id index: VC-02 has no row",
            ]
        );
    }

    #[test]
    fn q10_unknown_or_invalid_ids() {
        for id in ["ZZ-01", "cmd-01", "CV2-1", "ŞŞ-01"] {
            let (_, got) = index_lines(&format!("{CMD1}{CMD2}{VC2}| {id} | x | y.md |\n"));
            assert_eq!(
                got,
                [format!(
                    "skill-check: ts.md: rule id index: {id} is not a catalog rule"
                )]
            );
        }
    }

    #[test]
    fn q11_duplicate_row() {
        assert_eq!(
            index_lines(&format!("{CMD1}{CMD1}{CMD2}{VC2}")).1,
            ["skill-check: ts.md: rule id index: CMD-01 listed twice"]
        );
    }

    #[test]
    fn q12_order() {
        assert_eq!(
            index_lines(&format!("{CMD2}{CMD1}{VC2}")).1,
            [
                "skill-check: ts.md: rule id index: rows are not in catalog order (first difference: CMD-02)"
            ]
        );
    }

    #[test]
    fn q13_q14_row_cells() {
        for row in [
            "| CMD-01 | |\n",
            "| CMD-01 | a |\n",
            "| CMD-01 | | x.md |\n",
        ] {
            assert_eq!(
                index_lines(&format!("{row}{CMD2}{VC2}")).1,
                ["skill-check: ts.md: rule id index: CMD-01 row needs id, meaning and fix cells"],
                "{row:?}"
            );
        }
        assert_eq!(
            index_lines(&format!("| CMD-01 | a | see above |\n{CMD2}{VC2}")).1,
            [
                "skill-check: ts.md: rule id index: CMD-01 fix cell must name a skill reference (*.md)"
            ]
        );
    }

    #[test]
    fn q16_cheatsheet() {
        let mut problems = Problems::default();
        cheatsheet(
            "c.md",
            "CMD-01, XCMD-02 CMD-021 (VC-02)",
            &ids(CAT),
            &mut problems,
        );
        assert_eq!(lines(&problems), ["skill-check: c.md: CMD-02 is missing"]);
    }

    fn ud7(text: &str) -> Vec<String> {
        let mut problems = Problems::default();
        ud7_examples("f.md", text, &mut problems);
        lines(&problems)
    }

    #[test]
    fn q19_ud7_in_rust_examples() {
        let text = "msg_collector\n```rust,ignore\nlet c = msg_collector();\n// wait_for here\nlet s = \"on_listener\";\n```\n```rust recipe=x\napp.on_listener(f);\n```\n```rust\nctx.wait_for(x); /* listener */\n```\n```text\nwait_for\n```\n";
        assert_eq!(
            ud7(text),
            [
                format!("skill-check: f.md:11: {UD7_AWAIT}"),
                format!("skill-check: f.md:3: {UD7_COLLECTOR}"),
                format!("skill-check: f.md:8: {UD7_LISTENER}"),
            ]
        );
        // A multi-line string literal hides its contents.
        assert!(ud7("```rust\nlet s = \"a\nwait_for\";\n```\n").is_empty());
    }

    #[test]
    fn fences_and_cells() {
        let kinds: Vec<LineKind> = md_lines("a\n````rust\n```\n````\n~~~\nb\n~~~\n")
            .into_iter()
            .map(|(_, _, k)| k)
            .collect();
        assert_eq!(
            kinds,
            [
                LineKind::Prose,
                LineKind::Fence,
                LineKind::Code { rust: true },
                LineKind::Fence,
                LineKind::Fence,
                LineKind::Code { rust: false },
                LineKind::Fence,
            ]
        );
        assert_eq!(cells("| a | b \\| c | |"), ["a", "b \\| c", ""]);
        assert!(is_separator(&cells("|---|:-:|")));
        assert!(!is_separator(&cells("| - | x |")));
    }
}
