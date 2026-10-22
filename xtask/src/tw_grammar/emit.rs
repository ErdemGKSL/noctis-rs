//! Combines the extracted takumi tables with the closed table and writes the
//! text of `crates/noctis-image/src/tw/generated.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use super::GrammarError;
use super::extract::{Extracted, MAP_RS, MOD_RS, NAMESPACE_RS};
use super::table::{EXACT_RENAMES, FINGERPRINTS, HINTS, PREFIX_RENAMES, VALUE_GRAMMARS};

/// The table file, named in errors about it.
const TABLE_RS: &str = "xtask/src/tw_grammar/table.rs";

/// Checks the extracted tables against the closed table and the fingerprints.
pub(crate) fn check(ex: &Extracted) -> Result<(), GrammarError> {
    check_fingerprints(&ex.fingerprints, FINGERPRINTS)?;
    check_parse_types(ex, VALUE_GRAMMARS)?;
    check_names(ex)
}

/// Every `PREFIX_PARSERS` variant and every namespace is known.
pub(crate) fn check_names(ex: &Extracted) -> Result<(), GrammarError> {
    let variants: BTreeSet<&str> = ex.parsers.iter().map(|p| p.variant.as_str()).collect();
    for (prefix, parsers) in &ex.prefixes {
        if let Some(unknown) = parsers
            .iter()
            .find(|p| !variants.contains(p.variant.as_str()))
        {
            return Err(GrammarError::new(
                MAP_RS,
                "PREFIX_PARSERS",
                format!(
                    "`{prefix}`: unknown PropertyParser variant `{}`",
                    unknown.variant
                ),
            ));
        }
    }
    let namespaces: BTreeSet<&str> = ex.namespaces.iter().map(|(v, _)| v.as_str()).collect();
    for (prefix, list) in &ex.var_targets {
        if let Some(unknown) = list.iter().find(|n| !namespaces.contains(n.as_str())) {
            return Err(GrammarError::new(
                MAP_RS,
                "VAR_TARGETS",
                format!("`{prefix}`: unknown namespace `Namespace::{unknown}`"),
            ));
        }
    }
    for parser in &ex.parsers {
        for ns in parser.namespaces.iter().flatten() {
            if !namespaces.contains(ns.as_str()) {
                return Err(GrammarError::new(
                    MAP_RS,
                    "property_parsers!",
                    format!("`{}`: unknown namespace `Namespace::{ns}`", parser.variant),
                ));
            }
        }
    }
    if ex.namespaces.iter().any(|(v, _)| v == "Breakpoint") {
        return Err(GrammarError::new(
            NAMESPACE_RS,
            "enum Namespace",
            "has a `Breakpoint` variant, which the generator adds itself",
        ));
    }
    Ok(())
}

/// Every parse type has exactly one closed-table entry, and vice versa.
pub(crate) fn check_parse_types(
    ex: &Extracted,
    table: &[(&str, &str)],
) -> Result<(), GrammarError> {
    let types: BTreeSet<&str> = ex.parsers.iter().map(|p| p.parse_type.as_str()).collect();
    let keys: BTreeSet<&str> = table.iter().map(|(k, _)| *k).collect();
    if let Some(missing) = types.difference(&keys).next() {
        return Err(GrammarError::new(
            MAP_RS,
            "property_parsers!",
            format!("parse type `{missing}` has no entry in the closed suffix table ({TABLE_RS})"),
        ));
    }
    if let Some(stale) = keys.difference(&types).next() {
        return Err(GrammarError::new(
            TABLE_RS,
            "VALUE_GRAMMARS",
            format!("entry `{stale}` is not a parse type of {MAP_RS}"),
        ));
    }
    if keys.len() != table.len() {
        return Err(GrammarError::new(
            TABLE_RS,
            "VALUE_GRAMMARS",
            "has a duplicate entry",
        ));
    }
    Ok(())
}

