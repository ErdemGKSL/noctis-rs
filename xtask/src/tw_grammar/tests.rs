//! Fixture tests for the generator: one per xtask row of the Story 10.1
//! matrix, plus a check against the real pinned takumi-core when present.

use std::collections::{BTreeMap, BTreeSet};

use super::emit::{check, check_fingerprints, check_names, check_parse_types};
use super::extract::{
    Extracted, MAP_RS, extract, extract_breakpoints, extract_map, extract_namespaces,
    extract_try_neg,
};
use super::table::{FINGERPRINTS, VALUE_GRAMMARS};
use super::{GrammarError, read_pin, write_or_check};

/// A minimal `map.rs` with every item the generator reads.
const MAP_FIXTURE: &str = r#"
macro_rules! property_parsers {
  ($($variant:ident($arg:ty) => $parse:ty $([$($namespace:expr),+ $(,)?])?),+ $(,)?) => {
    impl PropertyParser {
      pub fn parse(&self, suffix: &str) -> Option<TailwindProperty> {
        match self {
          $(Self::$variant(f) => <$parse>::parse_tw_with_arbitrary(suffix).map(f),)+
          Self::GradientPosition(f) => {
            TwGradientPosition::parse_tw_with_arbitrary(suffix).map(|p| f(p.0))
          }
        }
      }
    }
  };
}

property_parsers! {
  LengthAuto(Length) => Length,
  ContainerLength(Length) => Length [Namespace::Container, Namespace::Spacing],
  StopColor(TwVarColor) => TwVarColor,
}

pub(crate) static VAR_TARGETS: phf::Map<&str, &[(Namespace, &[LonghandId])]> = phf_map! {
  "w" => &[(Namespace::Spacing, &[LonghandId::Width])],
  "text" => &[(Namespace::Text, &[LonghandId::FontSize]), (Namespace::Color, &[LonghandId::Color])],
};

pub(crate) static PREFIX_PARSERS: phf::Map<&str, &[PropertyParser]> = phf_map! {
  "w" => &[PropertyParser::LengthAuto(TailwindProperty::Width)],
  "grow" | "flex-grow" => &[PropertyParser::LengthAuto(TailwindProperty::FlexGrow)],
  "from" => &[
    PropertyParser::GradientPosition(TailwindProperty::GradientFromPosition),
    PropertyParser::StopColor(TailwindProperty::GradientFrom),
  ],
};

pub(crate) static FIXED_PROPERTIES: phf::Map<&str, TailwindProperty> = phf_map! {
  "flex" => TailwindProperty::Display(Display::Flex),
  "grow" | "flex-grow" => TailwindProperty::FlexGrow(FlexGrow(1.0)),
};
"#;

const NAMESPACE_FIXTURE: &str = r#"
pub enum Namespace { Color, Spacing, Container, Text }
impl Namespace {
  pub(crate) const fn prefix(self) -> &'static str {
    match self {
      Namespace::Color => "--color-",
      Namespace::Spacing => "--spacing-",
      Namespace::Container => "--container-",
      Namespace::Text => "--text-",
    }
  }
}
"#;

const MOD_FIXTURE: &str = r#"
fn split_variant(token: &str) -> Option<(&str, &str)> {
  token.split_once(':')
}
impl Breakpoint {
  pub fn parse(token: &str) -> Option<Self> {
    let width = match_ignore_ascii_case! {token,
      "sm" => Some(Length::Rem(40.0)),
      "2xl" => Some(Length::Rem(96.0)),
      _ => None,
    };
    None
  }
}
impl TailwindProperty {
  fn try_neg(self) -> Option<Self> {
    try_neg!(self;
      try_negative: Margin, Top;
      unary: Rotate;
      grid: GridRowEnd
    )
  }
}
"#;

fn file(text: &str) -> syn::File {
    syn::parse_file(text).expect("fixture parses")
}

fn map_error(text: &str) -> GrammarError {
    let mut out = Extracted::default();
    extract_map(&file(text), &mut out).expect_err("fixture must fail")
}

