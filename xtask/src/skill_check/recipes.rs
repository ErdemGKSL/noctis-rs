//! The recipe sync check (R1-R6): every ```` ```rust recipe=<name> ```` block of the
//! Usage Skill's `views-and-recipes.md` equals the region of the same name in
//! `examples/ui-recipes/src`, the compiled and tested copy (AD-27, FR-50).
//!
//! A **region** is the lines strictly between a `// recipe: <name>` line and the
//! next `// recipe-end: <name>` line of one `.rs` file (both trimmed). A **block** is
//! the lines strictly between a ```` ```rust recipe=<name> ```` fence and the next
//! ```` ``` ```` line. Both are compared after trimming trailing whitespace on every
//! line and dropping leading and trailing blank lines.

use super::{Problems, Repo};

/// The example crate whose regions the skill copies.
pub(super) const RECIPES_DIR: &str = "examples/ui-recipes/src";
/// The recipes the skill must show.
const REQUIRED: [&str; 4] = ["tabs", "pagination", "step-flow", "settings"];

const START: &str = "// recipe: ";
const END: &str = "// recipe-end: ";
const FENCE: &str = "```rust recipe=";

/// A named run of lines and where it starts (`path:line` of its opening line).
#[derive(Debug)]
struct Named {
    name: String,
    at: String,
    lines: Vec<String>,
}

/// Checks the recipes against `md` (the skill file at `md_path`); returns how many
/// recipes are in sync.
pub(super) fn check(repo: &Repo<'_>, md_path: &str, md: &str, problems: &mut Problems) -> usize {
    let Some(files) = repo.files(RECIPES_DIR, "rs") else {
        problems.add(RECIPES_DIR, "cannot read: not a directory");
        return 0;
    };
    let sources: Vec<(String, String)> = files
        .into_iter()
        .filter_map(|file| repo.read(&file, problems).map(|src| (file, src)))
        .collect();
    compare(md_path, md, &sources, problems)
}

/// [`check`] over sources already read.
fn compare(
    md_path: &str,
    md: &str,
    sources: &[(String, String)],
    problems: &mut Problems,
) -> usize {
    let mut regions = Vec::new();
    for (file, src) in sources {
        regions.extend(regions_of(file, src, problems));
    }
    let blocks = blocks_of(md_path, md, problems);
    let regions = unique(regions, "recipe region", problems);
    let blocks = unique(blocks, "recipe block", problems);

    let mut in_sync = 0;
    for block in &blocks {
        let Some(region) = regions.iter().find(|r| r.name == block.name) else {
            problems.add(
                md_path,
                format!("recipe `{}` has no region in {RECIPES_DIR}", block.name),
            );
            continue;
        };
        let (a, b) = (normalized(&block.lines), normalized(&region.lines));
        match (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
            Some(i) => problems.add(
                md_path,
                format!(
                    "recipe `{}` differs from {} (recipe line {})",
                    block.name,
                    file_of(&region.at),
                    i + 1
                ),
            ),
            None => in_sync += 1,
        }
    }
    for region in &regions {
        if !blocks.iter().any(|b| b.name == region.name) {
            problems.add(
                &region.at,
                format!("recipe region `{}` is not in {md_path}", region.name),
            );
        }
    }
    for name in REQUIRED {
        let known = |set: &[Named]| set.iter().any(|n| n.name == name);
        if !known(&blocks) && !known(&regions) {
            problems.add(
                md_path,
                format!(
                    "recipe `{name}` is missing (required: {})",
                    REQUIRED.join(", ")
                ),
            );
        }
    }
    in_sync
}

/// The file part of a `path:line` location.
fn file_of(at: &str) -> &str {
    at.rsplit_once(':').map_or(at, |(file, _)| file)
}

/// Keeps the first of each name; reports every later one.
fn unique(items: Vec<Named>, what: &str, problems: &mut Problems) -> Vec<Named> {
    let mut out: Vec<Named> = Vec::new();
    for item in items {
        if let Some(first) = out.iter().find(|n| n.name == item.name) {
            problems.add(
                &item.at,
                format!("duplicate {what} `{}` (first at {})", item.name, first.at),
            );
        } else {
            out.push(item);
        }
    }
    out
}