/// The source's fingerprints equal the recorded ones, item by item.
pub(crate) fn check_fingerprints(
    found: &BTreeMap<String, String>,
    recorded: &[(&str, &str)],
) -> Result<(), GrammarError> {
    let recorded: BTreeMap<&str, &str> = recorded.iter().copied().collect();
    let file_of = |item: &str| {
        if item.starts_with("impl ") {
            "src/**/*.rs"
        } else if item.starts_with("macro_rules! property_parsers") {
            MAP_RS
        } else {
            MOD_RS
        }
    };
    for (item, new) in found {
        match recorded.get(item.as_str()) {
            Some(old) if old == new => {}
            Some(old) => {
                return Err(GrammarError::new(
                    file_of(item),
                    item,
                    format!(
                        "fingerprint changed (old {old}, new {new}); re-audit the matcher and \
                         the closed table, then update the hash in {TABLE_RS}"
                    ),
                ));
            }
            None => {
                return Err(GrammarError::new(
                    file_of(item),
                    item,
                    format!(
                        "has no recorded fingerprint (new {new}); audit it and add it to {TABLE_RS}"
                    ),
                ));
            }
        }
    }
    if let Some(gone) = recorded.keys().find(|item| !found.contains_key(**item)) {
        return Err(GrammarError::new(
            file_of(gone),
            gone,
            format!("is fingerprinted in {TABLE_RS} but missing from the source"),
        ));
    }
    Ok(())
}