#[test]
fn map_fixture_extracts_every_table() {
    let mut ex = Extracted::default();
    extract_map(&file(MAP_FIXTURE), &mut ex).expect("fixture extracts");
    assert_eq!(
        ex.fixed,
        BTreeSet::from(["flex".into(), "flex-grow".into(), "grow".into()])
    );
    assert_eq!(ex.prefixes.len(), 4, "`|` alternatives are separate keys");
    let from: Vec<&str> = ex.prefixes["from"]
        .iter()
        .map(|p| p.variant.as_str())
        .collect();
    assert_eq!(
        from,
        ["GradientPosition", "StopColor"],
        "source order is kept"
    );
    assert_eq!(ex.var_targets["text"], ["Text", "Color"]);
    let parsers: Vec<(&str, &str)> = ex
        .parsers
        .iter()
        .map(|p| (p.variant.as_str(), p.parse_type.as_str()))
        .collect();
    assert_eq!(
        parsers,
        [
            ("LengthAuto", "Length"),
            ("ContainerLength", "Length"),
            ("StopColor", "TwVarColor"),
            ("GradientPosition", "TwGradientPosition"),
        ]
    );
    assert_eq!(
        ex.parsers[1].namespaces.as_deref(),
        Some(&["Container".to_owned(), "Spacing".to_owned()][..])
    );
    assert!(
        ex.fingerprints
            .contains_key("macro_rules! property_parsers")
    );
}

#[test]
fn unknown_parse_type_fails_naming_it_and_map_rs() {
    let fixture = MAP_FIXTURE.replace(
        "StopColor(TwVarColor) => TwVarColor,",
        "StopColor(TwVarColor) => TwVarColor,\n  Foo(X) => NewType,",
    );
    let mut ex = Extracted::default();
    extract_map(&file(&fixture), &mut ex).expect("fixture extracts");
    let table: Vec<(&str, &str)> = ["Length", "TwVarColor", "TwGradientPosition"]
        .iter()
        .map(|t| (*t, "&[]"))
        .collect();
    let message = check_parse_types(&ex, &table)
        .expect_err("NewType has no entry")
        .to_string();
    assert!(
        message.contains("NewType") && message.contains(MAP_RS),
        "{message}"
    );
}

#[test]
fn stale_table_entry_fails() {
    let mut ex = Extracted::default();
    extract_map(&file(MAP_FIXTURE), &mut ex).expect("fixture extracts");
    let table = [
        ("Length", "&[]"),
        ("TwVarColor", "&[]"),
        ("TwGradientPosition", "&[]"),
        ("Gone", "&[]"),
    ];
    let message = check_parse_types(&ex, &table)
        .expect_err("Gone is stale")
        .to_string();
    assert!(message.contains("`Gone`"), "{message}");
}

#[test]
fn missing_var_targets_fails_naming_it() {
    let start = MAP_FIXTURE
        .find("pub(crate) static VAR_TARGETS")
        .expect("fixture item");
    let end = MAP_FIXTURE
        .find("pub(crate) static PREFIX_PARSERS")
        .expect("fixture item");
    let fixture = format!("{}{}", &MAP_FIXTURE[..start], &MAP_FIXTURE[end..]);
    let message = map_error(&fixture).to_string();
    assert!(
        message.contains("VAR_TARGETS") && message.contains(MAP_RS),
        "{message}"
    );
}

#[test]
fn unexpected_prefix_parser_shape_fails() {
    let fixture = MAP_FIXTURE.replace(
        "PropertyParser::LengthAuto(TailwindProperty::Width)",
        "PropertyParser::LengthAuto(Width)",
    );
    let message = map_error(&fixture).to_string();
    assert!(
        message.contains("PREFIX_PARSERS") && message.contains("`w`"),
        "{message}"
    );
}

#[test]
fn missing_hard_coded_parser_fails() {
    let fixture = MAP_FIXTURE.replace(
        "Self::GradientPosition(f) =>",
        "Other::GradientPosition(f) =>",
    );
    let message = map_error(&fixture).to_string();
    assert!(
        message.contains("macro_rules! property_parsers"),
        "{message}"
    );
}

#[test]
fn unknown_prefix_variant_and_namespace_fail() {
    let mut ex = Extracted::default();
    extract_map(&file(MAP_FIXTURE), &mut ex).expect("fixture extracts");
    ex.namespaces = extract_namespaces(&file(NAMESPACE_FIXTURE)).expect("fixture extracts");
    check_names(&ex).expect("fixture names are known");

    let mut bogus = ex.clone();
    bogus.prefixes.get_mut("w").expect("w")[0].variant = "Bogus".to_owned();
    let message = check_names(&bogus)
        .expect_err("Bogus is unknown")
        .to_string();
    assert!(
        message.contains("`Bogus`") && message.contains("PREFIX_PARSERS"),
        "{message}"
    );

    let mut nope = ex;
    nope.var_targets
        .insert("x".to_owned(), vec!["Nope".to_owned()]);
    let message = check_names(&nope).expect_err("Nope is unknown").to_string();
    assert!(
        message.contains("Namespace::Nope") && message.contains("VAR_TARGETS"),
        "{message}"
    );
}