/// The regions of one `.rs` file; marker mistakes are reported.
fn regions_of(file: &str, src: &str, problems: &mut Problems) -> Vec<Named> {
    let mut out = Vec::new();
    let mut open: Option<Named> = None;
    for (n, line) in src.lines().enumerate() {
        let at = format!("{file}:{}", n + 1);
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix(START) {
            if let Some(outer) = &open {
                problems.add(
                    &at,
                    format!(
                        "recipe region `{}` starts inside `{}` (regions do not nest)",
                        name.trim(),
                        outer.name
                    ),
                );
                continue;
            }
            open = Some(Named {
                name: name.trim().to_owned(),
                at,
                lines: Vec::new(),
            });
        } else if let Some(name) = trimmed.strip_prefix(END) {
            match open.take() {
                Some(region) if region.name == name.trim() => out.push(region),
                Some(region) => {
                    problems.add(
                        &at,
                        format!(
                            "recipe-end `{}` does not close region `{}`",
                            name.trim(),
                            region.name
                        ),
                    );
                    open = Some(region);
                }
                None => problems.add(
                    &at,
                    format!("recipe-end `{}` without a recipe start", name.trim()),
                ),
            }
        } else if let Some(region) = &mut open {
            region.lines.push(line.to_owned());
        }
    }
    if let Some(region) = open {
        problems.add(
            &region.at,
            format!("recipe region `{}` is not closed", region.name),
        );
    }
    out
}

/// The `rust recipe=` blocks of the Markdown file.
fn blocks_of(md_path: &str, md: &str, problems: &mut Problems) -> Vec<Named> {
    let mut out = Vec::new();
    let mut open: Option<Named> = None;
    for (n, line) in md.lines().enumerate() {
        match &mut open {
            Some(_) if line.trim_end() == "```" => out.extend(open.take()),
            Some(block) => block.lines.push(line.to_owned()),
            None => {
                if let Some(name) = line.trim_end().strip_prefix(FENCE) {
                    open = Some(Named {
                        name: name.trim().to_owned(),
                        at: format!("{md_path}:{}", n + 1),
                        lines: Vec::new(),
                    });
                }
            }
        }
    }
    if let Some(block) = open {
        problems.add(
            &block.at,
            format!("recipe block `{}` is not closed", block.name),
        );
    }
    out
}