/// `TwGradientPosition` -> `TW_GRADIENT_POSITION`.
fn const_name(ty: &str) -> String {
    let mut out = String::new();
    for (i, c) in ty.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn quoted(list: &[&str]) -> String {
    list.iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The unformatted text of `generated.rs`.
pub(crate) fn render(ex: &Extracted, version: &str) -> String {
    let mut out = String::new();
    let w = &mut out;
    let _ = writeln!(
        w,
        "// @generated by `cargo run --release -p xtask -- tw-grammar` from takumi-core {version}. Do not edit.\n"
    );
    let _ = writeln!(
        w,
        "//! The Tailwind tables of takumi-core {version} and the closed suffix grammar.\n"
    );
    let _ = writeln!(
        w,
        "use crate::tw::suffix::{{ArbitraryKind, FONT_SIZES, LENGTH_KEYWORDS, Parser, Rule, SuffixRule}};\n"
    );
    let _ = writeln!(
        w,
        "/// The takumi-core version these tables were generated from.\npub const TAKUMI_CORE_VERSION: &str = {version:?};\n"
    );

    render_namespaces(w, ex);

    let breakpoints: Vec<&str> = ex.breakpoints.iter().map(String::as_str).collect();
    let _ = writeln!(
        w,
        "/// The built-in breakpoints of `Breakpoint::parse`.\npub(crate) const BREAKPOINTS: &[&str] = &[{}];\n",
        quoted(&breakpoints)
    );
    let fixed: Vec<&str> = ex.fixed.iter().map(String::as_str).collect();
    let _ = writeln!(
        w,
        "/// `FIXED_PROPERTIES` keys, sorted.\npub(crate) const FIXED: &[&str] = &[{}];\n",
        quoted(&fixed)
    );

    let mut grammars = VALUE_GRAMMARS.to_vec();
    grammars.sort_unstable();
    for (ty, expr) in grammars {
        let _ = writeln!(
            w,
            "/// Value grammar of takumi's `{ty}`.\nconst {}: &[Rule] = {expr};\n",
            const_name(ty)
        );
    }

    render_lookup_tables(w, ex);
    render_renames(w);
    out
}

/// Pipes `text` through `rustfmt --edition 2024`, run in `dir`.
pub(crate) fn rustfmt(text: &str, dir: &Path) -> Result<String, GrammarError> {
    let fail = |message: String| GrammarError::new("generated.rs", "rustfmt", message);
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(format!("could not start rustfmt: {e}")))?;
    child
        .stdin
        .take()
        .ok_or_else(|| fail("rustfmt has no stdin".to_owned()))?
        .write_all(text.as_bytes())
        .map_err(|e| fail(format!("could not write to rustfmt: {e}")))?;
    let output = child
        .wait_with_output()
        .map_err(|e| fail(format!("rustfmt failed: {e}")))?;
    if !output.status.success() {
        return Err(fail(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8(output.stdout).map_err(|e| fail(format!("rustfmt output is not UTF-8: {e}")))
}

/// The `ThemeNamespace` enum and its `css_prefix`.
fn render_namespaces(w: &mut String, ex: &Extracted) {
    let _ = writeln!(
        w,
        "/// A theme namespace: the CSS custom-property prefix a theme token is defined under.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n#[non_exhaustive]\npub enum ThemeNamespace {{"
    );
    let mut namespaces = ex.namespaces.clone();
    namespaces.push(("Breakpoint".to_owned(), "--breakpoint-".to_owned()));
    for (variant, prefix) in &namespaces {
        let _ = writeln!(w, "    /// `{prefix}*`\n    {variant},");
    }
    let _ = writeln!(
        w,
        "}}\n\nimpl ThemeNamespace {{\n    /// The CSS custom-property prefix, such as `--color-`.\n    \
         pub const fn css_prefix(self) -> &'static str {{\n        match self {{"
    );
    for (variant, prefix) in &namespaces {
        let _ = writeln!(w, "            Self::{variant} => {prefix:?},");
    }
    let _ = writeln!(w, "        }}\n    }}\n}}\n");
}

/// `PREFIXES` and `VAR_TARGETS`.
fn render_lookup_tables(w: &mut String, ex: &Extracted) {
    let parse_type: BTreeMap<&str, &str> = ex
        .parsers
        .iter()
        .map(|p| (p.variant.as_str(), p.parse_type.as_str()))
        .collect();
    let _ = writeln!(
        w,
        "/// `PREFIX_PARSERS`, sorted by prefix; parsers keep the source order.\n\
         pub(crate) const PREFIXES: &[(&str, &[Parser])] = &["
    );
    for (prefix, parsers) in &ex.prefixes {
        let _ = write!(w, "    ({prefix:?}, &[");
        for parser in parsers {
            let ty = parse_type
                .get(parser.variant.as_str())
                .copied()
                .unwrap_or_default();
            let _ = write!(
                w,
                "Parser {{ variant: {:?}, property: {:?}, value: {}, negatable: {} }}, ",
                parser.variant,
                parser.property,
                const_name(ty),
                ex.negatable.contains(&parser.property)
            );
        }
        let _ = writeln!(w, "]),");
    }
    let _ = writeln!(w, "];\n");

    let _ = writeln!(
        w,
        "/// `VAR_TARGETS`, sorted by prefix: the namespaces a theme token is read from.\n\
         pub(crate) const VAR_TARGETS: &[(&str, &[ThemeNamespace])] = &["
    );
    for (prefix, list) in &ex.var_targets {
        let list: Vec<String> = list
            .iter()
            .map(|n| format!("ThemeNamespace::{n}"))
            .collect();
        let _ = writeln!(w, "    ({prefix:?}, &[{}]),", list.join(", "));
    }
    let _ = writeln!(w, "];\n");
}

/// The renames and hints, sorted.
fn render_renames(w: &mut String) {
    let mut prefix_renames = PREFIX_RENAMES.to_vec();
    prefix_renames.sort_unstable();
    let _ = writeln!(
        w,
        "/// v3 -> v4 prefix renames: `(v3 prefix, v4 prefix, renamed suffixes)`.\n\
         pub(crate) const PREFIX_RENAMES: &[(&str, &str, &[&str])] = &["
    );
    for (from, to, suffixes) in prefix_renames {
        let _ = writeln!(w, "    ({from:?}, {to:?}, &[{}]),", quoted(suffixes));
    }
    let _ = writeln!(w, "];\n");
    let mut exact = EXACT_RENAMES.to_vec();
    exact.sort_unstable();
    let _ = writeln!(
        w,
        "/// v3 -> v4 exact renames.\npub(crate) const EXACT_RENAMES: &[(&str, &str)] = &["
    );
    for (from, to) in exact {
        let _ = writeln!(w, "    ({from:?}, {to:?}),");
    }
    let _ = writeln!(w, "];\n");
    let mut hints = HINTS.to_vec();
    hints.sort_unstable();
    let _ = writeln!(
        w,
        "/// Hints for v3 utilities v4 dropped: `(prefix, hint)`.\npub(crate) const HINTS: &[(&str, &str)] = &["
    );
    for (prefix, hint) in hints {
        let _ = writeln!(w, "    ({prefix:?}, {hint:?}),");
    }
    let _ = writeln!(w, "];");
}