#[test]
fn changed_algorithm_fails_with_both_hashes() {
    let mut found: BTreeMap<String, String> = FINGERPRINTS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    check_fingerprints(&found, FINGERPRINTS).expect("recorded hashes match themselves");

    let changed = file(MOD_FIXTURE)
        .items
        .into_iter()
        .find(|item| matches!(item, syn::Item::Fn(f) if f.sig.ident == "split_variant"))
        .expect("fixture fn");
    let new = super::extract::fingerprint(&changed);
    let old = found
        .insert("split_variant".to_owned(), new.clone())
        .expect("recorded");
    let message = check_fingerprints(&found, FINGERPRINTS)
        .expect_err("hash differs")
        .to_string();
    assert!(
        message.contains("split_variant") && message.contains(&old) && message.contains(&new),
        "{message}"
    );

    found.insert("split_variant".to_owned(), old);
    found.insert("impl TailwindPropertyParser for Brand".to_owned(), new);
    let message = check_fingerprints(&found, FINGERPRINTS)
        .expect_err("new impl")
        .to_string();
    assert!(message.contains("for Brand"), "{message}");
}

#[test]
fn mod_and_namespace_fixtures_extract() {
    let mod_rs = file(MOD_FIXTURE);
    assert_eq!(extract_breakpoints(&mod_rs).expect("keys"), ["sm", "2xl"]);
    assert_eq!(
        extract_try_neg(&mod_rs).expect("lists"),
        BTreeSet::from(["GridRowEnd", "Margin", "Rotate", "Top"].map(str::to_owned))
    );
    let wrong = MOD_FIXTURE.replace("unary:", "unary_ops:");
    let message = extract_try_neg(&file(&wrong))
        .expect_err("label")
        .to_string();
    assert!(message.contains("try_neg!"), "{message}");

    let namespaces = extract_namespaces(&file(NAMESPACE_FIXTURE)).expect("prefixes");
    assert_eq!(namespaces[3], ("Text".to_owned(), "--text-".to_owned()));
    let missing_arm = NAMESPACE_FIXTURE.replace("      Namespace::Text => \"--text-\",\n", "");
    let message = extract_namespaces(&file(&missing_arm))
        .expect_err("arm")
        .to_string();
    assert!(message.contains("Namespace::prefix"), "{message}");
}

#[test]
fn pin_must_be_exact() {
    let manifest = |dep: &str| format!("[workspace.dependencies]\n{dep}\n");
    assert_eq!(
        read_pin(&manifest(
            "takumi-core = { version = \"=0.31.0\", default-features = false }"
        )),
        Ok("0.31.0".to_owned())
    );
    assert_eq!(
        read_pin(&manifest("takumi-core = \"=1.2.3\"")),
        Ok("1.2.3".to_owned())
    );
    for bad in [
        "takumi-core = { version = \"0.31\" }",
        "takumi-core = \"=0.31\"",
        "takumi-core = \"^0.31.0\"",
        "other = \"1\"",
    ] {
        let message = read_pin(&manifest(bad)).expect_err(bad).to_string();
        assert!(message.contains("takumi-core"), "{bad}: {message}");
    }
}

#[test]
fn check_mode_fails_on_a_stale_file_and_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("tw-grammar-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("generated.rs");
    std::fs::write(&path, "// edited by hand\n").expect("write");
    let result = write_or_check(&path, "// fresh\n", true);
    let after = std::fs::read_to_string(&path).expect("read");
    assert!(result.is_err());
    assert_eq!(after, "// edited by hand\n");
    assert!(write_or_check(&path, "// fresh\n", false).is_ok());
    assert!(write_or_check(&path, "// fresh\n", true).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The real pinned crate, when `cargo metadata` can find it.
fn real_sources() -> Option<BTreeMap<String, String>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let manifest = root.join("Cargo.toml");
    let version = read_pin(&std::fs::read_to_string(&manifest).ok()?).ok()?;
    let crate_root = super::locate(&manifest, &version).ok()?;
    super::read_sources(&crate_root).ok()
}

#[test]
fn table_covers_exactly_the_real_parse_types() {
    let Some(sources) = real_sources() else {
        eprintln!("takumi-core sources not available; skipped");
        return;
    };
    let ex = extract(&sources).expect("real sources extract");
    let types: BTreeSet<&str> = ex.parsers.iter().map(|p| p.parse_type.as_str()).collect();
    let keys: BTreeSet<&str> = VALUE_GRAMMARS.iter().map(|(k, _)| *k).collect();
    assert_eq!(types.len(), 50);
    assert_eq!(types, keys);
    check(&ex).expect("fingerprints and tables match the pinned source");
}
