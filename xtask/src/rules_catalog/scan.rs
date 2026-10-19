//! Text scanners over Rust sources: `// [<RULE-ID>]` tags, `compile_fail`
//! doctests and rule-citing test function names. No parsing beyond lines.

use super::is_rule_id;

/// A `// [<RULE-ID>]` tag on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tag {
    pub(crate) id: String,
    /// Whether code precedes the `//` on that line (only then is it evidence).
    pub(crate) after_code: bool,
}

/// Every `// [<RULE-ID>]` tag on `line` (ids of rule-id shape only).
pub(crate) fn tags(line: &str) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = line[from..].find("// [") {
        let at = from + pos;
        let rest = &line[at + 4..];
        if let Some(end) = rest.find(']')
            && is_rule_id(&rest[..end])
        {
            out.push(Tag {
                id: rest[..end].to_owned(),
                after_code: !line[..at].trim().is_empty(),
            });
        }
        from = at + 4;
    }
    out
}

/// Whether a fence info string marks a counted `compile_fail` doctest (D2, D3).
fn is_compile_fail(info: &str) -> bool {
    let words: Vec<&str> = info
        .split([',', ' ', '\t'])
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect();
    words.contains(&"compile_fail") && !words.contains(&"ignore")
}

/// The doc-comment payload of `line` (`///` or `//!`, one leading space
/// dropped), or `None` for any other line.
fn doc_payload(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("///")
        .filter(|r| !r.starts_with('/'))
        .or_else(|| trimmed.strip_prefix("//!"))?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// The tags inside counted `compile_fail` doctest blocks of `source`, with
/// their 1-based line numbers (D1-D7). Hidden lines (`# …`) count.
pub(crate) fn doctest_tags(source: &str) -> Vec<(usize, Tag)> {
    let mut out = Vec::new();
    // `(fence char, fence length, counted)` of the open fence.
    let mut fence: Option<(char, usize, bool)> = None;
    for (ix, line) in source.lines().enumerate() {
        let Some(payload) = doc_payload(line) else {
            fence = None;
            continue;
        };
        let trimmed = payload.trim_start();
        match fence {
            None => {
                if let Some((c, len)) = fence_marker(trimmed) {
                    fence = Some((c, len, is_compile_fail(&trimmed[len..])));
                }
            }
            Some((c, len, counted)) => {
                if fence_marker(trimmed)
                    .is_some_and(|(c2, l2)| c2 == c && l2 >= len && trimmed[l2..].trim().is_empty())
                {
                    fence = None;
                } else if counted {
                    let code = if trimmed == "#" {
                        ""
                    } else {
                        trimmed.strip_prefix("# ").unwrap_or(trimmed)
                    };
                    out.extend(tags(code).into_iter().map(|t| (ix + 1, t)));
                }
            }
        }
    }
    out
}

/// A code fence (three or more backticks or tildes) at the start of `text`.
fn fence_marker(text: &str) -> Option<(char, usize)> {
    let c = text.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = text.chars().take_while(|&x| x == c).count();
    (len >= 3).then_some((c, len))
}

/// How many lines after a test attribute the `fn` may appear (U4).
const ATTR_WINDOW: usize = 10;

/// Whether `trimmed` starts with a test attribute; returns what follows it.
fn test_attr(trimmed: &str) -> Option<&str> {
    if let Some(rest) = trimmed.strip_prefix("#[test]") {
        return Some(rest);
    }
    let rest = trimmed.strip_prefix("#[tokio::test")?;
    if let Some(rest) = rest.strip_prefix(']') {
        return Some(rest);
    }
    rest.strip_prefix('(')?;
    let end = rest.find(")]")?;
    Some(&rest[end + 2..])
}

/// The function name declared on `text`, if it declares one.
fn fn_name(text: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(pos) = text[from..].find("fn ") {
        let at = from + pos;
        if at == 0 || text[..at].ends_with([' ', '\t']) {
            let rest = text[at + 3..].trim_start();
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if end > 0 {
                return Some(&rest[..end]);
            }
        }
        from = at + 3;
    }
    None
}

/// The names of the non-ignored test functions in `source` (U1-U7).
pub(crate) fn test_fns(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    for (ix, line) in lines.iter().enumerate() {
        let Some(after) = test_attr(line.trim_start()) else {
            continue;
        };
        if let Some(name) = fn_name(after) {
            out.push(name.to_owned());
            continue;
        }
        for next in lines.iter().skip(ix + 1).take(ATTR_WINDOW) {
            let trimmed = next.trim_start();
            if trimmed.starts_with("#[ignore") {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            if let Some(name) = fn_name(trimmed) {
                out.push(name.to_owned());
                break;
            }
        }
    }
    out
}

/// The rule ids a test function name cites: every `<area>_<nn>` token pair,
/// `<area>` lowercase `[a-z][a-z0-9]{1,3}` and `<nn>` exactly two digits (U2, U3).
pub(crate) fn cited_ids(name: &str) -> Vec<String> {
    let tokens: Vec<&str> = name.split('_').collect();
    tokens
        .windows(2)
        .filter_map(|pair| {
            let (area, num) = (pair[0], pair[1]);
            let lower = area
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
            let id = format!("{}-{num}", area.to_ascii_uppercase());
            (lower && is_rule_id(&id)).then_some(id)
        })
        .collect()
}

/// The snake form a test name uses for `id` (`CV2-03` → `cv2_03`).
pub(crate) fn snake(id: &str) -> String {
    id.to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(source: &str) -> Vec<String> {
        doctest_tags(source)
            .into_iter()
            .filter(|(_, t)| t.after_code)
            .map(|(_, t)| t.id)
            .collect()
    }

    #[test]
    fn d1_tagged_block() {
        let src =
            "/// ```compile_fail\n/// let _ = Spacing::Medium; // [CV2-13]\n/// ```\nfn f() {}\n";
        assert_eq!(ids(src), ["CV2-13"]);
        assert_eq!(doctest_tags(src)[0].0, 2);
    }

    #[test]
    fn d2_info_strings() {
        for open in [
            "```compile_fail,E0599",
            "```rust,compile_fail",
            "~~~compile_fail",
        ] {
            let close = &open[..3];
            let src = format!("//! {open}\n//! x(); // [CV2-13]\n//! {close}\n");
            assert_eq!(ids(&src), ["CV2-13"], "{open}");
        }
    }

    #[test]
    fn d3_ignored_or_other_blocks() {
        for open in ["```compile_fail,ignore", "```rust", "```text", "```"] {
            let src = format!("/// {open}\n/// x(); // [CV2-13]\n/// ```\n");
            assert!(ids(&src).is_empty(), "{open}");
        }
    }

    #[test]
    fn d4_tag_outside_fence_or_alone() {
        let src = "/// x(); // [CV2-13]\n/// ```compile_fail\n/// // [CV2-13]\n///     // [CV2-13]\n/// ```\n";
        assert!(ids(src).is_empty());
        assert_eq!(doctest_tags(src).len(), 2);
    }

    #[test]
    fn d5_non_doc_line_closes_fence() {
        let src = "/// ```compile_fail\nfn f() {}\n/// x(); // [CV2-13]\n";
        assert!(ids(src).is_empty());
    }

    #[test]
    fn d7_hidden_line() {
        let src = "/// ```compile_fail\n/// # let _ = x; // [CV2-13]\n/// ```\n";
        assert_eq!(ids(src), ["CV2-13"]);
        let alone = "/// ```compile_fail\n/// # // [CV2-13]\n/// ```\n";
        assert!(ids(alone).is_empty());
    }

    #[test]
    fn tags_need_rule_id_shape() {
        assert!(tags("x(); // [not-an-id]").is_empty());
        assert_eq!(tags("x(); // [MSG-03]")[0].id, "MSG-03");
        assert!(!tags("   // [MSG-03]")[0].after_code);
    }

    #[test]
    fn u1_cited() {
        let src = "#[test]\nfn cv2_03_forty_one_fails() {}\n";
        assert_eq!(test_fns(src), ["cv2_03_forty_one_fails"]);
        assert_eq!(cited_ids("cv2_03_forty_one_fails"), ["CV2-03"]);
    }

    #[test]
    fn u2_token_positions() {
        assert_eq!(cited_ids("cv2_03"), ["CV2-03"]);
        assert_eq!(cited_ids("x_cv2_03"), ["CV2-03"]);
        assert_eq!(cited_ids("x_cv2_03_y"), ["CV2-03"]);
        assert_eq!(
            cited_ids("meta_01_and_meta_02_conflict"),
            ["META-01", "META-02"]
        );
    }

    #[test]
    fn u3_near_misses() {
        for name in ["cv2_031_x", "cv2_3_x", "CV2_03"] {
            assert!(cited_ids(name).is_empty(), "{name}");
        }
        // `xcv2` has rule-id shape but is no catalog area; the caller drops it.
        assert_eq!(cited_ids("xcv2_03"), ["XCV2-03"]);
    }

    #[test]
    fn u4_attributes_between() {
        let src = "#[test]\n#[should_panic]\n/// doc\n// note\npub fn msg_05_x() {}\n";
        assert_eq!(test_fns(src), ["msg_05_x"]);
        let far = format!("#[test]\n{}fn msg_05_x() {{}}\n", "#[a]\n".repeat(10));
        assert!(test_fns(&far).is_empty());
    }

    #[test]
    fn u5_ignored() {
        let src = "#[test]\n#[ignore = \"slow\"]\nfn msg_05_x() {}\n";
        assert!(test_fns(src).is_empty());
    }

    #[test]
    fn u6_async_and_proptest() {
        let src = "#[tokio::test]\nasync fn a_cmd_01() {}\n\
                   #[tokio::test(flavor = \"multi_thread\")]\nasync fn b_cmd_02() {}\n\
                   proptest! {\n    #[test]\n    fn c_cmd_03(x in 0..1) {}\n}\n\
                   #[test] fn d_cmd_04() {}\n";
        assert_eq!(
            test_fns(src),
            ["a_cmd_01", "b_cmd_02", "c_cmd_03", "d_cmd_04"]
        );
    }

    #[test]
    fn u7_in_doc_comment() {
        let src = "/// #[test]\n/// fn cmd_01() {}\n//! #[test]\nfn cmd_02() {}\n";
        assert!(test_fns(src).is_empty());
    }

    #[test]
    fn snake_form() {
        assert_eq!(snake("META-01"), "meta_01");
    }
}