/// `lines` with trailing whitespace trimmed and edge blank lines dropped.
fn normalized(lines: &[String]) -> Vec<&str> {
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();
    let start = trimmed
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(trimmed.len());
    let end = trimmed
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(start, |i| i + 1);
    trimmed[start..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "s/views.md";

    fn rs(name: &str, body: &str) -> String {
        format!(
            "//! Header.\n\n// recipe: {name}\n{body}\n// recipe-end: {name}\n\n#[cfg(test)]\nmod t {{}}\n"
        )
    }

    fn md(name: &str, body: &str) -> String {
        format!("# Recipes\n\n```rust recipe={name}\n{body}\n```\n\nProse.\n")
    }

    fn all_required(
        extra_rs: &[(String, String)],
        extra_md: &str,
    ) -> (Vec<(String, String)>, String) {
        let mut sources: Vec<(String, String)> = REQUIRED
            .iter()
            .map(|n| {
                (
                    format!("src/{n}.rs"),
                    rs(n, &format!("fn {}() {{}}", n.replace('-', "_"))),
                )
            })
            .collect();
        sources.extend(extra_rs.iter().cloned());
        let mut text: String = REQUIRED
            .iter()
            .map(|n| md(n, &format!("fn {}() {{}}", n.replace('-', "_"))))
            .collect();
        text.push_str(extra_md);
        (sources, text)
    }

    fn run(sources: &[(String, String)], text: &str) -> (usize, Vec<String>) {
        let mut problems = Problems::default();
        let n = compare(MD, text, sources, &mut problems);
        (n, problems.0.iter().map(ToString::to_string).collect())
    }

    /// R1: equal up to trailing spaces and edge blank lines.
    #[test]
    fn r1_equal_regions_are_in_sync() {
        let (mut sources, mut text) = all_required(&[], "");
        sources.push((
            "src/a.rs".to_owned(),
            rs("app", "\nuse x;  \nfn a() {}\n\n"),
        ));
        text.push_str(&md("app", "use x;\nfn a() {}   "));
        assert_eq!(run(&sources, &text), (5, vec![]));
    }

    /// R2: the first differing line is named.
    #[test]
    fn r2_a_differing_line_is_reported() {
        let (mut sources, mut text) = all_required(&[], "");
        sources.push(("src/a.rs".to_owned(), rs("app", "use x;\nfn a() {}")));
        text.push_str(&md("app", "use x;\nfn b() {}"));
        assert_eq!(
            run(&sources, &text),
            (
                4,
                vec![format!(
                    "skill-check: {MD}: recipe `app` differs from src/a.rs (recipe line 2)"
                )]
            )
        );
        // A missing last line is a difference too.
        let (mut sources, mut text) = all_required(&[], "");
        sources.push(("src/a.rs".to_owned(), rs("app", "use x;\nfn a() {}")));
        text.push_str(&md("app", "use x;"));
        assert_eq!(run(&sources, &text).1.len(), 1);
    }

    /// R3: a block without a region, a region without a block.
    #[test]
    fn r3_unpaired_recipes_are_reported() {
        let (sources, text) = all_required(
            &[("src/a.rs".to_owned(), rs("lonely", "x"))],
            &md("ghost", "y"),
        );
        assert_eq!(
            run(&sources, &text).1,
            [
                "skill-check: s/views.md: recipe `ghost` has no region in examples/ui-recipes/src"
                    .to_owned(),
                "skill-check: src/a.rs:3: recipe region `lonely` is not in s/views.md".to_owned(),
            ]
        );
    }

    /// R4: a required recipe in neither place.
    #[test]
    fn r4_required_recipes_must_exist() {
        let sources = vec![("src/tabs.rs".to_owned(), rs("tabs", "x"))];
        let (n, problems) = run(&sources, &md("tabs", "x"));
        assert_eq!(n, 1);
        assert_eq!(
            problems,
            ["pagination", "settings", "step-flow"].map(|name| format!(
                "skill-check: s/views.md: recipe `{name}` is missing (required: tabs, pagination, step-flow, settings)"
            ))
        );
    }

    /// R5: marker mistakes, one problem each.
    #[test]
    fn r5_marker_mistakes() {
        let cases = [
            (
                "// recipe: a\nx\n",
                "skill-check: f.rs:1: recipe region `a` is not closed",
            ),
            (
                "x\n// recipe-end: a\n",
                "skill-check: f.rs:2: recipe-end `a` without a recipe start",
            ),
            (
                "// recipe: a\n// recipe: b\n// recipe-end: a\n",
                "skill-check: f.rs:2: recipe region `b` starts inside `a` (regions do not nest)",
            ),
            (
                "// recipe: a\n// recipe-end: b\n// recipe-end: a\n",
                "skill-check: f.rs:2: recipe-end `b` does not close region `a`",
            ),
        ];
        for (src, want) in cases {
            let mut problems = Problems::default();
            let _ = regions_of("f.rs", src, &mut problems);
            let got: Vec<String> = problems.0.iter().map(ToString::to_string).collect();
            assert_eq!(got, [want.to_owned()], "{src:?}");
        }
        let (sources, text) = all_required(
            &[("src/again.rs".to_owned(), rs("tabs", "fn tabs() {}"))],
            &md("tabs", "fn tabs() {}"),
        );
        assert_eq!(
            run(&sources, &text).1,
            [
                "skill-check: s/views.md:31: duplicate recipe block `tabs` (first at s/views.md:3)"
                    .to_owned(),
                "skill-check: src/again.rs:3: duplicate recipe region `tabs` (first at src/tabs.rs:3)"
                    .to_owned(),
            ]
        );
        let mut problems = Problems::default();
        let _ = blocks_of(MD, "```rust recipe=a\nx\n", &mut problems);
        assert_eq!(problems.0.len(), 1);
    }

    /// R6: the example directory is unreadable.
    #[test]
    fn r6_missing_directory() {
        let root = std::env::temp_dir().join(format!("xtask-recipes-r6-{}", std::process::id()));
        let repo = Repo { root: &root };
        let mut problems = Problems::default();
        assert_eq!(check(&repo, MD, "", &mut problems), 0);
        let got: Vec<String> = problems.0.iter().map(ToString::to_string).collect();
        assert_eq!(
            got,
            ["skill-check: examples/ui-recipes/src: cannot read: not a directory"]
        );
    }
}
